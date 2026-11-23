//! Однострочный ввод.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::tui::state::AppState;
use crate::tui::theme;
use crate::tui::widgets::modal::centered;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let Some(input) = &state.input else {
        return;
    };
    let rect = centered(area, 76, 7);
    frame.render_widget(Clear, rect);
    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .margin(1)
        .split(rect);
    let length = input.buffer.chars().count();
    let lines = vec![
        Line::from(Span::styled(
            format!("{} ({length}/500)", input.label),
            theme::accent(),
        )),
        Line::from(format!("{}▏", input.buffer)),
        Line::from(Span::styled(
            "Enter — подтвердить · Esc — отмена · ←/→/Home/End",
            theme::muted(),
        )),
    ];
    let paragraph =
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Ввод "));
    frame.render_widget(paragraph, rect);
    let _ = inner;
}
