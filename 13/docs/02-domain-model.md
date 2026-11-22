# 02. Доменная модель

## 1. Слои

| Слой | Единица | Горизонт | Кто пишет | Блок промпта |
|---|---|---|---|---|
| STM | сообщение диалога | окно N + сводка | Agent, пользователь | STM |
| WM | пункт задачи | одна задача | Router, tool, пользователь | WM |
| LTM | запись знания | постоянно | Router (pending), tool, пользователь, импорт | LTM |

Системный промпт, схемы инструментов и текущая реплика не являются слоями.
Сводка вытесненных сообщений — атрибут STM, а не четвёртый слой.

## 2. Rust-типы

Контракты живут в `src/domain/memory.rs`. Все структуры сериализуются
`serde` в **snake_case**; enum-значения — в нижнем регистре.

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type Id = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryLayer { Stm, Wm, Ltm }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LtmKind {
    Profile,     // кто пользователь: имя, роль, язык
    Project,     // проект: название, заказчик, тимлид, цель
    Preference,  // устойчивое предпочтение
    Decision,    // решение или договорённость
    Knowledge,   // сроки, бюджеты, ограничения, риски
    Glossary,    // термин проекта
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WmKind {
    Goal,
    Plan,
    Status,
    Entity,
    #[serde(rename = "open-question")]
    OpenQuestion,
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WmStatus { Active, Done, Dropped }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemorySource {
    MigrationFacts,
    User,
    Agent,
    Router,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CandidateScope { Turn, Task, Project, Global }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LtmRecord {
    pub id: Id,
    pub kind: LtmKind,
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source: MemorySource,
    pub confidence: f32,
    pub pinned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub superseded_by: Option<Id>,
    pub superseded_at: Option<DateTime<Utc>>,
    pub legacy_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LtmState {
    pub version: u32,
    pub conversation_id: Option<String>,
    pub records: Vec<LtmRecord>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WmItem {
    pub id: Id,
    pub kind: WmKind,
    pub key: String,
    pub value: String,
    pub status: WmStatus,
    pub order: Option<u32>,
    pub ref_ltm_id: Option<Id>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub source_message_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkingMemoryState {
    pub version: u32,
    pub task_id: Option<String>,
    pub task_title: String,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub items: Vec<WmItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StmState {
    pub version: u32,
    pub conversation_id: String,
    pub messages: Vec<ChatMessage>,
    pub summary: Option<String>,
    pub covered_messages: usize,
    pub updated_at: DateTime<Utc>,
}
```

Сообщение и кандидат:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole { System, User, Assistant, Tool }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCandidate {
    pub key: String,
    pub value: String,
    pub kind: LtmKind,
    #[serde(default)]
    pub tags: Vec<String>,
    pub confidence: f32,
    pub scope: CandidateScope,
    pub ttl_turns: Option<u32>,
    pub legacy_category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingWrite {
    pub id: Id,
    pub candidate: MemoryCandidate,
    pub target_layer: MemoryLayer,
    pub target_kind: LtmKind,
    pub rule_id: String,          // R1..R6
    pub reason: String,
    pub old_value: Option<String>,
    pub message_index: usize,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryWriteEvent {
    pub id: Id,
    pub at: DateTime<Utc>,
    pub layer: MemoryLayer,
    pub op: WriteOp,              // create/update/delete/promote/demote/reject/clear
    pub key: String,
    pub value: Option<String>,    // секреты маскируются
    pub source: MemorySource,
    pub rule_id: Option<String>,
    pub reason: String,
    pub message_index: Option<usize>,
}
```

`WriteOp` и `MemoryBundle`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WriteOp { Create, Update, Delete, Promote, Demote, Reject, Clear }

#[derive(Debug, Clone)]
pub struct MemoryBundle {
    pub stm: StmState,
    pub wm: WorkingMemoryState,
    pub ltm: LtmState,
    pub pending: Vec<PendingWrite>,
    pub meta: MemoryMetaState,
}
```

Метаданные:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetaState {
    pub version: u32,
    pub conversation_id: Option<String>,
    pub processed_messages: usize,
    pub last_route_at: Option<DateTime<Utc>>,
}
```

Разбивка токенов:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryBreakdown {
    pub system_tokens: usize,
    pub ltm_profile_tokens: usize,
    pub ltm_relevant_tokens: usize,
    pub wm_tokens: usize,
    pub stm_summary_tokens: usize,
    pub stm_tokens: usize,
    pub current_tokens: usize,
    pub tools_tokens: usize,
    pub total_tokens: usize,
    pub layer_delta: LayerDelta,   // ltm / wm / stm
    pub counts: MemoryCounts,      // ltm_records / wm_items / stm_messages
    pub included_ltm_ids: Vec<Id>,
    pub dropped_ltm_ids: Vec<Id>,
    pub deduped_wm_ids: Vec<Id>,
}
```

Настройки:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub max_iterations: usize,
    pub memory: MemoryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub stm: StmConfig,
    pub wm: WmConfig,
    pub ltm: LtmConfig,
    pub extraction_mode: ExtractionMode,
    pub approval: Approval,
    pub total_budget: usize,
}
```

`StmConfig { window, max_tokens, summary_enabled, summarize_every_messages }`,
`WmConfig { max_items, max_tokens, ttl_days }`,
`LtmConfig { max_records, profile_max_tokens, relevant_max_tokens, top_k }`.
`ExtractionMode { Local, Llm }`, `Approval { Confirm, Auto }`.

## 3. Инварианты

1. `LtmRecord.key` уникален среди **активных** записей LTM.
2. `WmItem.key` уникален в пределах задачи.
3. Активная LTM-запись: `superseded_by.is_none()`. Записи с `superseded_by`
   хранятся для истории, но не рендерятся и не выбираются retrieval.
4. `WmStatus::Done` не рендерится, кроме `Plan`: выполненный шаг выводится
   как `[x]`. `Dropped` не рендерится никогда.
5. В STM нет `ChatRole::System`.
6. `processed_messages` монотонно растёт; если STM стал короче (импорт),
   курсор сбрасывается в `0`, данные других слоёв не удаляются.
7. Все даты — ISO 8601 UTC (`DateTime<Utc>`).
8. `key` ≤ 64 символов, `value` ≤ 500, `tags` ≤ 8.
9. `confidence ∈ [0,1]`; импорт ставит `0.9`.
10. Секреты не попадают ни в слой, ни в pending, ни в аудит.
11. Очистка слоя не меняет остальные слои; очистка LTM дополнительно очищает
    pending.
12. Изменение значения LTM создаёт новую активную версию; старая получает
    `superseded_by`. Прямая перезапись значения запрещена.
13. Последняя user-реплика — текущий вопрос. Сборщик выносит её в
    `current_tokens` и не включает в окно STM; `layers.stm=false` убирает
    историю, но не вопрос.

## 4. Жизненный цикл

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
                              active LTM   active LTM
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

- **STM**: сообщение → в окне → вытеснено → в summary → удалено при очистке.
- **WM**: active → done → GC через `ttl_days`; «Завершить задачу» переводит
  active в dropped, `done` остаются для истории.
- **LTM**: pending → active → superseded → удалено вручную/GC версий.

## 5. Файлы и каталоги

Data-dir: `--data-dir <path>` → `$LAB11_HOME` → `$XDG_DATA_HOME/lab11` →
`~/.local/share/lab11`. Для демо: `./data`.

| Путь | Владелец | Содержимое | Лимит |
|---|---|---|---|
| `stm/conversation.json` | STM | `StmState` без summary | 8 МБ |
| `stm/summary.json` | STM | сводка | 32 КБ |
| `wm/working.json` | WM | `WorkingMemoryState` | 128 КБ |
| `ltm/ltm.json` | LTM | `LtmState` | 512 КБ |
| `pending/pending.json` | Router | `Vec<PendingWrite>` | 64 КБ |
| `meta/meta.json` | Router | `MemoryMetaState` | 4 КБ |
| `migration/report.json` | Импорт | `MigrationReport` | 16 КБ |
| `settings.json` | CLI/TUI | `Settings` без ключа | 16 КБ |
| `secrets.json` | CLI/TUI | ключ, если разрешён | 0600 |
| `lock` | Процесс | lock-файл (fs2) | — |

Правила:

- запись атомарна: `*.tmp` → `fsync` → `rename`; перед заменой — `*.bak`;
- `load_stm` склеивает `conversation.json` и `summary.json` в `StmState`;
  `save_stm` пишет оба файла независимо;
- на Unix `settings.json`/`secrets.json` — режим `0600`;
- неверная `version` — ошибка, а не молчаливая потеря;
- битый файл не перезаписывается: приложение показывает ошибку и предлагает
  `memory reset --layer` (файл уходит в `*.corrupt-<ts>`).

## 6. Миграция facts Дня 10

Источник: JSON `lab10.facts.v1` или `{"facts":[...]}` или массив facts.
Приёмник: `ltm/ltm.json`.

| `category` Дня 10 | `LtmKind` | Комментарий |
|---|---|---|
| `имя` | `Project` | ключи `project.*`, `customer`, `team.*` |
| `имя` | `Profile` | ключи `user.*`, `profile.*` |
| `имя` | `Knowledge` | остальные |
| `цель` | `Project` | цель проекта |
| `срок` | `Knowledge` | дедлайны и вехи |
| `бюджет` | `Knowledge` | суммы |
| `ограничение` | `Knowledge` | инфраструктура, команда |
| `решение` | `Decision` | БД, API, стек |
| `договорённость` | `Decision` | созвоны, форматы |
| `предпочтение` | `Preference` | стиль ответов |
| `риск` | `Knowledge` | тег `risk` |
| `другое` | `Knowledge` | — |

Алгоритм:

```text
1. Прочитать файл; проверить version (если есть) и массив facts.
2. Для каждого факта:
   key = fact.key; value = fact.value (дословно);
   kind = map(category, key);
   tags = ["migrated:day10", category];
   confidence = 0.9; source = MigrationFacts; legacy_key = fact.key.
3. Upsert по key: совпадает → skip; отличается → новая версия (supersede).
4. Сохранить LTM и отчёт.
5. Повторный запуск: added = 0, updated = 0.
```

`MigrationReport`:

```rust
pub struct MigrationReport {
    pub at: DateTime<Utc>,
    pub source: String,
    pub found: usize,
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}
```

Пример `ltm/ltm.json`:

```json
{
  "version": 1,
  "conversation_id": null,
  "records": [
    {
      "id": "ltm_01",
      "kind": "profile",
      "key": "user.name",
      "value": "Алексей",
      "tags": ["migrated:day10", "имя"],
      "source": "migration-facts",
      "confidence": 0.9,
      "pinned": false,
      "created_at": "2026-11-17T10:00:00Z",
      "updated_at": "2026-11-17T10:00:00Z",
      "last_used_at": null,
      "superseded_by": null,
      "superseded_at": null,
      "legacy_key": "user.name"
    }
  ],
  "updated_at": "2026-11-17T10:00:00Z"
}
```

Пример `wm/working.json`:

```json
{
  "version": 1,
  "task_id": "task-marketing-estimate",
  "task_title": "Подготовить смету маркетинга к релизу",
  "started_at": "2026-11-17T10:00:00Z",
  "updated_at": "2026-11-17T10:10:00Z",
  "items": [
    { "kind": "goal", "key": "task.goal", "value": "Смета маркетинга к релизу", "status": "active", "order": 0 },
    { "kind": "plan", "key": "plan.2", "value": "Оценить каналы", "status": "active", "order": 2 },
    { "kind": "entity", "key": "estimate.variant", "value": "Вариант B — 140 000 ₽", "status": "active" }
  ]
}
```

(В реальном файле у пунктов есть `id`, `created_at`, `updated_at`,
`ref_ltm_id`, `source_message_index`; в примере опущены для краткости.)
