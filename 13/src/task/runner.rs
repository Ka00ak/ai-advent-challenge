//! Движок задачи: этап за этапом, с проверкой перехода кодом.
//!
//! Модель заполняет артефакт текущего этапа. Переход к следующему этапу
//! выполняет только `TaskState::transition`, который проверяет предусловие.
//! Пользователь может поставить выполнение на паузу; пауза проверяется
//! между запросами, поэтому TUI остаётся отзывчивым.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::Utc;
use thiserror::Error;

use crate::domain::memory::ChatMessage;
use crate::domain::settings::Settings;
use crate::domain::task::{TaskMessage, TaskStage, TaskState, TaskStatus, TaskTransitionError};
use crate::llm::dto::{ChatClient, ChatRequest};

#[derive(Debug, Error)]
pub enum TaskRunError {
    #[error("ошибка модели: {0}")]
    Llm(String),
    #[error("переход отклонён: {0}")]
    Transition(#[from] TaskTransitionError),
    #[error("пустой ответ модели на этапе {0}")]
    EmptyResponse(TaskStage),
    #[error("задача отменена")]
    Cancelled,
    #[error("пустая задача")]
    EmptyGoal,
}

/// Снимок состояния после очередного шага. TUI обновляет вкладку «Состояния».
#[derive(Debug, Clone)]
pub struct TaskProgress {
    pub state: TaskState,
    pub message: String,
}

/// Полный прогон задачи: `planning → execution → validation → done`.
///
/// Если валидация не пройдена, возвращается состояние на этапе `validation`
/// со статусом `failed`: переход в `done` блокируется кодом.
pub fn run_task(
    goal: &str,
    history: &[TaskMessage],
    settings: &Settings,
    client: &dyn ChatClient,
    cancel: &AtomicBool,
    pause: &AtomicBool,
    progress: &mut dyn FnMut(TaskProgress),
) -> Result<TaskState, TaskRunError> {
    let goal = goal.trim();
    if goal.is_empty() {
        return Err(TaskRunError::EmptyGoal);
    }
    let mut state = TaskState::start_with_history(goal, history.to_vec(), Utc::now());
    emit(&state, "задача запущена", progress);

    // --- planning ---
    checkpoint(cancel, pause, &mut state, progress)?;
    let plan = ask(client, settings, &state, TaskStage::Planning)?;
    state.push_assistant(TaskStage::Planning, plan.clone());
    state.set_step(
        2,
        2,
        "Утверждение плана",
        "Проверить, что план содержит минимум два шага.",
    );
    state.record_plan(plan, Utc::now())?;
    if let Err(error) = state.approve_plan(Utc::now()) {
        state.mark_failed(error.to_string(), Utc::now());
        emit(&state, "план не утверждён", progress);
        return Ok(state);
    }
    emit(&state, "план утверждён", progress);
    state.transition(TaskStage::Execution, "план утверждён кодом", Utc::now())?;
    emit(&state, "переход planning → execution", progress);

    // --- execution ---
    checkpoint(cancel, pause, &mut state, progress)?;
    state.set_step(
        1,
        1,
        "Выполнение плана",
        TaskStage::Execution.expected_action(),
    );
    let execution = ask(client, settings, &state, TaskStage::Execution)?;
    state.push_assistant(TaskStage::Execution, execution.clone());
    state.record_execution(execution, Utc::now())?;
    emit(&state, "результат реализации получен", progress);
    state.transition(
        TaskStage::Validation,
        "результат реализации не пуст",
        Utc::now(),
    )?;
    emit(&state, "переход execution → validation", progress);

    // --- validation ---
    checkpoint(cancel, pause, &mut state, progress)?;
    state.set_step(
        1,
        2,
        "Проверка результата",
        TaskStage::Validation.expected_action(),
    );
    let validation = ask(client, settings, &state, TaskStage::Validation)?;
    state.push_assistant(TaskStage::Validation, validation.clone());
    let passed = parse_validation_verdict(&validation);
    state.record_validation(validation, passed, Utc::now())?;
    state.set_step(
        2,
        2,
        if passed {
            "Вердикт: OK"
        } else {
            "Вердикт: FAIL"
        },
        "Зафиксировать вердикт валидации.",
    );
    emit(&state, "валидация завершена", progress);
    if !passed {
        state.mark_failed("валидация не пройдена: переход в done запрещён", Utc::now());
        emit(&state, "переход validation → done заблокирован", progress);
        return Ok(state);
    }
    state.transition(TaskStage::Done, "валидация пройдена", Utc::now())?;

    // --- done ---
    state.status = TaskStatus::Done;
    state.set_step(1, 1, "Задача завершена", TaskStage::Done.expected_action());
    state.updated_at = Utc::now();
    emit(&state, "переход validation → done", progress);
    Ok(state)
}

fn emit(state: &TaskState, message: &str, progress: &mut dyn FnMut(TaskProgress)) {
    progress(TaskProgress {
        state: state.clone(),
        message: message.to_string(),
    });
}

/// Ждёт снятия паузы; при отмене завершает прогон.
fn checkpoint(
    cancel: &AtomicBool,
    pause: &AtomicBool,
    state: &mut TaskState,
    progress: &mut dyn FnMut(TaskProgress),
) -> Result<(), TaskRunError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(TaskRunError::Cancelled);
    }
    if !pause.load(Ordering::Relaxed) {
        return Ok(());
    }
    state.mark_paused(Utc::now());
    emit(state, "пауза", progress);
    while pause.load(Ordering::Relaxed) {
        if cancel.load(Ordering::Relaxed) {
            return Err(TaskRunError::Cancelled);
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    state.mark_running(Utc::now());
    emit(state, "выполнение продолжено", progress);
    Ok(())
}

fn ask(
    client: &dyn ChatClient,
    settings: &Settings,
    state: &TaskState,
    stage: TaskStage,
) -> Result<String, TaskRunError> {
    let system = format!(
        "Ты исполняешь задачу пользователя как конечный автомат.\n\
         TASK_STAGE={stage}\n\
         Этап: {title} ({stage}).\n\
         Ожидаемое действие: {action}\n\
         Не меняй этап сам и не пропускай этапы: переходы проверяет код.",
        stage = stage.as_str(),
        title = stage.title(),
        action = stage.expected_action(),
    );
    let user = stage_prompt(state, stage);
    let mut messages = vec![ChatMessage::system(system)];
    if let Some(history) = history_block(state) {
        messages.push(ChatMessage::system(history));
    }
    messages.push(ChatMessage::user(user));
    let request = ChatRequest {
        model: settings.model.clone(),
        messages,
        temperature: settings.temperature,
        max_tokens: settings.max_tokens,
        tools: Vec::new(),
    };
    let response = client
        .complete(&request)
        .map_err(|error| TaskRunError::Llm(error.to_string()))?;
    let text = response.content.unwrap_or_default();
    if text.trim().is_empty() {
        return Err(TaskRunError::EmptyResponse(stage));
    }
    Ok(text)
}

/// Последние реплики диалога задачи — контекст для промпта этапа.
fn history_block(state: &TaskState) -> Option<String> {
    if state.messages.is_empty() {
        return None;
    }
    let mut text = String::from("## История диалога задачи\n");
    for message in state.messages.iter().rev().take(12).rev() {
        let stage = message
            .stage
            .map(|stage| format!(" · {}", stage.as_str()))
            .unwrap_or_default();
        let snippet: String = message.text.chars().take(1200).collect();
        text.push_str(&format!("{}:{} {}\n", message.role.label(), stage, snippet));
    }
    Some(text)
}

fn stage_prompt(state: &TaskState, stage: TaskStage) -> String {
    match stage {
        TaskStage::Planning => format!(
            "Задача пользователя:\n{}\n\nСоставь пошаговый план решения задачи. \
             Верни только план: минимум два пункта, каждый с номером.",
            state.goal
        ),
        TaskStage::Execution => format!(
            "Задача пользователя:\n{}\n\nУтверждённый план:\n{}\n\n\
             Выполни план и верни результат реализации. Не переходи к финалу: \
             результат сначала проверит валидатор.",
            state.goal,
            state.plan.as_deref().unwrap_or("—")
        ),
        TaskStage::Validation => format!(
            "Задача пользователя:\n{}\n\nУтверждённый план:\n{}\n\n\
             Результат реализации:\n{}\n\nПроверь результат по плану и задаче. \
             Ответь строго в формате:\nВЕРДИКТ: OK\n<1–3 предложения обоснования>\n\
             или\nВЕРДИКТ: FAIL\n<что именно не выполнено>.",
            state.goal,
            state.plan.as_deref().unwrap_or("—"),
            state.execution.as_deref().unwrap_or("—")
        ),
        TaskStage::Done => "Задача завершена.".to_string(),
    }
}

/// Вердикт валидации. По умолчанию — не пройдена: без явного `OK` финал
/// запрещён, чтобы модель не могла «проскочить» проверку.
pub fn parse_validation_verdict(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains("вердикт: fail") || lower.contains("вердикт: не") {
        return false;
    }
    lower.contains("вердикт: ok") || lower.contains("вердикт: ок")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::task::TaskStage;
    use crate::llm::dto::{ChatResponse, LlmError};
    use std::sync::Mutex;

    struct Scripted {
        responses: Mutex<Vec<String>>,
        calls: Mutex<Vec<ChatRequest>>,
    }

    impl Scripted {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().map(str::to_string).collect()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn with_validation_fail() -> Self {
            Self::new(vec![
                "1. Собрать данные\n2. Подготовить отчёт",
                "Отчёт подготовлен.",
                "ВЕРДИКТ: FAIL\nНе хватает сроков.",
            ])
        }
    }

    impl ChatClient for Scripted {
        fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
            self.calls.lock().unwrap().push(request.clone());
            let mut responses = self.responses.lock().unwrap();
            let content = if responses.is_empty() {
                "ВЕРДИКТ: OK\nготово".to_string()
            } else {
                responses.remove(0)
            };
            Ok(ChatResponse {
                content: Some(content),
                tool_calls: Vec::new(),
                usage: None,
            })
        }
    }

    struct Failing;

    impl ChatClient for Failing {
        fn complete(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
            Err(LlmError::Http("нет сети".into()))
        }
    }

    fn settings() -> Settings {
        Settings::default()
    }

    fn run(client: &dyn ChatClient, cancel: &AtomicBool, pause: &AtomicBool) -> TaskState {
        run_task(
            "Подготовить отчёт",
            &[],
            &settings(),
            client,
            cancel,
            pause,
            &mut |_| {},
        )
        .unwrap()
    }

    #[test]
    fn full_route_ends_in_done_after_three_requests() {
        let client = Scripted::new(vec![
            "1. Собрать данные\n2. Подготовить отчёт",
            "Отчёт подготовлен.",
            "ВЕРДИКТ: OK\nПлан выполнен.",
        ]);
        let cancel = AtomicBool::new(false);
        let pause = AtomicBool::new(false);
        let state = run(&client, &cancel, &pause);
        assert_eq!(state.stage, TaskStage::Done);
        assert_eq!(state.status, TaskStatus::Done);
        assert!(state.plan_approved);
        assert!(state.validation_passed.unwrap());
        assert_eq!(client.calls.lock().unwrap().len(), 3);
        let history: Vec<TaskStage> = state.history.iter().map(|record| record.to).collect();
        assert_eq!(
            history,
            vec![
                TaskStage::Planning,
                TaskStage::Execution,
                TaskStage::Validation,
                TaskStage::Done
            ]
        );
    }

    #[test]
    fn history_is_included_in_stage_prompts() {
        let client = Scripted::new(vec![
            "1. Собрать данные\n2. Подготовить отчёт",
            "Отчёт подготовлен.",
            "ВЕРДИКТ: OK\nПлан выполнен.",
        ]);
        let cancel = AtomicBool::new(false);
        let pause = AtomicBool::new(false);
        let history = vec![
            TaskMessage::user("Раньше мы решили делать отчёт в пятницу", Utc::now()),
            TaskMessage::assistant(TaskStage::Execution, "Пятница согласована", Utc::now()),
        ];
        let state = run_task(
            "Продолжи отчёт",
            &history,
            &settings(),
            &client,
            &cancel,
            &pause,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(state.stage, TaskStage::Done);
        assert_eq!(state.messages.len(), history.len() + 4);
        assert_eq!(&state.messages[..history.len()], history.as_slice());

        let calls = client.calls.lock().unwrap();
        let first_prompt = calls[0]
            .messages
            .iter()
            .map(|message| message.text())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(first_prompt.contains("Раньше мы решили делать отчёт в пятницу"));
        assert!(first_prompt.contains("Пятница согласована"));
    }

    #[test]
    fn failed_validation_stays_on_validation() {
        let client = Scripted::with_validation_fail();
        let cancel = AtomicBool::new(false);
        let pause = AtomicBool::new(false);
        let state = run(&client, &cancel, &pause);
        assert_eq!(state.stage, TaskStage::Validation);
        assert_eq!(state.status, TaskStatus::Failed);
        assert_eq!(state.validation_passed, Some(false));
        assert!(state.error.is_some());
    }

    #[test]
    fn too_short_plan_blocks_execution() {
        let client = Scripted::new(vec![
            "Один общий план без шагов",
            "не должно быть вызвано",
            "ВЕРДИКТ: OK",
        ]);
        let cancel = AtomicBool::new(false);
        let pause = AtomicBool::new(false);
        let state = run(&client, &cancel, &pause);
        assert_eq!(state.stage, TaskStage::Planning);
        assert_eq!(state.status, TaskStatus::Failed);
        assert_eq!(client.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn cancel_before_start_makes_no_requests() {
        let client = Scripted::new(vec!["1. a\n2. b"]);
        let cancel = AtomicBool::new(true);
        let pause = AtomicBool::new(false);
        let error = run_task(
            "задача",
            &[],
            &settings(),
            &client,
            &cancel,
            &pause,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, TaskRunError::Cancelled));
        assert!(client.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn api_error_is_reported() {
        let cancel = AtomicBool::new(false);
        let pause = AtomicBool::new(false);
        let error = run_task(
            "задача",
            &[],
            &settings(),
            &Failing,
            &cancel,
            &pause,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, TaskRunError::Llm(_)));
    }

    #[test]
    fn verdict_requires_explicit_ok() {
        assert!(parse_validation_verdict("ВЕРДИКТ: OK\nвсё хорошо"));
        assert!(parse_validation_verdict("вердикт: ок"));
        assert!(!parse_validation_verdict("ВЕРДИКТ: FAIL"));
        assert!(!parse_validation_verdict("выглядит нормально"));
    }
}
