//! Экран «API»: журнал HTTP-запросов сессии (один вызов модели — строка).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let exchanges = state.filtered_exchanges();
    let errors = exchanges
        .iter()
        .filter(|exchange| exchange.status != "ok")
        .count();
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "Фильтр: {} (f) · запросов: {} · ошибок: {} · Enter — детали · e — экспорт JSON",
            state.api_filter.label(),
            exchanges.len(),
            errors
        ),
        theme::muted(),
    ))];
    lines.push(Line::from(Span::styled(
        "#  время    kind     HTTP  model         prompt/comp/всего  длит.   запрос/ответ",
        theme::muted(),
    )));
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
        let http = exchange
            .status_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| {
                if exchange.status == "ok" {
                    "—".into()
                } else {
                    exchange.status.clone()
                }
            });
        lines.push(Line::from(Span::styled(
            format!(
                "{marker}{:<2} {:<8} {:<8} {:<5} {:<13} {:>6}/{:<5}/{:<6} {:>5}мс  {}/{}",
                position + 1,
                exchange.at.format("%H:%M:%S"),
                exchange.kind.as_str(),
                http.chars().take(5).collect::<String>(),
                exchange.model.chars().take(13).collect::<String>(),
                exchange.prompt_tokens,
                exchange.completion_tokens,
                exchange.total_tokens,
                exchange.duration_ms,
                format_size(exchange.request_size()),
                format_size(exchange.response_size()),
            ),
            style,
        )));
    }
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" API-журнал (сессия) "),
    );
    let inner_height = area.height.saturating_sub(2) as usize;
    let total_lines = 2 + exchanges.len();
    let selected_line = 2 + state.api_selection;
    let max_scroll = total_lines.saturating_sub(inner_height);
    let scroll = if total_lines > inner_height {
        (selected_line + 1)
            .saturating_sub(inner_height)
            .min(max_scroll)
    } else {
        0
    };
    frame.render_widget(paragraph.scroll((scroll as u16, 0)), area);
}

fn format_size(bytes: usize) -> String {
    if bytes >= 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else {
        bytes.to_string()
    }
}
