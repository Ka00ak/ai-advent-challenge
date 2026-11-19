mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lab12::app::state::Screen;
use lab12::domain::memory::{MemoryBreakdown, MemoryLayer};
use lab12::experiment::ExperimentProgress;
use lab12::tui::keymap::{UiAction, handle_key};
use lab12::tui::state::{AppState, Modal};
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
    handle_key(&mut state, key(KeyCode::Char('3')));
    assert_eq!(state.screen, Screen::Experiment);
    handle_key(&mut state, key(KeyCode::Char('1')));
    assert_eq!(state.screen, Screen::Memory);
    handle_key(&mut state, key(KeyCode::Char('8')));
    assert_eq!(state.screen, Screen::Branches);
    handle_key(&mut state, key(KeyCode::Char('9')));
    assert_eq!(state.screen, Screen::Profile);
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
    handle_key(&mut state, key(KeyCode::Char('9')));
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
fn experiment_progress_updates_tokens_tab_live() {
    let (_dir, mut state) = state();
    state.screen = Screen::Tokens;
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
