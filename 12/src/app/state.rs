//! Общие view-модели для TUI (без зависимости от ratatui).

use crate::domain::memory::MemoryLayer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Chat,
    Memory,
    Pending,
    Experiment,
    Tokens,
    Settings,
    ApiLog,
    Profile,
}

impl Screen {
    pub const ALL: [Screen; 8] = [
        Screen::Chat,
        Screen::Memory,
        Screen::Pending,
        Screen::Experiment,
        Screen::Tokens,
        Screen::Settings,
        Screen::ApiLog,
        Screen::Profile,
    ];

    /// Номер экрана в строке вкладок: `1…8`, без пропусков.
    pub fn number(self) -> usize {
        match self {
            Screen::Chat => 1,
            Screen::Memory => 2,
            Screen::Pending => 3,
            Screen::Experiment => 4,
            Screen::Tokens => 5,
            Screen::Settings => 6,
            Screen::ApiLog => 7,
            Screen::Profile => 8,
        }
    }

    pub fn from_number(number: usize) -> Option<Screen> {
        match number {
            1 => Some(Screen::Chat),
            2 => Some(Screen::Memory),
            3 => Some(Screen::Pending),
            4 => Some(Screen::Experiment),
            5 => Some(Screen::Tokens),
            6 => Some(Screen::Settings),
            7 => Some(Screen::ApiLog),
            8 => Some(Screen::Profile),
            _ => None,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Screen::Chat => "Чат",
            Screen::Memory => "Память",
            Screen::Pending => "Подтверждения",
            Screen::Experiment => "Проверка",
            Screen::Tokens => "Токены",
            Screen::Settings => "Настройки",
            Screen::ApiLog => "API",
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
