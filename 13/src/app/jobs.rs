//! Worker-поток TUI: один job за раз, результаты через mpsc.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use crate::app::controller::{AppController, AppRuntime};
use crate::app::events::{AppEvent, Job};

pub fn spawn_job(runtime: AppRuntime, job: Job, tx: Sender<AppEvent>, cancel: Arc<AtomicBool>) {
    spawn_job_with_pause(runtime, job, tx, cancel, Arc::new(AtomicBool::new(false)));
}

/// Job задачи получает отдельный флаг паузы: TUI переключает его клавишей `p`,
/// worker проверяет между HTTP-запросами.
pub fn spawn_task_job(
    runtime: AppRuntime,
    job: Job,
    tx: Sender<AppEvent>,
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
) {
    spawn_job_with_pause(runtime, job, tx, cancel, pause);
}

fn spawn_job_with_pause(
    runtime: AppRuntime,
    job: Job,
    tx: Sender<AppEvent>,
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
) {
    let label = job_label(&job);
    let _ = tx.send(AppEvent::JobStarted(label));
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            run_job(runtime, job, &tx, &cancel, &pause)
        }));
        if let Err(panic) = result {
            let message = panic
                .downcast_ref::<&str>()
                .map(|text| (*text).to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "worker panic".to_string());
            let _ = tx.send(AppEvent::Error(format!("внутренняя ошибка: {message}")));
        }
        let _ = tx.send(AppEvent::JobFinished);
    });
}

fn run_job(
    runtime: AppRuntime,
    job: Job,
    tx: &Sender<AppEvent>,
    cancel: &Arc<AtomicBool>,
    pause: &Arc<AtomicBool>,
) {
    let mut controller = match AppController::load(runtime) {
        Ok(controller) => controller,
        Err(error) => {
            let _ = tx.send(AppEvent::Error(error.to_string()));
            return;
        }
    };
    let api_tx = tx.clone();
    controller.set_api_listener(Some(Arc::new(move |exchange| {
        let _ = api_tx.send(AppEvent::ApiLogged(vec![exchange]));
    })));
    match job {
        Job::Chat(text) => {
            let result = controller.send_user_message(&text, &mut |progress| {
                let _ = tx.send(AppEvent::ChatProgress(progress));
            });
            match result {
                Ok(outcome) => {
                    let _ = tx.send(AppEvent::MemoryUpdated(outcome.memory.clone()));
                    let _ = tx.send(AppEvent::ChatFinished(Box::new(outcome)));
                }
                Err(error) => {
                    let _ = tx.send(AppEvent::MemoryUpdated(controller.bundle().clone()));
                    let _ = tx.send(AppEvent::Error(error.to_string()));
                }
            }
        }
        Job::Task(goal) => {
            let result = controller.run_task_cancellable(&goal, cancel, pause, &mut |progress| {
                let _ = tx.send(AppEvent::TaskProgress(progress));
            });
            match result {
                Ok(state) => {
                    let _ = tx.send(AppEvent::TaskFinished(state));
                }
                Err(error) => {
                    let _ = tx.send(AppEvent::Error(error.to_string()));
                }
            }
        }
        Job::AcceptPending(id) => match controller.accept_pending(&id) {
            Ok(()) => {
                let _ = tx.send(AppEvent::MemoryUpdated(controller.bundle().clone()));
            }
            Err(error) => {
                let _ = tx.send(AppEvent::Error(error.to_string()));
            }
        },
        Job::RejectPending(id) => match controller.reject_pending(&id) {
            Ok(()) => {
                let _ = tx.send(AppEvent::MemoryUpdated(controller.bundle().clone()));
            }
            Err(error) => {
                let _ = tx.send(AppEvent::Error(error.to_string()));
            }
        },
        Job::ChangePending { id, layer } => match controller.change_pending_layer(&id, layer) {
            Ok(()) => {
                let _ = tx.send(AppEvent::MemoryUpdated(controller.bundle().clone()));
            }
            Err(error) => {
                let _ = tx.send(AppEvent::Error(error.to_string()));
            }
        },
        Job::ClearLayer(layer) => match controller.clear_layer(layer) {
            Ok(()) => {
                let _ = tx.send(AppEvent::MemoryUpdated(controller.bundle().clone()));
            }
            Err(error) => {
                let _ = tx.send(AppEvent::Error(error.to_string()));
            }
        },
        Job::ImportFacts(path) => match controller.import_facts(&path) {
            Ok(report) => {
                let _ = tx.send(AppEvent::MemoryUpdated(controller.bundle().clone()));
                let _ = tx.send(AppEvent::MigrationFinished(report));
            }
            Err(error) => {
                let _ = tx.send(AppEvent::Error(error.to_string()));
            }
        },
        Job::Experiment(options) => {
            let result = controller.run_experiment_cancellable(options, cancel, &mut |progress| {
                let _ = tx.send(AppEvent::ExperimentProgress(progress));
            });
            match result {
                Ok(report) => {
                    let _ = tx.send(AppEvent::ExperimentFinished(report));
                }
                Err(error) => {
                    let _ = tx.send(AppEvent::Error(error.to_string()));
                }
            }
        }
        Job::Personalization(options) => {
            let result =
                controller.run_personalization_cancellable(options, cancel, &mut |progress| {
                    let _ = tx.send(AppEvent::PersonalizationProgress(progress));
                });
            match result {
                Ok(report) => {
                    let _ = tx.send(AppEvent::PersonalizationFinished(report));
                }
                Err(error) => {
                    let _ = tx.send(AppEvent::Error(error.to_string()));
                }
            }
        }
    }
    if cancel.load(Ordering::Relaxed) {
        let _ = tx.send(AppEvent::Error("job отменён".into()));
    }
}

fn job_label(job: &Job) -> &'static str {
    match job {
        Job::Chat(_) => "chat",
        Job::Task(_) => "task",
        Job::AcceptPending(_) => "accept",
        Job::RejectPending(_) => "reject",
        Job::ChangePending { .. } => "change",
        Job::ClearLayer(_) => "clear",
        Job::ImportFacts(_) => "import",
        Job::Experiment(_) => "experiment",
        Job::Personalization(_) => "personalization",
    }
}
