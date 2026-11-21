//! Монохромная/цветная тема. При NO_COLOR цвета не используются.

use ratatui::style::{Color, Modifier, Style};

pub fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

pub fn accent() -> Style {
    if color_enabled() {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}

pub fn active_border() -> Style {
    if color_enabled() {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}

pub fn selected() -> Style {
    if color_enabled() {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::REVERSED)
    }
}

pub fn ok() -> Style {
    if color_enabled() {
        Style::default().fg(Color::Green)
    } else {
        Style::default()
    }
}

pub fn warn() -> Style {
    if color_enabled() {
        Style::default().fg(Color::Red)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}

pub fn muted() -> Style {
    if color_enabled() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    }
}

pub fn json_key() -> Style {
    if color_enabled() {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}

pub fn json_string() -> Style {
    if color_enabled() {
        Style::default().fg(Color::Green)
    } else {
        Style::default()
    }
}

pub fn json_number() -> Style {
    if color_enabled() {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    }
}

pub fn json_bool() -> Style {
    if color_enabled() {
        Style::default().fg(Color::Magenta)
    } else {
        Style::default().add_modifier(Modifier::UNDERLINED)
    }
}
