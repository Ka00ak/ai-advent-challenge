//! Экран «0 Чат»: простая переписка с моделью поверх STM/WM/LTM и профиля.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::domain::memory::{ChatMessage, ChatRole};
use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    draw_summary(frame, chunks[0], state);
    draw_messages(frame, chunks[1], state);
    draw_input(frame, chunks[2], state);
}

fn draw_summary(frame: &mut Frame, area: Rect, state: &AppState) {
    let profile = state
        .active_profile()
        .map(|profile| format!("{} · {}", profile.id, profile.summary()))
        .unwrap_or_else(|| "не активен".into());
    let active_ltm = state
        .bundle
        .ltm
        .records
        .iter()
        .filter(|record| record.is_active())
        .count();
    let breakdown = &state.last_breakdown;
    let prompt = if breakdown.total_tokens == 0 {
        "ещё не было запросов".to_string()
    } else {
        format!(
            "STM {} · WM {} · LTM {} · профиль {} · всего {} ток.",
            breakdown.stm_summary_tokens + breakdown.stm_tokens,
            breakdown.wm_tokens,
            breakdown.ltm_profile_tokens + breakdown.ltm_relevant_tokens,
            breakdown.profile_tokens,
            breakdown.total_tokens,
        )
    };
    let lines = vec![
        Line::from(Span::styled(format!("Профиль: {profile}"), theme::accent())),
        Line::from(format!(
            "STM {} сообщ. · окно {} · WM {} · LTM {} · прошлый промпт: {prompt}",
            state.bundle.stm.messages.len(),
            state.settings.memory.stm.window,
            state.bundle.wm.items.len(),
            active_ltm,
        )),
    ];
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" 0 Чат · STM + WM + LTM + профиль "),
    );
    frame.render_widget(paragraph, area);
}

fn draw_messages(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let mut lines = Vec::new();
    for (index, message) in state.bundle.stm.messages.iter().enumerate() {
        append_message(&mut lines, index, message);
    }
    if let Some(pending) = &state.chat_pending {
        lines.push(Line::from(Span::styled("Вы", theme::accent())));
        for part in pending.lines() {
            lines.push(Line::from(format!("  {part}")));
        }
        if state.busy.is_some() {
            lines.push(Line::from(Span::styled("Ассистент", theme::ok())));
            lines.push(Line::from(Span::styled("  …", theme::muted())));
        }
        lines.push(Line::from(""));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Начните диалог: i или Enter — ввод, Enter — отправить.",
            theme::muted(),
        )));
        lines.push(Line::from(Span::styled(
            "К каждому запросу применяются STM, WM, LTM и активный профиль.",
            theme::muted(),
        )));
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let inner_width = area.width.saturating_sub(2);
    let inner_height = area.height.saturating_sub(2) as usize;
    let total_lines = paragraph.line_count(inner_width);
    let max_scroll = total_lines.saturating_sub(inner_height);
    // Храним прокрутку в пределах фактической высоты: иначе после «перелёта»
    // к началу одно нажатие ↓ не сдвигало бы экран.
    let scroll = (state.chat_scroll as usize).min(max_scroll);
    state.chat_scroll = scroll.min(u16::MAX as usize) as u16;
    let offset = max_scroll.saturating_sub(scroll);
    let paragraph = paragraph.block(Block::default().borders(Borders::ALL).title(" Диалог "));
    frame.render_widget(paragraph.scroll((offset as u16, 0)), area);
}

fn append_message(lines: &mut Vec<Line<'static>>, index: usize, message: &ChatMessage) {
    let (role, style) = match message.role {
        ChatRole::User => ("Вы", theme::accent()),
        ChatRole::Assistant => ("Ассистент", theme::ok()),
        ChatRole::Tool => ("Инструмент", theme::muted()),
        // Системные блоки памяти не показываем как реплики диалога.
        ChatRole::System => return,
    };
    let suffix = message
        .name
        .as_deref()
        .map(|name| format!(" · {name}"))
        .unwrap_or_default();
    lines.push(Line::from(Span::styled(
        format!("{role}{suffix} · #{}", index + 1),
        style,
    )));
    let text = message.text();
    if text.trim().is_empty() {
        lines.push(Line::from(Span::styled("  (пусто)", theme::muted())));
    } else {
        for part in text.lines() {
            lines.push(Line::from(format!("  {part}")));
        }
    }
    lines.push(Line::from(""));
}

fn draw_input(frame: &mut Frame, area: Rect, state: &AppState) {
    let length = state.chat_input.buffer.chars().count();
    let title = if state.busy.is_some() {
        " Ассистент отвечает… "
    } else if state.chat_focused {
        " Сообщение · Enter отправить "
    } else {
        " Сообщение "
    };
    let line = if state.chat_focused {
        Line::from(vec![
            Span::styled("> ", theme::accent()),
            Span::raw(state.chat_input.buffer.clone()),
            Span::styled("▏", theme::accent()),
            Span::styled(format!("  {length}/500"), theme::muted()),
        ])
    } else {
        Line::from(Span::styled(
            "> i или Enter — написать модели",
            theme::muted(),
        ))
    };
    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(if state.chat_focused {
                theme::active_border()
            } else {
                Default::default()
            }),
    );
    frame.render_widget(paragraph, area);
}
