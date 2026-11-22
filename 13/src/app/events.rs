//! События job-потока TUI.

use std::path::PathBuf;

use crate::domain::chat::{LlmExchange, TurnOutcome};
use crate::domain::memory::{MemoryBundle, MemoryLayer};
use crate::domain::task::{TaskMessage, TaskState};
use crate::experiment::personalization::{
    PersonalizationOptions, PersonalizationProgress, PersonalizationReport,
};
use crate::experiment::runner::{ExperimentOptions, ExperimentProgress, ExperimentReport};
use crate::llm::agent::Progress;
use crate::memory::migration::MigrationReport;
use crate::task::TaskProgress;

#[derive(Debug, Clone)]
pub enum Job {
    Chat(String),
    Task {
        goal: String,
        history: Vec<TaskMessage>,
    },
    AcceptPending(String),
    RejectPending(String),
    ChangePending {
        id: String,
        layer: MemoryLayer,
    },
    ClearLayer(MemoryLayer),
    ImportFacts(PathBuf),
    Experiment(ExperimentOptions),
    Personalization(PersonalizationOptions),
}

#[derive(Debug)]
pub enum AppEvent {
    JobStarted(&'static str),
    ChatProgress(Progress),
    ApiLogged(Vec<LlmExchange>),
    MemoryUpdated(MemoryBundle),
    ChatFinished(Box<TurnOutcome>),
    TaskProgress(TaskProgress),
    TaskFinished(TaskState),
    ExperimentProgress(ExperimentProgress),
    ExperimentFinished(ExperimentReport),
    PersonalizationProgress(PersonalizationProgress),
    PersonalizationFinished(PersonalizationReport),
    MigrationFinished(MigrationReport),
    Error(String),
    JobFinished,
}
