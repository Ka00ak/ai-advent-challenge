//! Экран «API»: список HTTP-запросов слева, детали и JSON — справа.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::domain::chat::LlmExchange;
use crate::tui::state::{ApiFocus, AppState};
use crate::tui::theme;
use crate::tui::widgets::json;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    draw_list(frame, chunks[0], state);
    draw_details(frame, chunks[1], state);
}

fn draw_list(frame: &mut Frame, area: Rect, state: &AppState) {
    let exchanges = state.filtered_exchanges();
    let errors = exchanges
        .iter()
        .filter(|exchange| exchange.status != "ok")
        .count();
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "{} (f) · {} · ошибок {}",
            state.api_filter.label(),
            exchanges.len(),
            errors
        ),
        theme::muted(),
    ))];
    lines.push(Line::from(Span::styled(
        "#  вр.    kind   HTTP  P/C",
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
                    exchange.status.chars().take(4).collect()
                }
            });
        lines.push(Line::from(Span::styled(
            format!(
                "{marker}{:<2} {:<5} {:<6} {:<4} {:>5}/{:<4}",
                position + 1,
                exchange.at.format("%H:%M"),
                exchange.kind.as_str(),
                http.chars().take(4).collect::<String>(),
                exchange.prompt_tokens,
                exchange.completion_tokens,
            ),
            style,
        )));
    }
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" HTTP-запросы ")
            .border_style(if state.api_focus == ApiFocus::List {
                theme::active_border()
            } else {
                Default::default()
            }),
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

fn draw_details(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let selected = state.selected_exchange();
    let summary = selected
        .map(summary_lines)
        .unwrap_or_else(|| vec![Line::from(Span::styled("Выберите запрос", theme::muted()))]);
    let body = selected
        .map(|exchange| {
            if state.api_body_tab == crate::tui::state::ApiBodyTab::Request {
                json::body_lines(
                    exchange.request_body.as_deref(),
                    "Тело запроса не сохранено.",
                )
            } else {
                json::body_lines(
                    exchange.response_body.as_deref(),
                    "Тело ответа не сохранено.",
                )
            }
        })
        .unwrap_or_else(|| vec![Line::from("—")]);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(area);

    let summary = Paragraph::new(summary).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Детали запроса "),
    );
    frame.render_widget(summary, right[0]);

    let mut tabs = Vec::new();
    for tab in [
        crate::tui::state::ApiBodyTab::Request,
        crate::tui::state::ApiBodyTab::Response,
    ] {
        let style = if tab == state.api_body_tab {
            theme::selected()
        } else {
            theme::muted()
        };
        tabs.push(Span::styled(format!(" {} ", tab.title()), style));
        tabs.push(Span::raw(" "));
    }
    tabs.push(Span::styled(
        "Tab — фокус · ←→ запрос/ответ",
        theme::muted(),
    ));
    frame.render_widget(Paragraph::new(Line::from(tabs)), right[1]);

    let body_paragraph = Paragraph::new(body).wrap(Wrap { trim: false });
    let inner_width = right[2].width.saturating_sub(2);
    let total = body_paragraph.line_count(inner_width);
    let max_scroll = total.saturating_sub(right[2].height.saturating_sub(2) as usize);
    let scroll = (state.api_json_scroll as usize).min(max_scroll);
    state.api_json_scroll = scroll.min(u16::MAX as usize) as u16;
    let body_paragraph = body_paragraph.block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", state.api_body_tab.title()))
            .border_style(if state.api_focus == ApiFocus::Body {
                theme::active_border()
            } else {
                Default::default()
            }),
    );
    frame.render_widget(body_paragraph.scroll((scroll as u16, 0)), right[2]);

    let hint = if state.api_focus == ApiFocus::Body {
        "↑↓/PgUp/PgDn прокрутка JSON · Home/End · Tab/Esc к списку · ←→ запрос/ответ"
    } else {
        "↑↓ выбор запроса · Tab к телу · ←→ запрос/ответ · f фильтр · e экспорт"
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(hint, theme::muted()))),
        right[3],
    );
}

fn summary_lines(exchange: &LlmExchange) -> Vec<Line<'static>> {
    let http = exchange
        .status_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "—".into());
    let mut lines = vec![
        Line::from(vec![
            Span::styled(exchange.kind.as_str(), theme::accent()),
            Span::raw(format!(
                " · {} · HTTP {http} · {} мс",
                exchange.model, exchange.duration_ms
            )),
        ]),
        Line::from(format!("{} {}", exchange.method, exchange.url)),
        Line::from(format!(
            "prompt {} · completion {} · всего {} · ${:.6}",
            exchange.prompt_tokens,
            exchange.completion_tokens,
            exchange.total_tokens,
            exchange.cost_usd
        )),
        Line::from(format!(
            "размеры: запрос {} Б · ответ {} Б · статус {}",
            exchange.request_size(),
            exchange.response_size(),
            exchange.status
        )),
    ];
    if let Some(memory) = &exchange.memory {
        lines.push(Line::from(format!(
            "system {} · профиль {} · LTM {} · WM {}",
            memory.system_tokens,
            memory.profile_tokens,
            memory.ltm_profile_tokens + memory.ltm_relevant_tokens,
            memory.wm_tokens
        )));
        lines.push(Line::from(format!(
            "STM {} · вопрос {} · tools {} · слои L/W/S {}/{}/{}",
            memory.stm_summary_tokens + memory.stm_tokens,
            memory.current_tokens,
            memory.tools_tokens,
            memory.layer_delta.ltm,
            memory.layer_delta.wm,
            memory.layer_delta.stm
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Breakdown по блокам недоступен",
            theme::muted(),
        )));
    }
    if let Some(error) = &exchange.error {
        lines.push(Line::from(Span::styled(
            format!("ошибка: {error}"),
            theme::warn(),
        )));
    }
    lines
}
