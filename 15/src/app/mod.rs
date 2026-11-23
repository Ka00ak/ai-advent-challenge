//! Общая логика TUI и CLI.

pub mod controller;
pub mod events;
pub mod jobs;
pub mod state;

pub use controller::{AppController, AppError, AppRuntime};
pub use events::{AppEvent, Job};
