//! TUI (ratatui + crossterm).

pub mod app;
pub mod keymap;
pub mod state;
mod theme;
mod widgets;

pub use app::{render_for_test, run};
