//! События job-потока TUI.

use std::path::PathBuf;

use crate::domain::chat::{LlmExchange, TurnOutcome};
use crate::domain::memory::{MemoryBundle, MemoryLayer};
use crate::experiment::runner::{ExperimentOptions, ExperimentProgress, ExperimentReport};
use crate::llm::agent::Progress;
use crate::memory::migration::MigrationReport;

#[derive(Debug, Clone)]
pub enum Job {
    Chat(String),
    AcceptPending(String),
    RejectPending(String),
    ChangePending { id: String, layer: MemoryLayer },
    ClearLayer(MemoryLayer),
    ImportFacts(PathBuf),
    Experiment(ExperimentOptions),
}

#[derive(Debug)]
pub enum AppEvent {
    JobStarted(&'static str),
    ChatProgress(Progress),
    ApiLogged(Vec<LlmExchange>),
    MemoryUpdated(MemoryBundle),
    ChatFinished(Box<TurnOutcome>),
    ExperimentProgress(ExperimentProgress),
    ExperimentFinished(ExperimentReport),
    MigrationFinished(MigrationReport),
    Error(String),
    JobFinished,
}
