//! Экран «Профиль»: активный профиль, список и история обучения.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::tui::state::AppState;
use crate::tui::theme;

pub fn draw(frame: &mut Frame, area: Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let mut list = vec![Line::from(Span::styled(
        "↑↓ выбор · Enter/u активировать",
        theme::muted(),
    ))];
    for (index, profile) in state.profiles.profiles.iter().enumerate() {
        let marker = if index == state.profile_selection {
            "> "
        } else {
            "  "
        };
        let active = if state.profiles.active.as_deref() == Some(profile.id.as_str()) {
            "*"
        } else {
            " "
        };
        let style = if index == state.profile_selection {
            theme::selected()
        } else {
            Default::default()
        };
        list.push(Line::from(Span::styled(
            format!("{marker}{active} {} · {}", profile.id, profile.title),
            style,
        )));
        list.push(Line::from(Span::styled(
            format!("     {}", profile.summary()),
            theme::muted(),
        )));
    }
    let list = Paragraph::new(list)
        .block(Block::default().borders(Borders::ALL).title(" Профили "))
        .wrap(Wrap { trim: true });
    frame.render_widget(list, chunks[0]);

    let mut detail = Vec::new();
    if let Some(profile) = state
        .profiles
        .profiles
        .get(state.profile_selection)
        .or_else(|| state.active_profile())
    {
        detail.push(Line::from(Span::styled(
            format!("{} · {}", profile.id, profile.title),
            theme::accent(),
        )));
        detail.push(Line::from(format!("Предпочтения: {}", profile.summary())));
        detail.push(Line::from(format!(
            "Пользователь: {} · роль: {} · язык: {}",
            profile.identity.name.as_deref().unwrap_or("—"),
            profile.identity.role.as_deref().unwrap_or("—"),
            profile.identity.language
        )));
        if !profile.constraints.is_empty() {
            detail.push(Line::from(format!(
                "Ограничения: {}",
                profile.constraints.join("; ")
            )));
        }
        detail.push(Line::from(format!(
            "Источник: {} · revision: {}",
            profile.source.as_str(),
            profile.revision
        )));
        detail.push(Line::from(""));
        detail.push(Line::from(Span::styled("История обучения", theme::muted())));
        if profile.preferences.is_empty() {
            detail.push(Line::from("Профиль ещё не пополнялся из диалога"));
        }
        for preference in profile.preferences.iter().rev().take(12) {
            detail.push(Line::from(format!(
                "{} = {} · {}",
                preference.key,
                preference.value,
                preference.source.as_str()
            )));
            if let Some(evidence) = &preference.evidence {
                detail.push(Line::from(Span::styled(
                    format!("  «{}»", evidence.chars().take(90).collect::<String>()),
                    theme::muted(),
                )));
            }
        }
    } else {
        detail.push(Line::from("Профилей пока нет"));
    }
    let detail = Paragraph::new(detail)
        .block(Block::default().borders(Borders::ALL).title(" Детали "))
        .wrap(Wrap { trim: true });
    frame.render_widget(detail, chunks[1]);
}
