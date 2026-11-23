# AGENTS.md — реализация Дня 15

Ты работаешь над **Днём 15: устаревание и конфликты памяти** на
**Rust + TUI** по SDD-спецификации в `docs/`. Источник истины — `docs/`,
особенно `docs/15-memory-lifecycle.md`.

## 1. Что уже есть

- `docs/00`…`13` — память (11), профиль (12), автомат задачи (13);
- `docs/14` — инварианты и кодовая проверка запросов/ответов;
- `src/memory/lifecycle.rs` — правила `replace / append / task-scoped`,
  канонические ключи, журнал «было → стало», маскирование устаревших
  значений, очистка после задачи;
- `src/memory/ltm.rs` — `recorded_at`, `revision`, `superseded_by`,
  `LtmState.revisions`;
- `src/memory/router.rs` — R3 разрешает конфликт последней записью, R7
  пополняет append-списки;
- вкладка `10 Обновления` TUI — журнал замен, правила, очистка;
- CLI `memory conflicts`, `memory policies`.

## 2. Жёсткие ограничения

- У replace-ключа одно активное значение. Противоречащий факт заменяет
  старое, а не добавляется рядом.
- Приоритет у более позднего `recorded_at`; более ранняя запись отклоняется.
- Замена сохраняет `old_value`, `new_value`, ID, время, источник и причину
  в `LtmState.revisions`.
- Старые значения не попадают в промпт: LTM рендерит только активные записи,
  история STM маскируется маркером `«устарело»` (кроме текущей реплики).
- `append` не вытесняет элементы; `task-scoped` удаляется после завершения
  задачи. WM при завершении очищается полностью.
- Инварианты, секреты, профиль и task-scoped очистка остаются в силе.
- UI и сообщения — на русском. `unwrap()`/`expect()` в production-коде — только
  с очевидной причиной.
- Тесты не ходят в сеть и не требуют реального терминала.

## 3. Гейт

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
cargo test
```

Отчёт сценария: `cargo test --test lifecycle dialogue_updates -- --nocapture`.

## 4. Полезные команды

```sh
cargo run -- --data-dir ./data tui
cargo run -- --data-dir ./data chat "Какой сейчас срок проекта?"
cargo run -- --data-dir ./data memory conflicts
cargo run -- --data-dir ./data memory policies
cargo run -- --data-dir ./data invariants probe
LAB15_FAKE_LLM=1 cargo run -- --data-dir ./data experiment --quick --check
LAB15_FAKE_LLM=1 cargo run -- --data-dir ./data personalize --check
```

## 5. Перед изменениями

1. Прочитай `docs/15-memory-lifecycle.md` и `docs/15-traceability.md`.
2. Проверь, что новое поведение не оставляет старое значение в промпте.
3. Добавь unit/integration-тест вместе с изменением.
4. Обнови `docs/15-traceability.md` и при необходимости отчёт в
   `docs/results/`.
