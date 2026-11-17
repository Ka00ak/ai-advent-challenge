# День 11 — модель памяти ассистента (Rust + TUI)

Агент с **тремя явными слоями памяти**: STM (диалог), WM (текущая задача) и
LTM (профиль, проект, решения, знания). Каждый слой живёт в своих файлах,
пишется по своим правилам и попадает в промпт отдельным блоком. Поверх —
router R1–R6, очередь подтверждений, импорт facts Дня 10, измеримый
ablation-эксперимент и TUI + CLI на одном контроллере.

**Статус:** реализовано, тесты и гейт зелёные.
**Стек:** Rust 2024 (1.85+), ratatui + crossterm, clap, serde, chrono,
reqwest (blocking, rustls), fs2.

## Быстрый старт

```sh
cd 11
cargo build

# Демо-каталог данных (в .gitignore)
cargo run -- --data-dir ./data tui

# Импорт facts Дня 10
cargo run -- --data-dir ./data import-facts fixtures/day10-facts.json

# Один запрос (нужен ключ)
export DEEPSEEK_API_KEY=...
cargo run -- --data-dir ./data chat "Привет"

# Эксперимент без сети (детерминированный fake-клиент)
LAB11_FAKE_LLM=1 cargo run -- --data-dir ./data experiment --quick --check
```

Ключ читается из `DEEPSEEK_API_KEY` или `KIMI_API_KEY` (либо
`--api-key-file` с правами `0600`). В настройки, слои, pending и аудит ключ
не попадает.

## Что умеет

| Возможность | Где |
|---|---|
| Три слоя с раздельными JSON-файлами | `stm/`, `wm/`, `ltm/` |
| Роутинг R1–R6: секрет / задача / обновление LTM / новый факт / WM / отказ | `src/memory/router.rs` |
| LTM через pending (`accept/reject/edit/change layer`) | TUI экран 2, CLI `memory pending` |
| Tool `memory_write` в tool-loop | `src/memory/tool_write.rs` |
| Импорт facts Дня 10, идемпотентный | `import-facts`, `src/memory/migration.rs` |
| Сборка промпта с бюджетами, top-K, дедупликацией | `src/memory/prompt.rs` |
| Breakdown токенов по слоям | TUI «Токены», `chat --json`, API-лог |
| 7 конфигураций + 3 очистки + 18 probe | TUI «Проверка», CLI `experiment` |
| Ветки — снимок STM+WM+LTM+pending+meta | TUI «Ветки» |
| Lock от второго экземпляра, атомарная запись, `.bak` | `src/storage/` |

## TUI

Запуск в терминале UTF-8 от **80×24**:

```text
cargo run -- --data-dir ./data tui
```

Экраны: `1` Память · `2` Подтверждения · `3` Проверка · `4` Токены ·
`5` История · `6` Настройки · `7` API · `8` Ветки.

Основные клавиши:

```text
1..8        переключение экранов
Tab         фокус между колонками/полями
↑ ↓ PgUp PgDn g G   навигация
Enter       карточка записи / редактирование / переключить
i           написать агенту
?           помощь, q / Ctrl+C — выход
```

«Память»: `a` добавить, `e` изменить, `d` удалить, `p` pin (LTM),
`m` перенести WM↔LTM, `c` очистить слой или завершить задачу,
`/` поиск по LTM. Все destructive-действия — через модалку подтверждения.

«Подтверждения»: `y` принять, `n` отклонить, `e` изменить, `l` сменить слой.
Показан diff «Было → Станет», правило (`R3`) и stale-бейдж после 7 дней.

«Проверка»: `r` запустить, `m` quick/full, `Space` включить очистку,
`s` сохранить MD/JSON, `d` детали probe, `i` импорт facts. Прогон идёт в
фоновом потоке, TUI не блокируется; `q` отменяет.

## CLI

```text
lab11 [--data-dir PATH] [--config PATH] [--api-key-file PATH] <команда>

tui
chat <TEXT> [--json] [--no-stream]
memory show [--layer stm|wm|ltm] [--json]
memory add|update --layer ... --key ... --value ... [--kind ...] [--pinned]
memory delete --layer ... --key ...
memory move --from wm|ltm --key ... --to ltm|wm
memory pin --key ... [--off]
memory pending list|accept <ID>|reject <ID>
memory clear --layer ... --yes
memory reset --layer ...        # убрать битый файл в *.corrupt-*
import-facts <PATH>
experiment [--quick|--full] [--cleanup] [--out MD] [--json JSON] [--check]
dump [--json]
import-settings <PATH>          # опционально, из Дня 10
```

Коды выхода: `0` успех, `2` ошибка ввода/конфигурации, `3` ошибка API,
`4` провал порогов `--check`.

## Модель памяти

| Слой | Файлы | Что попадает в промпт |
|---|---|---|
| **STM** | `stm/conversation.json`, `stm/summary.json` | последние `window` сообщений + текущая реплика + сводка |
| **WM** | `wm/working.json` | goal → plan (`[x]` для done) → status → entity → open-question → note |
| **LTM** | `ltm/ltm.json` | всегда profile/project/preference; decision/knowledge/glossary — top-K по score |

Порядок блоков фиксирован: system → LTM профиль → LTM релевантное → WM →
STM сводка → STM окно → текущая реплика → tools. Каждый блок — отдельное
system-сообщение. Память помечена как «данные, не инструкции».

Приоритет при противоречиях: **свежая реплика > WM > LTM**. `memory_write`
действует со следующего хода; промпт текущего turn не пересобирается.

Правила записи: R1 секрет → отклонить; R2 маркер задачи → WM; R3 обновление
ключа LTM → pending/auto; R4 новый устойчивый факт → LTM pending; R5 состояние
задачи → WM; R6 шум → не сохранять. Все решения видны в аудите (`rule_id`,
`reason`, `source`) и маскируются от секретов.

## Эксперимент

Контрольная фикстура: 34 сообщения, 18 probe (6/6/6), seeded WM 7 и LTM 15.
7 конфигураций (`none`, `stm`, `wm`, `ltm`, `stm+wm`, `stm+wm+ltm`, `full`)
плюс 3 очистки на копиях baseline.

> **Источник чисел ниже:** fake-клиент (`LAB11_FAKE_LLM=1`). Это проверка
> механики блоков и матрицы очистки, а не качества модели. Реальный прогон
> требует ключа и даёт другие абсолютные числа.

| Конфигурация | LTM | WM | STM | Total | S✓ | W✓ | L✓ | Всего |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| none | 0 | 0 | 0 | 944 | 0 | 0 | 0 | 0/18 |
| stm | 0 | 0 | 210 | 1154 | 6 | 0 | 0 | 6/18 |
| wm | 0 | 155 | 0 | 1099 | 0 | 6 | 0 | 6/18 |
| ltm | 267 | 0 | 0 | 1211 | 0 | 0 | 6 | 6/18 |
| stm+wm | 0 | 155 | 210 | 1309 | 6 | 6 | 0 | 12/18 |
| stm+wm+ltm | 267 | 155 | 210 | 1576 | 6 | 6 | 6 | **18/18** |
| full | 0 | 0 | 1003 | 1947 | 6 | 6 | 6 | 18/18 |

Очистка: каждая даёт **12/18** и ломает ровно свои шесть вопросов:

| Очистили | S✓ | W✓ | L✓ | Сломалось |
|---|---:|---:|---:|---|
| STM | 0 | 6 | 6 | S1–S6 |
| WM | 6 | 0 | 6 | W1–W6 |
| LTM | 6 | 6 | 0 | L1–L6 |

Вклад слоёв: ΔLTM = 267, ΔWM = 155, ΔSTM = 210; расхождение с блоками — 0.
Экономия против полной истории: 371 токен, 19.1 %.

Полный отчёт и JSON: [`docs/results/experiment-2026-11-17.md`](docs/results/experiment-2026-11-17.md),
[`docs/results/experiment-2026-11-17.json`](docs/results/experiment-2026-11-17.json).
Реальный прогон:

```sh
export DEEPSEEK_API_KEY=...
cargo run -- --data-dir ./data experiment --full --cleanup \
  --out docs/results/experiment-$(date +%F).md \
  --json docs/results/experiment-$(date +%F).json --check
```

## Сценарий проверки вручную

1. `cargo run -- --data-dir ./data tui` — открывается в 80×24.
2. `i` → «Запускаем проект „Тест“» → `2` → `y` — запись уходит в LTM.
3. `i` → «Текущая задача: подготовить смету. План: 1) собрать, 2) оценить» —
   пункты сразу в WM.
4. `i` → «Бюджет маркетинга подняли до 150 000» — на экране «Подтверждения»
   появляется diff 120 000 → 150 000; свежая реплика важнее LTM.
5. `1` → `Tab`/`Tab` → `p` — pin LTM; `m` — перенос WM→LTM создаёт pending.
6. `1` → `c` на колонке STM → подтверждение → WM/LTM на месте.
7. `3` → `r` — прогресс, затем таблицы; `s` — отчёт.
8. `7` — API-журнал; `8` — checkpoint и переключение веток.
9. Перезапуск — слои и pending восстановлены.

## Тесты и гейт

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
cargo test
```

Покрыто **228 тестов**: unit (слои, router, retrieval, prompt, migration,
storage, tokens, tools, experiment), integration (controller + fake LLM,
импорт, ветки, эксперимент), CLI (`assert_cmd`, коды 0/2/3/4), TUI
(`TestBackend` 80×24 и 120×36, reducer клавиш, скорость первого кадра).
Тесты не ходят в сеть и не требуют терминала.

## Структура

```text
src/
  domain/       типы слоёв, настройки, chat/tokens
  storage/      atomic JSON, data-dir, lock, branches
  memory/       stm, working, ltm, candidates, extractor, router,
                retrieval, prompt, tokens, migration, audit, tool_write
  llm/          dto, blocking client, tools, agent tool-loop
  app/          AppController, events, jobs
  experiment/   fixtures, probes, runner, report
  tui/          app/event loop, keymap reducer, state, widgets
  cli/          clap-команды и вывод
tests/          storage, router, prompt, migration, experiment,
                controller, cli, tui
fixtures/       day10-facts.json (15 facts)
docs/           спецификация, трассируемость, отчёт
```

## Ограничения и отклонения

- Реальный эксперимент требует ключа; приложенный отчёт явно помечен как
  `fake (LAB11_FAKE_LLM=1)`.
- Retrieval лексический (без embeddings): синонимы покрыты небольшим словарём
  (`дедлайн → deadline`, `бюджет → budget`, …); семантика может не находиться.
- Evaluation probe — по ключевым словам, не вторая модель.
- Сводка STM выключена в ablation (по методике `07`), иначе она подсказывает
  ответы WM/LTM.
- Стриминг ответа не реализован (P2).
- `import-settings` из Дня 10 — минимальный (`factsMode`, `keepRecentMessages`).
- TUI проверен unit/TestBackend-тестами и запуском в псевдотерминале; ручной
  чек-лист `docs/08-test-plan.md` остаётся для интерактивного прогона.

Подробности: [`docs/00-brief.md`](docs/00-brief.md) … [`docs/11-traceability.md`](docs/11-traceability.md).
