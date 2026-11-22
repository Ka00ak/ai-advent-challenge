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
    assert!(!progress.is_empty());
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
