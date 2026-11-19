//! Экран «История»: полная история STM.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::domain::memory::ChatRole;
use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut lines = vec![Line::from(Span::styled(
        "STM: полная история; окно влияет только на запрос. Enter — сообщение целиком.",
        theme::muted(),
    ))];
    if state.bundle.stm.messages.is_empty() {
        lines.push(Line::from("Пока пусто"));
    }
    for (index, message) in state.bundle.stm.messages.iter().enumerate() {
        let role = match message.role {
            ChatRole::User => "user",
            ChatRole::Assistant => "assistant",
            ChatRole::Tool => "tool",
            ChatRole::System => "system",
        };
        let marker = if index == state.history_selection {
            "> "
        } else {
            "  "
        };
        let style = if index == state.history_selection {
            theme::selected()
        } else {
            Default::default()
        };
        lines.push(Line::from(Span::styled(
            format!(
                "{marker}#{:<3} {:<9} {}",
                index + 1,
                role,
                message.preview(110)
            ),
            style,
        )));
    }
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" История STM "),
    );
    frame.render_widget(paragraph, area);
}
