//! Экран «10 Обновления»: правила LTM, журнал «было → стало», очистка.
//!
//! Здесь видно, что память не только пополняется: конфликты разрешаются
//! последней записью, старые значения не рендерятся, task-scoped факты
//! удаляются после завершения задачи.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::domain::memory::FactPolicy;
use crate::memory::lifecycle::stale_values;
use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(8),
            Constraint::Length(7),
        ])
        .split(area);

    draw_summary(frame, chunks[0], state);
    draw_revisions(frame, chunks[1], state);
    draw_details(frame, chunks[2], state);
}

fn draw_summary(frame: &mut Frame, area: Rect, state: &AppState) {
    let active = state
        .bundle
        .ltm
        .records
        .iter()
        .filter(|record| record.is_active());
    let mut replace = 0;
    let mut append = 0;
    let mut task = 0;
    for record in active {
        match record.policy {
            FactPolicy::Replace => replace += 1,
            FactPolicy::Append => append += 1,
            FactPolicy::TaskScoped => task += 1,
        }
    }
    let stale = stale_values(&state.bundle.ltm).len();
    let revisions = state.bundle.ltm.revisions.len();
    let cleanup = state
        .bundle
        .ltm
        .revisions
        .iter()
        .rev()
        .find(|revision| revision.op == crate::domain::memory::LtmRevisionOp::TaskCleanup)
        .map(|revision| revision.at.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "ещё не было".into());
    let lines = vec![
        Line::from(Span::styled(
            format!(
                "Активных фактов: replace {replace} · append {append} · task-scoped {task}"
            ),
            theme::accent(),
        )),
        Line::from(
            "Срок, статус, версия, подход, требование — одно актуальное значение; история изменений — append."
                .to_string(),
        ),
        Line::from(format!(
            "Журнал замен: {revisions} · устаревших значений скрыто из промпта: {stale}"
        )),
        Line::from(format!(
            "Последняя очистка после завершения задачи: {cleanup}"
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 10 Обновления · правила LTM и разрешение конфликтов "),
        ),
        area,
    );
}

fn draw_revisions(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let revisions = state.recent_updates();
    let mut lines: Vec<Line<'static>> = Vec::new();
    if revisions.is_empty() {
        lines.push(Line::from(Span::styled(
            "Журнал пуст. Измените срок, статус, версию или подход в чате.",
            theme::muted(),
        )));
    }
    for (index, revision) in revisions.iter().enumerate() {
        let style = if index == state.updates_selection {
            theme::selected()
        } else {
            match revision.op {
                crate::domain::memory::LtmRevisionOp::Replace => theme::accent(),
                crate::domain::memory::LtmRevisionOp::TaskCleanup => theme::warn(),
                _ => theme::muted(),
            }
        };
        lines.push(Line::from(Span::styled(
            format!(
                "{} · {} · {}",
                revision.at.format("%Y-%m-%d %H:%M:%S"),
                revision.op.label(),
                revision.key
            ),
            style,
        )));
        lines.push(Line::from(format!(
            "    {} → {}",
            revision
                .old_value
                .as_deref()
                .unwrap_or("—")
                .chars()
                .take(70)
                .collect::<String>(),
            revision
                .new_value
                .as_deref()
                .unwrap_or("—")
                .chars()
                .take(70)
                .collect::<String>()
        )));
        lines.push(Line::from(Span::styled(
            format!("    {}", revision.reason),
            theme::muted(),
        )));
    }
    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Журнал обновлений · Enter карточка · g/G начало/конец "),
        )
        .wrap(Wrap { trim: false });
    let inner_width = area.width.saturating_sub(2);
    let inner_height = area.height.saturating_sub(2) as usize;
    let total_lines = paragraph.line_count(inner_width);
    let max_scroll = total_lines.saturating_sub(inner_height);
    let selected_line = state
        .updates_selection
        .min(revisions.len().saturating_sub(1))
        * 3;
    let offset = selected_line
        .saturating_sub(inner_height / 2)
        .min(max_scroll);
    frame.render_widget(paragraph.scroll((offset as u16, 0)), area);
    state.updates_selection = state
        .updates_selection
        .min(revisions.len().saturating_sub(1));
}

fn draw_details(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Some(revision) = state.selected_update() {
        lines.push(Line::from(Span::styled(
            format!("{} · {}", revision.op.label(), revision.key),
            theme::accent(),
        )));
        lines.push(Line::from(format!(
            "Было: {}",
            revision.old_value.as_deref().unwrap_or("—")
        )));
        lines.push(Line::from(format!(
            "Стало: {}",
            revision.new_value.as_deref().unwrap_or("—")
        )));
        if let Some(record) = crate::memory::ltm::find_active_ltm(&state.bundle.ltm, &revision.key)
        {
            lines.push(Line::from(format!(
                "Сейчас: {} · policy={} · редакция {} · записано {}",
                record.value,
                record.policy.as_str(),
                record.revision,
                record.write_time().format("%Y-%m-%d %H:%M:%S")
            )));
        }
        lines.push(Line::from(Span::styled(
            format!("Причина: {}", revision.reason),
            theme::muted(),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "Выберите запись стрелками ↑↓ и нажмите Enter.",
            theme::muted(),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Что было и что стало "),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}
