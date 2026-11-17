//! Экран «API»: журнал запросов сессии.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "Фильтр: {} (f) · Enter — детали · e — экспорт JSON",
            state.api_filter.label()
        ),
        theme::muted(),
    ))];
    lines.push(Line::from(Span::styled(
        "#  время   kind        model        prompt  comp  cost      статус",
        theme::muted(),
    )));
    let exchanges = state.filtered_exchanges();
    if exchanges.is_empty() {
        lines.push(Line::from("Пока пусто"));
    }
    for (position, exchange) in exchanges.iter().enumerate() {
        let marker = if position == state.api_selection {
            "> "
        } else {
            "  "
        };
        let style = if position == state.api_selection {
            theme::selected()
        } else {
            Default::default()
        };
        lines.push(Line::from(Span::styled(
            format!(
                "{marker}{:<2} {:<8} {:<11} {:<12} {:>6} {:>5}  {:>8.6}  {}",
                position + 1,
                exchange.at.format("%H:%M:%S"),
                exchange.kind.as_str(),
                exchange.model.chars().take(12).collect::<String>(),
                exchange.prompt_tokens,
                exchange.completion_tokens,
                exchange.cost_usd,
                exchange.status
            ),
            style,
        )));
    }
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" API-журнал (сессия) "),
    );
    frame.render_widget(paragraph, area);
}
