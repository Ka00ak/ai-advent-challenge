//! Воспроизводимый эксперимент: фикстура, probe, ablation, отчёт.

pub mod fixtures;
pub mod probes;
pub mod report;
pub mod runner;

pub use runner::{ExperimentMode, ExperimentOptions, ExperimentProgress, ExperimentReport};
