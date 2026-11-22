//! Экран «8 Состояния»: маршрут задачи, текущий этап и артефакты.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::domain::task::{TaskStage, TaskStatus};
use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(area);

    draw_route(frame, chunks[0], state);
    draw_details(frame, chunks[1], state);
    draw_input(frame, chunks[2], state);
}

fn draw_route(frame: &mut Frame, area: Rect, state: &AppState) {
    let current = state.task.stage;
    let current_index = current.index();
    let idle = state.task.status == TaskStatus::Idle;
    let mut route: Vec<Span<'static>> = Vec::new();
    for (index, stage) in TaskStage::ALL.iter().enumerate() {
        if index > 0 {
            route.push(Span::styled(" → ", theme::muted()));
        }
        let style = if idle {
            theme::muted()
        } else if stage.index() < current_index {
            theme::ok()
        } else if *stage == current {
            theme::selected()
        } else {
            theme::muted()
        };
        route.push(Span::styled(
            format!("[{} {}]", stage.index(), stage.as_str()),
            style,
        ));
    }

    let mut lines = vec![
        Line::from(route),
        Line::from(Span::styled(
            format!(
                "Этап: {} · шаг {}/{} · статус: {}{}",
                state.task.stage.title(),
                state.task.step_index,
                state.task.step_total.max(1),
                state.task.status.label(),
                if state.task_paused {
                    " · пауза"
                } else {
                    ""
                }
            ),
            if state.task.status == TaskStatus::Failed {
                theme::warn()
            } else {
                theme::accent()
            },
        )),
        Line::from(format!("Текущий шаг: {}", state.task.current_step)),
        Line::from(format!(
            "Ожидаемое действие: {}",
            state.task.expected_action
        )),
    ];
    if state.task.status == TaskStatus::Idle {
        lines[2] = Line::from(Span::styled(
            "Маршрут: planning → execution → validation → done",
            theme::muted(),
        ));
    }
    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Маршрут "))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn draw_details(frame: &mut Frame, area: Rect, state: &mut AppState) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(format!(
        "Задача: {}",
        if state.task.goal.is_empty() {
            "—"
        } else {
            &state.task.goal
        }
    )));
    lines.push(Line::from(format!(
        "План: {}",
        if state.task.plan_approved {
            "утверждён"
        } else if state.task.plan.is_some() {
            "составлен, не утверждён"
        } else {
            "ещё не составлен"
        }
    )));
    if let Some(plan) = &state.task.plan {
        lines.push(Line::from(Span::styled("— План —", theme::muted())));
        push_text(&mut lines, plan);
    }
    if let Some(execution) = &state.task.execution {
        lines.push(Line::from(Span::styled(
            "— Результат реализации —",
            theme::muted(),
        )));
        push_text(&mut lines, execution);
    }
    if let Some(validation) = &state.task.validation {
        let verdict = match state.task.validation_passed {
            Some(true) => "OK",
            Some(false) => "FAIL",
            None => "—",
        };
        lines.push(Line::from(Span::styled(
            format!("— Валидация: {verdict} —"),
            if state.task.validation_passed == Some(true) {
                theme::ok()
            } else {
                theme::warn()
            },
        )));
        push_text(&mut lines, validation);
    }
    if let Some(error) = &state.task.error {
        lines.push(Line::from(Span::styled(
            format!("Ошибка: {error}"),
            theme::warn(),
        )));
    }
    if state.task.history.is_empty() {
        lines.push(Line::from(Span::styled(
            "Введите задачу — агент пройдёт этапы по порядку.",
            theme::muted(),
        )));
    } else {
        lines.push(Line::from(Span::styled("— Переходы —", theme::muted())));
        for record in state.task.history.iter().rev().take(6) {
            let from = record.from.map(|stage| stage.as_str()).unwrap_or("start");
            lines.push(Line::from(format!(
                "{} → {} · {}",
                from,
                record.to.as_str(),
                record.reason
            )));
        }
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Состояние задачи "),
        )
        .wrap(Wrap { trim: false });
    let inner_width = area.width.saturating_sub(2);
    let inner_height = area.height.saturating_sub(2) as usize;
    let total_lines = paragraph.line_count(inner_width);
    let max_scroll = total_lines.saturating_sub(inner_height);
    let scroll = (state.task_scroll as usize).min(max_scroll);
    state.task_scroll = scroll.min(u16::MAX as usize) as u16;
    frame.render_widget(paragraph.scroll((scroll as u16, 0)), area);
}

fn push_text(lines: &mut Vec<Line<'static>>, text: &str) {
    for part in text.lines() {
        lines.push(Line::from(format!("  {part}")));
    }
}

fn draw_input(frame: &mut Frame, area: Rect, state: &AppState) {
    let active = state.task.is_active();
    let title = if state.task_paused {
        " Задача · пауза · p продолжить "
    } else if active {
        " Задача · выполняется "
    } else if state.task_focused {
        " Новая задача · Enter запустить "
    } else {
        " Новая задача "
    };
    let line = if active {
        Line::from(vec![
            Span::styled("> ", theme::accent()),
            Span::raw(state.task.goal.clone()),
            Span::styled(
                if state.task_paused {
                    "  [пауза: p — продолжить]"
                } else {
                    "  [p — пауза]"
                },
                theme::muted(),
            ),
        ])
    } else if state.task_focused {
        let length = state.task_input.buffer.chars().count();
        Line::from(vec![
            Span::styled("> ", theme::accent()),
            Span::raw(state.task_input.buffer.clone()),
            Span::styled("▏", theme::accent()),
            Span::styled(format!("  {length}/500"), theme::muted()),
        ])
    } else if state.task_input.buffer.is_empty() {
        Line::from(Span::styled(
            "> i или Enter — ввести задачу для агента",
            theme::muted(),
        ))
    } else {
        Line::from(format!("> {}", state.task_input.buffer))
    };
    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(if state.task_focused {
                theme::active_border()
            } else {
                Default::default()
            }),
    );
    frame.render_widget(paragraph, area);
}
