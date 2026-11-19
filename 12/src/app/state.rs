//! Общие view-модели для TUI (без зависимости от ratatui).

use crate::domain::memory::MemoryLayer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Memory,
    Pending,
    Experiment,
    Tokens,
    History,
    Settings,
    ApiLog,
    Branches,
    Profile,
}

impl Screen {
    pub const ALL: [Screen; 9] = [
        Screen::Memory,
        Screen::Pending,
        Screen::Experiment,
        Screen::Tokens,
        Screen::History,
        Screen::Settings,
        Screen::ApiLog,
        Screen::Branches,
        Screen::Profile,
    ];

    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|screen| *screen == self)
            .unwrap_or(0)
    }

    pub fn from_index(index: usize) -> Option<Screen> {
        Self::ALL.get(index).copied()
    }

    pub fn title(self) -> &'static str {
        match self {
            Screen::Memory => "Память",
            Screen::Pending => "Подтверждения",
            Screen::Experiment => "Проверка",
            Screen::Tokens => "Токены",
            Screen::History => "История",
            Screen::Settings => "Настройки",
            Screen::ApiLog => "API",
            Screen::Branches => "Ветки",
            Screen::Profile => "Профиль",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryColumn {
    Stm,
    Wm,
    Ltm,
}

impl MemoryColumn {
    pub fn layer(self) -> MemoryLayer {
        match self {
            MemoryColumn::Stm => MemoryLayer::Stm,
            MemoryColumn::Wm => MemoryLayer::Wm,
            MemoryColumn::Ltm => MemoryLayer::Ltm,
        }
    }
}
