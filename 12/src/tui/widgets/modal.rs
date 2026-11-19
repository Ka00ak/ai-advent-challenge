//! Модалки поверх экрана.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::tui::state::{AppState, LayerTarget, Modal};
use crate::tui::theme;
use crate::tui::widgets::help::HELP_LINES;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let Some(modal) = &state.modal else {
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
                let marker = if index == *choice { "> " } else { "  " };
                let style = if index == *choice {
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
