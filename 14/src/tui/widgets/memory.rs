//! Экран «Память»: три независимые колонки STM / WM / LTM.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::domain::memory::ChatRole;
use crate::memory::tokens::estimate_text_tokens;
use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(34),
            Constraint::Percentage(33),
        ])
        .split(area);
    draw_stm(frame, chunks[0], state);
    draw_wm(frame, chunks[1], state);
    draw_ltm(frame, chunks[2], state);
    state.clamp_selections();
}

fn block(title: String, active: bool) -> Block<'static> {
    let block = Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .title(title);
    if active {
        block.border_style(theme::active_border())
    } else {
        block
    }
}

fn draw_stm(frame: &mut Frame, area: Rect, state: &AppState) {
    let stm = &state.bundle.stm;
    let window = crate::memory::stm::select_stm_window(
        stm,
        state.settings.memory.stm.window,
        state.settings.memory.stm.max_tokens,
    );
    let title = format!(
        " Краткосрочная (STM) · {} сообщений · {} в окне ",
        stm.messages.len(),
        window.indices.len()
    );
    let mut lines: Vec<Line> = Vec::new();
    if stm.messages.is_empty() {
        lines.push(Line::from("Пока пусто"));
        lines.push(Line::from("Нажмите i и напишите агенту"));
    } else {
        lines.push(Line::from(Span::styled(
            if stm.summary.is_some() {
                "── сводка ──"
            } else {
                "── сводка: нет ──"
            },
            theme::muted(),
        )));
        if let Some(summary) = &stm.summary {
            lines.push(Line::from(summary.chars().take(300).collect::<String>()));
        }
        lines.push(Line::from(Span::styled(
            format!("── окно ({}) ──", window.indices.len()),
            theme::muted(),
        )));
        for (position, index) in window.indices.iter().enumerate() {
            let message = &stm.messages[*index];
            let role = match message.role {
                ChatRole::User => "user",
                ChatRole::Assistant => "assistant",
                ChatRole::Tool => "tool",
                ChatRole::System => "system",
            };
            let marker = if state.focus == 0 && state.memory_selection[0] == *index {
                "> "
            } else {
                "  "
            };
            let line = format!("{marker}{role}: {}", message.preview(90));
            let mut style = Style::default();
            if state.focus == 0 && state.memory_selection[0] == *index {
                style = theme::selected();
            }
            lines.push(Line::from(Span::styled(line, style)));
            let _ = position;
        }
    }
    let paragraph = Paragraph::new(lines)
        .block(block(title, state.focus == 0))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn draw_wm(frame: &mut Frame, area: Rect, state: &AppState) {
    let wm = &state.bundle.wm;
    let tokens: usize = estimate_text_tokens(&wm.task_title)
        + wm.items
            .iter()
            .map(|item| estimate_text_tokens(&item.value))
            .sum::<usize>();
    let title = format!(
        " Рабочая (WM) · {} пунктов · ~{} ток. ",
        wm.items.len(),
        tokens
    );
    let mut lines: Vec<Line> = Vec::new();
    if wm.items.is_empty() {
        lines.push(Line::from("Пока пусто"));
        lines.push(Line::from("Текущая задача появится из диалога"));
    } else if !wm.task_title.is_empty() {
        lines.push(Line::from(format!("Задача: {}", wm.task_title)));
    }
    for (index, item) in wm.items.iter().enumerate() {
        let marker = if state.focus == 1 && state.memory_selection[1] == index {
            "> "
        } else {
            "  "
        };
        let status = item.status.as_str();
        let line = format!(
            "{marker}{} [{}] {}: {}",
            item.kind.as_str(),
            status,
            item.key,
            item.value
        );
        let style = if state.focus == 1 && state.memory_selection[1] == index {
            theme::selected()
        } else if item.status != crate::domain::memory::WmStatus::Active {
            theme::muted()
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(line, style)));
    }
    let paragraph = Paragraph::new(lines)
        .block(block(title, state.focus == 1))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn draw_ltm(frame: &mut Frame, area: Rect, state: &AppState) {
    let records = state.visible_ltm();
    let pending = state.bundle.pending.len();
    let mut title = format!(" Долговременная (LTM) · {} записей ", records.len());
    if pending > 0 {
        title.push_str(&format!("· pending {pending} "));
    }
    let mut lines: Vec<Line> = Vec::new();
    if let Some(query) = &state.ltm_search {
        lines.push(Line::from(Span::styled(
            format!("поиск: {query} (Esc — сброс)"),
            theme::accent(),
        )));
    }
    if records.is_empty() {
        lines.push(Line::from("Пока пусто"));
        lines.push(Line::from("import-facts или a — добавить"));
    }
    for (index, record) in records.iter().enumerate() {
        let marker = if state.focus == 2 && state.memory_selection[2] == index {
            "> "
        } else {
            "  "
        };
        let pinned = if record.pinned { "★ " } else { "  " };
        let line = format!(
            "{marker}{pinned}[{}] {}: {}",
            record.kind.as_str(),
            record.key,
            record.value.chars().take(90).collect::<String>()
        );
        let style = if state.focus == 2 && state.memory_selection[2] == index {
            theme::selected()
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(line, style)));
    }
    let paragraph = Paragraph::new(lines)
        .block(block(title, state.focus == 2))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}
