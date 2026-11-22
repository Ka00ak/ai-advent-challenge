# День 13. Трассируемость состояния задачи

Статусы: `spec`, `todo`, `done`, `p2`.

| ID | Требование | Реализация | Тесты | Статус |
|---|---|---|---|---|
| TS-1 | Этапы задачи: planning, execution, validation, done | `src/domain/task.rs::TaskStage` | domain unit | done |
| TS-2 | Текущий шаг и ожидаемое действие | `TaskState::{step_index,step_total,current_step,expected_action}` | domain unit, `tests/tui.rs` | done |
| TS-3 | Разрешённые переходы между этапами | `TaskState::can_transition` | domain unit | done |
| TS-4 | Переход проверяется кодом до применения | `TaskState::transition`, `TaskTransitionError` | domain unit, runner unit | done |
| TS-5 | Нельзя перепрыгнуть этап | `StageSkipped` | domain unit | done |
| TS-6 | Нельзя реализацию без утверждённого плана | `approve_plan`, `PlanNotApproved` | domain unit, runner unit | done |
| TS-7 | Нельзя финал без валидации | `ValidationNotPassed` | domain unit, runner unit | done |
| TS-8 | Агент проходит этапы запросами к LLM | `src/task/runner.rs::run_task` | runner unit, `tests/task.rs` | done |
| TS-9 | TUI-вкладка 8 «Состояния» | `Screen::Task`, `src/tui/widgets/task.rs` | `tests/tui.rs` | done |
| TS-10 | Поле ввода задачи и запуск | `task_input`, `UiAction::Task`, `Job::Task` | `tests/tui.rs`, `tests/task.rs` | done |
| TS-11 | Отображение маршрута и текущего этапа | `src/tui/widgets/task.rs::draw_route` | `tests/tui.rs` | done |
| TS-12 | Пауза и продолжение выполнения | `Arc<AtomicBool>` pause, `ToggleTaskPause`, `checkpoint` | `tests/task.rs` | done |
| TS-13 | API-журнал для запросов задачи | `AppController::run_task_cancellable`, `RecordingChatClient` | `tests/task.rs` | done |
| TS-14 | Детерминированный fake-режим задачи | `FakeChatClient`, `TASK_STAGE=` | `tests/task.rs` | done |
| TS-15 | Вкладка «Состояния» выглядит как чат | `TaskMessage`, `src/tui/widgets/task.rs::draw_chat` | `tests/tui.rs` | done |
| TS-16 | Можно дописывать сообщения после завершения | `TaskState::start_with_history`, `UiAction::Task` | `tests/tui.rs`, `tests/task.rs` | done |
| TS-17 | Контекст прошлых реплик уходит в промпты | `history_block`, `TaskState.messages` | `src/task/runner.rs` unit | done |
| TS-18 | История чата задачи сохраняется между запусками | `task/session.json`, `load_task_state`/`save_task_state` | `tests/task.rs` reload | done |

## Фактические тесты

| Файл | Что покрывает |
|---|---|
| `src/domain/task.rs` | маршрут, запрет прыжков, план ≥ 2 шагов, валидация, serde, пауза/ошибка, реплики чата |
| `src/task/runner.rs` | полный прогон 3 запросов, `FAIL` без `done`, короткий план, отмена, ошибка API, вердикт, контекст истории |
| `tests/task.rs` | controller + fake LLM, API-журнал, пауза/продолжение, отмена при паузе, контекст и перезагрузка |
| `tests/tui.rs` | вкладка 8, ввод и запуск задачи, `p`, маршрут и чат задачи в 80×24, прокрутка |
