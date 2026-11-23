//! Экран «Подтверждения».

use chrono::{Duration, Utc};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let pending = &state.bundle.pending;
    let title = format!(" Ждут подтверждения ({}) ", pending.len());
    let mut lines: Vec<Line> = Vec::new();
    if pending.is_empty() {
        lines.push(Line::from("Все записи подтверждены"));
        lines.push(Line::from(Span::styled(
            "Новые устойчивые факты появятся здесь после реплики",
            theme::muted(),
        )));
    }
    for (index, item) in pending.iter().enumerate() {
        let marker = if state.pending_selection == index {
            "> "
        } else {
            "  "
        };
        let stale = Utc::now() - item.created_at > Duration::days(7);
        let stale_badge = if stale {
            format!(
                " · stale ({} дн.)",
                (Utc::now() - item.created_at).num_days()
            )
        } else {
            String::new()
        };
        let line = format!(
            "{marker}{} · {} · {}{}",
            item.candidate.key,
            item.target_kind.as_str(),
            item.rule_id,
            stale_badge
        );
        let style = if state.pending_selection == index {
            theme::selected()
        } else {
            Default::default()
        };
        lines.push(Line::from(Span::styled(line, style)));
        lines.push(Line::from(format!(
            "    Было: {} → Станет: {}",
            item.old_value.as_deref().unwrap_or("—"),
            item.candidate.value
        )));
        lines.push(Line::from(""));
    }
    if let Some(item) = pending.get(state.pending_selection) {
        lines.push(Line::from(Span::styled(
            format!(
                "Детали: source=router, message #{}, reason=\"{}\"",
                item.message_index + 1,
                item.reason
            ),
            theme::muted(),
        )));
    }
    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}
