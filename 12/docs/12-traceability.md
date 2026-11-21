# День 12. Трассируемость персонализации

Статусы: `spec`, `todo`, `done`, `p2`.

| ID | Требование | Реализация | Тесты | Статус |
|---|---|---|---|---|
| PR-1 | Профиль пользователя как отдельная сущность | `src/domain/profile.rs`, `profiles/profiles.json` | profile unit, `tests/storage.rs` | done |
| PR-2 | Несколько профилей, один активный | `ProfileState::active/set_active` | profile unit, `tests/controller.rs` | done |
| PR-3 | Предпочтения: стиль (объём, тон) | `Verbosity`, `Tone` | profile unit, prompt, CLI | done |
| PR-4 | Предпочтения: формат (код, списки, таблицы, эмодзи) | `ProfileFormat`, `CodePolicy` | profile unit, evaluator | done |
| PR-5 | Ограничения профиля | `constraints`, CLI `--constraint` | learning unit, evaluator | done |
| PR-6 | Профиль в каждом запросе отдельным блоком | `render_profile_block`, `AssembleInput.profile` | `src/memory/prompt.rs`, `tests/prompt.rs` | done |
| PR-7 | Учёт токенов профиля | `MemoryBreakdown.profile_tokens` | prompt unit, `tests/prompt.rs` | done |
| PR-8 | Пополнение из диалога | `learn_from_message` | learning unit, `tests/controller.rs`, CLI | done |
| PR-9 | История и происхождение изменений | `LearnedPreference` | learning unit, `profile show` | done |
| PR-10 | Ручное создание/изменение профилей | CLI `profile add/set/use/delete` | `tests/cli.rs` | done |
| PR-11 | TUI-экран профиля | `src/tui/widgets/profile.rs`, экран 9 | `tests/tui.rs` | done |
| PR-12 | Один вопрос под полярными профилями | `experiment/personalization.rs` | fake unit, CLI, реальный отчёт | done |
| PR-13 | Подсчёт соблюдения предпочтений | `evaluate_profile`, `compliant_profiles` | fake unit, реальный отчёт | done |
| PR-14 | 10 вопросов подряд и первое нарушение | `StabilityReport.first_violation` | fake unit, реальный отчёт | done |
| PR-15 | Markdown/JSON-отчёт и `--check` | `render_markdown`, `save_personalization_report` | unit, CLI | done |
| PR-16 | Реальные прогоны 2026-11-19 | `docs/results/personalization-2026-11-19*` | приложены | done |
| PR-17 | TUI-вкладка «1 Чат» поверх STM/WM/LTM и профиля | `src/tui/widgets/chat.rs`, `Screen::Chat` | `tests/tui.rs` | done |
| PR-18 | API-журнал: каждый HTTP-запрос отдельно, полные request/response | `src/llm/record.rs`, `src/tui/widgets/api_log.rs`, модалка `ApiDetail` | `src/llm/record.rs`, `tests/controller.rs`, `tests/experiment.rs`, `tests/tui.rs` | done |
| PR-19 | История STM перенесена в чат, вкладка 5 удалена | `Screen::Chat`, `src/tui/widgets/chat.rs`, keymap чата | `tests/tui.rs` | done |
| PR-20 | Ветки удалены из проекта, вкладки перенумерованы 1…8 | удалены `branches.rs`, branch-state и storage | `tests/tui.rs` | done |

## Фактические тесты

| Файл | Что покрывает |
|---|---|
| `src/domain/profile.rs` | serde, пресеты, active, remove |
| `src/memory/personalization.rs` | обучение по всем осям, конфликт, ложные срабатывания, блок |
| `src/memory/prompt.rs` | первое место блока, profile_tokens, отсутствие профиля |
| `src/experiment/personalization.rs` | fake 2/2 и 10/10, нарушения, Markdown/JSON |
| `src/llm/record.rs` | запись request/response, ошибки, маскирование, отдельные записи |
| `tests/controller.rs` | обучение и сохранение профиля, переключение, раздельный API-лог HTTP-запросов |
| `tests/cli.rs` | `profile list/use/learn`, `personalize --check`, отчёты |
| `tests/storage.rs` | roundtrip и битый `profiles/profiles.json` |
| `tests/experiment.rs` | 126 отдельных HTTP-записей эксперимента, событие `ApiLogged` для TUI |
| `tests/tui.rs` | экран 9, профиль, чат с историей STM, API: разделы «Запрос»/«Ответ», прокрутка длинного журнала, замена сводки эксперимента запросами |

Итог: **268 тестов**, `cargo clippy --all-targets -- -D warnings` и
`cargo fmt --check` чистые.
