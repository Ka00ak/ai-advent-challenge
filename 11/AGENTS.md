# AGENTS.md — реализация Дня 11

Ты реализуешь **День 11: модель памяти ассистента** на **Rust + TUI** по
SDD-спецификации в `docs/`. Этот файл — краткий протокол запуска; источник
истины — `docs/`.

## 1. Перед началом

1. Прочитай по порядку:
   `docs/00-brief.md`, `01-requirements.md`, `02-domain-model.md`,
   `03-architecture.md`, `04-memory-policy.md`, `05-prompt-assembly.md`,
   `07-verification.md`, `09-tasks.md`, `10-risks-adr.md`.
2. Проверь окружение:

   ```sh
   rustc --version   # нужен Rust stable, edition 2024 (1.85+)
   cargo --version
   ```

   Если Rust не установлен:

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   source "$HOME/.cargo/env"
   rustup component add rustfmt clippy
   ```

3. API-ключ нужен только для реального прогона:
   `export DEEPSEEK_API_KEY=...` (или `KIMI_API_KEY`). В файлы не сохранять.

## 2. Как выбрать задачу

1. Открой `docs/09-tasks.md` — там T0…T14 с зависимостями.
2. Открой `docs/11-traceability.md` — там статусы `spec/todo/done/p2`.
3. Возьми **первую незакрытую P0-задачу**, у которой выполнены зависимости.
4. Поставь ей `todo` в трассировке и работай только над ней.

Старт: **T0** (Cargo-проект и каркас), затем **T1** (контракты).

## 3. Цикл одной задачи

1. Прочитай карточку задачи в `09-tasks.md` и связанные FR.
2. Реализуй код и тесты **вместе**.
3. Прогони гейт:

   ```sh
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo build
   cargo test
   ```

4. Обнови `docs/11-traceability.md`: `done` + фактические тесты.
5. Если требование неоднозначно — возьми дефолт из `10-risks-adr.md`
   (раздел «Открытые вопросы») и зафиксируй решение.
6. Один коммит на задачу, без случайных файлов.

## 4. Жёсткие ограничения

- Не менять контракты из `02`/`03` без правки спецификации.
- Не трогать `../10/` (День 10 остаётся рабочим).
- Не добавлять сервер, БД, embeddings, веб-интерфейс.
- Не блокировать TUI-поток сетью: только job thread.
- Не сохранять API-ключ в settings/слои/логи; ключ — env или `0600`.
- Не использовать `unwrap()`/`expect()` в production-коде без причины.
- Не подгонять фикстуру под ожидаемые ответы.
- Тесты не должны ходить в сеть и требовать реальный терминал.
- UI и сообщения — на русском.

## 5. Контрольные точки

| После | Что проверяем |
|---|---|
| T0 | `cargo build`, `cargo run -- --help`, структура каталогов |
| T6 | CLI `chat`, tool-loop, запись/чтение слоёв |
| T7 | `import-facts` переносит 12 деталей Дня 10, повтор идемпотентен |
| T8 | fake-эксперимент: 18/18, после очистки 12/18, `--check` |
| T11 | TUI в 80×24, прогон и сохранение отчёта |
| T12 | `cargo test` + clippy + fmt чистые |
| T14 | чек-лист `08`, раздел 7; отчёт в `docs/results/` |

## 6. Параллельная работа (если несколько агентов)

Сначала один агент делает **T0 → T1** и фиксирует контракты. Затем:

| Поток | Задачи | Зона |
|---|---|---|
| Core | T2 → T3 → T4 → T5 → T6 | `src/domain`, `src/storage`, `src/memory`, `src/llm`, `src/app` |
| Data | T7, T8 | `src/memory/migration.rs`, `src/experiment` |
| TUI | T9, T10, T11 | `src/tui` |
| QA | T12 | `tests/` |
| Docs | T13 | `README.md`, `docs/results` |

TUI зависит от T6/T8; Data — от T2/T5. QA пишет тесты с T4. Перед merge —
общий гейт. Конфликты решает оркестратор; контракты меняет только он.

## 7. Команды

```sh
cargo run -- --data-dir ./data tui
cargo run -- --data-dir ./data chat "Привет"
cargo run -- --data-dir ./data import-facts fixtures/day10-facts.json
cargo run -- --data-dir ./data experiment --quick
cargo run -- --data-dir ./data experiment --full \
  --out docs/results/experiment-$(date +%F).md \
  --json docs/results/experiment-$(date +%F).json \
  --check
```

## 8. Итоговый отчёт агента

В конце каждой сессии сообщи:

- какие задачи/FR закрыты;
- какие тесты добавлены и их статус;
- что не сделано и почему;
- какие отклонения от спецификации зафиксированы.
