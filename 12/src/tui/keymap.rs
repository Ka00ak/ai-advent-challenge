//! Reducer клавиш: чистые переходы состояния и команды приложению.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::state::Screen;
use crate::domain::memory::MemoryLayer;
use crate::experiment::runner::{ExperimentMode, ExperimentOptions};
use crate::tui::state::{AppState, ConfirmAction, FormStep, InputPurpose, LayerTarget, Modal};

#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    None,
    Quit,
    Chat(String),
    AcceptPending(String),
    RejectPending(String),
    ChangePending(String, MemoryLayer),
    EditPending {
        id: String,
        key: String,
        value: String,
        kind: String,
    },
    ClearLayer(MemoryLayer),
    ArchiveTask,
    AddRecord {
        layer: MemoryLayer,
        key: String,
        value: String,
        kind: String,
    },
    UpdateRecord {
        layer: MemoryLayer,
        key: String,
        value: String,
        kind: String,
        original_key: String,
    },
    DeleteRecord {
        layer: MemoryLayer,
        key: String,
    },
    MoveRecord {
        from: MemoryLayer,
        key: String,
        to: MemoryLayer,
    },
    Pin {
        key: String,
        pinned: bool,
    },
    ImportFacts(PathBuf),
    Experiment(ExperimentOptions),
    SaveReport {
        md: PathBuf,
        json: PathBuf,
    },
    SaveSettings,
    ExportHistory(PathBuf),
    ImportHistory(PathBuf),
    ExportApi(PathBuf),
    UseProfile(String),
    Error(String),
}

pub fn handle_key(state: &mut AppState, key: KeyEvent) -> UiAction {
    if key.kind != crossterm::event::KeyEventKind::Press {
        return UiAction::None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return UiAction::Quit;
    }
    if state.input.is_some() {
        return handle_input_key(state, key);
    }
    if state.modal.is_some() {
        return handle_modal_key(state, key);
    }
    // В режиме ввода чата печатные клавиши принадлежат сообщению, а не
    // глобальным горячим клавишам (q, 1…8, ?).
    if state.screen == Screen::Chat && state.chat_focused {
        return handle_chat_input(state, key);
    }

    match key.code {
        KeyCode::Char('?') => {
            state.modal = Some(Modal::Help);
            UiAction::None
        }
        KeyCode::Char('q') => UiAction::Quit,
        KeyCode::Esc => {
            state.clear_banner();
            state.ltm_search = None;
            UiAction::None
        }
        KeyCode::Char('i') => {
            if state.screen == Screen::Chat {
                state.chat_focused = true;
            } else {
                state.open_input(InputPurpose::Chat, "Сообщение агенту", "");
            }
            UiAction::None
        }
        KeyCode::Tab => {
            if state.screen == Screen::Chat {
                state.chat_focused = true;
            } else if state.screen == Screen::Memory {
                state.toggle_focus(false);
            } else if state.screen == Screen::Settings {
                state.settings_selection =
                    (state.settings_selection + 1) % state.settings_rows().len();
            } else {
                state.tokens_selection = (state.tokens_selection + 1) % 2;
            }
            UiAction::None
        }
        KeyCode::BackTab => {
            if state.screen == Screen::Memory {
                state.toggle_focus(true);
            } else if state.screen == Screen::Settings {
                let len = state.settings_rows().len();
                state.settings_selection = (state.settings_selection + len - 1) % len;
            }
            UiAction::None
        }
        KeyCode::Char(ch @ '0'..='9') => {
            if let Some(number) = ch.to_digit(10) {
                if let Some(screen) = Screen::from_number(number as usize) {
                    state.screen = screen;
                    state.chat_focused = false;
                }
            }
            UiAction::None
        }
        KeyCode::Up => {
            if state.screen == Screen::Chat {
                return handle_chat(state, key);
            }
            move_selection(state, -1);
            UiAction::None
        }
        KeyCode::Down => {
            if state.screen == Screen::Chat {
                return handle_chat(state, key);
            }
            move_selection(state, 1);
            UiAction::None
        }
        KeyCode::PageUp => {
            if state.screen == Screen::Chat {
                return handle_chat(state, key);
            }
            move_selection(state, -10);
            UiAction::None
        }
        KeyCode::PageDown => {
            if state.screen == Screen::Chat {
                return handle_chat(state, key);
            }
            move_selection(state, 10);
            UiAction::None
        }
        _ => match state.screen {
            Screen::Chat => handle_chat(state, key),
            Screen::Memory => handle_memory(state, key),
            Screen::Pending => handle_pending(state, key),
            Screen::Experiment => handle_experiment(state, key),
            Screen::Tokens => handle_tokens(state, key),
            Screen::Settings => handle_settings(state, key),
            Screen::ApiLog => handle_api(state, key),
            Screen::Profile => handle_profile(state, key),
        },
    }
}

fn handle_input_key(state: &mut AppState, key: KeyEvent) -> UiAction {
    let Some(input) = state.input.as_mut() else {
        return UiAction::None;
    };
    match key.code {
        KeyCode::Esc => {
            let purpose = input.purpose;
            state.input = None;
            if matches!(
                purpose,
                InputPurpose::FormKey | InputPurpose::FormValue | InputPurpose::FormKind
            ) {
                state.form = None;
            }
            UiAction::None
        }
        KeyCode::Enter => {
            let input = state.input.take();
            match input {
                Some(input) => submit_input(state, input.purpose, &input.buffer),
                None => UiAction::None,
            }
        }
        KeyCode::Backspace => {
            input.backspace();
            UiAction::None
        }
        KeyCode::Delete => {
            input.delete();
            UiAction::None
        }
        KeyCode::Left => {
            input.left();
            UiAction::None
        }
        KeyCode::Right => {
            input.right();
            UiAction::None
        }
        KeyCode::Home => {
            input.home();
            UiAction::None
        }
        KeyCode::End => {
            input.end();
            UiAction::None
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            input.insert(ch);
            UiAction::None
        }
        _ => UiAction::None,
    }
}

fn submit_input(state: &mut AppState, purpose: InputPurpose, text: &str) -> UiAction {
    match purpose {
        InputPurpose::Chat => {
            let text = text.trim().to_string();
            if text.is_empty() {
                UiAction::None
            } else {
                UiAction::Chat(text)
            }
        }
        InputPurpose::SearchLtm => {
            let query = text.trim();
            state.ltm_search = if query.is_empty() {
                None
            } else {
                Some(query.to_string())
            };
            state.memory_selection[2] = 0;
            UiAction::None
        }
        InputPurpose::FormKey => {
            let key = text.trim().to_string();
            if key.is_empty() {
                state.form = None;
                return UiAction::Error("key не может быть пустым".into());
            }
            let Some(form) = state.form.as_mut() else {
                return UiAction::Error("форма потеряна".into());
            };
            form.key = key;
            form.step = FormStep::Value;
            let value = form.value.clone();
            state.open_input(InputPurpose::FormValue, "Значение", value);
            UiAction::None
        }
        InputPurpose::FormValue => {
            let value = text.trim().to_string();
            if value.is_empty() {
                state.form = None;
                return UiAction::Error("value не может быть пустым".into());
            }
            let Some(form) = state.form.as_mut() else {
                return UiAction::Error("форма потеряна".into());
            };
            form.value = value;
            form.step = FormStep::Kind;
            let kind = form.kind.clone();
            state.open_input(InputPurpose::FormKind, "Тип (Enter — оставить)", kind);
            UiAction::None
        }
        InputPurpose::FormKind => {
            let Some(mut form) = state.form.take() else {
                return UiAction::Error("форма потеряна".into());
            };
            let kind = text.trim();
            if !kind.is_empty() {
                form.kind = kind.to_string();
            }
            if let Some(pending_id) = form.pending_id {
                return UiAction::EditPending {
                    id: pending_id,
                    key: form.key,
                    value: form.value,
                    kind: form.kind,
                };
            }
            if let Some(original_key) = form.original_key {
                UiAction::UpdateRecord {
                    layer: form.layer,
                    key: form.key,
                    value: form.value,
                    kind: form.kind,
                    original_key,
                }
            } else {
                UiAction::AddRecord {
                    layer: form.layer,
                    key: form.key,
                    value: form.value,
                    kind: form.kind,
                }
            }
        }
        InputPurpose::ImportFacts => UiAction::ImportFacts(PathBuf::from(text.trim())),
        InputPurpose::ExportHistory => UiAction::ExportHistory(PathBuf::from(text.trim())),
        InputPurpose::ImportHistory => UiAction::ImportHistory(PathBuf::from(text.trim())),
        InputPurpose::SaveReport => {
            let md = PathBuf::from(text.trim());
            let json = md.with_extension("json");
            UiAction::SaveReport { md, json }
        }
        InputPurpose::ExportApi => UiAction::ExportApi(PathBuf::from(text.trim())),
        InputPurpose::Setting(index) => match state.apply_setting(index, text) {
            Ok(()) => {
                state.status = "Настройка изменена, нажмите s для сохранения".into();
                UiAction::None
            }
            Err(error) => UiAction::Error(error),
        },
    }
}

fn handle_modal_key(state: &mut AppState, key: KeyEvent) -> UiAction {
    let Some(modal) = state.modal.clone() else {
        return UiAction::None;
    };
    match modal {
        Modal::Confirm { action, .. } => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                state.modal = None;
                resolve_confirm(action)
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                state.modal = None;
                UiAction::None
            }
            _ => UiAction::None,
        },
        Modal::SelectLayer { target, choice } => match key.code {
            KeyCode::Up | KeyCode::Down | KeyCode::Tab => {
                state.modal = Some(Modal::SelectLayer {
                    target,
                    choice: 1 - choice,
                });
                UiAction::None
            }
            KeyCode::Enter => {
                state.modal = None;
                let layer = if choice == 0 {
                    MemoryLayer::Ltm
                } else {
                    MemoryLayer::Wm
                };
                match target {
                    LayerTarget::MoveRecord { from, key } => {
                        if from == layer {
                            state.set_banner("Слой назначения совпадает с исходным");
                            UiAction::None
                        } else {
                            UiAction::MoveRecord {
                                from,
                                key,
                                to: layer,
                            }
                        }
                    }
                    LayerTarget::Pending(id) => UiAction::ChangePending(id, layer),
                }
            }
            KeyCode::Esc => {
                state.modal = None;
                UiAction::None
            }
            _ => UiAction::None,
        },
        Modal::Help => {
            match key.code {
                KeyCode::Down => state.help_scroll = state.help_scroll.saturating_add(1),
                KeyCode::Up => state.help_scroll = state.help_scroll.saturating_sub(1),
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') => state.modal = None,
                _ => {}
            }
            UiAction::None
        }
        Modal::ApiDetail {
            exchange,
            tab,
            scroll,
        } => {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => state.modal = None,
                KeyCode::Tab | KeyCode::Right => {
                    state.modal = Some(Modal::ApiDetail {
                        exchange,
                        tab: tab.next(),
                        scroll: 0,
                    });
                }
                KeyCode::BackTab | KeyCode::Left => {
                    state.modal = Some(Modal::ApiDetail {
                        exchange,
                        tab: tab.prev(),
                        scroll: 0,
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.modal = Some(Modal::ApiDetail {
                        exchange,
                        tab,
                        scroll: scroll.saturating_add(1),
                    });
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.modal = Some(Modal::ApiDetail {
                        exchange,
                        tab,
                        scroll: scroll.saturating_sub(1),
                    });
                }
                KeyCode::PageDown => {
                    state.modal = Some(Modal::ApiDetail {
                        exchange,
                        tab,
                        scroll: scroll.saturating_add(15),
                    });
                }
                KeyCode::PageUp => {
                    state.modal = Some(Modal::ApiDetail {
                        exchange,
                        tab,
                        scroll: scroll.saturating_sub(15),
                    });
                }
                KeyCode::Char('g') => {
                    state.modal = Some(Modal::ApiDetail {
                        exchange,
                        tab,
                        scroll: 0,
                    });
                }
                KeyCode::Char('G') => {
                    state.modal = Some(Modal::ApiDetail {
                        exchange,
                        tab,
                        scroll: u16::MAX,
                    });
                }
                _ => {}
            }
            UiAction::None
        }
        Modal::Error(_) | Modal::Card { .. } | Modal::Details { .. } => {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                state.modal = None;
            }
            UiAction::None
        }
    }
}

fn resolve_confirm(action: ConfirmAction) -> UiAction {
    match action {
        ConfirmAction::ClearLayer(layer) => UiAction::ClearLayer(layer),
        ConfirmAction::ArchiveTask => UiAction::ArchiveTask,
        ConfirmAction::DeleteRecord { layer, key } => UiAction::DeleteRecord { layer, key },
    }
}

fn move_selection(state: &mut AppState, delta: isize) {
    let apply = |value: &mut usize, len: usize, delta: isize| {
        if len == 0 {
            *value = 0;
            return;
        }
        let next = *value as isize + delta;
        *value = next.clamp(0, len as isize - 1) as usize;
    };
    let ltm_len = state.visible_ltm().len();
    let experiment_len = state
        .experiment
        .as_ref()
        .map(|r| r.configs.len())
        .unwrap_or(0);
    let api_len = state.filtered_exchanges().len();
    let settings_len = state.settings_rows().len();
    match state.screen {
        Screen::Chat => {}
        Screen::Memory => match state.focus {
            0 => apply(
                &mut state.memory_selection[0],
                state.bundle.stm.messages.len(),
                delta,
            ),
            1 => apply(
                &mut state.memory_selection[1],
                state.bundle.wm.items.len(),
                delta,
            ),
            _ => apply(&mut state.memory_selection[2], ltm_len, delta),
        },
        Screen::Pending => apply(
            &mut state.pending_selection,
            state.bundle.pending.len(),
            delta,
        ),
        Screen::Experiment => apply(&mut state.experiment_selection, experiment_len, delta),
        Screen::Tokens => apply(&mut state.tokens_selection, state.exchanges.len(), delta),
        Screen::Settings => apply(&mut state.settings_selection, settings_len, delta),
        Screen::ApiLog => apply(&mut state.api_selection, api_len, delta),
        Screen::Profile => apply(
            &mut state.profile_selection,
            state.profiles.profiles.len(),
            delta,
        ),
    }
}

fn handle_chat(state: &mut AppState, key: KeyEvent) -> UiAction {
    match key.code {
        KeyCode::Enter | KeyCode::Char('i') => {
            state.chat_focused = true;
            UiAction::None
        }
        KeyCode::Up => {
            state.chat_scroll = state.chat_scroll.saturating_add(1);
            UiAction::None
        }
        KeyCode::Down => {
            state.chat_scroll = state.chat_scroll.saturating_sub(1);
            UiAction::None
        }
        KeyCode::PageUp => {
            state.chat_scroll = state.chat_scroll.saturating_add(10);
            UiAction::None
        }
        KeyCode::PageDown => {
            state.chat_scroll = state.chat_scroll.saturating_sub(10);
            UiAction::None
        }
        KeyCode::Char('g') => {
            state.chat_scroll = u16::MAX;
            UiAction::None
        }
        KeyCode::Char('G') => {
            state.chat_scroll = 0;
            UiAction::None
        }
        KeyCode::Char('c') => {
            if state.busy.is_some() {
                return UiAction::Error("идёт job — действия заблокированы".into());
            }
            state.confirm(
                "Очистить STM?",
                layer_clear_warning(MemoryLayer::Stm),
                ConfirmAction::ClearLayer(MemoryLayer::Stm),
            );
            UiAction::None
        }
        KeyCode::Char('e') => {
            if state.busy.is_some() {
                return UiAction::Error("идёт job — действия заблокированы".into());
            }
            state.open_input(
                InputPurpose::ExportHistory,
                "Экспорт истории STM",
                "history.json",
            );
            UiAction::None
        }
        KeyCode::Char('I') => {
            if state.busy.is_some() {
                return UiAction::Error("идёт job — действия заблокированы".into());
            }
            state.open_input(
                InputPurpose::ImportHistory,
                "Импорт истории STM",
                "history.json",
            );
            UiAction::None
        }
        _ => UiAction::None,
    }
}

fn handle_chat_input(state: &mut AppState, key: KeyEvent) -> UiAction {
    match key.code {
        KeyCode::Esc => {
            state.chat_focused = false;
            state.chat_scroll = 0;
            UiAction::None
        }
        KeyCode::Enter => send_chat_message(state),
        KeyCode::Up => {
            state.chat_scroll = state.chat_scroll.saturating_add(1);
            UiAction::None
        }
        KeyCode::Down => {
            state.chat_scroll = state.chat_scroll.saturating_sub(1);
            UiAction::None
        }
        KeyCode::PageUp => {
            state.chat_scroll = state.chat_scroll.saturating_add(10);
            UiAction::None
        }
        KeyCode::PageDown => {
            state.chat_scroll = state.chat_scroll.saturating_sub(10);
            UiAction::None
        }
        KeyCode::Backspace => {
            state.chat_input.backspace();
            UiAction::None
        }
        KeyCode::Delete => {
            state.chat_input.delete();
            UiAction::None
        }
        KeyCode::Left => {
            state.chat_input.left();
            UiAction::None
        }
        KeyCode::Right => {
            state.chat_input.right();
            UiAction::None
        }
        KeyCode::Home => {
            state.chat_input.home();
            UiAction::None
        }
        KeyCode::End => {
            state.chat_input.end();
            UiAction::None
        }
        KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.chat_input.insert(ch);
            UiAction::None
        }
        _ => UiAction::None,
    }
}

fn send_chat_message(state: &mut AppState) -> UiAction {
    if state.busy.is_some() {
        state.set_banner("Дождитесь ответа модели");
        return UiAction::None;
    }
    let text = state.chat_input.buffer.trim().to_string();
    if text.is_empty() {
        return UiAction::None;
    }
    state.chat_input.buffer.clear();
    state.chat_input.cursor = 0;
    state.chat_scroll = 0;
    state.chat_pending = Some(text.clone());
    UiAction::Chat(text)
}

fn handle_memory(state: &mut AppState, key: KeyEvent) -> UiAction {
    match key.code {
        KeyCode::Enter => {
            state.open_card_for_record();
            UiAction::None
        }
        KeyCode::Char('/') => {
            state.focus = 2;
            let value = state.ltm_search.clone().unwrap_or_default();
            state.open_input(InputPurpose::SearchLtm, "Поиск по LTM", value);
            UiAction::None
        }
        KeyCode::Char('g') => {
            match state.focus {
                0 => state.memory_selection[0] = 0,
                1 => state.memory_selection[1] = 0,
                _ => state.memory_selection[2] = 0,
            }
            UiAction::None
        }
        KeyCode::Char('G') => {
            match state.focus {
                0 => state.memory_selection[0] = state.bundle.stm.messages.len().saturating_sub(1),
                1 => state.memory_selection[1] = state.bundle.wm.items.len().saturating_sub(1),
                _ => state.memory_selection[2] = state.visible_ltm().len().saturating_sub(1),
            }
            UiAction::None
        }
        KeyCode::Char('a') => {
            if state.busy.is_some() {
                return UiAction::Error("идёт job — действия заблокированы".into());
            }
            state.start_add(state.memory_column().layer());
            UiAction::None
        }
        KeyCode::Char('e') => {
            if state.busy.is_some() {
                return UiAction::Error("идёт job — действия заблокированы".into());
            }
            state.start_edit();
            UiAction::None
        }
        KeyCode::Char('p') => {
            if state.focus != 2 {
                return UiAction::None;
            }
            if state.busy.is_some() {
                return UiAction::Error("идёт job — действия заблокированы".into());
            }
            if let Some(record) = state.selected_ltm() {
                return UiAction::Pin {
                    key: record.key.clone(),
                    pinned: !record.pinned,
                };
            }
            UiAction::None
        }
        KeyCode::Char('d') => {
            if state.busy.is_some() {
                return UiAction::Error("идёт job — действия заблокированы".into());
            }
            let layer = state.memory_column().layer();
            let key = match layer {
                MemoryLayer::Stm => Some(state.memory_selection[0].to_string()),
                MemoryLayer::Wm => state.selected_wm().map(|item| item.key.clone()),
                MemoryLayer::Ltm => state.selected_ltm().map(|record| record.key.clone()),
            };
            match key {
                Some(key) => {
                    state.confirm(
                        "Удалить запись?",
                        format!("{} · {key}", layer.as_str()),
                        ConfirmAction::DeleteRecord { layer, key },
                    );
                    UiAction::None
                }
                None => UiAction::None,
            }
        }
        KeyCode::Char('m') => {
            if state.busy.is_some() {
                return UiAction::Error("идёт job — действия заблокированы".into());
            }
            let target = match state.memory_column().layer() {
                MemoryLayer::Wm => state.selected_wm().map(|item| LayerTarget::MoveRecord {
                    from: MemoryLayer::Wm,
                    key: item.key.clone(),
                }),
                MemoryLayer::Ltm => state.selected_ltm().map(|record| LayerTarget::MoveRecord {
                    from: MemoryLayer::Ltm,
                    key: record.key.clone(),
                }),
                MemoryLayer::Stm => None,
            };
            if let Some(target) = target {
                state.modal = Some(Modal::SelectLayer { target, choice: 0 });
            }
            UiAction::None
        }
        KeyCode::Char('c') => {
            if state.busy.is_some() {
                return UiAction::Error("идёт job — действия заблокированы".into());
            }
            let layer = state.memory_column().layer();
            let (title, message, action): (&str, String, ConfirmAction) = match layer {
                MemoryLayer::Wm => (
                    "Завершить задачу?",
                    "Активные пункты станут dropped; done останутся для истории.".to_string(),
                    ConfirmAction::ArchiveTask,
                ),
                _ => (
                    "Очистить слой?",
                    layer_clear_warning(layer),
                    ConfirmAction::ClearLayer(layer),
                ),
            };
            state.confirm(title, message, action);
            UiAction::None
        }
        _ => UiAction::None,
    }
}

fn layer_clear_warning(layer: MemoryLayer) -> String {
    match layer {
        MemoryLayer::Stm => {
            "Будут удалены все сообщения и сводка STM. WM и LTM останутся.".to_string()
        }
        MemoryLayer::Wm => "Будут удалены пункты WM. STM и LTM останутся.".to_string(),
        MemoryLayer::Ltm => {
            "Будут удалены записи LTM и очередь pending. STM и WM останутся.".to_string()
        }
    }
}

fn handle_pending(state: &mut AppState, key: KeyEvent) -> UiAction {
    match key.code {
        KeyCode::Char('y') => state
            .pending_selected_id()
            .map(UiAction::AcceptPending)
            .unwrap_or(UiAction::None),
        KeyCode::Char('n') => state
            .pending_selected_id()
            .map(UiAction::RejectPending)
            .unwrap_or(UiAction::None),
        KeyCode::Char('e') => {
            state.start_edit_pending();
            UiAction::None
        }
        KeyCode::Char('l') => {
            if let Some(id) = state.pending_selected_id() {
                state.modal = Some(Modal::SelectLayer {
                    target: LayerTarget::Pending(id),
                    choice: 0,
                });
            }
            UiAction::None
        }
        KeyCode::Char('g') => {
            state.pending_selection = 0;
            UiAction::None
        }
        KeyCode::Char('G') => {
            state.pending_selection = state.bundle.pending.len().saturating_sub(1);
            UiAction::None
        }
        _ => UiAction::None,
    }
}

fn handle_experiment(state: &mut AppState, key: KeyEvent) -> UiAction {
    match key.code {
        KeyCode::Char(' ') => {
            state.experiment_cleanup = !state.experiment_cleanup;
            UiAction::None
        }
        KeyCode::Char('m') => {
            state.experiment_quick = !state.experiment_quick;
            UiAction::None
        }
        KeyCode::Char('r') => {
            if state.busy.is_some() {
                UiAction::Error("эксперимент уже идёт".into())
            } else {
                UiAction::Experiment(ExperimentOptions {
                    mode: if state.experiment_quick {
                        ExperimentMode::Quick
                    } else {
                        ExperimentMode::Full
                    },
                    include_cleanup: state.experiment_cleanup,
                })
            }
        }
        KeyCode::Char('s') => {
            if state.experiment.is_none() {
                UiAction::Error("сначала запустите эксперимент".into())
            } else {
                let (md, _json) = state.report_paths();
                state.open_input(
                    InputPurpose::SaveReport,
                    "Путь Markdown-отчёта",
                    md.display().to_string(),
                );
                UiAction::None
            }
        }
        KeyCode::Char('d') => {
            if let Some(report) = &state.experiment {
                let mut lines = Vec::new();
                for config in report.configs.iter().chain(report.cleanup.iter()) {
                    for result in config.results.iter().filter(|result| !result.matched) {
                        lines.push(format!(
                            "{} · {} · {} → {}",
                            result.id,
                            config.id,
                            result.question,
                            if result.answer.is_empty() {
                                "—"
                            } else {
                                &result.answer
                            }
                        ));
                    }
                }
                if lines.is_empty() {
                    lines.push("Все вопросы отвечены".into());
                }
                state.modal = Some(Modal::Details {
                    title: "Детали probe".into(),
                    lines,
                });
            }
            UiAction::None
        }
        KeyCode::Char('i') => {
            state.open_input(
                InputPurpose::ImportFacts,
                "Путь к facts Дня 10",
                "fixtures/day10-facts.json",
            );
            UiAction::None
        }
        _ => UiAction::None,
    }
}

fn handle_tokens(state: &mut AppState, key: KeyEvent) -> UiAction {
    if key.code == KeyCode::Enter {
        if let Some(exchange) = state.exchanges.get(state.tokens_selection) {
            let mut lines = vec![
                format!("kind: {}", exchange.kind.as_str()),
                format!("model: {}", exchange.model),
                format!("url: {}", exchange.url),
                format!(
                    "tokens: prompt={} completion={} total={}",
                    exchange.prompt_tokens, exchange.completion_tokens, exchange.total_tokens
                ),
                format!(
                    "cost: {:.6}$ · status: {}",
                    exchange.cost_usd, exchange.status
                ),
            ];
            if let Some(breakdown) = &exchange.memory {
                lines.push(format!("LTM: {}", breakdown.layer_delta.ltm));
                lines.push(format!("WM: {}", breakdown.layer_delta.wm));
                lines.push(format!("STM: {}", breakdown.layer_delta.stm));
                lines.push(format!("included LTM: {:?}", breakdown.included_ltm_ids));
                lines.push(format!("dropped LTM: {:?}", breakdown.dropped_ltm_ids));
            }
            state.modal = Some(Modal::Details {
                title: format!("Запрос #{}", state.tokens_selection + 1),
                lines,
            });
        }
    }
    UiAction::None
}

fn handle_settings(state: &mut AppState, key: KeyEvent) -> UiAction {
    match key.code {
        KeyCode::Enter => {
            state.init_input_for_setting();
            UiAction::None
        }
        KeyCode::Char('s') => UiAction::SaveSettings,
        _ => UiAction::None,
    }
}

fn handle_api(state: &mut AppState, key: KeyEvent) -> UiAction {
    match key.code {
        KeyCode::Char('f') => {
            state.api_filter = state.api_filter.next();
            state.api_selection = 0;
            UiAction::None
        }
        KeyCode::Char('g') => {
            state.api_selection = 0;
            UiAction::None
        }
        KeyCode::Char('G') => {
            state.api_selection = state.filtered_exchanges().len().saturating_sub(1);
            UiAction::None
        }
        KeyCode::Char('e') => {
            state.open_input(
                InputPurpose::ExportApi,
                "Экспорт API-журнала",
                "api-log.json",
            );
            UiAction::None
        }
        KeyCode::Enter => {
            if let Some(exchange) = state
                .filtered_exchanges()
                .get(state.api_selection)
                .map(|exchange| (*exchange).clone())
            {
                state.modal = Some(Modal::ApiDetail {
                    exchange: std::sync::Arc::new(exchange),
                    tab: crate::tui::state::ApiDetailTab::Overview,
                    scroll: 0,
                });
            }
            UiAction::None
        }
        _ => UiAction::None,
    }
}

fn handle_profile(state: &mut AppState, key: KeyEvent) -> UiAction {
    match key.code {
        KeyCode::Enter | KeyCode::Char('u') => state
            .selected_profile_id()
            .map(UiAction::UseProfile)
            .unwrap_or(UiAction::None),
        KeyCode::Char('g') => {
            state.profile_selection = 0;
            UiAction::None
        }
        KeyCode::Char('G') => {
            state.profile_selection = state.profiles.profiles.len().saturating_sub(1);
            UiAction::None
        }
        _ => UiAction::None,
    }
}
