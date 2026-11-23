//! Экран «9 Инварианты»: список, детали, история изменений и аудит попыток.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(7),
            Constraint::Length(8),
        ])
        .split(area);

    draw_summary(frame, chunks[0], state);
    draw_list(frame, chunks[1], state);
    draw_details(frame, chunks[2], state);
}

fn draw_summary(frame: &mut Frame, area: Rect, state: &AppState) {
    let active = state.invariants.active().count();
    let total = state.invariants.invariants.len();
    let attempts = state.invariant_audit.attempt_count();
    let blocked = state.invariant_audit.blocked_count();
    let probes = state.invariant_audit.probe_count();
    let lines = vec![
        Line::from(Span::styled(
            format!("Инварианты проекта: {active} активных из {total}"),
            theme::accent(),
        )),
        Line::from(
            "Хранятся отдельно от диалога: invariants/invariants.json · изменения только явной командой"
                .to_string(),
        ),
        Line::from(format!(
            "Попыток нарушения/обхода: {attempts} · заблокировано: {blocked} · probe: {probes}"
        )),
    ];
    let paragraph = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" 9 Инварианты · жёсткие ограничения "),
    );
    frame.render_widget(paragraph, area);
}

fn draw_list(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    if state.invariants.invariants.is_empty() {
        lines.push(Line::from(Span::styled(
            "Инвариантов нет. Создайте их явной командой CLI.",
            theme::muted(),
        )));
    }
    for (index, invariant) in state.invariants.invariants.iter().enumerate() {
        let style = if index == state.invariant_selection {
            theme::selected()
        } else if invariant.active {
            theme::accent()
        } else {
            theme::muted()
        };
        lines.push(Line::from(Span::styled(
            format!(
                "{} [{}] {} · ревизия {}",
                if invariant.active { "●" } else { "○" },
                invariant.category.label(),
                invariant.id,
                invariant.revision
            ),
            style,
        )));
        for part in invariant.title.lines().take(1) {
            lines.push(Line::from(format!("    {part}")));
        }
        lines.push(Line::from(Span::styled(
            format!(
                "    {}",
                invariant.rule.chars().take(110).collect::<String>()
            ),
            theme::muted(),
        )));
        lines.push(Line::from(""));
    }
    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Список · c/Enter изменить · d история · a аудит · p probe "),
        )
        .wrap(Wrap { trim: false });
    let inner_width = area.width.saturating_sub(2);
    let inner_height = area.height.saturating_sub(2) as usize;
    let total_lines = paragraph.line_count(inner_width);
    let max_scroll = total_lines.saturating_sub(inner_height);
    let selected_line = state
        .invariants
        .invariants
        .iter()
        .take(state.invariant_selection)
        .map(|_| 3)
        .sum::<usize>();
    let offset = selected_line
        .saturating_sub(inner_height / 2)
        .min(max_scroll);
    frame.render_widget(paragraph.scroll((offset as u16, 0)), area);
}

fn draw_details(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Some(invariant) = state.selected_invariant() {
        lines.push(Line::from(Span::styled(
            format!(
                "{} · {} · {}",
                invariant.id,
                invariant.category.label(),
                if invariant.active {
                    "активен"
                } else {
                    "выключен"
                }
            ),
            theme::accent(),
        )));
        lines.push(Line::from(format!("Заголовок: {}", invariant.title)));
        lines.push(Line::from(format!("Правило: {}", invariant.rule)));
        lines.push(Line::from(Span::styled(
            format!(
                "Код ищет признаки: {}",
                if invariant.forbidden_phrases.is_empty() {
                    "—".into()
                } else {
                    invariant.forbidden_phrases.join("; ")
                }
            ),
            theme::muted(),
        )));
        let last_change = state
            .invariants
            .changes
            .iter()
            .rev()
            .find(|change| change.invariant_id == invariant.id);
        match last_change {
            Some(change) => {
                lines.push(Line::from(format!(
                    "Последнее изменение ({}): {} → {}; причина: {}",
                    change.at.format("%Y-%m-%d %H:%M"),
                    change.old_rule.chars().take(50).collect::<String>(),
                    change.new_rule.chars().take(50).collect::<String>(),
                    change.reason
                )));
            }
            None => lines.push(Line::from(Span::styled(
                "Изменений ещё не было.",
                theme::muted(),
            ))),
        }
    } else {
        lines.push(Line::from(Span::styled(
            "Выберите инвариант стрелками ↑↓.",
            theme::muted(),
        )));
    }
    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Детали и последнее изменение "),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}
