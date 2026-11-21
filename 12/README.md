# День 12 — персонализация ассистента поверх памяти (Rust + TUI)

Агент с **тремя явными слоями памяти** (STM, WM, LTM) и **профилем
пользователя** поверх них. Профиль задаёт стиль, формат и ограничения,
передаётся отдельным системным блоком в каждый запрос, пополняется вручную и
из диалога. В комплекте — TUI, CLI и воспроизводимая проверка: один вопрос
под полярными профилями и серия из 10 вопросов на устойчивость.

**Статус:** реализовано, тесты и гейт зелёные; реальный прогон персонализации
приложен в `docs/results/`.
**Стек:** Rust 2024 (1.85+), ratatui + crossterm, clap, serde, chrono,
reqwest (blocking, rustls), fs2.

## Быстрый старт

```sh
cd 12
cargo build

# Демо-каталог данных (в .gitignore)
cargo run -- --data-dir ./data tui

# Профили: список, переключение, обучение из текста
cargo run -- --data-dir ./data profile list
cargo run -- --data-dir ./data profile use brief
cargo run -- --data-dir ./data profile learn "Отвечай кратко, без кода, неформально"

# Импорт facts Дня 10 (по желанию)
cargo run -- --data-dir ./data import-facts fixtures/day10-facts.json

# Один запрос (нужен ключ)
export DEEPSEEK_API_KEY=...
cargo run -- --data-dir ./data chat "Привет"

# Проверка персонализации (2 профиля + 10 вопросов устойчивости)
cargo run -- --data-dir ./data personalize --check \
  --out docs/results/personalization-$(date +%F).md \
  --json docs/results/personalization-$(date +%F).json

# Офлайн-проверка механики без сети
LAB12_FAKE_LLM=1 cargo run -- --data-dir ./data personalize --check

# Старый эксперимент с памятью тоже работает
LAB12_FAKE_LLM=1 cargo run -- --data-dir ./data experiment --quick --check
```

Ключ читается из `DEEPSEEK_API_KEY` или `KIMI_API_KEY` (либо
`--api-key-file` с правами `0600`). В настройки, слои, профиль, pending и
аудит ключ не попадает.

## Что умеет

| Возможность | Где |
|---|---|
| Чат с моделью поверх STM/WM/LTM и активного профиля | TUI экран `0`, CLI `chat` |
| Три слоя с раздельными JSON-файлами | `stm/`, `wm/`, `ltm/` |
| Профиль пользователя: стиль, формат, ограничения | `profiles/profiles.json`, CLI `profile`, TUI экран 9 |
| Профиль в каждом запросе отдельным system-блоком | `src/memory/personalization.rs`, `src/memory/prompt.rs` |
| Пополнение профиля из диалога с историей изменений | `learn_from_message`, `profile learn`, `chat` |
| Проверка: 2 профиля на один вопрос + 10 вопросов устойчивости | CLI `personalize`, `src/experiment/personalization.rs` |
| Роутинг R1–R6: секрет / задача / обновление LTM / новый факт / WM / отказ | `src/memory/router.rs` |
| LTM через pending (`accept/reject/edit/change layer`) | TUI экран 2, CLI `memory pending` |
| Tool `memory_write` в tool-loop | `src/memory/tool_write.rs` |
| Импорт facts Дня 10, идемпотентный | `import-facts`, `src/memory/migration.rs` |
| Сборка промпта с бюджетами, top-K, дедупликацией | `src/memory/prompt.rs` |
| Breakdown токенов по слоям | TUI «Токены», `chat --json`, API-лог |
| API-журнал: каждый HTTP-запрос отдельно, полные request/response | TUI экран `7`, `src/llm/record.rs` |
| 7 конфигураций + 3 очистки + 18 probe | TUI «Проверка», CLI `experiment` |
| Ветки — снимок STM+WM+LTM+pending+meta | TUI «Ветки» |
| Lock от второго экземпляра, атомарная запись, `.bak` | `src/storage/` |

## TUI

Запуск в терминале UTF-8 от **80×24**:

```text
cargo run -- --data-dir ./data tui
```

Экраны: `0` Чат · `1` Память · `2` Подтверждения · `3` Проверка · `4` Токены ·
`6` Настройки · `7` API · `8` Ветки · `9` Профиль. Вкладка `5` удалена:
её функции просмотра/очистки/экспорта STM перенесены в чат.

Основные клавиши:

```text
0..9        переключение экранов (0 — чат)
Tab         фокус между колонками/полями
↑ ↓ PgUp PgDn g G   навигация
Enter       карточка записи / редактирование / активировать профиль
i           написать агенту (на экране чата — ввод в строке)
?           помощь, q / Ctrl+C — выход
```

«Чат» (`0`): переписка STM с моделью и полная история диалога. `i`/`Enter`
захватывают строку ввода, `Enter` отправляет, `Esc` выходит из ввода,
`↑↓`/`PgUp/PgDn` прокручивают историю, `c` очищает STM, `e` экспортирует
историю в JSON, `I` импортирует её. Каждый запрос проходит через контроллер и
использует STM, WM, LTM и активный профиль; в шапке видны профиль, размеры
слоёв и breakdown последнего промпта. Пока модель отвечает, повторная отправка
заблокирована.

«Память»: `a` добавить, `e` изменить, `d` удалить, `p` pin (LTM),
`m` перенести WM↔LTM, `c` очистить слой или завершить задачу,
`/` поиск по LTM. Все destructive-действия — через модалку подтверждения.

«Подтверждения»: `y` принять, `n` отклонить, `e` изменить, `l` сменить слой.
Показан diff «Было → Станет», правило (`R3`) и stale-бейдж после 7 дней.

«Профиль»: список профилей, предпочтения и история обучения; `Enter`/`u`
делает выбранный профиль активным. Новые профили и точная настройка — через
CLI `profile add/set`.

«Проверка»: `r` запустить, `m` quick/full, `Space` включить очистку,
`s` сохранить MD/JSON, `d` детали probe, `i` импорт facts. Прогон идёт в
фоновом потоке, TUI не блокируется; `q` отменяет. Во время прогона строка
`API` показывает число завершённых запросов, prompt/completion-токены,
оценку стоимости и источник (`usage` от провайдера или локальная `оценка`).

«API» (`7`): одна строка — один HTTP-запрос (чат, tool-loop, извлечение
памяти, запросы эксперимента). `↑↓`/`g/G` выбирают запрос, `Enter` открывает
детали: `Tab`/`←→` переключают «Обзор», «Запрос» и «Ответ», `↑↓`/`PgUp/PgDn`
прокручивают полное тело. Запросы эксперимента появляются в журнале по мере
выполнения; сводная строка прогресса заменяется отдельными записями каждого
запроса. Секреты маскируются, API-ключ в журнал не пишется.

## CLI

```text
lab12 [--data-dir PATH] [--config PATH] [--api-key-file PATH] <команда>

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
profile list|show|use|add|set|delete|learn
personalize [--question TEXT] [--profiles brief,detailed] \
  [--stability-profile ID] [--no-stability] [--out MD] [--json JSON] [--check]
import-facts <PATH>
experiment [--quick|--full] [--cleanup] [--out MD] [--json JSON] [--check]
dump [--json]
import-settings <PATH>          # опционально, из Дня 10
```

`profile add`/`set` принимают `--verbosity brief|normal|detailed`,
`--tone formal|neutral|informal`, `--code with|without|neutral`,
`--language`, `--user-name`, `--role`, `--lists on|off`, `--tables on|off`,
`--emoji on|off`, `--constraint TEXT` и `--like default|brief|detailed`.

Коды выхода: `0` успех, `2` ошибка ввода/конфигурации, `3` ошибка API,
`4` провал порогов `--check`.

`experiment` выводит живой прогресс API в stderr: в TTY — одной обновляемой
строкой, в файл/пайп — строкой на каждый завершённый запрос. Итоговая таблица
идёт в stdout и не смешивается с прогрессом.

## Персонализация

Профиль — отдельный объект, не слой памяти. Файл `profiles/profiles.json`
хранит несколько профилей и активный ID. Предпочтения структурированы:

| Поле | Значения | Влияние на ответ |
|---|---|---|
| `verbosity` | `brief`, `normal`, `detailed` | «≤ 80 слов» / «≥ 70 слов» |
| `tone` | `formal`, `neutral`, `informal` | обращение на «вы» / «ты» |
| `format.code` | `with-code`, `no-code`, `neutral` | блок кода или запрет кода |
| `format.lists/tables/emoji` | да/нет | явные требования к разметке |
| `constraints` | строки | «без вступлений», «только факты», … |
| `identity` | имя, роль, язык | персонализация обращения |

Встроенные профили:

- `default` — нейтральный, активен при первом запуске;
- `brief` — кратко, без кода, неформально;
- `detailed` — подробно, формально, с примерами кода.

**В каждый запрос** профиль попадает отдельным system-блоком
`## Персонализация` сразу после ядра промпта и до блоков памяти. В блоке
записаны проверяемые требования: объём, тон, код, списки, таблицы, эмодзи,
ограничения. Текущая явная просьба пользователя важнее профиля.

**Пополнение из диалога.** Перед сборкой промпта реплика проходит
детерминированные правила (`src/memory/personalization.rs`): «отвечай
кратко/подробно», «без кода/с кодом», «формально/неформально», «на ты/на вы»,
«списком», «таблицей», «без эмодзи», «без вступлений», «меня зовут …»,
«отвечай на английском». Изменения сразу действуют на текущий ответ, пишутся
в профиль с источником `dialogue`, цитатой-основанием и временем; последняя
реплика выигрывает конфликт.

### Проверка персонализации

`personalize` задаёт **один и тот же вопрос** полярным профилям (`brief` и
`detailed`) и считает, в скольких ответах предпочтения реально соблюдены.
Затем задаёт **10 вопросов подряд** в одном профиле и фиксирует номер первого
ответа с нарушением. Оценка детерминированная: слова, наличие кода/списков/
таблиц, формальный или неформальный тон, эмодзи.

Реальные прогоны `deepseek-chat` (2026-11-19):

| Прогон | Один вопрос | Устойчивость |
|---|---|---|
| Основной (`detailed`) | **2 из 2** профилей | **10/10**, первое нарушение — нет |
| Дополнительный (`brief`) | **2 из 2** профилей | **6/10**, первое нарушение — ответ **#2** |

Основной отчёт: [`docs/results/personalization-2026-11-19.md`](docs/results/personalization-2026-11-19.md).
Дополнительный: [`docs/results/personalization-2026-11-19-brief.md`](docs/results/personalization-2026-11-19-brief.md).
Оба JSON лежат рядом. Fake-прогон (`LAB12_FAKE_LLM=1`) проверяет механику и
всегда даёт 2/2 и 10/10, но в отчёте явно помечен как mock.

## Модель памяти

| Слой | Файлы | Что попадает в промпт |
|---|---|---|
| **STM** | `stm/conversation.json`, `stm/summary.json` | последние `window` сообщений + текущая реплика + сводка |
| **WM** | `wm/working.json` | goal → plan (`[x]` для done) → status → entity → open-question → note |
| **LTM** | `ltm/ltm.json` | всегда profile/project/preference; decision/knowledge/glossary — top-K по score |

Порядок блоков фиксирован: system → **профиль персонализации** → LTM профиль →
LTM релевантное → WM → STM сводка → STM окно → текущая реплика → tools. Каждый
блок — отдельное system-сообщение. Память помечена как «данные, не инструкции».

Приоритет при противоречиях: **свежая реплика > WM > LTM > профиль**. `memory_write`
действует со следующего хода; промпт текущего turn не пересобирается.

Правила записи: R1 секрет → отклонить; R2 маркер задачи → WM; R3 обновление
ключа LTM → pending/auto; R4 новый устойчивый факт → LTM pending; R5 состояние
задачи → WM; R6 шум → не сохранять. Все решения видны в аудите (`rule_id`,
`reason`, `source`) и маскируются от секретов.

## Эксперимент

Контрольная фикстура: 34 сообщения, 18 probe (6/6/6), seeded WM 7 и LTM 15.
7 конфигураций (`none`, `stm`, `wm`, `ltm`, `stm+wm`, `stm+wm+ltm`, `full`)
плюс 3 очистки на копиях baseline.

> **Источник чисел ниже:** fake-клиент (`LAB12_FAKE_LLM=1`). Это проверка
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

Покрыто **254 теста**: unit (слои, router, retrieval, prompt, migration,
storage, tokens, tools, experiment, профиль и обучение персонализации),
integration (controller + fake LLM, импорт, ветки, эксперимент, персонализация),
CLI (`assert_cmd`, коды 0/2/3/4), TUI (`TestBackend` 80×24 и 120×36, reducer
клавиш, экран профиля, скорость первого кадра). Тесты не ходят в сеть и не
требуют терминала.

## Структура

```text
src/
  domain/       типы слоёв, профиль, настройки, chat/tokens
  storage/      atomic JSON, data-dir, lock, branches, profiles
  memory/       stm, working, ltm, candidates, extractor, router,
                retrieval, prompt, personalization, tokens, migration,
                audit, tool_write
  llm/          dto, blocking client, tools, agent tool-loop
  app/          AppController, events, jobs
  experiment/   fixtures, probes, runner, report, personalization
  tui/          app/event loop, keymap reducer, state, widgets
  cli/          clap-команды и вывод
tests/          storage, router, prompt, migration, experiment,
                controller, cli, tui
fixtures/       day10-facts.json (15 facts)
docs/           спецификация, трассируемость, отчёты
```

## Ограничения и отклонения

- Персонализация оценивается по наблюдаемым признакам (слова, код, списки,
  таблицы, тон), а не семантически; это эвристика, а не вторая модель-судья.
- В одном длинном прогоне модель может дрейфовать: `brief`-профиль в
  приложенном прогоне нарушил тон уже на втором ответе. Это зафиксировано, а
  не скрыто; один и тот же вопрос в коротком сравнении оба профиля прошли.
- Профиль глобальный: при переключении веток памяти он не откатывается.
- Retrieval лексический (без embeddings): синонимы покрыты небольшим словарём
  (`дедлайн → deadline`, `бюджет → budget`, …); семантика может не находиться.
- Evaluation probe эксперимента памяти — по ключевым словам, не вторая модель.
- Сводка STM выключена в ablation (по методике `07`), иначе она подсказывает
  ответы WM/LTM.
- Стриминг ответа не реализован (P2).
- `import-settings` из Дня 10 — минимальный (`factsMode`, `keepRecentMessages`).
- TUI проверен unit/TestBackend-тестами и запуском в псевдотерминале; ручной
  чек-лист `docs/08-test-plan.md` остаётся для интерактивного прогона.

Подробности: [`docs/00-brief.md`](docs/00-brief.md) … [`docs/11-traceability.md`](docs/11-traceability.md),
дизайн Дня 12 — [`docs/12-personalization.md`](docs/12-personalization.md).
