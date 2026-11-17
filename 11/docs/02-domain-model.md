# 02. Доменная модель

## 1. Слои

| Слой | Единица | Горизонт | Кто пишет | Кто читает промпт |
|---|---|---|---|---|
| STM | сообщение диалога | окно последних N + сводка | Agent, пользователь | блок STM |
| WM | пункт задачи | одна задача | Router, tool, пользователь | блок WM |
| LTM | запись знания | постоянно | Router (через pending), tool, пользователь, миграция | блоки LTM |

Системный промпт, схемы инструментов и текущая реплика не являются слоями
памяти. Сводка вытесненных сообщений — атрибут STM, а не четвёртый слой.

## 2. Типы

Контракты живут в `src/memory/types.ts`. Ниже — обязательная форма; имена
полей менять нельзя без обновления этого документа.

```ts
export type MemoryLayer = 'stm' | 'wm' | 'ltm'

export type LtmKind =
  | 'profile'      // кто пользователь: имя, роль, язык
  | 'project'      // проект: название, заказчик, тимлид, цель
  | 'preference'   // устойчивое предпочтение: формат, стиль, ограничения
  | 'decision'     // принятое решение или договорённость
  | 'knowledge'    // знание: сроки, бюджеты, ограничения, риски
  | 'glossary'     // термин проекта

export type WmKind =
  | 'goal'         // что делаем прямо сейчас
  | 'plan'         // шаг плана, order + done
  | 'status'       // что уже сделано / в работе
  | 'entity'       // выбранный вариант, файл, ссылка, значение
  | 'open-question'// нерешённый вопрос
  | 'note'         // временная заметка

export interface LtmRecord {
  id: string
  kind: LtmKind
  /** Уникальный стабильный ключ: project.name, decision.database… */
  key: string
  value: string
  tags: string[]
  /** Откуда взялось: миграция, пользователь, агент, router. */
  source: 'migration-facts' | 'user' | 'agent' | 'router'
  confidence: number            // 0..1
  pinned: boolean
  createdAt: string             // ISO
  updatedAt: string             // ISO
  lastUsedAt?: string
  /** Активная запись не имеет supersededBy. */
  supersededBy?: string
  supersededAt?: string
  /** След из Дня 10: исходный ключ facts. */
  legacyKey?: string
}

export interface LtmState {
  version: 1
  conversationId: string | null
  records: LtmRecord[]
  updatedAt: string
}

export interface WmItem {
  id: string
  kind: WmKind
  key: string
  value: string
  status: 'active' | 'done' | 'dropped'
  order?: number
  /** Ссылка на LTM вместо дублирования значения. */
  refLtmId?: string
  createdAt: string
  updatedAt: string
  sourceMessageIndex?: number
}

export interface WorkingMemoryState {
  version: 1
  taskId: string | null
  taskTitle: string
  startedAt: string | null
  updatedAt: string
  items: WmItem[]
}

export type StmRole = 'user' | 'assistant' | 'tool'

export interface StmState {
  /** Полная история, не только окно. */
  messages: ChatMessage[]
  /** Сводка вытесненной части (может быть null). */
  summary: string | null
  coveredMessages: number
  updatedAt: string
}
```

Кандидат и маршрутизация:

```ts
export interface MemoryCandidate {
  key: string
  value: string
  /** Предварительный тип; router может уточнить. */
  kind: LtmKind | WmKind
  tags: string[]
  confidence: number
  scope: 'turn' | 'task' | 'project' | 'global'
  ttlTurns?: number
  /** Категория facts Дня 10, если кандидат пришёл из старого extractor. */
  legacyCategory?: FactCategory
}

export interface PendingWrite {
  id: string
  candidate: MemoryCandidate
  targetLayer: 'ltm' | 'wm'
  targetKind: LtmKind | WmKind
  ruleId: 'R1' | 'R2' | 'R3' | 'R4' | 'R5' | 'R6'
  reason: string
  messageIndex: number
  createdAt: string
}

export interface MemoryWriteEvent {
  id: string
  at: string
  layer: MemoryLayer
  op: 'create' | 'update' | 'delete' | 'promote' | 'demote' | 'reject' | 'clear'
  key: string
  value?: string          // секреты маскируются
  source: 'router' | 'agent-tool' | 'user' | 'migration' | 'gc'
  ruleId?: string
  reason: string
  messageIndex?: number
}
```

Метаданные и настройки:

```ts
export interface MemoryMetaState {
  version: 1
  conversationId: string | null
  /** Сколько сообщений STM уже прогнано через router. */
  processedMessages: number
  lastRouteAt: string
}

export interface MemoryConfig {
  stm: {
    window: number                  // 8 по умолчанию, 4..20
    maxTokens: number               // 1600
    summaryEnabled: boolean         // true
    summarizeEveryMessages: number  // 10
  }
  wm: {
    maxItems: number                // 24
    maxTokens: number               // 320
    ttlDays: number                 // 14
  }
  ltm: {
    maxRecords: number              // 300
    profileMaxTokens: number        // 220
    relevantMaxTokens: number       // 360
    topK: number                    // 8
  }
  extractionMode: 'local' | 'llm'
  approval: 'confirm' | 'auto'
  totalBudget: number               // 2600
}
```

Разбивка токенов:

```ts
export interface MemoryBreakdown {
  systemTokens: number
  ltmProfileTokens: number
  ltmRelevantTokens: number
  wmTokens: number
  stmSummaryTokens: number
  stmTokens: number
  /** Текущая реплика пользователя: добавляется всегда, вне окна STM. */
  currentTokens: number
  toolsTokens: number
  totalTokens: number
  layerDelta: { ltm: number; wm: number; stm: number }
  counts: { ltmRecords: number; wmItems: number; stmMessages: number }
  includedLtmIds: string[]
  droppedLtmIds: string[]
  dedupedWmIds: string[]
}
```

## 3. Инварианты

1. `LtmRecord.key` уникален среди **активных** записей LTM.
2. `WmItem.key` уникален в пределах задачи.
3. Активная LTM-запись: `supersededBy === undefined`. Записи с `supersededBy`
   хранятся для истории, но не рендерятся и не выбираются retrieval.
4. `WmItem.status === 'done'` не рендерится в промпт, кроме `plan`: выполненный шаг выводится как `[x]`, чтобы был виден прогресс. `dropped` не рендерится никогда.
5. В STM нет `system`-сообщений; системный промпт собирается заново.
6. `processedMessages` монотонно растёт; если STM стал короче (импорт, ветка),
   курсор сбрасывается в `0`, но данные других слоёв не удаляются.
7. Все даты — ISO 8601 UTC.
8. `key` ≤ 64 символов, `value` ≤ 500 символов, `tags` ≤ 8 штук.
9. `confidence` ∈ [0, 1]; миграция ставит `0.9`.
10. Секреты не попадают ни в один слой, ни в pending, ни в аудит.
11. Очистка слоя не меняет остальные слои.
12. Изменение значения LTM создаёт новую активную версию; старая получает `supersededBy`. Прямая перезапись значения «на месте» запрещена.
13. Последняя user-реплика — текущий вопрос. Сборщик промпта выносит её в
    `currentTokens` и не включает в окно STM; `layers.stm = false` убирает
    историю, но не вопрос.

## 4. Жизненный цикл записи

```text
реплика пользователя
        │
        ▼
  candidate (extract)
        │
        ▼
     router
   ┌────┴─────────────┬──────────────────┐
   ▼                  ▼                  ▼
reject (R1/R6)   WM item (R2/R5)     LTM pending (R3/R4)
                                        │
                              confirm ──┴── auto
                                 │           │
                                 ▼           ▼
                              accepted     active LTM
                                 │
                                 ▼
                              active LTM
        │
        ▼
  update / supersede / forget
        │
        ▼
  superseded (LTM) | done/dropped (WM)
        │
        ▼
      GC / archive
```

- **STM**: сообщение → в окне → вытеснено → в summary → удалено при очистке STM.
- **WM**: active → done → GC через `ttlDays`; при «Завершить задачу» все active
  либо архивируются, либо пользователь переносит нужное в LTM.
- **LTM**: pending → active → superseded → (опционально) удалено вручную.

## 5. Хранилища

| Ключ | Владелец | Содержимое | Лимит |
|---|---|---|---|
| `lab11.conversation.v1` | STM | `{version,id,createdAt,updatedAt,messages}` | 4 МБ общий |
| `lab11.summary.v1` | STM | сводка вытесненной части | 20 КБ |
| `lab11.working.v1` | WM | `WorkingMemoryState` | 64 КБ |
| `lab11.ltm.v1` | LTM | `LtmState` | 256 КБ |
| `lab11.pending.v1` | Router | `PendingWrite[]` | 32 КБ |
| `lab11.meta.v1` | Router | `MemoryMetaState` | 2 КБ |
| `lab11.migration.v1` | Миграция | отчёт последней миграции | 8 КБ |
| `lab11.settings.v1` | UI | `AppSettings` без ключа | 8 КБ |
| `lab11.apiKey.v1` | UI | ключ, если включено «запомнить» | — |
| `lab11.branches.v1` | Ветки | снимки STM+WM+LTM | 1 МБ |

Правила:

- Каждый ключ читается и валидируется своим модулем.
- Версия формата обязательна; при несовпадении — ошибка, а не молчаливая потеря.
- Импорт истории/смена беседы сбрасывает `lab11.meta.v1`, `lab11.pending.v1`,
  `lab11.summary.v1`; LTM и WM пользователь очищает отдельно (с предупреждением).

## 6. Миграция facts Дня 10

Источник: `lab10.facts.v1` (`StickyFactsState`).
Приёмник: `lab11.ltm.v1`.

Правила маппинга:

| `FactCategory` Дня 10 | `LtmKind` | Комментарий |
|---|---|---|
| `имя` | `project` | если ключ начинается с `project.`, `customer`, `team.` |
| `имя` | `profile` | если ключ начинается с `user.`, `profile.` |
| `имя` | `knowledge` | остальные имена |
| `цель` | `project` | цель проекта |
| `срок` | `knowledge` | дедлайны и вехи |
| `бюджет` | `knowledge` | суммы |
| `ограничение` | `knowledge` | инфраструктура, команда, правила |
| `решение` | `decision` | БД, API, стек |
| `договорённость` | `decision` | созвоны, форматы работы |
| `предпочтение` | `preference` | стиль ответов |
| `риск` | `knowledge` | тег `risk` |
| `другое` | `knowledge` | — |

Алгоритм:

```text
1. Прочитать lab10.facts.v1; проверить version и массив facts.
2. Для каждого факта:
   - key = fact.key (стабильный латинский ключ уже есть);
   - value = fact.value (дословно);
   - kind = map(category, key);
   - tags = ['migrated:day10', category];
   - confidence = 0.9;
   - source = 'migration-facts';
   - legacyKey = fact.key.
3. Upsert по key: если активная запись с таким key есть и value совпадает —
   skip; если отличается — обновить value/updatedAt и добавить тег.
4. Сохранить LTM и отчёт миграции.
5. Повторный запуск даёт added = 0, updated = 0 (если facts не изменились).
```

Отчёт:

```ts
export interface MigrationReport {
  at: string
  sourceKey: string
  found: number
  added: number
  updated: number
  skipped: number
  errors: string[]
}
```

Пример результата для контрольного диалога Дня 10:

```json
{
  "version": 1,
  "conversationId": null,
  "records": [
    {
      "id": "ltm_project_name",
      "kind": "project",
      "key": "project.name",
      "value": "«Атлас»",
      "tags": ["migrated:day10", "имя"],
      "source": "migration-facts",
      "confidence": 0.9,
      "pinned": false,
      "createdAt": "2026-11-17T10:00:00.000Z",
      "updatedAt": "2026-11-17T10:00:00.000Z",
      "legacyKey": "project.name"
    }
  ],
  "updatedAt": "2026-11-17T10:00:00.000Z"
}
```

## 7. Примеры каждого слоя

**STM** (только последние 4 из 34 сообщений):

```json
{
  "messages": [
    { "role": "user", "content": "Созвон с Мариной — в четверг в 11:00." },
    { "role": "assistant", "content": "Записал: четверг, 11:00." },
    { "role": "user", "content": "Черновик сметы лежит в файле smeta-v3.xlsx…" },
    { "role": "assistant", "content": "Понял: файл smeta-v3.xlsx…" }
  ],
  "summary": "Обсуждали смету маркетинга, выбрали вариант B, подняли бюджет до 150 000.",
  "coveredMessages": 30
}
```

**WM**:

```json
{
  "taskId": "task-marketing-estimate",
  "taskTitle": "Подготовить смету маркетинга к релизу",
  "items": [
    { "kind": "goal", "key": "task.goal", "value": "Смета маркетинга к релизу", "status": "active", "order": 0 },
    { "kind": "plan", "key": "plan.1", "value": "Собрать каналы", "status": "done", "order": 1 },
    { "kind": "plan", "key": "plan.2", "value": "Оценить каналы", "status": "active", "order": 2 },
    { "kind": "entity", "key": "estimate.variant", "value": "Вариант B, 140 000 ₽", "status": "active" },
    { "kind": "open-question", "key": "question.pr", "value": "Нужен ли бюджет на PR-подрядчика?", "status": "active" }
  ]
}
```

**LTM**:

```json
{
  "records": [
    { "kind": "project", "key": "project.name", "value": "«Атлас»", "pinned": true },
    { "kind": "decision", "key": "decision.database", "value": "PostgreSQL; MongoDB не используем" },
    { "kind": "knowledge", "key": "constraint.infra", "value": "Только российские ЦОД" },
    { "kind": "preference", "key": "preference.style", "value": "Отвечать коротко, без вступлений" }
  ]
}
```
