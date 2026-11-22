//! Исполнение задачи как конечного автомата.

pub mod runner;

pub use runner::{TaskProgress, TaskRunError, parse_validation_verdict, run_task};
