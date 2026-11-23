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
| PR-12 | Одна цепочка из 10 вопросов для каждого из 3 профилей | `experiment/personalization.rs`, `ProfileRun` | fake unit 30/30, CLI | done |
| PR-13 | Подсчёт соблюдённых ответов и полностью соблюдённых профилей | `evaluate_profile`, `compliant_answers` | fake unit, реальный отчёт | done |
| PR-14 | Первое нарушение в каждом профиле | `ProfileRun.first_violation` | fake unit, реальный отчёт | done |
| PR-15 | Markdown/JSON-отчёт и `--check` | `render_markdown`, `save_personalization_report` | unit, CLI | done |
| PR-16 | Реальные прогоны 2026-11-19 | `docs/results/personalization-2026-11-19*` | приложены | done |
| PR-17 | TUI-вкладка «1 Чат» поверх STM/WM/LTM и профиля | `src/tui/widgets/chat.rs`, `Screen::Chat` | `tests/tui.rs` | done |
| PR-18 | API-журнал: каждый HTTP-запрос отдельно, полные request/response | `src/llm/record.rs`, `src/tui/widgets/api_log.rs`, модалка `ApiDetail` | `src/llm/record.rs`, `tests/controller.rs`, `tests/experiment.rs`, `tests/tui.rs` | done |
| PR-19 | История STM перенесена в чат | `Screen::Chat`, `src/tui/widgets/chat.rs`, keymap чата | `tests/tui.rs` | done |
| PR-20 | Ветки удалены из проекта, нумерация вкладок пересобрана | удалены `branches.rs`, branch-state и storage | `tests/tui.rs` | done |
| PR-21 | Токены перенесены в API, отдельная вкладка удалена | `src/llm/record.rs`, панель деталей API | `src/llm/record.rs`, `tests/tui.rs` | done |
| PR-22 | API: двухпанельный просмотр, фокус списка/тела и подсветка JSON | `src/tui/widgets/api_log.rs`, `src/tui/widgets/json.rs` | `src/tui/widgets/json.rs`, `tests/tui.rs` | done |
| PR-23 | TUI «Проверка»: режим профилей 10 × 3 = 30 запросов | `src/tui/widgets/experiment.rs`, `Job::Personalization`, `VerifyMode::Profiles` | `tests/tui.rs`, `tests/experiment.rs` | done |

## Фактические тесты

| Файл | Что покрывает |
|---|---|
| `src/domain/profile.rs` | serde, пресеты, active, remove |
| `src/memory/personalization.rs` | обучение по всем осям, конфликт, ложные срабатывания, блок |
| `src/memory/prompt.rs` | первое место блока, profile_tokens, отсутствие профиля |
| `src/experiment/personalization.rs` | fake 3 × 10 = 30 запросов, изоляция контекста профилей, нарушения, Markdown/JSON |
| `src/llm/record.rs` | запись request/response, ошибки, маскирование, отдельные записи, breakdown по блокам |
| `src/tui/widgets/json.rs` | подсветка ключей, строк, чисел, bool/null, escaped quotes |
| `tests/controller.rs` | обучение и сохранение профиля, переключение, раздельный API-лог HTTP-запросов |
| `tests/cli.rs` | `profile list/use/learn`, `personalize --check`, отчёты |
| `tests/storage.rs` | roundtrip и битый `profiles/profiles.json` |
| `tests/experiment.rs` | 126 отдельных HTTP-записей эксперимента, событие `ApiLogged` для TUI, job проверки профилей |
| `tests/tui.rs` | экраны 1…7, профиль, режим профилей на «Проверке», чат с историей STM, API: список + детали без модалки, «Запрос»/«Ответ», JSON, токены, прокрутка |

Итог: **275 тестов**, `cargo clippy --all-targets -- -D warnings` и
`cargo fmt --check` чистые.
