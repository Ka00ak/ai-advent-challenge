mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::Duration;

use lab12::domain::task::{TaskStage, TaskStatus};
use lab12::llm::client::FakeChatClient;
use lab12::task::runner::{TaskProgress, run_task};

#[test]
fn controller_runs_task_to_done_and_logs_three_requests() {
    let dir = tempfile::tempdir().unwrap();
    let mut controller = common::controller_at(dir.path());
    let cancel = AtomicBool::new(false);
    let pause = AtomicBool::new(false);
    let mut progress = Vec::new();
    let state = controller
        .run_task_cancellable(
            "Подготовить отчёт по проекту",
            &[],
            &cancel,
            &pause,
            &mut |event: TaskProgress| progress.push(event),
        )
        .unwrap();
    assert_eq!(state.stage, TaskStage::Done);
    assert_eq!(state.status, TaskStatus::Done);
    assert!(state.plan_approved);
    assert_eq!(state.validation_passed, Some(true));
    assert_eq!(controller.exchanges.len(), 3);
    assert!(
        controller
            .exchanges
            .iter()
            .all(|exchange| exchange.kind == lab12::domain::chat::ExchangeKind::Chat)
    );
    assert_eq!(state.messages.len(), 4);
    assert_eq!(
        state.messages[0].role,
        lab12::domain::task::TaskMessageRole::User
    );
    assert!(!progress.is_empty());
}

#[test]
fn next_message_keeps_chat_context_and_survives_reload() {
    let dir = tempfile::tempdir().unwrap();
    let mut controller = common::controller_at(dir.path());
    let cancel = AtomicBool::new(false);
    let pause = AtomicBool::new(false);

    let first = controller
        .run_task_cancellable("Подготовить отчёт", &[], &cancel, &pause, &mut |_| {})
        .unwrap();
    assert_eq!(first.messages.len(), 4);
    let history = first.messages.clone();

    let second = controller
        .run_task_cancellable("Добавь сроки", &history, &cancel, &pause, &mut |_| {})
        .unwrap();
    assert_eq!(second.stage, TaskStage::Done);
    assert_eq!(second.messages.len(), 8);
    assert_eq!(&second.messages[..4], history.as_slice());
    assert_eq!(
        second.messages[4].role,
        lab12::domain::task::TaskMessageRole::User
    );
    assert!(
        second.messages[4..]
            .iter()
            .any(|message| message.text.contains("ВЕРДИКТ: OK"))
    );

    // Контекст сохраняется на диск: новый контроллер видит историю.
    let reloaded = common::controller_at(dir.path());
    let persisted = reloaded.task_state().unwrap();
    assert_eq!(persisted.messages.len(), 8);
    assert_eq!(persisted.messages[4].text, "Добавь сроки");
}

#[test]
fn reset_clears_task_and_persisted_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut controller = common::controller_at(dir.path());
    let cancel = AtomicBool::new(false);
    let pause = AtomicBool::new(false);
    controller
        .run_task_cancellable("Подготовить отчёт", &[], &cancel, &pause, &mut |_| {})
        .unwrap();
    let session = controller.runtime().paths.task_session();
    assert!(session.exists());
    assert!(controller.task_state().is_some());

    controller.reset_task().unwrap();
    assert!(controller.task_state().is_none());
    assert!(!session.exists());
}

#[test]
fn pause_stops_before_request_and_resume_finishes_task() {
    let pause = Arc::new(AtomicBool::new(true));
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let pause_worker = pause.clone();
    let cancel_worker = cancel.clone();
    let handle = std::thread::spawn(move || {
        run_task(
            "Проверить смету",
            &[],
            &common::test_settings(),
            &FakeChatClient,
            &cancel_worker,
            &pause_worker,
            &mut |progress| {
                let _ = tx.send(progress);
            },
        )
    });

    let first = loop {
        let progress = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        if progress.state.status == TaskStatus::Paused {
            break progress;
        }
    };
    assert_eq!(first.state.status, TaskStatus::Paused);
    assert_eq!(first.state.stage, TaskStage::Planning);
    assert_eq!(first.message, "пауза");

    pause.store(false, std::sync::atomic::Ordering::Relaxed);
    let state = handle.join().unwrap().unwrap();
    assert_eq!(state.stage, TaskStage::Done);
    assert_eq!(state.status, TaskStatus::Done);
}

#[test]
fn cancel_while_paused_stops_worker() {
    let pause = Arc::new(AtomicBool::new(true));
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let pause_worker = pause.clone();
    let cancel_worker = cancel.clone();
    let handle = std::thread::spawn(move || {
        run_task(
            "Долгая задача",
            &[],
            &common::test_settings(),
            &FakeChatClient,
            &cancel_worker,
            &pause_worker,
            &mut |progress| {
                let _ = tx.send(progress);
            },
        )
    });
    let first = loop {
        let progress = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        if progress.state.status == TaskStatus::Paused {
            break progress;
        }
    };
    assert_eq!(first.state.status, TaskStatus::Paused);
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let error = handle.join().unwrap().unwrap_err();
    assert!(matches!(
        error,
        lab12::task::runner::TaskRunError::Cancelled
    ));
}
