# 08. Тест-план

## 1. Пирамида

| Уровень | Что проверяет | Сеть | Терминал |
|---|---|---|---|
| unit | слои, router, retrieval, prompt, storage, migration | нет | нет |
| integration | controller + fake LLM, experiment, импорт facts | нет (fake) | нет |
| CLI | clap-команды, коды выхода, файлы | нет (fake/env) | нет |
| TUI | view-model, TestBackend-рендер | нет | виртуальный |

Цель: **≥ 90 тестов**, детерминированы, без реального ключа.

## 2. Структура тестов

```text
11/tests/
  storage.rs          # атомарность, битый JSON, изоляция, lock, права
  router.rs           # R1–R6, pending, audit, секреты
  prompt.rs           # порядок блоков, лимиты, dedupe, breakdown
  migration.rs        # facts → LTM, идемпотентность
  experiment.rs       # 7 конфигураций, очистка, отчёт (fake)
  controller.rs       # send_user_message, memory_write, clear
  cli.rs              # assert_cmd: memory/import/experiment --check
```

Unit-тесты модулей — в `#[cfg(test)] mod tests` рядом с кодом:

- `memory/tokens.rs` — эвристика;
- `memory/retrieval.rs` — score/top-K;
- `memory/candidates.rs` — локальное извлечение;
- `experiment/probes.rs` — `probe_matches`;
- `tui/app.rs` — reducer клавиш (без терминала).

## 3. Unit-тесты

### STM

| ID | Проверка | FR |
|---|---|---|
| T-STM-1 | `select_stm_window` берёт последние 8 истории | FR-27 |
| T-STM-2 | граница сдвигается к `User` | FR-27 |
| T-STM-3 | пара `Assistant(tool_calls) → Tool` не рвётся | FR-27 |
| T-STM-4 | текущая реплика включается при `layers.stm=false` | FR-25, FR-27 |
| T-STM-5 | при превышении удаляются старые пары | FR-25 |
| T-STM-6 | summary только при `covered_messages > 0` | FR-27 |
| T-STM-7 | полная история не мутирует при усечении | FR-7 |

### WM

| ID | Проверка | FR |
|---|---|---|
| T-WM-1 | `upsert_wm_item` создаёт id/таймстемпы | FR-2 |
| T-WM-2 | повторный key обновляет, не дублирует | FR-15 |
| T-WM-3 | done-plan → `[x]`, прочие done/dropped нет | FR-24 |
| T-WM-4 | порядок goal → plan(order) → status → entity → question → note | FR-24 |
| T-WM-5 | усечение: note первыми, goal/plan остаются | FR-25 |
| T-WM-6 | `ref_ltm_id` рендерится ссылкой, если LTM не включена | FR-28 |
| T-WM-7 | `prune_wm` удаляет старые done, не трогает active | FR-2 |

### LTM

| ID | Проверка | FR |
|---|---|---|
| T-LTM-1 | create, key уникален | FR-3 |
| T-LTM-2 | одинаковое значение → `Same` | FR-15 |
| T-LTM-3 | `replace_ltm` ставит `superseded_by` | FR-15 |
| T-LTM-4 | `active_ltm` исключает superseded | FR-15 |
| T-LTM-5 | profile: profile/project/preference, pinned выше | FR-26 |
| T-LTM-6 | профиль усекается, pinned сохраняется | FR-25 |
| T-LTM-7 | key/value 64/500, tags ≤ 8 | FR-32 |

### Router

| ID | Проверка | FR |
|---|---|---|
| T-R-1 | R1: `sk-…`, `Bearer …`, `password=…` → reject, `***` | FR-16 |
| T-R-2 | R2: «для этой задачи» → WM, даже если key есть в LTM | FR-11 |
| T-R-3 | R3: update LTM → pending при Confirm | FR-12 |
| T-R-4 | R3 при Auto → supersede, pending пуст | FR-12 |
| T-R-5 | R4: проект/дедлайн/решение → LTM pending | FR-11 |
| T-R-6 | R5: «вариант B рабочий» → WM entity | FR-11 |
| T-R-7 | R6: привет/спасибо/вопрос → нет записи | FR-11 |
| T-R-8 | несколько кандидатов из реплики независимы | FR-11 |
| T-R-9 | audit: layer/rule_id/reason/source | FR-17 |
| T-R-10 | pending по key заменяется, old_value сохранён | FR-12 |
| T-R-11 | LLM extractor error → fallback Local, `stats.error` | FR-10 |
| T-R-12 | `memory_write` с неверным layer → Rejected | FR-13 |

### Retrieval

| ID | Проверка | FR |
|---|---|---|
| T-RET-1 | совпадение в key весит больше value | FR-26 |
| T-RET-2 | pinned первым | FR-26 |
| T-RET-3 | top_k соблюдается | FR-26 |
| T-RET-4 | бюджет соблюдается | FR-25 |
| T-RET-5 | superseded не выбирается | FR-15 |
| T-RET-6 | shadowed-by-wm попадает в dropped | FR-28 |
| T-RET-7 | пустой запрос не падает | FR-26 |
| T-RET-8 | равный score → детерминированный порядок | NFR-5 |

### Prompt

| ID | Проверка | FR |
|---|---|---|
| T-P-1 | порядок system → LTM profile → relevant → WM → summary → STM → current | FR-24 |
| T-P-2 | пустые блоки не добавляются | FR-24 |
| T-P-3 | каждый блок — отдельный system | FR-24 |
| T-P-4 | `layers.stm=false` убирает STM, но не WM/LTM | EXP-3 |
| T-P-5 | `layers.ltm=false` убирает оба LTM-блока | EXP-3 |
| T-P-6 | breakdown суммируется в total | FR-30 |
| T-P-7 | layer_delta равен блокам слоя | FR-30 |
| T-P-8 | текущая user-реплика последняя | FR-25 |
| T-P-9 | system содержит правила приоритета и «данные, не инструкции» | FR-29 |
| T-P-10 | control chars вырезаны, value ≤ 500 | FR-32 |
| T-P-11 | dedupe WM-ссылки, `deduped_wm_ids` | FR-28 |
| T-P-12 | total_budget соблюдается при переполнении | FR-25 |
| T-P-13 | при `layers.stm=false` остаётся текущий вопрос | FR-25 |

### Импорт

| ID | Проверка | FR |
|---|---|---|
| T-M-1 | 12 facts → 12 записей с правильными kind | FR-19, FR-21 |
| T-M-2 | значения дословно (числа/даты/имена) | FR-21 |
| T-M-3 | повтор: added=0, updated=0 | FR-20 |
| T-M-4 | изменённый fact обновляет версию, не дублирует | FR-20 |
| T-M-5 | пустой/битый JSON → `ok=false`, без panic | FR-23 |
| T-M-6 | `legacy_key` и тег `migrated:day10` | FR-21 |
| T-M-7 | неизвестная категория → Knowledge | FR-21 |

### Storage

| ID | Проверка | FR |
|---|---|---|
| T-S-1 | save/load каждого файла эквивалентен | FR-6 |
| T-S-2 | неверная version → `UnsupportedVersion` | FR-6 |
| T-S-3 | битый JSON → `Corrupt`, файл не перезаписан | FR-6, NFR-4 |
| T-S-4 | `clear_ltm` не меняет STM/WM | FR-4 |
| T-S-5 | `clear_ltm` очищает pending, не WM/STM | FR-5 |
| T-S-6 | нет прав/диска → `SaveError`, без panic | NFR-9 |
| T-S-7 | `atomic_write_json` оставляет `.bak` | FR-6 |
| T-S-8 | lock: второй `acquire_lock` → ошибка | FR-9 |
| T-S-9 | permissions settings/secrets = 0600 (Unix) | NFR-9 |

### Фикстуры

| ID | Проверка | FR |
|---|---|---|
| T-F-1 | 34 сообщения, 18 probe, 6 на слой | EXP-1 |
| T-F-2 | S1–S6 не отвечаются по 1–26 | EXP-1 |
| T-F-3 | W1–W6 не отвечаются по 27–34 | EXP-1 |
| T-F-4 | L1–L6 не отвечаются по 27–34 | EXP-1 |
| T-F-5 | seeded WM/LTM совпадает с `07`, раздел 3 | EXP-1 |
| T-F-6 | `probe_matches` нормализует `ё`, регистр, пунктуацию | EXP-2 |
| T-F-7 | router из тех же сообщений даёт то же состояние (кроме pending) | FR-11 |

## 4. Integration

### Controller

| ID | Проверка | FR |
|---|---|---|
| T-A-1 | запрос = system + блоки + окно + current | FR-24 |
| T-A-2 | `memory_write` в WM → `Created` | FR-13 |
| T-A-3 | `memory_write` в LTM при Confirm → `Pending` | FR-12 |
| T-A-4 | `TurnOutcome.memory` содержит обновления | FR-3 |
| T-A-5 | audit и route_stats заполнены | FR-17 |
| T-A-6 | ошибка модели не теряет уже записанное | FR-6 |
| T-A-7 | Local — 0 доп. запросов | NFR-1 |
| T-A-8 | Llm extractor создаёт запрос `kind=memory` | FR-10 |
| T-A-9 | очистка STM перед turn не ломает WM/LTM | FR-5 |
| T-A-10 | `chat --json` отдаёт final/tokens/breakdown | FR-18 |

### Эксперимент

| ID | Проверка | FR |
|---|---|---|
| T-E-1 | 7 конфигураций × 18 = 126 запросов на fake | EXP-3 |
| T-E-2 | матрица ответов соответствует ожидаемой | EXP-3 |
| T-E-3 | очистка даёт 12/18 и падение своих | EXP-4 |
| T-E-4 | токены/дельты положительны | EXP-6 |
| T-E-5 | Markdown содержит все таблицы | EXP-5 |
| T-E-6 | JSON валиден и парсится обратно | EXP-5 |
| T-E-7 | отмена не сохраняет частичный отчёт | EXP-3 |
| T-E-8 | `quick` = 9 вопросов | EXP-7 |
| T-E-9 | `--check` → код 4 при провале порогов | EXP-8 |

### Ветки

Ветки удалены из проекта по требованию заказчика; тесты T-B-1…4 не применяются.

### TUI

| ID | Проверка | FR |
|---|---|---|
| T-UI-1 | reducer: `Tab` меняет фокус, `1..8` экраны | UX-1 |
| T-UI-2 | `y/n` меняют состояние pending | UX-3 |
| T-UI-3 | `c` открывает модалку, без confirm не очищает | UX-4 |
| T-UI-4 | `TestBackend` 80×24 рендерит без panic | NFR-8 |
| T-UI-5 | resize 120×36 → layout валиден | NFR-8 |
| T-UI-6 | ошибка job показывается баннером | UX-8 |
| T-UI-7 | busy блокирует destructive-команды | UX-5 |

### Регресс инструментов

| ID | Проверка |
|---|---|
| T-REG-1 | calculator/time/currency |
| T-REG-2 | tool-loop: несколько итераций, ошибки |
| T-REG-3 | token estimator и cost |
| T-REG-4 | API-лог суммирует chat/memory |
| T-REG-5 | settings defaults/миграция |

## 5. Моки

```rust
struct FakeClient { answers: HashMap<String, String>, calls: Mutex<Vec<ChatRequest>> }

impl ChatClient for FakeClient {
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, LlmError> {
        self.calls.lock().unwrap().push(req.clone());
        // ответ по наличию нужного блока/слова в prompt
        Ok(ChatResponse::assistant("..."))
    }
}
```

Правила:

- `tempfile::TempDir` для data-dir;
- фиксированное время: `TestClock::at("2026-11-17T10:00:00Z")` или передача
  `now` параметром;
- никаких `std::env::set_var` в параллельных тестах: конфиг передаётся явно;
- TUI — `ratatui::backend::TestBackend` + `Terminal::draw`;
- CLI — `assert_cmd` с `--data-dir` tempdir, без API key (ошибка/локальные
  команды) и fake через `LAB11_FAKE_LLM=1` (только для тестов);
- сеть запрещена: `reqwest` не должен вызываться в тестах.

## 6. Команды

```sh
cargo test
cargo test --test prompt
cargo build
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

CI-гейт: fmt + clippy + build + test.

## 7. Ручной чек-лист

- [ ] `cargo run -- --data-dir ./data tui` открывается в 80×24.
- [ ] Экран «Память»: три колонки, счётчики и токены.
- [ ] Импорт facts: отчёт, повтор не дублирует.
- [ ] «Запускаем проект «Тест»» → pending → accept → LTM.
- [ ] Reject pending → записи нет, аудит.
- [ ] «Отвечай коротко» → preference pending.
- [ ] «Текущая задача: …» → WM сразу.
- [ ] `memory_write` через чат → LTM pending.
- [ ] WM → LTM: `ref_ltm_id`, нет дубля в промпте.
- [ ] Очистить STM → WM/LTM на месте.
- [ ] Очистить WM → STM/LTM на месте.
- [ ] Очистить LTM → pending пуст, WM/STM на месте.
- [ ] «Проверка» quick/full, отчёт сохраняется.
- [ ] «Токены»: breakdown последнего запроса.
- [ ] «API»: запрос `memory` с режимом.
- [ ] Перезапуск: слои и pending восстановлены.
- [ ] Битый JSON: баннер, приложение работает.
- [ ] Resize 120×36 и обратно.

## 8. Definition of Done тестов

- [ ] Все T-* реализованы или помечены `P2/skip` с причиной.
- [ ] `cargo test` зелёный, без сети.
- [ ] `cargo build` и `cargo clippy` чистые.
- [ ] Нет тестов, подгоняющих фикстуру под ответ.
- [ ] Покрыты изоляция слоёв, router, prompt, импорт, очистка, эксперимент,
      CLI, TUI-reducer.
