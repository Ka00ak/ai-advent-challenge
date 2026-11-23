//! Конечный автомат задачи для Дня 13.
//!
//! Задача проходит ровно четыре этапа:
//! `planning → execution → validation → done`.
//! Переход разрешён только на следующий этап и только при выполненном
//! предусловии. Проверка выполняется кодом до изменения состояния; модель
//! не может «перепрыгнуть» этап своим ответом.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const TASK_STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStage {
    Planning,
    Execution,
    Validation,
    Done,
}

impl TaskStage {
    pub const ALL: [TaskStage; 4] = [
        TaskStage::Planning,
        TaskStage::Execution,
        TaskStage::Validation,
        TaskStage::Done,
    ];

    /// Идентификатор в маршруте: `planning`, `execution`, `validation`, `done`.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStage::Planning => "planning",
            TaskStage::Execution => "execution",
            TaskStage::Validation => "validation",
            TaskStage::Done => "done",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            TaskStage::Planning => "Планирование",
            TaskStage::Execution => "Выполнение",
            TaskStage::Validation => "Валидация",
            TaskStage::Done => "Готово",
        }
    }

    /// Номер этапа в маршруте, начиная с 1.
    pub fn index(self) -> usize {
        match self {
            TaskStage::Planning => 1,
            TaskStage::Execution => 2,
            TaskStage::Validation => 3,
            TaskStage::Done => 4,
        }
    }

    /// Следующий допустимый этап; у `done` его нет.
    pub fn next(self) -> Option<TaskStage> {
        match self {
            TaskStage::Planning => Some(TaskStage::Execution),
            TaskStage::Execution => Some(TaskStage::Validation),
            TaskStage::Validation => Some(TaskStage::Done),
            TaskStage::Done => None,
        }
    }

    /// Ожидаемое действие ассистента на этапе.
    pub fn expected_action(self) -> &'static str {
        match self {
            TaskStage::Planning => {
                "Составить пошаговый план. Не начинать реализацию без утверждённого плана."
            }
            TaskStage::Execution => "Выполнить утверждённый план и описать полученный результат.",
            TaskStage::Validation => {
                "Проверить результат по плану и вынести вердикт «ВЕРДИКТ: OK» или «ВЕРДИКТ: FAIL»."
            }
            TaskStage::Done => "Задача завершена: этапы пройдены по порядку.",
        }
    }
}

impl std::fmt::Display for TaskStage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Idle,
    Running,
    Paused,
    Done,
    Failed,
}

impl TaskStatus {
    pub fn label(self) -> &'static str {
        match self {
            TaskStatus::Idle => "ожидание задачи",
            TaskStatus::Running => "выполняется",
            TaskStatus::Paused => "пауза",
            TaskStatus::Done => "завершена",
            TaskStatus::Failed => "ошибка",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, TaskStatus::Running | TaskStatus::Paused)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTransitionRecord {
    pub at: DateTime<Utc>,
    pub from: Option<TaskStage>,
    pub to: TaskStage,
    pub reason: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TaskTransitionError {
    #[error("нельзя перейти {from} → {to}: разрешён только следующий этап")]
    StageSkipped { from: TaskStage, to: TaskStage },
    #[error("нельзя начать реализацию без утверждённого плана")]
    PlanNotApproved,
    #[error("нельзя начать валидацию без результата реализации")]
    ExecutionIncomplete,
    #[error("нельзя завершить задачу без пройденной валидации")]
    ValidationNotPassed,
    #[error("задача уже завершена")]
    AlreadyDone,
    #[error("действие {action} недоступно на этапе {stage}")]
    WrongStage {
        stage: TaskStage,
        action: &'static str,
    },
    #[error("план должен содержать минимум два шага")]
    PlanTooShort,
    #[error("пустой результат этапа")]
    EmptyResult,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskMessageRole {
    User,
    Assistant,
}

impl TaskMessageRole {
    pub fn label(self) -> &'static str {
        match self {
            TaskMessageRole::User => "Вы",
            TaskMessageRole::Assistant => "Ассистент",
        }
    }
}

/// Одна реплика в чате задачи. Хранится вместе с состоянием, чтобы контекст
/// следующих задач и перезапуск TUI не теряли историю.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskMessage {
    pub at: DateTime<Utc>,
    pub role: TaskMessageRole,
    /// Этап, на котором ассистент выдал сообщение.
    pub stage: Option<TaskStage>,
    pub text: String,
}

impl TaskMessage {
    pub fn user(text: impl Into<String>, now: DateTime<Utc>) -> Self {
        Self {
            at: now,
            role: TaskMessageRole::User,
            stage: None,
            text: text.into(),
        }
    }

    pub fn assistant(stage: TaskStage, text: impl Into<String>, now: DateTime<Utc>) -> Self {
        Self {
            at: now,
            role: TaskMessageRole::Assistant,
            stage: Some(stage),
            text: text.into(),
        }
    }
}

/// Состояние задачи: этап, текущий шаг, ожидаемое действие, артефакты,
/// журнал переходов и чат-история. Это единственный источник истины для
/// экрана «Состояния».
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskState {
    pub version: u32,
    pub id: String,
    pub goal: String,
    pub stage: TaskStage,
    pub status: TaskStatus,
    /// Текущий шаг внутри этапа (1-based, для отображения).
    pub step_index: usize,
    pub step_total: usize,
    pub current_step: String,
    pub expected_action: String,
    pub plan: Option<String>,
    pub plan_approved: bool,
    pub execution: Option<String>,
    pub validation: Option<String>,
    /// `Some(true)` — валидация пройдена, `Some(false)` — не пройдена.
    pub validation_passed: Option<bool>,
    pub error: Option<String>,
    pub history: Vec<TaskTransitionRecord>,
    /// ID инвариантов, из-за которых этап задачи был заблокирован кодом.
    #[serde(default)]
    pub invariant_violations: Vec<String>,
    /// Чат задачи: реплики пользователя и ответы ассистента по этапам.
    #[serde(default)]
    pub messages: Vec<TaskMessage>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TaskState {
    /// Пустое состояние для экрана до запуска задачи.
    pub fn idle(now: DateTime<Utc>) -> Self {
        Self {
            version: TASK_STATE_VERSION,
            id: String::new(),
            goal: String::new(),
            stage: TaskStage::Planning,
            status: TaskStatus::Idle,
            step_index: 0,
            step_total: 0,
            current_step: "Ожидание задачи".into(),
            expected_action: "Введите задачу и нажмите Enter.".into(),
            plan: None,
            plan_approved: false,
            execution: None,
            validation: None,
            validation_passed: None,
            error: None,
            history: Vec::new(),
            invariant_violations: Vec::new(),
            messages: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Новая задача всегда начинается с `planning`.
    pub fn start(goal: impl Into<String>, now: DateTime<Utc>) -> Self {
        Self::start_with_history(goal, Vec::new(), now)
    }

    /// Новый цикл задачи с сохранением прошлой переписки. История уходит в
    /// промпты следующих этапов, поэтому модель видит контекст диалога.
    pub fn start_with_history(
        goal: impl Into<String>,
        history: Vec<TaskMessage>,
        now: DateTime<Utc>,
    ) -> Self {
        let goal = goal.into();
        let mut state = Self::idle(now);
        state.id = format!("task_{}", uuid::Uuid::new_v4().simple());
        state.goal = goal.clone();
        state.messages = history;
        state.messages.push(TaskMessage::user(goal, now));
        state.status = TaskStatus::Running;
        state.step_index = 1;
        state.step_total = 2;
        state.current_step = "Составление плана".into();
        state.expected_action = TaskStage::Planning.expected_action().into();
        state.history.push(TaskTransitionRecord {
            at: now,
            from: None,
            to: TaskStage::Planning,
            reason: "задача запущена".into(),
        });
        state
    }

    pub fn push_assistant(&mut self, stage: TaskStage, text: impl Into<String>) {
        self.messages
            .push(TaskMessage::assistant(stage, text, Utc::now()));
        self.updated_at = Utc::now();
    }

    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }

    pub fn is_finished(&self) -> bool {
        matches!(self.status, TaskStatus::Done | TaskStatus::Failed)
    }

    pub fn set_step(
        &mut self,
        index: usize,
        total: usize,
        title: impl Into<String>,
        action: impl Into<String>,
    ) {
        self.step_index = index;
        self.step_total = total;
        self.current_step = title.into();
        self.expected_action = action.into();
        self.updated_at = Utc::now();
    }

    pub fn mark_paused(&mut self, now: DateTime<Utc>) {
        if self.is_active() {
            self.status = TaskStatus::Paused;
            self.updated_at = now;
        }
    }

    pub fn mark_running(&mut self, now: DateTime<Utc>) {
        if self.status == TaskStatus::Paused {
            self.status = TaskStatus::Running;
            self.updated_at = now;
        }
    }

    pub fn mark_failed(&mut self, message: impl Into<String>, now: DateTime<Utc>) {
        self.status = TaskStatus::Failed;
        self.error = Some(message.into());
        self.updated_at = now;
    }

    /// Блокировка этапа инвариантом: этап не меняется, задача не может
    /// продолжаться без явного изменения инварианта.
    pub fn block_by_invariants(
        &mut self,
        ids: Vec<String>,
        message: impl Into<String>,
        now: DateTime<Utc>,
    ) {
        self.invariant_violations = ids;
        self.mark_failed(message, now);
    }

    pub fn record_plan(
        &mut self,
        plan: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<(), TaskTransitionError> {
        if self.stage != TaskStage::Planning {
            return Err(TaskTransitionError::WrongStage {
                stage: self.stage,
                action: "записать план",
            });
        }
        let plan = plan.into();
        if plan.trim().is_empty() {
            return Err(TaskTransitionError::EmptyResult);
        }
        self.plan = Some(plan);
        self.updated_at = now;
        Ok(())
    }

    /// Утверждает план только при наличии минимум двух пронумерованных шагов.
    /// Это и есть кодовая проверка «нельзя реализовывать без утверждённого плана».
    pub fn approve_plan(&mut self, now: DateTime<Utc>) -> Result<(), TaskTransitionError> {
        if self.stage != TaskStage::Planning {
            return Err(TaskTransitionError::WrongStage {
                stage: self.stage,
                action: "утвердить план",
            });
        }
        let plan = self.plan.as_deref().unwrap_or_default();
        if plan_step_count(plan) < 2 {
            return Err(TaskTransitionError::PlanTooShort);
        }
        self.plan_approved = true;
        self.updated_at = now;
        Ok(())
    }

    pub fn record_execution(
        &mut self,
        execution: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<(), TaskTransitionError> {
        if self.stage != TaskStage::Execution {
            return Err(TaskTransitionError::WrongStage {
                stage: self.stage,
                action: "записать реализацию",
            });
        }
        if !self.plan_approved {
            return Err(TaskTransitionError::PlanNotApproved);
        }
        let execution = execution.into();
        if execution.trim().is_empty() {
            return Err(TaskTransitionError::EmptyResult);
        }
        self.execution = Some(execution);
        self.updated_at = now;
        Ok(())
    }

    pub fn record_validation(
        &mut self,
        report: impl Into<String>,
        passed: bool,
        now: DateTime<Utc>,
    ) -> Result<(), TaskTransitionError> {
        if self.stage != TaskStage::Validation {
            return Err(TaskTransitionError::WrongStage {
                stage: self.stage,
                action: "записать валидацию",
            });
        }
        if self
            .execution
            .as_deref()
            .unwrap_or_default()
            .trim()
            .is_empty()
        {
            return Err(TaskTransitionError::ExecutionIncomplete);
        }
        let report = report.into();
        if report.trim().is_empty() {
            return Err(TaskTransitionError::EmptyResult);
        }
        self.validation = Some(report);
        self.validation_passed = Some(passed);
        self.updated_at = now;
        Ok(())
    }

    /// Проверяет допустимость перехода, ничего не меняя.
    pub fn can_transition(&self, to: TaskStage) -> Result<(), TaskTransitionError> {
        if self.stage == TaskStage::Done {
            return Err(TaskTransitionError::AlreadyDone);
        }
        if self.stage == to {
            return Ok(());
        }
        let expected = self.stage.next();
        if expected != Some(to) {
            return Err(TaskTransitionError::StageSkipped {
                from: self.stage,
                to,
            });
        }
        if self.stage == TaskStage::Planning
            && to == TaskStage::Execution
            && (!self.plan_approved || self.plan.as_deref().unwrap_or_default().trim().is_empty())
        {
            return Err(TaskTransitionError::PlanNotApproved);
        }
        if self.stage == TaskStage::Execution
            && to == TaskStage::Validation
            && self
                .execution
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            return Err(TaskTransitionError::ExecutionIncomplete);
        }
        if self.stage == TaskStage::Validation && to == TaskStage::Done {
            let validation = self.validation.as_deref().unwrap_or_default().trim();
            if self.validation_passed != Some(true) || validation.is_empty() {
                return Err(TaskTransitionError::ValidationNotPassed);
            }
        }
        Ok(())
    }

    /// Применяет переход после проверки предусловий.
    pub fn transition(
        &mut self,
        to: TaskStage,
        reason: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<(), TaskTransitionError> {
        self.can_transition(to)?;
        if self.stage == to {
            return Ok(());
        }
        let from = self.stage;
        self.stage = to;
        self.updated_at = now;
        self.history.push(TaskTransitionRecord {
            at: now,
            from: Some(from),
            to,
            reason: reason.into(),
        });
        self.expected_action = to.expected_action().into();
        self.step_index = 1;
        self.step_total = 1;
        self.current_step = to.title().into();
        Ok(())
    }
}

/// Количество шагов плана: строки с `1.`/`1)` или маркерами `-`/`*`.
pub fn plan_step_count(plan: &str) -> usize {
    plan.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                return true;
            }
            let lower = trimmed.to_lowercase();
            if lower.starts_with("шаг ") {
                return true;
            }
            let mut chars = trimmed.chars();
            matches!(chars.next(), Some(ch) if ch.is_ascii_digit())
                && matches!(chars.next(), Some('.' | ')'))
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started() -> TaskState {
        TaskState::start("Подготовить отчёт", Utc::now())
    }

    fn planned() -> TaskState {
        let now = Utc::now();
        let mut state = started();
        state
            .record_plan("1. Собрать данные\n2. Подготовить отчёт", now)
            .unwrap();
        state.approve_plan(now).unwrap();
        state
    }

    #[test]
    fn new_task_starts_in_planning() {
        let state = started();
        assert_eq!(state.stage, TaskStage::Planning);
        assert_eq!(state.status, TaskStatus::Running);
        assert!(
            state
                .history
                .iter()
                .any(|record| record.to == TaskStage::Planning)
        );
        assert_eq!(TaskStage::Planning.next(), Some(TaskStage::Execution));
    }

    #[test]
    fn implementation_requires_approved_plan() {
        let now = Utc::now();
        let mut state = started();
        state.record_plan("1. Один шаг", now).unwrap();
        assert_eq!(
            state.approve_plan(now),
            Err(TaskTransitionError::PlanTooShort)
        );
        assert_eq!(
            state.transition(TaskStage::Execution, "рано", now),
            Err(TaskTransitionError::PlanNotApproved)
        );
        assert_eq!(state.stage, TaskStage::Planning);
    }

    #[test]
    fn stages_cannot_be_skipped() {
        let now = Utc::now();
        let mut state = planned();
        assert_eq!(
            state.transition(TaskStage::Validation, "прыжок", now),
            Err(TaskTransitionError::StageSkipped {
                from: TaskStage::Planning,
                to: TaskStage::Validation,
            })
        );
        assert_eq!(
            state.transition(TaskStage::Done, "прыжок", now),
            Err(TaskTransitionError::StageSkipped {
                from: TaskStage::Planning,
                to: TaskStage::Done,
            })
        );
        assert_eq!(state.stage, TaskStage::Planning);
    }

    #[test]
    fn validation_requires_execution_result() {
        let now = Utc::now();
        let mut state = planned();
        state
            .transition(TaskStage::Execution, "план утверждён", now)
            .unwrap();
        assert_eq!(
            state.transition(TaskStage::Validation, "рано", now),
            Err(TaskTransitionError::ExecutionIncomplete)
        );
        state.record_execution("Результат готов", now).unwrap();
        state
            .transition(TaskStage::Validation, "реализация завершена", now)
            .unwrap();
        assert_eq!(state.stage, TaskStage::Validation);
    }

    #[test]
    fn done_requires_passed_validation() {
        let now = Utc::now();
        let mut state = planned();
        state.transition(TaskStage::Execution, "план", now).unwrap();
        state.record_execution("Результат", now).unwrap();
        state
            .transition(TaskStage::Validation, "реализация", now)
            .unwrap();
        assert_eq!(
            state.transition(TaskStage::Done, "без проверки", now),
            Err(TaskTransitionError::ValidationNotPassed)
        );
        state
            .record_validation("ВЕРДИКТ: FAIL", false, now)
            .unwrap();
        assert_eq!(
            state.transition(TaskStage::Done, "провал", now),
            Err(TaskTransitionError::ValidationNotPassed)
        );
        state.record_validation("ВЕРДИКТ: OK", true, now).unwrap();
        state
            .transition(TaskStage::Done, "проверка пройдена", now)
            .unwrap();
        assert_eq!(state.stage, TaskStage::Done);
        assert_eq!(
            state.transition(TaskStage::Done, "ещё раз", now),
            Err(TaskTransitionError::AlreadyDone)
        );
    }

    #[test]
    fn full_route_records_history_and_serializes() {
        let now = Utc::now();
        let mut state = planned();
        state
            .transition(TaskStage::Execution, "план утверждён", now)
            .unwrap();
        state.record_execution("Сделано", now).unwrap();
        state
            .transition(TaskStage::Validation, "реализация завершена", now)
            .unwrap();
        state.record_validation("ВЕРДИКТ: OK", true, now).unwrap();
        state
            .transition(TaskStage::Done, "валидация пройдена", now)
            .unwrap();
        assert_eq!(state.stage, TaskStage::Done);
        assert_eq!(state.history.len(), 4);
        let json = serde_json::to_string(&state).unwrap();
        let parsed: TaskState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, parsed);
    }

    #[test]
    fn plan_step_count_recognizes_numbered_and_bullet_lists() {
        assert_eq!(plan_step_count("1. Первый\n2) Второй"), 2);
        assert_eq!(plan_step_count("- один\n* два"), 2);
        assert_eq!(plan_step_count("Шаг 1: собрать\nШаг 2: проверить"), 2);
        assert_eq!(plan_step_count("просто текст"), 0);
    }

    #[test]
    fn pause_and_failure_change_status_but_not_stage() {
        let now = Utc::now();
        let mut state = started();
        state.mark_paused(now);
        assert_eq!(state.status, TaskStatus::Paused);
        assert_eq!(state.stage, TaskStage::Planning);
        state.mark_running(now);
        assert_eq!(state.status, TaskStatus::Running);
        state.mark_failed("нет сети", now);
        assert_eq!(state.status, TaskStatus::Failed);
        assert_eq!(state.error.as_deref(), Some("нет сети"));
    }
}
