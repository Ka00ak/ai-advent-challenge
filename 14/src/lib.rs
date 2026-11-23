//! День 13 — состояние задачи как конечный автомат поверх памяти и профиля.

pub mod app;
pub mod cli;
pub mod domain;
pub mod experiment;
pub mod invariants;
pub mod llm;
pub mod memory;
pub mod storage;
pub mod task;
pub mod tui;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
