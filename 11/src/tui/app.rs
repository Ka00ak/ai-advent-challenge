//! Терминал, event loop, исполнение UiAction и job-события.

use std::io::stdout;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::controller::{AppController, AppError, AppRuntime};
use crate::app::events::{AppEvent, Job};
use crate::app::jobs::spawn_job;
use crate::app::state::Screen;
use crate::domain::memory::MemoryLayer;
use crate::tui::keymap::{UiAction, handle_key};
use crate::tui::state::{AppState, Modal};
use crate::tui::theme;
use crate::tui::widgets;

pub fn run(runtime: AppRuntime) -> Result<(), AppError> {
    let mut controller = AppController::load(runtime.clone())?;
    let mut state = AppState::new(&controller);
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));

    enable_raw_mode().map_err(|error| AppError::Io(error.to_string()))?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen).map_err(|error| AppError::Io(error.to_string()))?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend).map_err(|error| AppError::Io(error.to_string()))?;

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
        previous_hook(info);
    }));

    let result = event_loop(
        &mut terminal,
        &runtime,
        &mut controller,
        &mut state,
        &tx,
        &rx,
        &cancel,
    );

    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
    let _ = std::panic::take_hook();
    result
}

#[allow(clippy::too_many_arguments)]
fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    runtime: &AppRuntime,
    controller: &mut AppController,
    state: &mut AppState,
    tx: &Sender<AppEvent>,
    rx: &Receiver<AppEvent>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), AppError> {
    loop {
        terminal
            .draw(|frame| draw(frame, state))
            .map_err(|error| AppError::Io(error.to_string()))?;

        if event::poll(Duration::from_millis(50))
            .map_err(|error| AppError::Io(error.to_string()))?
        {
            match event::read().map_err(|error| AppError::Io(error.to_string()))? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let action = handle_key(state, key);
                    execute_action(action, runtime, controller, state, tx, cancel)?;
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        while let Ok(app_event) = rx.try_recv() {
            handle_app_event(app_event, runtime, controller, state);
        }

        if state.should_quit {
            cancel.store(true, Ordering::Relaxed);
            return Ok(());
        }
    }
}

fn execute_action(
    action: UiAction,
    runtime: &AppRuntime,
    controller: &mut AppController,
    state: &mut AppState,
    tx: &Sender<AppEvent>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), AppError> {
    match action {
        UiAction::None => {}
        UiAction::Quit => state.should_quit = true,
        UiAction::Chat(text) => {
            if state.busy.is_none() {
                cancel.store(false, Ordering::Relaxed);
                spawn_job(runtime.clone(), Job::Chat(text), tx.clone(), cancel.clone());
            }
        }
        UiAction::AcceptPending(id) => spawn_job(
            runtime.clone(),
            Job::AcceptPending(id),
            tx.clone(),
            cancel.clone(),
        ),
        UiAction::RejectPending(id) => spawn_job(
            runtime.clone(),
            Job::RejectPending(id),
            tx.clone(),
            cancel.clone(),
        ),
        UiAction::ChangePending(id, layer) => spawn_job(
            runtime.clone(),
            Job::ChangePending { id, layer },
            tx.clone(),
            cancel.clone(),
        ),
        UiAction::ClearLayer(layer) => spawn_job(
            runtime.clone(),
            Job::ClearLayer(layer),
            tx.clone(),
            cancel.clone(),
        ),
        UiAction::ImportFacts(path) => spawn_job(
            runtime.clone(),
            Job::ImportFacts(path),
            tx.clone(),
            cancel.clone(),
        ),
        UiAction::Experiment(options) => {
            cancel.store(false, Ordering::Relaxed);
            spawn_job(
                runtime.clone(),
                Job::Experiment(options),
                tx.clone(),
                cancel.clone(),
            )
        }
        UiAction::ArchiveTask => {
            if let Err(error) = controller.archive_task() {
                state.error_modal(error.to_string());
            } else {
                state.refresh(controller);
                state.set_banner("Задача завершена: active → dropped");
            }
        }
        UiAction::AddRecord {
            layer,
            key,
            value,
            kind,
        } => match controller.memory_add(layer, &key, &value, Some(&kind), false) {
            Ok(status) => {
                state.refresh(controller);
                state.set_banner(format!("{status}: {key}"));
            }
            Err(error) => state.error_modal(error.to_string()),
        },
        UiAction::UpdateRecord {
            layer,
            key,
            value,
            kind,
            original_key,
        } => {
            let result: Result<(), AppError> = if key != original_key {
                controller
                    .memory_delete(layer, &original_key)
                    .and_then(|_| {
                        controller
                            .memory_add(layer, &key, &value, Some(&kind), false)
                            .map(|_| ())
                    })
            } else {
                controller
                    .memory_update(layer, &key, &value, Some(&kind))
                    .map(|_| ())
            };
            match result {
                Ok(()) => {
                    state.refresh(controller);
                    state.set_banner(format!("обновлено: {key}"));
                }
                Err(error) => state.error_modal(error.to_string()),
            }
        }
        UiAction::DeleteRecord { layer, key } => match controller.memory_delete(layer, &key) {
            Ok(count) => {
                state.refresh(controller);
                state.set_banner(format!("удалено записей: {count}"));
            }
            Err(error) => state.error_modal(error.to_string()),
        },
        UiAction::MoveRecord { from, key, to } => match controller.memory_move(from, &key, to) {
            Ok(status) => {
                state.refresh(controller);
                state.set_banner(format!("{status}: {key} → {}", to.as_str()));
            }
            Err(error) => state.error_modal(error.to_string()),
        },
        UiAction::Pin { key, pinned } => match controller.memory_pin(&key, pinned) {
            Ok(()) => {
                state.refresh(controller);
                state.set_banner(format!(
                    "{}: {key}",
                    if pinned { "pinned" } else { "unpinned" }
                ));
            }
            Err(error) => state.error_modal(error.to_string()),
        },
        UiAction::EditPending {
            id,
            key,
            value,
            kind,
        } => match controller
            .edit_pending_candidate(&id, &key, &value, &kind)
            .and_then(|_| controller.accept_pending(&id))
        {
            Ok(()) => {
                state.refresh(controller);
                state.set_banner(format!("pending изменён и принят: {key}"));
            }
            Err(error) => state.error_modal(error.to_string()),
        },
        UiAction::SaveReport { md, json } => {
            if let Some(report) = state.experiment.clone() {
                if let Some(parent) = md.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Some(parent) = json.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match controller.save_report(&report, &md, &json) {
                    Ok(()) => {
                        state.set_banner(format!(
                            "Отчёт сохранён: {} и {}",
                            md.display(),
                            json.display()
                        ));
                    }
                    Err(error) => state.error_modal(error.to_string()),
                }
            }
        }
        UiAction::Checkpoint(label) => match controller.checkpoint_branch(&label) {
            Ok(_) => {
                state.refresh(controller);
                state.set_banner(format!("checkpoint: {label}"));
            }
            Err(error) => state.error_modal(error.to_string()),
        },
        UiAction::CheckpointTwo => {
            let mut errors = Vec::new();
            for label in ["ветка A", "ветка B"] {
                if let Err(error) = controller.checkpoint_branch(label) {
                    errors.push(error.to_string());
                }
            }
            state.refresh(controller);
            if errors.is_empty() {
                state.set_banner("созданы две ветки: A и B");
            } else {
                state.error_modal(errors.join("; "));
            }
        }
        UiAction::SwitchBranch(id) => match controller.switch_branch(&id) {
            Ok(()) => {
                state.refresh(controller);
                state.set_banner(format!("переключено на ветку {id}"));
            }
            Err(error) => state.error_modal(error.to_string()),
        },
        UiAction::DeleteBranch(id) => match controller.delete_branch(&id) {
            Ok(()) => {
                state.refresh(controller);
                state.set_banner("ветка удалена");
            }
            Err(error) => state.error_modal(error.to_string()),
        },
        UiAction::SaveSettings => {
            *controller.settings_mut() = state.settings.clone();
            match controller.save_settings() {
                Ok(()) => state.set_banner("Настройки сохранены"),
                Err(error) => state.error_modal(error.to_string()),
            }
        }
        UiAction::ExportHistory(path) => match controller.export_history(&path) {
            Ok(()) => state.set_banner(format!("История экспортирована: {}", path.display())),
            Err(error) => state.error_modal(error.to_string()),
        },
        UiAction::ImportHistory(path) => match controller.import_history(&path) {
            Ok(()) => {
                state.refresh(controller);
                state.set_banner(format!("История импортирована: {}", path.display()));
            }
            Err(error) => state.error_modal(error.to_string()),
        },
        UiAction::ExportApi(path) => {
            match crate::storage::atomic::atomic_write_json(&path, &state.exchanges) {
                Ok(()) => state.set_banner(format!("API-журнал экспортирован: {}", path.display())),
                Err(error) => state.error_modal(error.to_string()),
            }
        }
        UiAction::Error(message) => {
            state.error_modal(message);
        }
    }
    Ok(())
}

fn handle_app_event(
    event: AppEvent,
    runtime: &AppRuntime,
    controller: &mut AppController,
    state: &mut AppState,
) {
    match event {
        AppEvent::JobStarted(label) => {
            state.busy = Some(label);
            state.status = format!("job: {label}");
        }
        AppEvent::ChatProgress(progress) => {
            state.status = format!("chat: {progress:?}");
        }
        AppEvent::MemoryUpdated(bundle) => {
            state.bundle = bundle;
            state.clamp_selections();
        }
        AppEvent::ChatFinished(outcome) => {
            let outcome = *outcome;
            state.record_turn(&outcome);
            state.last_turn = Some(outcome.clone());
            state.status = format!(
                "ответ: {}",
                outcome.final_text.chars().take(80).collect::<String>()
            );
            state.banner = Some((outcome.final_text.clone(), false));
        }
        AppEvent::ExperimentProgress(progress) => {
            state.experiment_progress = Some(progress.clone());
            state.status = format!("{} · {}/{}", progress.config, progress.done, progress.total);
        }
        AppEvent::ExperimentFinished(report) => {
            state.record_experiment(&report);
            state.experiment_progress = None;
            let passed = report.passed();
            state.experiment = Some(report);
            state.status = if passed {
                "Эксперимент: пороги пройдены".into()
            } else {
                "Эксперимент: пороги не пройдены".into()
            };
        }
        AppEvent::MigrationFinished(report) => {
            state.migration = Some(report.clone());
            state.status = format!(
                "import-facts: added={}, updated={}, skipped={}",
                report.added, report.updated, report.skipped
            );
        }
        AppEvent::Error(message) => {
            state.busy = None;
            state.error_modal(message);
        }
        AppEvent::JobFinished => {
            state.busy = None;
            match AppController::load(runtime.clone()) {
                Ok(reloaded) => {
                    *controller = reloaded;
                    state.refresh(controller);
                }
                Err(error) => state.set_banner(format!("Не удалось перечитать слои: {error}")),
            }
        }
    }
}

pub fn render_for_test(frame: &mut ratatui::Frame, state: &mut AppState) {
    draw(frame, state);
}

fn draw(frame: &mut ratatui::Frame, state: &mut AppState) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);

    let busy = state
        .busy
        .map(|label| format!(" [busy: {label}]"))
        .unwrap_or_default();
    let header = format!(
        " lab11 · {} · {}{}",
        state.settings.model,
        state.data_dir.display(),
        busy
    );
    let header = Paragraph::new(Line::from(Span::styled(header, theme::accent())));
    frame.render_widget(header, chunks[0]);

    let mut tabs: Vec<Span> = Vec::new();
    for (index, screen) in Screen::ALL.iter().enumerate() {
        let title = if *screen == Screen::Pending && !state.bundle.pending.is_empty() {
            format!(
                "{} Подтверждения({})",
                index + 1,
                state.bundle.pending.len()
            )
        } else {
            format!("{} {}", index + 1, screen.title())
        };
        let style = if *screen == state.screen {
            theme::selected()
        } else {
            Style::default()
        };
        tabs.push(Span::styled(format!(" {title} "), style));
        tabs.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(tabs)), chunks[1]);

    match state.screen {
        Screen::Memory => widgets::memory::draw(frame, chunks[2], state),
        Screen::Pending => widgets::pending::draw(frame, chunks[2], state),
        Screen::Experiment => widgets::experiment::draw(frame, chunks[2], state),
        Screen::Tokens => widgets::tokens::draw(frame, chunks[2], state),
        Screen::History => widgets::history::draw(frame, chunks[2], state),
        Screen::Settings => widgets::settings::draw(frame, chunks[2], state),
        Screen::ApiLog => widgets::api_log::draw(frame, chunks[2], state),
        Screen::Branches => widgets::branches::draw(frame, chunks[2], state),
    }

    let footer = footer_lines(state);
    let footer = Paragraph::new(footer);
    frame.render_widget(footer, chunks[3]);

    if let Some((message, is_error)) = &state.banner {
        let rect = ratatui::layout::Rect {
            x: 1,
            y: chunks[2].y,
            width: chunks[2].width.saturating_sub(2),
            height: 1,
        };
        let style = if *is_error {
            theme::warn()
        } else {
            theme::accent()
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(
                    " {}",
                    message
                        .chars()
                        .take(rect.width as usize)
                        .collect::<String>()
                ),
                style,
            ))),
            rect,
        );
    }

    widgets::modal::draw(frame, area, state);
    widgets::input::draw(frame, area, state);

    if area.width < 60 || area.height < 20 {
        let rect = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: area.width,
            height: 1,
        };
        let _ = state;
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Узкий терминал: рекомендуем 80×24",
                theme::warn(),
            ))),
            rect,
        );
    }
}

fn footer_lines(state: &AppState) -> Vec<Line<'static>> {
    let context = match state.screen {
        Screen::Memory => "Tab фокус · a/e/d/p/m · c очистка · Enter карточка · / поиск",
        Screen::Pending => "y принять · n отклонить · e изменить · l сменить слой · g/G",
        Screen::Experiment => {
            "r запустить · m режим · Space очистка · s отчёт · d детали · i импорт"
        }
        Screen::Tokens => "↑↓ выбор запроса · Enter детали",
        Screen::History => "Enter сообщение · c очистить STM · e экспорт · i импорт",
        Screen::Settings => "Enter изменить · s сохранить · Tab поле",
        Screen::ApiLog => "f фильтр · Enter детали · e экспорт",
        Screen::Branches => "c checkpoint · b две ветки · Enter переключить · x удалить",
    };
    let second = if let Some(busy) = state.busy {
        format!("{busy}: идёт job · q/Ctrl+C — отмена и выход · ? помощь")
    } else {
        "i написать · ? помощь · q выход".to_string()
    };
    vec![
        Line::from(Span::styled(context.to_string(), theme::muted())),
        Line::from(Span::styled(second, theme::muted())),
    ]
}

pub fn _assert_layers() {
    let _ = (MemoryLayer::Stm, Modal::Error(String::new()));
}
