# 09. План работ для агентов

## 1. Правила исполнения

1. **Сначала прочитать** `00` → `09`. Не начинать код до T1.
2. **Контракты неприкосновенны.** Типы и сигнатуры из `02`/`03` фиксируются
   в T1; менять — только с правкой спецификации и `11-traceability.md`.
3. **Не трогать `10/`.** День 11 — отдельный Cargo-проект.
4. **Одна задача — один коммит.** После каждой:
   `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.
5. **Тесты вместе с кодом.** Не откладывать на T12.
6. **Никаких фейковых результатов.** Отчёт — реальный прогон или явная
   пометка `mock`.
7. **Русский UI**, код и комментарии — по стилю проекта.
8. **Не расширять scope.** Сервер, БД, embeddings, веб — вне задачи.
9. **Сомнение → дефолт из docs.** Иначе минимальный вариант + запись в
   `10-risks-adr.md`.
10. **Никаких `unwrap()`/`expect()` в production-коде** без доказанной
    невозможности ошибки; ошибки — `Result` + `thiserror`.
11. **TUI-поток не блокировать** сетевыми вызовами: только job thread.
12. **Ключ API** не логировать и не сохранять в слои/настройки.

## 2. Граф зависимостей

```text
T0 ──► T1 ──► T2 ──► T3 ──► T4 ──► T5 ──► T6 ──► T9 ──► T11 ──► T13 ──► T14
                │           │           │      ├──► T10
                │           │           │      └──► T12
                │           └──► T7     └──► T8
                └──────────────────────────────► T12
```

Потоки после T1:

- **Core:** T2 → T3 → T4 → T5 → T6.
- **Data:** T7 (после T2), T8 (после T3/T5).
- **UI:** T9, T10, T11 (после T6/T8).
- **QA/Docs:** T12 (с T4), T13 (после T11).

| Роль | Задачи |
|---|---|
| Core / память | T1–T6 |
| Data / эксперимент | T7, T8 |
| TUI | T9, T10, T11 |
| QA | T12 + тесты в каждой задаче |
| Docs / release | T13, T14 |

## 3. Задачи

### T0. Cargo-проект и каркас

**Зависимости:** нет. **Размер:** S.
**FR:** FR-36, FR-37.

Шаги:

1. `cargo init --name lab11` в `11/`; `edition = "2024"`, `rust-version = "1.85"`.
2. `Cargo.toml` с зависимостями из `03`, dev-deps.
3. `rust-toolchain.toml` (stable, rustfmt, clippy).
4. `.gitignore`: `target/`, `data/`, `*.log`, `.lab11/`.
5. `git mv`/удалить старые Vue/TS остатки, если они есть; в `11/` не должно
   быть `package.json`, `src/*.vue`, `node_modules`.
6. `fixtures/day10-facts.json` — sample facts Дня 10 (15 записей из `07`).
7. Корневой `README.md`: строка Дня 11 (Rust, TUI,
   `cd 11 && cargo run -- --data-dir ./data tui`).
8. Корневой `package.json`: опционально скрипты `dev:11`/`test:11` через
   `cd 11 && cargo …`; workspace `11` не должен ломать npm.
9. `src/main.rs` — clap-каркас: `tui`, `chat`, `memory`, `import-facts`,
   `experiment`, `dump`; команды возвращают `unimplemented`, но `--help`
   работает.
10. `cargo build`, `cargo test` (хотя бы smoke).

**DoD:** проект собирается; `cargo run -- --help`; root README обновлён;
`git status` не содержит `target/`.

---

### T1. Доменные типы, настройки, токены

**Зависимости:** T0. **Размер:** M.
**FR:** FR-1…FR-3, FR-24, FR-30; NFR-6.

Шаги:

1. `domain/memory.rs` — все типы из `02` с serde.
2. `domain/settings.rs` — `Settings`, `MemoryConfig`, defaults,
   `load_or_default`.
3. `domain/chat.rs` — `ChatMessage`, `TurnOutcome`, `TokenStats`.
4. `memory/tokens.rs` — эвристика Дня 10 (кириллица/латиница/пунктуация/
   пробелы/wide), тесты на границы.
5. `memory/audit.rs` — `mask_secret`, `sanitize_value`, события.
6. Тесты: serde round-trip, defaults, sanitizer, маскирование.

**DoD:** типы компилируются; serde JSON совпадает со схемой `02`;
`cargo test` зелёный.

---

### T2. Файловое хранилище

**Зависимости:** T1. **Размер:** M.
**FR:** FR-1…FR-9; NFR-4, NFR-9.

Шаги:

1. `storage/paths.rs` — data-dir (CLI → `LAB11_HOME` → XDG → home), создание
   каталогов.
2. `storage/atomic.rs` — tmp + `sync_all` + rename, `.bak`, права 0600.
3. `storage/stm.rs`, `working.rs`, `ltm.rs`, `pending.rs`, `meta.rs`,
   `settings.rs`.
4. `storage/lock.rs` — `fs2::try_lock_exclusive`, guard.
5. `load_bundle`/`save_bundle`; `LoadError::{NotFound,Corrupt,UnsupportedVersion,Io}`.
6. Битый JSON не перезаписывается; `.corrupt-<ts>` — только по команде reset.
7. Тесты T-S-1…9.

**DoD:** каждый слой в своём файле; atomic; lock; тесты.

---

### T3. Логика слоёв

**Зависимости:** T2. **Размер:** L.
**FR:** FR-1…FR-3, FR-15, FR-24…FR-28.

Шаги:

1. `memory/stm.rs` — `select_stm_window`, границы, tool-пары, summary.
2. `memory/working.rs` — upsert/status/remove/archive/prune/render.
3. `memory/ltm.rs` — active/find/upsert/replace/remove/pin/render.
4. Правила усечения и приоритеты из `05`.
5. Тесты T-STM/WM/LTM.

**DoD:** рендер возвращает `RenderedBlock { text, tokens, included, deduped }`;
unit-тесты зелёные.

---

### T4. Кандидаты, router, pending, аудит

**Зависимости:** T3. **Размер:** L.
**FR:** FR-10…FR-17; NFR-1.

Шаги:

1. `memory/candidates.rs` — локальная эвристика Дня 10 + маркеры задачи/scope.
2. `memory/extractor.rs` — LLM-извлечение, JSON-парсинг, fallback.
3. `memory/router.rs` — R1–R6 строго по порядку; Confirm/Auto; pending;
   audit; stats.
4. `memory/tool_write.rs` — валидация `memory_write`.
5. Тесты T-R-1…12.

**DoD:** таблица примеров `04` даёт ожидаемый слой; секреты отклоняются;
pending сохраняется.

---

### T5. Retrieval и сборка промпта

**Зависимости:** T3, T4. **Размер:** L.
**FR:** FR-24…FR-32.

Шаги:

1. `memory/retrieval.rs` — нормализация, стоп-слова, score, top-K, бюджет,
   `dropped` с причинами.
2. `memory/prompt.rs` — `assemble_prompt`, шаблоны, порядок, усечение, dedupe,
   breakdown, notes.
3. `llm/dto.rs` — `ChatRequest`/`ChatResponse`, tools.
4. System prompt — текст из `05`, раздел 3.
5. Golden-тесты T-P-1…13, T-RET-1…8.

**DoD:** промпт структурно совпадает с `05`; breakdown сходится; тесты.

---

### T6. LLM-клиент, Agent, Controller, CLI chat

**Зависимости:** T4, T5. **Размер:** L.
**FR:** FR-13, FR-18, FR-30, FR-31, FR-33.

Шаги:

1. `llm/client.rs` — `ChatClient` trait, `ReqwestClient` (blocking, rustls),
   таймауты, ретрай 429/5xx (1 раз).
2. `llm/tools.rs` — calculator/time/currency + схемы.
3. `llm/agent.rs` — tool-loop, `memory_write` до registry, token stats, cost.
4. `app/controller.rs` — `AppRuntime`, `AppController`, load/save bundle.
5. `cli` — `chat`, `dump`; `--json`.
6. Fake-клиент для тестов; T-A-1…10.
7. Проверить: Local-режим не делает доп. запросов.

**DoD:** CLI `chat` отвечает и сохраняет слои; tool-loop работает; тесты.

---

### T7. Импорт facts Дня 10

**Зависимости:** T2, T4. **Размер:** M.
**FR:** FR-19…FR-23.

Шаги:

1. `memory/migration.rs` — парсинг `lab10.facts.v1`/массива, маппинг категорий,
   upsert, отчёт.
2. `storage/migration.rs` — `migration/report.json`.
3. CLI `import-facts`, TUI-хук (кнопка — T9).
4. Sample `fixtures/day10-facts.json` (15 записей).
5. Тесты T-M-1…7.

**DoD:** 12 деталей Дня 10 в LTM; повтор не дублирует; битый вход — код 2.

---

### T8. Фикстура, probe, движок эксперимента

**Зависимости:** T3, T5. **Размер:** L.
**FR:** EXP-1…EXP-8.

Шаги:

1. `experiment/fixtures.rs` — 34 сообщения из `07`, seeded bundle.
2. `experiment/probes.rs` — 18 probe, `probe_matches`, quick-набор.
3. `experiment/runner.rs` — 7 конфигураций, `LayerFlags`, full, очистка на
   копиях, прогресс/отмена, fake-friendly.
4. `experiment/report.rs` — Markdown/JSON, дельты, пороги.
5. CLI `experiment --quick/--full --out --json --check`.
6. Тесты T-F-1…7, T-E-1…9.

**DoD:** fake-прогон даёт 18/18 и 12/18 после очистки; `--check` работает.

---

### T9. TUI: каркас, «Память», «Подтверждения»

**Зависимости:** T6, T7. **Размер:** L.
**FR:** UX-1…UX-4, UX-7, UX-8; FR-14.

Шаги:

1. `tui/app.rs` — terminal init/restore, panic hook, event loop (50 ms),
   `AppState`, `mpsc` событий, job thread.
2. `tui/keymap.rs` — `1..8`, Tab, arrows, Enter, Esc, `?`, `q`.
3. `tui/widgets/memory.rs` — 3 колонки, фокус, прокрутка, счётчики/токены.
4. `tui/widgets/pending.rs` — список + diff + y/n/e/l.
5. `tui/widgets/modal.rs` — confirm, edit, error, help.
6. `tui/widgets/input.rs` — однострочный редактор.
7. Тесты T-UI-1…3, T-UI-6…7.

**DoD:** TUI запускается, импорт и принятие pending работают, 80×24 ок.

---

### T10. TUI: настройки, токены, история в чате, API

**Зависимости:** T6, T9. **Размер:** M.
**FR:** UX-5, UX-6; FR-31, FR-34, FR-35.

Шаги:

1. `settings.rs` — форма, валидация, `s` сохранить; ключ API только env.
2. `tokens.rs` — breakdown, таблица сессии, стоимость.
3. история STM — в экране чата: экспорт/импорт, предупреждение о cursor.
4. `api_log.rs` — таблица chat/memory, фильтр, детали.
5. Ветки удалены из проекта (по требованию заказчика).
6. Тесты T-UI-4…5.

**DoD:** все экраны читаемы, resize, данные сохраняются.

---

### T11. TUI: экран «Проверка»

**Зависимости:** T8, T9. **Размер:** M.
**FR:** EXP-3…EXP-8.

Шаги:

1. Конфигурации, quick/full, чекбокс очистки.
2. Прогресс через `AppEvent::ExperimentProgress`, отмена.
3. Таблицы ablation, вклад слоёв, матрица очистки, неотвеченные.
4. `s` — сохранить MD/JSON по путям.
5. Тесты T-E + T-UI-5.

**DoD:** полный прогон на fake/реальном API, отчёт сохраняется, отмена чистая.

---

### T12. Тесты: полнота и регресс

**Зависимости:** T4, T5, T6, T8. **Размер:** M.
**FR:** NFR-5, FR-33.

Шаги:

1. Свести все T-* из `08`; отметить реализованные.
2. Edge-cases: пустые слои, переполнение, битые файлы, секреты, pending.
3. CLI-тесты `assert_cmd`: коды 0/2/3/4.
4. `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`.
5. Проверить отсутствие сети в тестах.

**DoD:** все тесты реализованы/помечены; clippy чистый; нет флаков.

---

### T13. Документация и трассируемость

**Зависимости:** T11, T12. **Размер:** M.

Шаги:

1. Переписать `11/README.md` под пользователя: запуск, клавиши, сценарий,
   результаты, ограничения, выводы.
2. Заполнить `docs/results/experiment-YYYY-MM-DD.md` (+ `.json`).
3. Обновить `docs/11-traceability.md`.
4. Отметить отклонения в `10-risks-adr.md`.
5. Проверить ссылки.

**DoD:** README понятен без SDD; отчёт заполнен (или явно mock); трассировка
полная.

---

### T14. Финальная приёмка

**Зависимости:** все. **Размер:** S.

- [ ] `cargo build` — успех.
- [ ] `cargo test` — успех, без сети.
- [ ] `cargo clippy --all-targets -- -D warnings` — чисто.
- [ ] `cargo fmt --check` — чисто.
- [ ] TUI в 80×24 и 120×36.
- [ ] Ручной чек-лист `08`, раздел 7.
- [ ] Отчёт эксперимента приложен.
- [ ] В `10/` ничего не сломано.

## 4. Definition of Done задачи

1. Код реализует пункты задачи и не нарушает контракты.
2. Тесты добавлены/обновлены, зелёные.
3. `cargo build` + `cargo clippy` + `cargo fmt --check` проходят.
4. Обновлён `11-traceability.md`.
5. Нет `TODO`, `unwrap()` в prod без причины, логов с секретами.
6. UI-задача проверена вручную.
7. Коммит содержит только эту задачу.

## 5. Порядок сдачи и минимальный срез

```text
T0 → T1 → T2 → T3 → T4 → T5 → T6
   → (T9 ∥ T10 ∥ T8) → T11 → T12 → T13 → T14
```

Минимальный срез:

1. T0–T6 + T8 (core + эксперимент на fake).
2. T9 + T11 (память и проверка в TUI).
3. T13 (README + отчёт).

T7 (импорт facts) и T10 — следующие по важности. T12 частично закрывается
тестами внутри задач.

## 6. Стартовый промпт для агента

```text
Ты реализуешь День 11 в /11 (Rust + ratatui + clap), база — только facts /10.

1. Прочитай 11/README.md и docs/00..09.
2. Возьми первую незакрытую задачу из docs/09-tasks.md (начни с T0/T1).
3. Не меняй контракты из docs/02 и docs/03 без правки спецификации.
4. Пиши тесты вместе с кодом по docs/08-test-plan.md.
5. После задачи: cargo fmt --check && cargo clippy --all-targets -- -D warnings
   && cargo test.
6. Обнови docs/11-traceability.md.
7. Не трогай /10, не добавляй сервер, БД, embeddings, веб.
8. Не блокируй TUI-поток сетью; используй job thread.
9. Не сохраняй API-ключ нигде, кроме env/0600 secrets.
10. В конце отчитайся: FR закрыты, тесты, что осталось.
```

## 7. Приёмка по задачам

| Задача | Артефакт | Проверка |
|---|---|---|
| T0 | Cargo-проект | build/test/help |
| T1 | `domain/*`, `memory/tokens.rs` | serde, тесты |
| T2 | `storage/*` | T-S-* |
| T3 | `memory/{stm,working,ltm}.rs` | T-STM/WM/LTM |
| T4 | `memory/router.rs` | T-R-* |
| T5 | `memory/{retrieval,prompt}.rs` | T-RET/P-* |
| T6 | `llm/*`, `app/controller.rs` | T-A-* |
| T7 | `memory/migration.rs` | T-M-* |
| T8 | `experiment/*` | T-F/E-* |
| T9 | `tui/app.rs`, memory/pending | T-UI-*, ручной |
| T10 | settings/tokens/history-in-chat/api | T-UI, ручной |
| T11 | `tui/widgets/experiment.rs` | прогон + отчёт |
| T12 | тесты | fmt/clippy/test |
| T13 | README/отчёт/трассировка | ревью |
| T14 | приёмка | чек-лист `08` |
