//! Экран «Ветки».

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut lines = vec![Line::from(Span::styled(
        "c — checkpoint · b — две ветки · Enter — переключиться · x — удалить",
        theme::muted(),
    ))];
    if state.branch_file.branches.is_empty() {
        lines.push(Line::from("Пока нет веток"));
        lines.push(Line::from(
            "Создайте checkpoint, чтобы сравнить варианты памяти",
        ));
    }
    for (index, branch) in state.branch_file.branches.iter().enumerate() {
        let marker = if index == state.branch_selection {
            "> "
        } else {
            "  "
        };
        let active = if state.branch_file.active_branch.as_deref() == Some(branch.id.as_str()) {
            "*"
        } else {
            " "
        };
        let line = format!(
            "{marker}{active} {} · STM {} · WM {} · LTM {} · pending {} · {}",
            branch.label,
            branch.stm.messages.len(),
            branch.wm.items.len(),
            branch.ltm.records.iter().filter(|r| r.is_active()).count(),
            branch.pending.len(),
            branch.created_at.format("%Y-%m-%d %H:%M")
        );
        let style = if index == state.branch_selection {
            theme::selected()
        } else {
            Default::default()
        };
        lines.push(Line::from(Span::styled(line, style)));
    }
    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Ветки (снимок всех слоёв) "),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}
