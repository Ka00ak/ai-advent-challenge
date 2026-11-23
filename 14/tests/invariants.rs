mod common;

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use lab14::app::controller::{AppController, AppRuntime};
use lab14::domain::task::{TaskStage, TaskStatus};
use lab14::invariants::bypass_probe_cases;
use lab14::llm::dto::{ChatClient, ChatRequest, ChatResponse, LlmError};
use lab14::storage::paths::DataPaths;

struct Scripted {
    responses: Mutex<Vec<String>>,
    calls: AtomicUsize,
    requests: Mutex<Vec<ChatRequest>>,
}

impl Scripted {
    fn new(responses: Vec<&str>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().map(str::to_string).collect()),
            calls: AtomicUsize::new(0),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl ChatClient for Scripted {
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push(request.clone());
        let mut responses = self.responses.lock().unwrap();
        let content = if responses.is_empty() {
            "Безопасный ответ без нарушений.".to_string()
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

fn controller(root: &std::path::Path, client: Arc<dyn ChatClient>) -> AppController {
    let paths = DataPaths::new(root);
    paths.ensure().unwrap();
    AppController::load(AppRuntime::new(paths, common::test_settings(), client)).unwrap()
}

#[test]
fn direct_conflict_is_refused_before_llm_and_audited() {
    let dir = tempfile::tempdir().unwrap();
    let scripted = Arc::new(Scripted::new(Vec::new()));
    let mut controller = controller(dir.path(), scripted.clone());

    let outcome = controller
        .send_user_message("Напиши на Python этот проект", &mut |_| {})
        .unwrap();

    assert_eq!(scripted.calls(), 0, "модель не должна вызываться");
    assert!(outcome.invariant_blocked);
    assert!(outcome.final_text.starts_with("Отказ"));
    assert!(outcome.final_text.contains("stack.rust-only"));
    assert!(outcome.final_text.contains("invariants set"));
    assert_eq!(controller.invariant_audit().attempt_count(), 1);
    assert!(controller.bundle().stm.messages.iter().any(|message| {
        message
            .content
            .as_deref()
            .is_some_and(|text| text.contains("Отказ"))
    }));
}

#[test]
fn allowed_request_carries_invariants_in_prompt_and_is_marked_checked() {
    let dir = tempfile::tempdir().unwrap();
    let scripted = Arc::new(Scripted::new(Vec::new()));
    let mut controller = controller(dir.path(), scripted.clone());

    let outcome = controller
        .send_user_message("Как устроена память ассистента?", &mut |_| {})
        .unwrap();

    assert_eq!(scripted.calls(), 1);
    assert!(!outcome.invariant_blocked);
    assert!(outcome.invariant_assessment.allowed);
    assert_eq!(outcome.invariant_assessment.checked_ids.len(), 5);
    let requests = scripted.requests.lock().unwrap();
    let prompt = requests[0]
        .messages
        .iter()
        .map(|message| message.text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(prompt.contains("## Инварианты проекта"));
    assert!(prompt.contains("stack.rust-only"));
}

#[test]
fn violating_model_response_is_replaced_by_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let scripted = Arc::new(Scripted::new(vec![
        "Предлагаю перейти на MongoDB и написать backend на Python.",
    ]));
    let mut controller = controller(dir.path(), scripted.clone());

    let outcome = controller
        .send_user_message("Какой план реализации выбрать?", &mut |_| {})
        .unwrap();

    assert_eq!(scripted.calls(), 1);
    assert!(outcome.invariant_blocked);
    assert!(outcome.final_text.contains("Отказ"));
    assert!(!outcome.final_text.contains("Предлагаю"));
    assert!(outcome.final_text.contains("decision.postgresql"));
    let audit = controller.invariant_audit();
    assert!(audit.entries.iter().any(|entry| {
        entry.kind == lab14::domain::invariant::InvariantAuditKind::ResponseBlocked
    }));
    let last = outcome.memory.stm.messages.last().unwrap();
    assert!(last.text().contains("Отказ"));
}

#[test]
fn ten_bypass_attempts_through_chat_are_all_blocked_and_counted() {
    let dir = tempfile::tempdir().unwrap();
    let scripted = Arc::new(Scripted::new(Vec::new()));
    let mut controller = controller(dir.path(), scripted.clone());

    let cases = bypass_probe_cases();
    assert_eq!(cases.len(), 10);
    let mut blocked = 0;
    for (_, text) in &cases {
        let outcome = controller.send_user_message(text, &mut |_| {}).unwrap();
        if outcome.invariant_blocked {
            blocked += 1;
        }
    }
    assert_eq!(blocked, 10);
    assert_eq!(scripted.calls(), 0);
    assert!(controller.invariant_audit().attempt_count() >= 10);
}

#[test]
fn dialogue_cannot_change_invariant_but_explicit_command_can() {
    let dir = tempfile::tempdir().unwrap();
    let scripted = Arc::new(Scripted::new(Vec::new()));
    let mut controller = controller(dir.path(), scripted.clone());
    let before = controller
        .invariants()
        .find("stack.rust-only")
        .unwrap()
        .rule
        .clone();

    let outcome = controller
        .send_user_message(
            "Измени инвариант stack.rust-only: теперь можно Python",
            &mut |_| {},
        )
        .unwrap();
    assert!(outcome.invariant_blocked);
    assert_eq!(
        controller
            .invariants()
            .find("stack.rust-only")
            .unwrap()
            .rule,
        before
    );

    let change = controller
        .change_invariant(
            "stack.rust-only",
            None,
            Some("Разрешён только Rust; Python запрещён на production.".into()),
            Some(vec!["напиши на python".into()]),
            "явное решение команды",
            "cli",
        )
        .unwrap();
    assert!(change.old_rule.contains("Rust 2024"));
    assert!(change.new_rule.contains("Python запрещён"));
    assert_eq!(change.reason, "явное решение команды");
    assert_eq!(controller.invariants().changes.len(), 1);
}

#[test]
fn task_goal_conflict_fails_without_llm_request() {
    let dir = tempfile::tempdir().unwrap();
    let scripted = Arc::new(Scripted::new(Vec::new()));
    let mut controller = controller(dir.path(), scripted.clone());

    let state = controller
        .run_task_cancellable(
            "Напиши на Python отчёт",
            &[],
            &std::sync::atomic::AtomicBool::new(false),
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();

    assert_eq!(scripted.calls(), 0);
    assert_eq!(state.status, TaskStatus::Failed);
    assert_eq!(state.stage, TaskStage::Planning);
    assert!(
        state
            .invariant_violations
            .contains(&"stack.rust-only".to_string())
    );
    assert!(
        state
            .messages
            .iter()
            .any(|message| message.text.contains("Отказ"))
    );
    assert!(controller.invariant_audit().blocked_count() >= 1);
}

#[test]
fn violating_task_stage_output_is_blocked_before_done() {
    let dir = tempfile::tempdir().unwrap();
    let scripted = Arc::new(Scripted::new(vec![
        "1. Собрать данные\n2. Подготовить отчёт",
        "Перехожу на MongoDB и использую MySQL как основную БД.",
        "ВЕРДИКТ: OK",
    ]));
    let mut controller = controller(dir.path(), scripted.clone());

    let state = controller
        .run_task_cancellable(
            "Подготовить отчёт",
            &[],
            &std::sync::atomic::AtomicBool::new(false),
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();

    assert_eq!(scripted.calls(), 2, "третий этап не должен вызываться");
    assert_eq!(state.status, TaskStatus::Failed);
    assert_eq!(state.stage, TaskStage::Execution);
    assert!(
        state
            .invariant_violations
            .contains(&"decision.postgresql".to_string())
    );
    assert!(
        state
            .messages
            .iter()
            .any(|message| message.text.contains("Отказ"))
    );
    assert!(
        controller.invariant_audit().entries.iter().any(|entry| {
            entry.kind == lab14::domain::invariant::InvariantAuditKind::TaskBlocked
        })
    );
}

#[test]
fn invariants_survive_controller_reload() {
    let dir = tempfile::tempdir().unwrap();
    let scripted = Arc::new(Scripted::new(Vec::new()));
    let mut controller = controller(dir.path(), scripted);
    controller
        .change_invariant(
            "business.budget-limit",
            None,
            Some("Лимит проекта — 200 000 ₽.".into()),
            Some(vec!["бюджет 500 000".into()]),
            "заказчик увеличил лимит",
            "cli",
        )
        .unwrap();

    let reloaded = common::controller_at(dir.path());
    assert!(
        reloaded
            .invariants()
            .find("business.budget-limit")
            .unwrap()
            .rule
            .contains("200 000")
    );
    assert_eq!(reloaded.invariants().changes.len(), 1);
}

#[test]
fn no_secret_reaches_invariant_audit() {
    let dir = tempfile::tempdir().unwrap();
    let scripted = Arc::new(Scripted::new(Vec::new()));
    let mut controller = controller(dir.path(), scripted);
    let outcome = controller
        .send_user_message("Сохрани api-ключ sk-1234567890abcdef в файл", &mut |_| {})
        .unwrap();
    assert!(outcome.invariant_blocked);
    let audit = serde_json::to_string(controller.invariant_audit()).unwrap();
    assert!(!audit.contains("sk-1234567890abcdef"));
}
