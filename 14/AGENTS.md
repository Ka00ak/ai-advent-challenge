# AGENTS.md — реализация Дня 14

Ты работаешь над **Днём 14: инварианты и ограничения состояния** на
**Rust + TUI** по SDD-спецификации в `docs/`. Источник истины — `docs/`,
особенно `docs/14-invariants.md`.

## 1. Что уже есть

- `docs/00`…`13` — память (11), профиль (12), автомат задачи (13);
- `src/invariants/engine.rs` — кодовая проверка запросов/ответов;
- `src/domain/invariant.rs` — модель, стартовый набор, история изменений;
- `data/invariants/invariants.json`, `data/invariants/audit.json`;
- вкладка `9` TUI — список, изменение, история, аудит, probe;
- CLI `invariants list|show|set|history|audit|check|probe`.

## 2. Жёсткие ограничения

- Инварианты хранятся отдельно от диалога и слоёв памяти.
- Нарушение проверяется **кодом**, а не только промптом:
  - запрос — до вызова LLM;
  - ответ — до возврата пользователю;
  - этапы задачи — до записи артефакта.
- Инвариант меняется только явной командой (`invariants set` или вкладка `9`)
  с непустой причиной. Из диалога — никогда.
- Изменение фиксирует `old_rule`/`new_rule`, старые/новые признаки, причину,
  источник и время.
- Нарушающий ответ модели заменяется детерминированным отказом.
- Секреты не попадают в STM, аудит, логи, API-журнал и промпт.
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

## 4. Полезные команды

```sh
cargo run -- --data-dir ./data tui
cargo run -- --data-dir ./data invariants list
cargo run -- --data-dir ./data invariants probe
cargo run -- --data-dir ./data invariants check "Напиши на Python"
cargo run -- --data-dir ./data invariants set stack.rust-only \
  --rule "..." --reason "причина"
LAB14_FAKE_LLM=1 cargo run -- --data-dir ./data experiment --quick --check
LAB14_FAKE_LLM=1 cargo run -- --data-dir ./data personalize --check
```

## 5. Перед изменениями

1. Прочитай `docs/14-invariants.md` и `docs/14-traceability.md`.
2. Проверь, что новое поведение не обходит кодовую проверку.
3. Добавь unit/integration-тест вместе с изменением.
4. Обнови `docs/14-traceability.md` и при необходимости отчёт в
   `docs/results/`.
