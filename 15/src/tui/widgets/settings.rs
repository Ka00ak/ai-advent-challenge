//! Экран «Настройки».

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut lines = vec![Line::from(Span::styled(
        "Enter — изменить · s — сохранить · ключ API задаётся только через env",
        theme::muted(),
    ))];
    for (index, (label, value)) in state.settings_rows().iter().enumerate() {
        let marker = if index == state.settings_selection {
            "> "
        } else {
            "  "
        };
        let style = if index == state.settings_selection {
            theme::selected()
        } else {
            Default::default()
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{label:<24} {value}"),
            style,
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Опасная зона: очистка слоёв доступна на экране «Память» через c.",
        theme::warn(),
    )));
    let key_source = std::env::var("DEEPSEEK_API_KEY")
        .map(|_| "env:DEEPSEEK_API_KEY")
        .or_else(|_| std::env::var("KIMI_API_KEY").map(|_| "env:KIMI_API_KEY"))
        .unwrap_or("не задан");
    lines.push(Line::from(format!(
        "API-ключ: {key_source} · маска: {}",
        mask_source(key_source)
    )));
    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Настройки "))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn mask_source(source: &str) -> &'static str {
    if source == "не задан" {
        "—"
    } else {
        "***"
    }
}
