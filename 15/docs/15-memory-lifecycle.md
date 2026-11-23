# День 15. Устаревание и конфликты памяти

## 1. Задача

Память ассистента должна не только пополняться, но и обновляться. Нужно:

1. определить правила жизни фактов в LTM;
2. заменять противоречащий факт новым значением, а не добавлять рядом;
3. отдавать приоритет факту с более поздним временем записи;
4. фиксировать замену в журнале «что было → что стало»;
5. не пропускать старые значения в промпт;
6. очищать рабочую память после завершения задачи, сохраняя LTM проекта.

Всё это работает через вкладку `1 Чат`: роутер обновляет память до сборки
промпта, поэтому ответ модели уже видит актуальное значение. Профиль,
инварианты, автомат задачи и три слоя памяти остаются частью того же ядра
`AppController`.

## 2. Правила жизни фактов

`src/domain/memory.rs`:

```rust
pub enum FactPolicy {
    Replace,
    Append,
    TaskScoped,
}
```

| Правило | Поведение | Какие ключи |
|---|---|---|
| `replace` | одно актуальное значение; новая запись создаёт редакцию и помечает старые `superseded_by` | `project.deadline`, `project.status`, `project.version`, `requirement.current`, `decision.approach`, `budget.*`, `decision.database`, `constraint.*` |
| `append` | новое уникальное значение добавляется рядом; старые остаются активными | `decisions.accepted`, `history.*`, `log.*`, `changes.*`, `accepted.*` |
| `task-scoped` | как `replace`, но все версии удаляются после завершения задачи | `task.*`, `plan.*`, `status.*`, `estimate.*`, `question.*`, `note.*`, `file.*` |

Правило выводится детерминированно в `src/memory/lifecycle.rs`:

```rust
pub fn policy_for(key: &str, kind: LtmKind, scope: CandidateScope) -> FactPolicy;
```

У записи LTM есть:

```rust
pub struct LtmRecord {
    // ...
    pub policy: FactPolicy,
    pub scope: CandidateScope,
    pub revision: u32,
    pub recorded_at: Option<DateTime<Utc>>,
    pub superseded_by: Option<Id>,
    pub superseded_at: Option<DateTime<Utc>>,
}
```

`recorded_at` — время записи факта, `revision` — номер редакции ключа.
Для старых JSON без этих полей используются безопасные значения по
умолчанию: `replace`, `project`, редакция 1, `updated_at` как время записи.

### Канонические ключи

`canonical_key` приводит синонимы к одному ключу:

| Вход | Ключ |
|---|---|
| `срок`, `дедлайн`, `срок проекта`, `project.deadline` | `project.deadline` |
| `статус проекта`, `project.status` | `project.status` |
| `версия`, `версия продукта`, `project.version` | `project.version` |
| `требование`, `requirement` | `requirement.current` |
| `подход`, `выбранный подход`, `decision.approach` | `decision.approach` |

Поэтому `upsert_ltm` не создаёт второй факт срока: он находит активную
запись по каноническому ключу и заменяет её.

## 3. Разрешение конфликтов

### Алгоритм

```text
incoming = факт с key, value, recorded_at
existing = последняя активная запись canonical_key(key)

if existing.value == incoming.value:
    Same, ничего не пишем
elif incoming.recorded_at < existing.recorded_at:
    Stale, сохраняем existing (более поздняя запись приоритетнее)
elif policy == append:
    создаём новую активную запись рядом
    журнал: op=append
else:
    создаём новую редакцию revision+1
    старые активные записи помечаем superseded_by/superseded_at
    журнал: op=replace, old_value, new_value, old/new id
```

Ключевые свойства:

- **не добавляется рядом**: у replace-ключа в активных записях остаётся
  ровно одно значение;
- **позже — значит главнее**: `recorded_at` сравнивается до записи;
- **след остаётся**: журнал append-only, старые редакции хранятся как
  `superseded` (активная + 2 версии, затем GC версий).

### Журнал

```rust
pub enum LtmRevisionOp {
    Create,
    Replace,
    Append,
    Delete,
    TaskCleanup,
}

pub struct LtmRevision {
    pub id: String,
    pub at: DateTime<Utc>,
    pub op: LtmRevisionOp,
    pub key: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub old_record_id: Option<String>,
    pub new_record_id: Option<String>,
    pub source: MemorySource,
    pub reason: String,
}
```

Журнал лежит в `data/ltm/ltm.json` рядом с записями (`LtmState.revisions`),
поэтому переживает перезапуск. Лимит — 500 последних записей. Секреты
маскируются тем же санитайзером, что и значения.

Пример:

```json
{
  "op": "replace",
  "key": "project.deadline",
  "old_value": "15 декабря 2026",
  "new_value": "20 декабря 2026",
  "source": "router",
  "reason": "конфликт: «15 декабря 2026» → «20 декабря 2026» (запись ... приоритетнее)"
}
```

## 4. Промпт без старых значений

`assemble_prompt` делает две вещи:

1. LTM рендерит только `is_active()`; `superseded` и task-scoped удалённые
   записи не попадают ни в профиль, ни в relevant top-K.
2. `mask_stale_values` собирает устаревшие значения из superseded-записей и
   журнала замен и заменяет их в истории STM/сводке на `«устарело»`.
   Текущая (последняя) реплика пользователя не маскируется: в ней может быть
   явное «было → стало».

Дополнительно каждая строка LTM содержит время записи:

```text
- [knowledge, replace, записано 2026-11-23 03:28] project.deadline: 20 декабря 2026
```

System prompt получает правила 18–20:

- при конфликте актуально значение с более поздним временем;
- `«устарело»` нельзя называть или восстанавливать;
- replace/append/task-scoped не смешиваются.

В результате модель не видит старый срок даже в исторической реплике.
Проверка в тесте: 8 промптов после 4 изменений — **0** вхождений старых
значений.

### Почему конфликт не уходит в pending

В режиме `confirm` новый факт может ждать подтверждения, но **конфликт
существующего ключа** разрешается сразу. Иначе в промпте осталось бы старое
активное значение, и агент ответил бы устаревшими данными. `confirm`
по-прежнему применяется к созданию новых фактов и `memory_write`; режим
по умолчанию Дня 15 — `auto`.

`memory_write` при обновлении существующего ключа тоже вызывает
`replace_ltm` и пишет журнал.

## 5. Очистка после завершения задачи

`cleanup_after_task` (`src/memory/lifecycle.rs`):

1. считает пункты WM и полностью заменяет WM на пустое состояние;
2. удаляет все записи LTM с `policy == TaskScoped`;
3. для каждой активной удалённой записи пишет `LtmRevisionOp::TaskCleanup`
   с ключом и старым значением;
4. удаляет task-scoped pending;
5. не трогает replace/append факты проекта и профиля.

Вызывается:

- `AppController::finish_task_memory()` — вкладка `8 Состояния` клавиша `c`,
  вкладка `2 Память` фокус WM клавиша `c`;
- автоматически из `run_task_cancellable`, когда автомат дошёл до `done`
  после `validation → done`.

Итог: WM пуста, `task.rate` и подобные факты удалены, `project.deadline`,
`project.version`, `decisions.accepted` сохранены. Отчёт очистки доступен в
`last_cleanup()` и виден на вкладке `10`.

## 6. CLI

```sh
lab15 --data-dir ./data memory conflicts
lab15 --data-dir ./data memory conflicts --key project.deadline --json
lab15 --data-dir ./data memory policies
lab15 --data-dir ./data memory policies --json
lab15 --data-dir ./data task finish --json
lab15 --data-dir ./data task status
```

`memory conflicts` печатает время, операцию, ключ, «было → стало», причину и
источник. `memory policies` печатает правила и распределение активных фактов
по правилам. `task finish` выполняет ту же очистку, что клавиша `c` на
вкладках `8`/`2`: WM очищается, task-scoped LTM удаляется, проект остаётся.

## 7. TUI: вкладка 10

```text
┌ 10 Обновления · правила LTM и разрешение конфликтов ─────────────┐
│ Активных фактов: replace 5 · append 2 · task-scoped 0             │
│ Срок, статус, версия, подход, требование — одно актуальное значение│
│ Журнал замен: 4 · устаревших значений скрыто из промпта: 4        │
│ Последняя очистка после завершения задачи: 2026-11-23 04:00       │
├ Журнал обновлений · Enter карточка · g/G начало/конец ────────────┤
│ 2026-11-23 03:40:12 · заменено · project.deadline                 │
│     15 декабря 2026 → 20 декабря 2026                             │
│     конфликт: более поздняя запись заменила старую                │
└ Что было и что стало ─────────────────────────────────────────────┘
```

Клавиша `0` открывает экран. `↑↓` — выбор, `Enter` — карточка записи с
текущим активным значением, `g/G` — начало/конец журнала.

## 8. Проверка

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
cargo test
cargo test --test lifecycle dialogue_updates -- --nocapture
```

Интеграционный сценарий `tests/lifecycle.rs`:

1. четыре исходных факта приходят репликами и остаются в STM;
2. по очереди меняются срок, требование, подход и версия;
3. после каждого изменения задаётся вопрос об актуальном значении;
4. проверяются: активная запись, журнал `replace` с `old/new`, отсутствие
   старого значения в 8 промптах и ответах;
5. отдельно проверяются append-список и очистка WM + task-scoped LTM.

Фактический прогон:

```text
lifecycle check: changes=4 prompts_checked=8 stale_prompts=0 stale_answers=0
```

Отчёт: [`results/memory-conflicts-2026-11-23.md`](results/memory-conflicts-2026-11-23.md).

## 9. Ограничения

- Канонизация ключей охватывает типовые синонимы; произвольные формулировки
  одного факта не мержатся без явного ключа.
- Маскирование устаревших значений лексическое и не трогает текущую реплику.
- Append-список ограничен общим `max_records` и ручной очисткой.
- Хранится активная запись + 2 superseded; более старые версии удаляются,
  но журнал замен остаётся.
- Семантическое противоречие разных ключей («сервер в облаке» и «только
  локально») требует одинакового канонического ключа или явного правила.
