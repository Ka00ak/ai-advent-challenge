//! Модалки поверх экрана.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::domain::chat::LlmExchange;
use crate::tui::state::{ApiDetailTab, AppState, LayerTarget, Modal};
use crate::tui::theme;
use crate::tui::widgets::help::HELP_LINES;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let Some(modal) = state.modal.clone() else {
        return;
    };
    match modal {
        Modal::Confirm { title, message, .. } => {
            let rect = centered(area, 60, 9);
            frame.render_widget(Clear, rect);
            let lines = vec![
                Line::from(message.as_str()),
                Line::from(""),
                Line::from(Span::styled(
                    "[y] подтвердить   [n/Esc] отмена",
                    theme::accent(),
                )),
            ];
            let paragraph = Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!(" {title} ")),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(paragraph, rect);
        }
        Modal::Error(message) => {
            let rect = centered(area, 70, 10);
            frame.render_widget(Clear, rect);
            let lines = vec![
                Line::from(Span::styled("Ошибка", theme::warn())),
                Line::from(""),
                Line::from(message.as_str()),
                Line::from(""),
                Line::from("Enter/Esc — закрыть"),
            ];
            let paragraph = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title(" Сообщение "))
                .wrap(Wrap { trim: true });
            frame.render_widget(paragraph, rect);
        }
        Modal::Help => {
            let rect = centered(area, 74, 28);
            frame.render_widget(Clear, rect);
            let lines: Vec<Line> = HELP_LINES.iter().map(|line| Line::from(*line)).collect();
            let paragraph = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title(" Помощь · ? "))
                .scroll((state.help_scroll, 0));
            frame.render_widget(paragraph, rect);
        }
        Modal::Card { title, lines } | Modal::Details { title, lines } => {
            let height = (lines.len() as u16 + 4).clamp(8, area.height.saturating_sub(2));
            let rect = centered(area, 78, height);
            frame.render_widget(Clear, rect);
            let mut all: Vec<Line> = lines.iter().map(|line| Line::from(line.as_str())).collect();
            all.push(Line::from(""));
            all.push(Line::from(Span::styled(
                "Enter/Esc — закрыть",
                theme::muted(),
            )));
            let paragraph = Paragraph::new(all)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!(" {title} ")),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(paragraph, rect);
        }
        Modal::ApiDetail {
            exchange,
            tab,
            scroll,
        } => draw_api_detail(frame, area, state, &exchange, tab, scroll),
        Modal::SelectLayer { target, choice } => {
            let rect = centered(area, 60, 9);
            frame.render_widget(Clear, rect);
            let target_label = match target {
                LayerTarget::MoveRecord { from, key } => {
                    format!("Перенести {} · {}", from.as_str(), key)
                }
                LayerTarget::Pending(id) => format!("Сменить слой pending {id}"),
            };
            let layers = ["LTM — долговременная", "WM — рабочая"];
            let mut lines = vec![Line::from(target_label), Line::from("")];
            for (index, label) in layers.iter().enumerate() {
                let marker = if index == choice { "> " } else { "  " };
                let style = if index == choice {
                    theme::selected()
                } else {
                    Default::default()
                };
                lines.push(Line::from(Span::styled(format!("{marker}{label}"), style)));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "↑/↓ выбор · Enter применить · Esc отмена",
                theme::muted(),
            )));
            let paragraph = Paragraph::new(lines)
                .block(Block::default().borders(Borders::ALL).title(" Выбор слоя "));
            frame.render_widget(paragraph, rect);
        }
    }
}

fn draw_api_detail(
    frame: &mut Frame,
    area: Rect,
    state: &mut AppState,
    exchange: &std::sync::Arc<LlmExchange>,
    tab: ApiDetailTab,
    scroll: u16,
) {
    let entry = exchange.as_ref();
    let rect = centered(
        area,
        area.width.saturating_sub(4).min(110),
        area.height.saturating_sub(2),
    );
    frame.render_widget(Clear, rect);
    let status = entry
        .status_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "—".into());
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" API · {} · HTTP {status} ", entry.kind.as_str()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let mut tabs = Vec::new();
    for candidate in [
        ApiDetailTab::Overview,
        ApiDetailTab::Request,
        ApiDetailTab::Response,
    ] {
        let style = if candidate == tab {
            theme::selected()
        } else {
            theme::muted()
        };
        tabs.push(Span::styled(format!(" {} ", candidate.title()), style));
        tabs.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(tabs)), parts[0]);

    let body = Paragraph::new(api_detail_lines(entry, tab)).wrap(Wrap { trim: false });
    let total = body.line_count(parts[1].width);
    let max_scroll = total.saturating_sub(parts[1].height as usize);
    let clamped = (scroll as usize).min(max_scroll);
    if clamped as u16 != scroll {
        state.modal = Some(Modal::ApiDetail {
            exchange: std::sync::Arc::clone(exchange),
            tab,
            scroll: clamped as u16,
        });
    }
    frame.render_widget(body.scroll((clamped as u16, 0)), parts[1]);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Tab/←→ раздел · ↑↓/PgUp/PgDn прокрутка · g/G · Enter/Esc закрыть",
            theme::muted(),
        ))),
        parts[2],
    );
}

fn api_detail_lines(exchange: &LlmExchange, tab: ApiDetailTab) -> Vec<Line<'static>> {
    match tab {
        ApiDetailTab::Overview => {
            let mut rows = vec![
                format!("ID: {}", exchange.id),
                format!("Запрос: {} {}", exchange.method, exchange.url),
                format!(
                    "Тип: {} · модель: {}",
                    exchange.kind.as_str(),
                    exchange.model
                ),
                format!("Время: {}", exchange.at.to_rfc3339()),
                format!("Длительность: {} мс", exchange.duration_ms),
                format!(
                    "HTTP-статус: {}",
                    exchange
                        .status_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "—".into())
                ),
                format!("Статус: {}", exchange.status),
            ];
            if let Some(error) = &exchange.error {
                rows.push(format!("Ошибка: {error}"));
            }
            rows.push(format!(
                "Токены: prompt {} · completion {} · всего {}",
                exchange.prompt_tokens, exchange.completion_tokens, exchange.total_tokens
            ));
            rows.push(format!("Стоимость: ${:.6}", exchange.cost_usd));
            rows.push(format!(
                "Размеры: запрос {} Б · ответ {} Б",
                exchange.request_size(),
                exchange.response_size()
            ));
            if let Some(memory) = &exchange.memory {
                rows.push(format!(
                    "Breakdown: system {} · профиль {} · LTM {} · WM {} · STM {} · tools {} · всего {}",
                    memory.system_tokens,
                    memory.profile_tokens,
                    memory.ltm_profile_tokens + memory.ltm_relevant_tokens,
                    memory.wm_tokens,
                    memory.stm_summary_tokens + memory.stm_tokens,
                    memory.tools_tokens,
                    memory.total_tokens
                ));
            }
            if let Some(preview) = &exchange.request_preview {
                rows.push(format!("Превью запроса: {preview}"));
            }
            if let Some(preview) = &exchange.response_preview {
                rows.push(format!("Превью ответа: {preview}"));
            }
            rows.into_iter().map(Line::from).collect()
        }
        ApiDetailTab::Request => body_lines(
            exchange.request_body.as_deref(),
            "Тело запроса не сохранено (запись до включения подробного журнала).",
        ),
        ApiDetailTab::Response => body_lines(
            exchange.response_body.as_deref(),
            "Тело ответа не сохранено (запись до включения подробного журнала).",
        ),
    }
}

fn body_lines(body: Option<&str>, empty: &str) -> Vec<Line<'static>> {
    match body.filter(|text| !text.trim().is_empty()) {
        Some(text) => text
            .lines()
            .map(|line| Line::from(line.to_string()))
            .collect(),
        None => vec![Line::from(empty.to_string())],
    }
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(20);
    let height = height.min(area.height.saturating_sub(2)).max(5);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((area.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length((area.width.saturating_sub(width)) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vertical[1]);
    horizontal[1]
}
