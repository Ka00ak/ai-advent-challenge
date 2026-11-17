# 03. Архитектура и контракты

## 1. Принципы

1. **Слой — модуль.** STM, WM, LTM не импортируют друг друга; их соединяет
   только `MemoryService` / сборщик промпта.
2. **Хранилище — снаружи логики.** Функции слоёв работают с чистыми объектами;
   localStorage подключается в `src/storage/*` и в `App.vue`.
3. **Один вход записи.** Все изменения памяти идут через router, tool-обработчик
   или UI-операции; прямых `push` в массивы из компонентов нет.
4. **Контракт раньше кода.** Типы и сигнатуры из этого документа фиксируются в
   задаче T1; дальнейшие изменения — только с правкой спецификации.
5. **Детерминизм.** Сортировки, scoring и усечение не зависят от порядка ключей
   объекта и текущего времени, кроме явно переданного `now()`.

## 2. Структура файлов

```text
11/src/
  memory/
    types.ts            # все интерфейсы из 02-domain-model.md
    config.ts           # DEFAULT_MEMORY_CONFIG, normalizeMemoryConfig
    stm.ts              # StmState, окно, сводка, токены
    working.ts          # WM: upsert, status, archive, prune, render
    ltm.ts              # LTM: upsert, supersede, active, render
    candidates.ts       # локальное извлечение кандидатов (бывший facts.ts)
    candidateExtractor.ts # LLM-извлечение (бывший factExtractor.ts)
    router.ts           # R1–R6, pending, audit
    retrieval.ts        # score + top-K + бюджеты
    prompt.ts           # assembleMemoryPrompt + MemoryBreakdown
    service.ts          # MemoryService: prepare, applyToolWrite, clear
    migration.ts        # facts Дня 10 → LTM
    audit.ts            # события, маскирование секретов
    fixtures.ts         # 34 сообщения контрольного диалога
    probes.ts           # 18 probe-вопросов + probeMatches
    experiment.ts       # 7 конфигураций + очистка + Markdown/JSON отчёт
  agent/
    Agent.ts            # интеграция памяти, tool-loop, memory_write
    compression.ts      # без изменений: summary STM
    summarizer.ts       # без изменений
    history.ts          # без изменений
    ToolRegistry.ts     # без изменений
  storage/
    conversation.ts     # STM-сообщения (ключ lab11)
    summary.ts          # сводка STM (ключ lab11)
    working.ts          # lab11.working.v1
    ltm.ts              # lab11.ltm.v1
    pending.ts          # lab11.pending.v1
    meta.ts             # lab11.meta.v1
    migration.ts        # lab11.migration.v1
    branches.ts         # снимок трёх слоёв
  components/
    MemoryPanel.vue     # три слоя + операции
    MemoryItemCard.vue  # карточка записи
    PendingPanel.vue    # очередь подтверждений
    ExperimentPanel.vue # прогоны, матрица, отчёт
    TokenPanel.vue      # + разбивка по слоям
    SettingsPanel.vue   # + лимиты и режимы памяти
    ApiLogPanel.vue     # + kind='memory'
    HistoryPanel.vue    # STM = история
    BranchPanel.vue     # снимки всех слоёв
  App.vue               # загрузка/сохранение, запуск, эксперимент
```

Удаляются/переименовываются: `agent/facts.ts` → `memory/candidates.ts`,
`agent/factExtractor.ts` → `memory/candidateExtractor.ts`,
`agent/context.ts` → `memory/prompt.ts` + `memory/service.ts`,
`memory/benchmark.ts` → `memory/experiment.ts` (старый benchmark можно оставить
для регрессии, но UI Дня 11 использует новый).

## 3. Контракты модулей

### STM

```ts
export function emptyStm(): StmState
export function stmFromConversation(messages: ChatMessage[], summary: string | null, covered: number): StmState

export interface StmWindow {
  messages: ChatMessage[]
  dropped: number
  tokens: number
}

/** Последние N сообщений с границей по user; последняя user-реплика всегда внутри. */
export function selectStmWindow(stm: StmState, window: number, maxTokens: number): StmWindow

export function stmSummaryTokens(stm: StmState): number
```

Требования: не разрывать `assistant(tool_calls) → tool`; использовать
`findRecentStart`/`repairToolCallSequence`; если последняя реплика одна больше
бюджета — включить её целиком и пометить `overflow` в breakdown.

### WM

```ts
export function emptyWorkingMemory(): WorkingMemoryState

export interface WmUpsert {
  item: Omit<WmItem, 'id' | 'createdAt' | 'updatedAt'>
  now?: () => Date
}

export function upsertWmItem(state: WorkingMemoryState, input: WmUpsert): {
  state: WorkingMemoryState
  op: 'create' | 'update'
  item: WmItem
}

export function setWmItemStatus(state: WorkingMemoryState, id: string, status: WmItem['status']): WorkingMemoryState
export function removeWmItem(state: WorkingMemoryState, id: string): WorkingMemoryState
export function archiveTask(state: WorkingMemoryState): WorkingMemoryState
export function pruneWm(state: WorkingMemoryState, config: MemoryConfig['wm'], now: Date): WorkingMemoryState

export interface RenderedBlock { text: string; tokens: number; includedIds: string[]; dedupedIds: string[] }
export function renderWorkingMemory(state: WorkingMemoryState, maxTokens: number): RenderedBlock
```

Правила: active — по `order`, затем по `updatedAt`; `done`-пункты плана
рендерятся как `[x]`, остальные `done`/`dropped` — нет; при превышении лимита
сначала выкидываются `note`, затем старые `status`, затем `entity`; `goal` и
активный `plan` — в последнюю очередь.

`archiveTask` завершает задачу: `taskId = null`, `taskTitle = ''`, все
`active`-пункты переводятся в `dropped`; `done` остаются для истории и
удаляются GC по `ttlDays`. Автоматического переноса в LTM нет.

### LTM

```ts
export function emptyLtm(conversationId?: string | null): LtmState
export function activeLtmRecords(state: LtmState): LtmRecord[]
export function findActiveLtm(state: LtmState, key: string): LtmRecord | null

export interface UpsertLtmInput {
  kind: LtmKind
  key: string
  value: string
  tags?: string[]
  source: LtmRecord['source']
  confidence?: number
  pinned?: boolean
  legacyKey?: string
  now?: () => Date
}

export function upsertLtm(state: LtmState, input: UpsertLtmInput): {
  state: LtmState
  record: LtmRecord
  op: 'create' | 'update' | 'same'
}

/** Обновление с сохранением истории: старая запись получает supersededBy. */
export function replaceLtm(state: LtmState, input: UpsertLtmInput): {
  state: LtmState
  record: LtmRecord
  supersededId: string | null
}

/*
 * Семантика:
 * - upsertLtm: create при отсутствии key, same при совпадении значения,
 *   при другом значении делегирует replaceLtm и возвращает op='update';
 * - replaceLtm: всегда создаёт новую активную версию, старая получает
 *   supersededBy; прямое обновление значения «на месте» запрещено.
 * - router R3 использует replaceLtm; миграция и UI — upsertLtm.
 */

export function removeLtmRecord(state: LtmState, id: string): LtmState
export function setLtmPinned(state: LtmState, id: string, pinned: boolean): LtmState
export function renderLtmProfile(state: LtmState, maxTokens: number): RenderedBlock
```

`renderLtmProfile` берёт `profile`, `project`, `preference`, сортирует `pinned` →
`kind` → `updatedAt`, усекает по бюджету.

### Кандидаты и router

```ts
export interface CandidateExtraction {
  candidates: MemoryCandidate[]
  requests: number
  promptTokens: number
  completionTokens: number
  costUsd: number
  mode: 'local' | 'llm'
  error?: string
}

export type CandidateExtractor = (input: {
  message: ChatMessage
  activeLtm: LtmRecord[]
}) => Promise<CandidateExtraction>

export function createLocalCandidateExtractor(): CandidateExtractor
export function createLlmCandidateExtractor(options: {
  settings: LlmSettings
  profile: ModelProfile
  signal?: AbortSignal
  onExchangeStart?: (exchange: LlmExchange) => void
  onExchangeEnd?: (exchange: LlmExchange) => void
}): CandidateExtractor

export interface RouteInput {
  message: ChatMessage
  messageIndex: number
  wm: WorkingMemoryState
  ltm: LtmState
  pending: PendingWrite[]
  config: MemoryConfig
  extractor?: CandidateExtractor
  now?: () => Date
}

export interface RouteOutcome {
  wm: WorkingMemoryState
  ltm: LtmState
  pending: PendingWrite[]
  audit: MemoryWriteEvent[]
  rejected: Array<{ candidate: MemoryCandidate; ruleId: 'R1'; reason: string }>
  stats: {
    candidates: number
    wmWrites: number
    ltmWrites: number
    pendingWrites: number
    rejected: number
    requests: number
    promptTokens: number
    completionTokens: number
    costUsd: number
    mode: 'local' | 'llm'
    error?: string
  }
}

export async function routeCandidates(input: RouteInput): Promise<RouteOutcome>
```

### Retrieval и промпт

```ts
export interface RetrievalResult {
  included: LtmRecord[]
  dropped: LtmRecord[]
  tokens: number
}

export function selectRelevantLtm(input: {
  records: LtmRecord[]
  query: string
  wm: WorkingMemoryState
  maxTokens: number
  topK: number
}): RetrievalResult

export interface LayerFlags { stm: boolean; wm: boolean; ltm: boolean }

export interface AssembleInput {
  systemPrompt: string
  stm: StmState
  wm: WorkingMemoryState
  ltm: LtmState
  config: MemoryConfig
  tools: ToolSchema[]
  layers?: LayerFlags     // по умолчанию все true; нужно для ablation
  now?: () => Date
}

export interface AssembleResult {
  messages: ChatMessage[]
  breakdown: MemoryBreakdown
  stmWindow: StmWindow
  includedLtm: LtmRecord[]
  droppedLtm: LtmRecord[]
  notes: string[]
}

export function assembleMemoryPrompt(input: AssembleInput): AssembleResult
```

Текущая реплика пользователя (последняя в `stm.messages`) добавляется всегда,
вне окна STM. При `layers.stm = false` в промпте остаются только системные
блоки, схемы инструментов и эта реплика. Её токены учитываются в
`currentTokens`.

### MemoryService

```ts
export interface MemoryBundle {
  stm: StmState
  wm: WorkingMemoryState
  ltm: LtmState
  pending: PendingWrite[]
  meta: MemoryMetaState
}

export class MemoryService {
  constructor(config: MemoryConfig, options?: { extractor?: CandidateExtractor; now?: () => Date })

  /** Маршрутизирует новые сообщения и собирает промпт. */
  prepare(input: {
    bundle: MemoryBundle
    systemPrompt: string
    tools: ToolSchema[]
    layers?: LayerFlags
    signal?: AbortSignal
  }): Promise<{
    bundle: MemoryBundle
    assembled: AssembleResult
    route: RouteOutcome | null
  }>

  /** Обрабатывает tool memory_write. bundle мутируется иммутабельно. */
  applyToolWrite(bundle: MemoryBundle, args: MemoryWriteArgs): {
    bundle: MemoryBundle
    result: MemoryWriteResult
    audit: MemoryWriteEvent
  }

  clearLayer(bundle: MemoryBundle, layer: MemoryLayer): MemoryBundle
}
```

`clearLayer('ltm')` также очищает `pending` (он относится только к LTM);
`clearLayer('stm')` сбрасывает `meta.processedMessages`; остальные слои не
меняются.

`prepare` берёт `meta.processedMessages`, прогоняет router по новым сообщениям
(начиная с курсора), обновляет `bundle.meta`. Если `approval === 'confirm'`,
LTM-записи не применяются, а попадают в `pending`; WM применяются сразу.
Промпт собирается **один раз перед первым запросом**.

### Миграция и эксперимент

```ts
export function migrateFactsToLtm(input: {
  rawFacts: string | null
  ltm: LtmState
  now?: () => Date
}): { ltm: LtmState; report: MigrationReport }

export type ExperimentMode = 'none' | 'stm' | 'wm' | 'ltm' | 'stm+wm' | 'stm+wm+ltm' | 'full'

export function runMemoryExperiment(input: {
  modes: ExperimentMode[]
  probes: Probe[]
  settings: LlmSettings
  profile: ModelProfile
  config: MemoryConfig
  signal?: AbortSignal
  onProgress?: (event: ExperimentProgress) => void
}): Promise<ExperimentReport>

export function experimentToMarkdown(report: ExperimentReport): string
export function experimentToJson(report: ExperimentReport): string
```

`full` — вся история без слоёв (референс). `none` — system + tools + probe, без
истории. Остальные — соответствующие `LayerFlags`.

## 4. Интеграция с Agent

`AgentConfig` (изменения):

```ts
export interface AgentConfig extends LlmSettings {
  systemPrompt: string
  memory: MemoryBundle
  memoryConfig: MemoryConfig
  memoryExtractor?: CandidateExtractor
  /** legacy-поля contextStrategy/compression/facts удаляются */
}
```

`Agent.run()`:

```text
1. repairToolCallSequence(history), отфильтровать system.
2. Синхронизировать bundle.stm.messages = history (history — источник истины;
   второй копии STM быть не должно).
3. Добавить новую user-реплику в STM (через callback в App).
4. memoryService.prepare():
   - router по новым сообщениям (включая новую реплику);
   - собрать промпт и breakdown;
   - вернуть обновлённый bundle.
5. Отправить запрос; в tool-loop:
   - memory_write обрабатывается до ToolRegistry;
   - запись в память не пересобирает промпт текущего прогона (ADR-6).
6. Вернуть полную STM, WM, LTM, pending, meta, breakdown, audit, route stats.
```

`AgentRunResult` дополняется:

```ts
memory: MemoryBundle
memoryBreakdown: MemoryBreakdown | null
memoryAudit: MemoryWriteEvent[]
memoryRoute: RouteOutcome['stats'] | null
```

`memory_write` — не обычный инструмент реестра, а встроенный вызов агента:

```json
{
  "name": "memory_write",
  "parameters": {
    "layer": { "enum": ["ltm", "wm"] },
    "kind": { "type": "string" },
    "key": { "type": "string" },
    "value": { "type": "string" },
    "scope": { "enum": ["task", "project", "global"] },
    "ttlTurns": { "type": "number" }
  },
  "required": ["layer", "kind", "key", "value"]
}
```

Результат:

```ts
export interface MemoryWriteResult {
  ok: boolean
  layer: 'ltm' | 'wm'
  key: string
  status: 'created' | 'updated' | 'pending' | 'rejected'
  id?: string
  reason?: string
}
```

Схема добавляется в `tools` запроса, чтобы модель знала о возможности. При
вызове Agent перехватывает имя до `ToolRegistry.invoke`.

## 5. Поток данных

```text
App.vue
  │  user text
  ▼
Agent.run
  │
  ├─ STM: append user message ──────────────► onMessageAppended
  │
  ├─ MemoryService.prepare
  │    ├─ candidates: local | llm
  │    ├─ router R1–R6 ──► WM (active)
  │    │                 ├─► LTM (auto)
  │    │                 └─► pending (confirm)
  │    ├─ retrieval LTM (profile + topK)
  │    └─ assembleMemoryPrompt ──► messages + breakdown
  │
  ├─ LLM tool-loop ──► memory_write ──► bundle
  │
  └─ result: messages, memory, breakdown, audit
         │
         ▼
App.vue: save STM / WM / LTM / pending / meta separately
         │
         ▼
UI: MemoryPanel, TokenPanel, ApiLogPanel
```

Сохранение — после каждого завершённого запуска и после ручных операций.
Отдельные ключи сохраняются независимо: сбой записи LTM не мешает сохранить STM.

## 6. Обработка ошибок

| Ситуация | Поведение |
|---|---|
| LLM-извлечение вернуло мусор | откат на `local`, `stats.error`, запись в журнал API с `kind='memory'` |
| LTM/WM/STM JSON повреждён | слой пустой, баннер с текстом ошибки, данные не перезаписываются до действия пользователя |
| `localStorage` переполнен | сохранение возвращает `{ok:false}`; приложение работает в памяти, баннер с советом очистить слой |
| pending содержит невалидный кандидат | запись отбрасывается, причина в аудите |
| секрет в кандидате | `R1`, запись отклонена, значение маскировано |
| ветка ссылается на удалённую LTM | `refLtmId` игнорируется, WM-пункт рендерится своим текстом, в UI бейдж «ссылка потеряна» |
| experiment отменён | частичный отчёт не сохраняется, прогресс сбрасывается |

## 7. Настройки

`AppSettings` дополняется `memory: MemoryConfig`; удаляются
`contextStrategy`, `factsMode`, `compressionEnabled` (заменяются полями STM).
Загрузка старых настроек:

```text
lab11.settings.v1 отсутствует → прочитать lab10.settings.v1
  factsMode → memory.extractionMode
  keepRecentMessages → memory.stm.window
  summarizeEveryMessages → memory.stm.summarizeEveryMessages
  summaryMode → memory.stm.summaryEnabled (llm только если явно включено)
  остальное → DEFAULT_MEMORY_CONFIG
```

Ключ API по-прежнему хранится только в `lab11.apiKey.v1` при явном согласии.

## 8. Ветки

Снимок ветки:

```ts
export interface BranchSnapshot {
  id: string
  label: string
  createdAt: string
  stm: StmState
  wm: WorkingMemoryState
  ltm: LtmState
  pending: PendingWrite[]
  meta: MemoryMetaState
}
```

При переключении все пять частей заменяются целиком. LTM не делится между
ветками: продолжение ветки A не меняет LTM ветки B. `checkpoint` хранит снимок
всех слоёв и позволяет создать две ветки от одной точки.
