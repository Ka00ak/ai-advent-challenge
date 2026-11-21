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
    Branches,
    Profile,
}

impl Screen {
    pub const ALL: [Screen; 9] = [
        Screen::Chat,
        Screen::Memory,
        Screen::Pending,
        Screen::Experiment,
        Screen::Tokens,
        Screen::Settings,
        Screen::ApiLog,
        Screen::Branches,
        Screen::Profile,
    ];

    /// Номер экрана в строке вкладок. Номера стабильны: вкладка `5`
    /// (история) удалена, её функции перенесены в чат `0`.
    pub fn number(self) -> usize {
        match self {
            Screen::Chat => 0,
            Screen::Memory => 1,
            Screen::Pending => 2,
            Screen::Experiment => 3,
            Screen::Tokens => 4,
            Screen::Settings => 6,
            Screen::ApiLog => 7,
            Screen::Branches => 8,
            Screen::Profile => 9,
        }
    }

    /// Экран по номеру вкладки. `0` — чат, `1…9` без удалённой `5`.
    pub fn from_number(number: usize) -> Option<Screen> {
        match number {
            0 => Some(Screen::Chat),
            1 => Some(Screen::Memory),
            2 => Some(Screen::Pending),
            3 => Some(Screen::Experiment),
            4 => Some(Screen::Tokens),
            6 => Some(Screen::Settings),
            7 => Some(Screen::ApiLog),
            8 => Some(Screen::Branches),
            9 => Some(Screen::Profile),
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
