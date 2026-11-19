//! День 12 — персонализация ассистента поверх модели памяти (STM, WM, LTM).

pub mod app;
pub mod cli;
pub mod domain;
pub mod experiment;
pub mod llm;
pub mod memory;
pub mod storage;
pub mod tui;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
