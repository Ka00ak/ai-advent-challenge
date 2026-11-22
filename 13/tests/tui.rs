mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lab12::app::state::Screen;
use lab12::domain::memory::{MemoryBreakdown, MemoryLayer};
use lab12::domain::task::{TaskStage, TaskState, TaskStatus};
use lab12::experiment::ExperimentProgress;
use lab12::experiment::personalization::{
    PersonalizationOptions, SAME_QUESTION, run_personalization,
};
use lab12::tui::keymap::{UiAction, handle_key};
use lab12::tui::state::{AppState, InputPurpose, Modal, VerifyMode};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn state() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let controller = common::seeded_controller(dir.path());
    let state = AppState::new(&controller);
    (dir, state)
}

#[test]
fn number_keys_switch_screens() {
    let (_dir, mut state) = state();
    handle_key(&mut state, key(KeyCode::Char('1')));
    assert_eq!(state.screen, Screen::Chat);
    handle_key(&mut state, key(KeyCode::Char('4')));
    assert_eq!(state.screen, Screen::Experiment);
    handle_key(&mut state, key(KeyCode::Char('2')));
    assert_eq!(state.screen, Screen::Memory);
    handle_key(&mut state, key(KeyCode::Char('5')));
    assert_eq!(state.screen, Screen::Settings);
    handle_key(&mut state, key(KeyCode::Char('8')));
    assert_eq!(state.screen, Screen::Task);
    handle_key(&mut state, key(KeyCode::Char('6')));
    assert_eq!(state.screen, Screen::ApiLog);
    handle_key(&mut state, key(KeyCode::Char('7')));
    assert_eq!(state.screen, Screen::Profile);
}

#[test]
fn chat_has_stm_history_actions() {
    let (_dir, mut state) = state();
    state.screen = Screen::Chat;
    assert!(matches!(
        handle_key(&mut state, key(KeyCode::Char('c'))),
        UiAction::None
    ));
    assert!(matches!(state.modal, Some(Modal::Confirm { .. })));
    handle_key(&mut state, key(KeyCode::Esc));

    handle_key(&mut state, key(KeyCode::Char('e')));
    let input = state.input.as_ref().unwrap();
    assert_eq!(
        input.purpose,
        lab12::tui::state::InputPurpose::ExportHistory
    );
    handle_key(&mut state, key(KeyCode::Esc));

    handle_key(&mut state, key(KeyCode::Char('I')));
    let input = state.input.as_ref().unwrap();
    assert_eq!(
        input.purpose,
        lab12::tui::state::InputPurpose::ImportHistory
    );
}

#[test]
fn chat_tab_collects_input_and_returns_chat_action() {
    let (_dir, mut state) = state();
    handle_key(&mut state, key(KeyCode::Char('1')));
    assert_eq!(state.screen, Screen::Chat);

    handle_key(&mut state, key(KeyCode::Enter));
    assert!(state.chat_focused);
    for ch in "Привет".chars() {
        handle_key(&mut state, key(KeyCode::Char(ch)));
    }
    assert_eq!(state.chat_input.buffer, "Привет");

    let action = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(action, UiAction::Chat("Привет".into()));
    assert!(state.chat_input.buffer.is_empty());
    assert_eq!(state.chat_pending.as_deref(), Some("Привет"));

    // В режиме ввода q — обычный символ, а не выход.
    handle_key(&mut state, key(KeyCode::Char('q')));
    assert!(!state.should_quit);
    assert_eq!(state.chat_input.buffer, "q");
    handle_key(&mut state, key(KeyCode::Esc));
    assert!(!state.chat_focused);
}

#[test]
fn chat_tab_shows_profile_memory_and_dialog() {
    let (_dir, mut state) = state();
    state.screen = Screen::Chat;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
    assert!(text.contains("1 Чат"));
    assert!(text.contains("Профиль:"));
    assert!(text.contains("STM + WM + LTM + профиль"));
    assert!(text.contains("Диалог"));
    assert!(text.contains("полная история STM"));
}

#[test]
fn chat_tab_renders_last_turn_with_layers_and_answer() {
    let (_dir, mut state) = state();
    state.screen = Screen::Chat;
    let mut memory = state.bundle.clone();
    memory
        .stm
        .messages
        .push(lab12::domain::memory::ChatMessage::assistant(
            "Ответ модели",
        ));
    state.bundle = memory.clone();
    let outcome = lab12::domain::chat::TurnOutcome {
        final_text: "Ответ модели".into(),
        memory,
        messages: Vec::new(),
        breakdown: MemoryBreakdown {
            profile_tokens: 7,
            ltm_profile_tokens: 3,
            ltm_relevant_tokens: 5,
            wm_tokens: 11,
            stm_summary_tokens: 2,
            stm_tokens: 13,
            total_tokens: 123,
            ..MemoryBreakdown::default()
        },
        token_stats: Default::default(),
        audit: Vec::new(),
        profile_id: Some("brief".into()),
        profile_changes: Vec::new(),
    };
    state.record_turn(&outcome);

    let backend = TestBackend::new(120, 36);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let text: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
    assert!(text.contains("Ответ модели"));
    assert!(text.contains("профиль 7"));
    assert!(text.contains("всего 123 ток."));
}

#[test]
fn chat_scroll_clamps_and_moves_back_down() {
    let (_dir, mut state) = state();
    state.screen = Screen::Chat;
    state.chat_scroll = u16::MAX;
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
    let top = state.chat_scroll;
    assert!(top > 0, "история должна быть длиннее окна");
    handle_key(&mut state, key(KeyCode::Down));
    assert_eq!(state.chat_scroll, top - 1);
}

#[test]
fn api_screen_shows_details_and_highlighted_request_without_modal() {
    let (_dir, mut state) = state();
    state.screen = Screen::ApiLog;
    let mut first = lab12::domain::chat::LlmExchange::new(
        lab12::domain::chat::ExchangeKind::Chat,
        "deepseek-chat",
        "https://api.deepseek.com/chat/completions",
    );
    first.status_code = Some(200);
    first.duration_ms = 42;
    first.request_body = Some(
        "{\n  \"messages\": [\n    {\"role\": \"user\", \"content\": \"привет API\"}\n  ]\n}"
            .into(),
    );
    first.response_body = Some({
        let mut body = String::from("{\n  \"content\": \"ответ API\",\n  \"data\": [\n");
        for index in 0..60 {
            body.push_str(&format!("    {index},\n"));
        }
        body.push_str("  ]\n}");
        body
    });
    first.memory = Some(MemoryBreakdown {
        system_tokens: 100,
        profile_tokens: 20,
        ltm_profile_tokens: 30,
        ltm_relevant_tokens: 10,
        wm_tokens: 15,
        stm_tokens: 25,
        current_tokens: 5,
        tools_tokens: 7,
        total_tokens: 212,
        ..MemoryBreakdown::default()
    });
    let mut second = lab12::domain::chat::LlmExchange::new(
        lab12::domain::chat::ExchangeKind::Chat,
        "deepseek-chat",
        "https://api.deepseek.com/chat/completions",
    );
    second.status_code = Some(200);
    second.duration_ms = 30;
    second.request_body = Some("{\n  \"content\": \"второй запрос\"\n}".into());
    second.response_body = Some("{\n  \"content\": \"второй ответ\"\n}".into());
    state.merge_exchanges(vec![first, second]);
    state.api_selection = 0;

    let backend = TestBackend::new(140, 36);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Детали запроса"));
    assert!(text.contains("привет API"));
    assert!(text.contains("system 100"));
    assert!(text.contains("tools 7"));
    assert!(state.modal.is_none(), "детали показываются без модалки");

    handle_key(&mut state, key(KeyCode::Right));
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("ответ API"));

    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(
        state.api_focus,
        lab12::tui::state::ApiFocus::Body,
        "Tab переводит фокус в тело"
    );
    handle_key(&mut state, key(KeyCode::PageDown));
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
    assert!(
        state.api_json_scroll > 0,
        "JSON прокручивается стрелками/PageDown"
    );

    handle_key(&mut state, key(KeyCode::Esc));
    assert_eq!(state.api_focus, lab12::tui::state::ApiFocus::List);
    handle_key(&mut state, key(KeyCode::Down));
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("второй ответ"));
    assert!(state.modal.is_none());
}

#[test]
fn api_list_g_g_and_long_log_render_without_losing_selection() {
    let (_dir, mut state) = state();
    state.screen = Screen::ApiLog;
    let entries = (0..40)
        .map(|index| {
            let mut exchange = lab12::domain::chat::LlmExchange::new(
                lab12::domain::chat::ExchangeKind::Chat,
                "deepseek-chat",
                "https://api.deepseek.com/chat/completions",
            );
            exchange.request_body = Some(format!("{{\"index\":{index}}}"));
            exchange.response_body = Some(format!("{{\"answer\":{index}}}"));
            exchange
        })
        .collect();
    state.merge_exchanges(entries);

    handle_key(&mut state, key(KeyCode::Char('G')));
    assert_eq!(state.api_selection, 39);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("40"));

    handle_key(&mut state, key(KeyCode::Char('g')));
    assert_eq!(state.api_selection, 0);
}

#[test]
fn experiment_summary_is_replaced_by_individual_api_requests() {
    let (_dir, mut state) = state();
    state.record_experiment_progress(&experiment_progress(1, 100));
    assert_eq!(state.exchanges.len(), 1);
    let summary_id = state.exchanges[0].id.clone();

    let entries = (0..3)
        .map(|index| {
            let mut exchange = lab12::domain::chat::LlmExchange::new(
                lab12::domain::chat::ExchangeKind::Experiment,
                "deepseek-chat",
                "https://api.deepseek.com/chat/completions",
            );
            exchange.request_body = Some(format!("{{\"probe\":{index}}}"));
            exchange.response_body = Some(format!("{{\"answer\":{index}}}"));
            exchange
        })
        .collect();
    state.merge_exchanges(entries);

    assert_eq!(state.exchanges.len(), 3);
    assert!(
        !state
            .exchanges
            .iter()
            .any(|exchange| exchange.id == summary_id)
    );
}

#[test]
fn busy_chat_does_not_send_second_message() {
    let (_dir, mut state) = state();
    state.screen = Screen::Chat;
    state.chat_focused = true;
    state.chat_input.insert('A');
    state.busy = Some("chat");
    let action = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(action, UiAction::None);
    assert_eq!(state.chat_input.buffer, "A");
    assert!(state.banner.is_some());
}

#[test]
fn tab_changes_memory_focus() {
    let (_dir, mut state) = state();
    assert_eq!(state.focus, 0);
    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(state.focus, 1);
    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(state.focus, 2);
    handle_key(&mut state, key(KeyCode::Tab));
    assert_eq!(state.focus, 0);
    handle_key(&mut state, key(KeyCode::BackTab));
    assert_eq!(state.focus, 2);
}

#[test]
fn pending_y_and_n_return_actions() {
    let (_dir, mut state) = state();
    state.screen = Screen::Pending;
    let id = state.bundle.pending[0].id.clone();
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('y'))),
        UiAction::AcceptPending(id.clone())
    );
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Char('n'))),
        UiAction::RejectPending(id)
    );
}

#[test]
fn clear_requires_confirmation() {
    let (_dir, mut state) = state();
    let action = handle_key(&mut state, key(KeyCode::Char('c')));
    assert_eq!(action, UiAction::None);
    assert!(matches!(state.modal, Some(Modal::Confirm { .. })));
    let action = handle_key(&mut state, key(KeyCode::Esc));
    assert_eq!(action, UiAction::None);
    assert!(state.modal.is_none());
    assert!(!state.bundle.stm.messages.is_empty());

    handle_key(&mut state, key(KeyCode::Char('c')));
    let action = handle_key(&mut state, key(KeyCode::Char('y')));
    assert_eq!(action, UiAction::ClearLayer(MemoryLayer::Stm));
}

#[test]
fn busy_blocks_destructive_actions() {
    let (_dir, mut state) = state();
    state.busy = Some("chat");
    let action = handle_key(&mut state, key(KeyCode::Char('d')));
    assert!(matches!(action, UiAction::Error(_)));
    let action = handle_key(&mut state, key(KeyCode::Char('c')));
    assert!(matches!(action, UiAction::Error(_)));
    assert!(state.modal.is_none());
}

#[test]
fn reducer_opens_help_and_card() {
    let (_dir, mut state) = state();
    handle_key(&mut state, key(KeyCode::Char('?')));
    assert!(matches!(state.modal, Some(Modal::Help)));
    handle_key(&mut state, key(KeyCode::Esc));
    state.focus = 2;
    handle_key(&mut state, key(KeyCode::Enter));
    assert!(matches!(state.modal, Some(Modal::Card { .. })));
}

#[test]
fn renders_in_80x24_and_120x36() {
    for (width, height) in [(80u16, 24u16), (120, 36)] {
        let (_dir, mut state) = state();
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
            .unwrap();
        assert_eq!(terminal.backend().buffer().area.width, width);
        assert_eq!(terminal.backend().buffer().area.height, height);
    }
}

#[test]
fn task_tab_accepts_goal_pauses_and_renders_route() {
    let (_dir, mut state) = state();
    handle_key(&mut state, key(KeyCode::Char('8')));
    assert_eq!(state.screen, Screen::Task);

    handle_key(&mut state, key(KeyCode::Enter));
    assert!(state.task_focused);
    for ch in "Подготовить отчёт".chars() {
        handle_key(&mut state, key(KeyCode::Char(ch)));
    }
    assert_eq!(state.task_input.buffer, "Подготовить отчёт");
    let action = handle_key(&mut state, key(KeyCode::Enter));
    assert_eq!(action, UiAction::Task("Подготовить отчёт".into()));
    assert!(!state.task_focused);
    assert!(state.task_input.buffer.is_empty());

    state.begin_task("Подготовить отчёт");
    state.busy = Some("task");
    assert_eq!(state.task.stage, TaskStage::Planning);
    assert_eq!(state.task.status, TaskStatus::Running);

    let action = handle_key(&mut state, key(KeyCode::Char('p')));
    assert_eq!(action, UiAction::ToggleTaskPause);
    state.task_paused = true;
    state.task.mark_paused(chrono::Utc::now());

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("planning"));
    assert!(text.contains("execution"));
    assert!(text.contains("validation"));
    assert!(text.contains("done"));
    assert!(text.contains("Диалог задачи"));
    assert!(text.contains("контекст сохраняется"));
    assert!(text.contains("Вы"));
    assert!(text.contains("пауза"));
    assert!(text.contains("Текущий шаг"));

    state.task = TaskState::start("Проверить смету", chrono::Utc::now());
    state.task.stage = TaskStage::Validation;
    state.task.status = TaskStatus::Running;
    state.busy = Some("task");
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Валидация"));
}

#[test]
fn task_screen_scrolls_details() {
    let (_dir, mut state) = state();
    state.screen = Screen::Task;
    assert_eq!(state.task_scroll, 0);
    handle_key(&mut state, key(KeyCode::Up));
    assert_eq!(state.task_scroll, 1);
    handle_key(&mut state, key(KeyCode::PageUp));
    assert_eq!(state.task_scroll, 11);
    handle_key(&mut state, key(KeyCode::Down));
    assert_eq!(state.task_scroll, 10);
    handle_key(&mut state, key(KeyCode::PageDown));
    assert_eq!(state.task_scroll, 0);
    handle_key(&mut state, key(KeyCode::Char('g')));
    assert_eq!(state.task_scroll, u16::MAX);
    handle_key(&mut state, key(KeyCode::Char('G')));
    assert_eq!(state.task_scroll, 0);
}

#[test]
fn task_reset_requires_confirmation_and_returns_action() {
    let (_dir, mut state) = state();
    state.screen = Screen::Task;
    state.begin_task("Подготовить отчёт");
    assert!(!state.task.messages.is_empty());

    let action = handle_key(&mut state, key(KeyCode::Char('r')));
    assert_eq!(action, UiAction::None);
    assert!(matches!(state.modal, Some(Modal::Confirm { .. })));

    let action = handle_key(&mut state, key(KeyCode::Char('y')));
    assert_eq!(action, UiAction::ResetTask);
    assert!(state.modal.is_none());
}

#[test]
fn every_screen_renders_without_panic() {
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let (_dir, mut state) = state();
    for screen in Screen::ALL {
        state.screen = screen;
        terminal
            .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
            .unwrap();
    }
}

#[test]
fn profile_screen_lists_and_activates_profiles() {
    let (_dir, mut state) = state();
    handle_key(&mut state, key(KeyCode::Char('7')));
    assert_eq!(state.screen, Screen::Profile);
    assert!(state.profiles.profiles.len() >= 2);
    handle_key(&mut state, key(KeyCode::Down));
    let selected = state.selected_profile_id().unwrap();
    assert_eq!(
        handle_key(&mut state, key(KeyCode::Enter)),
        UiAction::UseProfile(selected)
    );
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
}

fn experiment_progress(done: usize, prompt_tokens: usize) -> ExperimentProgress {
    ExperimentProgress {
        config: "stm".into(),
        probe: done,
        probes_total: 18,
        done,
        total: 126,
        prompt_tokens,
        completion_tokens: 10,
        cost_usd: 0.001,
        usage_reported: true,
        errors: 0,
        last_probe: None,
        last_matched: None,
        elapsed_ms: 0,
        breakdown: Some(MemoryBreakdown {
            system_tokens: 10,
            total_tokens: 20,
            ..MemoryBreakdown::default()
        }),
    }
}

#[test]
fn experiment_progress_updates_api_tokens_live() {
    let (_dir, mut state) = state();
    state.screen = Screen::ApiLog;
    state.record_experiment_progress(&experiment_progress(1, 100));
    assert_eq!(state.exchanges.len(), 1);
    let id = state.exchanges[0].id.clone();
    assert_eq!(state.exchanges[0].status, "running");
    assert_eq!(state.exchanges[0].total_tokens, 110);
    assert_eq!(state.last_breakdown.system_tokens, 10);

    state.record_experiment_progress(&experiment_progress(2, 200));
    assert_eq!(
        state.exchanges.len(),
        1,
        "живая запись обновляется, а не дублируется"
    );
    assert_eq!(state.exchanges[0].id, id);
    assert_eq!(state.exchanges[0].prompt_tokens, 200);
    assert_eq!(state.exchanges[0].total_tokens, 210);

    state.fail_experiment_progress();
    assert_eq!(state.exchanges[0].status, "error");
    state.record_experiment_progress(&experiment_progress(1, 50));
    assert_eq!(state.exchanges.len(), 2, "новый прогон — новая запись");

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
}

#[test]
fn settings_validation_and_rows() {
    let (_dir, mut state) = state();
    assert_eq!(state.settings_rows().len(), 13);
    assert!(state.apply_setting(7, "3").is_err());
    assert!(state.apply_setting(7, "10").is_ok());
    assert_eq!(state.settings.memory.stm.window, 10);
    assert!(state.apply_setting(6, "auto").is_ok());
    assert_eq!(
        state.settings.memory.approval,
        lab12::domain::settings::Approval::Auto
    );
}

#[test]
fn search_filters_ltm() {
    let (_dir, mut state) = state();
    state.ltm_search = Some("postgres".into());
    let visible = state.visible_ltm();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].key, "decision.database");
}

#[test]
fn first_frame_and_key_reaction_are_fast() {
    let (_dir, mut state) = state();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    let start = std::time::Instant::now();
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
    assert!(start.elapsed() < std::time::Duration::from_millis(300));
    let start = std::time::Instant::now();
    handle_key(&mut state, key(KeyCode::Tab));
    assert!(start.elapsed() < std::time::Duration::from_millis(100));
}

#[test]
fn experiment_tab_switches_to_profile_verification_mode() {
    let (_dir, mut state) = state();
    state.screen = Screen::Experiment;
    assert_eq!(state.verify_mode, VerifyMode::Ablation);

    let action = handle_key(&mut state, key(KeyCode::Char('p')));
    assert_eq!(action, UiAction::None);
    assert_eq!(state.verify_mode, VerifyMode::Profiles);

    let action = handle_key(&mut state, key(KeyCode::Char('r')));
    assert_eq!(
        action,
        UiAction::Personalization(PersonalizationOptions {
            question: Some(SAME_QUESTION.into()),
            profile_ids: Vec::new(),
            include_chain: true,
        })
    );

    handle_key(&mut state, key(KeyCode::Char(' ')));
    assert!(!state.personalization_chain);
    let action = handle_key(&mut state, key(KeyCode::Char('r')));
    assert!(matches!(
        action,
        UiAction::Personalization(PersonalizationOptions {
            include_chain: false,
            ..
        })
    ));

    handle_key(&mut state, key(KeyCode::Char('e')));
    assert_eq!(
        state.input.as_ref().unwrap().purpose,
        InputPurpose::PersonalizationQuestion
    );
    let input = state.input.as_mut().unwrap();
    input.buffer = "Как устроен Result?".into();
    input.cursor = input.buffer.chars().count();
    handle_key(&mut state, key(KeyCode::Enter));
    assert!(state.input.is_none());
    assert_eq!(state.personalization_question(), "Как устроен Result?");

    let action = handle_key(&mut state, key(KeyCode::Char('p')));
    assert_eq!(action, UiAction::None);
    assert_eq!(state.verify_mode, VerifyMode::Ablation);
}

#[test]
fn profile_verification_renders_scenario_and_results() {
    let (_dir, mut state) = state();
    state.screen = Screen::Experiment;
    state.verify_mode = VerifyMode::Profiles;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Проверка профилей"));
    assert!(text.contains("brief"));
    assert!(text.contains("detailed"));
    assert!(text.contains("default"));
    assert!(text.contains("Цепочка:"));

    let cancel = std::sync::atomic::AtomicBool::new(false);
    let report = run_personalization(
        &lab12::domain::profile::ProfileState::with_presets(chrono::Utc::now()),
        &common::test_settings(),
        &lab12::llm::client::FakeChatClient,
        &PersonalizationOptions::default(),
        &cancel,
        &mut |_| {},
    )
    .unwrap();
    state.personalization = Some(report);

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| lab12::tui::render_for_test(frame, &mut state))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Соблюдено ответов: 30 из 30"));
    assert!(text.contains("профилей полностью: 3 из 3"));
    assert!(text.contains("первое:"));
}
