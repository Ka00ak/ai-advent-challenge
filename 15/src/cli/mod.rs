//! CLI: clap-команды, тонкий адаптер над AppController.
//!
//! Коды выхода: 0 — успех, 2 — ошибка ввода/конфигурации, 3 — ошибка API,
//! 4 — провал порогов эксперимента.

pub mod commands;
pub mod output;

pub use commands::{Cli, Command, MemoryCommand, PendingCommand, TaskCommand, run};
