//! День 16 — подключение MCP к агенту Дня 6.
//!
//! Локальные инструменты (`calculate`, `get_current_time`, `convert_currency`)
//! и инструменты GitHub MCP попадают в один каталог. Агент получает их схемы
//! в теле запроса к модели, сам выбирает инструмент и аргументы, а исполнение
//! внешнего инструмента уходит в MCP-сервер.

pub mod agent;
pub mod catalog;
pub mod cli;
pub mod domain;
pub mod llm;
pub mod mcp;
pub mod settings;
pub mod tools;
pub mod tui;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
