//! Экран «Токены»: breakdown последнего запроса и таблица сессии.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([Constraint::Length(12), Constraint::Min(5)])
        .split(area);
    let b = &state.last_breakdown;
    let max = [
        b.system_tokens,
        b.ltm_profile_tokens,
        b.ltm_relevant_tokens,
        b.wm_tokens,
        b.stm_summary_tokens,
        b.stm_tokens,
        b.current_tokens,
        b.tools_tokens,
    ]
    .into_iter()
    .max()
    .unwrap_or(1)
    .max(1);
    let rows = [
        ("system", b.system_tokens),
        ("LTM профиль", b.ltm_profile_tokens),
        ("LTM релевант.", b.ltm_relevant_tokens),
        ("WM", b.wm_tokens),
        ("STM сводка", b.stm_summary_tokens),
        ("STM окно", b.stm_tokens),
        ("текущий вопрос", b.current_tokens),
        ("tools", b.tools_tokens),
    ];
    let mut lines = Vec::new();
    if b.total_tokens == 0 {
        lines.push(Line::from("Пока нет запросов"));
    }
    for (label, value) in rows {
        lines.push(Line::from(format!(
            "{:<16} {} {:>6}",
            label,
            bar(value, max, 22),
            value
        )));
    }
    if b.total_tokens > 0 {
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "Итого {} · слои: LTM {} · WM {} · STM {}",
            b.total_tokens, b.layer_delta.ltm, b.layer_delta.wm, b.layer_delta.stm
        )));
    }
    let breakdown = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Промпт последнего запроса "),
    );
    frame.render_widget(breakdown, chunks[0]);

    let mut table = vec![Line::from(Span::styled(
        "#  время   kind        prompt  completion  total  cost      слои",
        theme::muted(),
    ))];
    if state.exchanges.is_empty() {
        table.push(Line::from("Пока нет запросов"));
    }
    for (index, exchange) in state.exchanges.iter().enumerate().rev().take(30) {
        let style = if index == state.tokens_selection {
            theme::selected()
        } else {
            Default::default()
        };
        let layers = exchange
            .memory
            .as_ref()
            .map(|b| {
                format!(
                    "l{}/w{}/s{}",
                    b.layer_delta.ltm, b.layer_delta.wm, b.layer_delta.stm
                )
            })
            .unwrap_or_else(|| "—".into());
        table.push(Line::from(Span::styled(
            format!(
                "{:<2} {:<8} {:<11} {:>6} {:>11} {:>6}  {:>8.6}  {}",
                index + 1,
                exchange.at.format("%H:%M:%S"),
                exchange.kind.as_str(),
                exchange.prompt_tokens,
                exchange.completion_tokens,
                exchange.total_tokens,
                exchange.cost_usd,
                layers
            ),
            style,
        )));
    }
    let table = Paragraph::new(table).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Журнал токенов сессии "),
    );
    frame.render_widget(table, chunks[1]);
}

fn bar(value: usize, max: usize, width: usize) -> String {
    if value == 0 {
        return " ".repeat(width);
    }
    let filled = (value as f64 / max as f64 * width as f64).ceil() as usize;
    format!(
        "{}{}",
        "█".repeat(filled.min(width)),
        "·".repeat(width.saturating_sub(filled))
    )
}
