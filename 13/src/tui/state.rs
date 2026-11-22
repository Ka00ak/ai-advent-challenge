//! Состояние TUI и чистые переходы (без терминала) — тестируется reducer'ом.

use std::path::PathBuf;

use crate::app::controller::AppController;
use crate::app::state::{MemoryColumn, Screen};
use crate::domain::chat::{ExchangeKind, LlmExchange, TurnOutcome};
use crate::domain::memory::{
    LtmRecord, MemoryBreakdown, MemoryBundle, MemoryLayer, MemoryWriteEvent, WmItem,
};
use crate::domain::profile::ProfileState;
use crate::domain::settings::{Approval, ExtractionMode, Settings};
use crate::domain::task::TaskState;
use crate::experiment::personalization::{
    PersonalizationProgress, PersonalizationReport, SAME_QUESTION,
};
use crate::experiment::runner::{ExperimentProgress, ExperimentReport};
use crate::memory::migration::MigrationReport;

const MAX_INPUT: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPurpose {
    Chat,
    SearchLtm,
    FormKey,
    FormValue,
    FormKind,
    ImportFacts,
    ExportHistory,
    ImportHistory,
    SaveReport,
    SavePersonalizationReport,
    PersonalizationQuestion,
    ExportApi,
    TaskGoal,
    Setting(usize),
}

#[derive(Debug, Clone)]
pub struct InputState {
    pub purpose: InputPurpose,
    pub label: String,
    pub buffer: String,
    pub cursor: usize,
}

impl InputState {
    pub fn new(purpose: InputPurpose, label: impl Into<String>, value: impl Into<String>) -> Self {
        let buffer: String = value.into();
        let cursor = buffer.chars().count();
        Self {
            purpose,
            label: label.into(),
            buffer,
            cursor,
        }
    }

    pub fn insert(&mut self, ch: char) {
        if self.buffer.chars().count() >= MAX_INPUT {
            return;
        }
        let byte = char_to_byte(&self.buffer, self.cursor);
        self.buffer.insert(byte, ch);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let end = char_to_byte(&self.buffer, self.cursor);
        let start = char_to_byte(&self.buffer, self.cursor - 1);
        self.buffer.replace_range(start..end, "");
        self.cursor -= 1;
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.buffer.chars().count() {
            return;
        }
        let start = char_to_byte(&self.buffer, self.cursor);
        let end = char_to_byte(&self.buffer, self.cursor + 1);
        self.buffer.replace_range(start..end, "");
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.buffer.chars().count());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.buffer.chars().count();
    }
}

fn char_to_byte(value: &str, chars: usize) -> usize {
    value
        .char_indices()
        .nth(chars)
        .map(|(byte, _)| byte)
        .unwrap_or(value.len())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    ClearLayer(MemoryLayer),
    ArchiveTask,
    DeleteRecord { layer: MemoryLayer, key: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayerTarget {
    MoveRecord { from: MemoryLayer, key: String },
    Pending(String),
}

#[derive(Debug, Clone)]
pub enum Modal {
    Confirm {
        title: String,
        message: String,
        action: ConfirmAction,
    },
    Error(String),
    Help,
    Card {
        title: String,
        lines: Vec<String>,
    },
    Details {
        title: String,
        lines: Vec<String>,
    },
    SelectLayer {
        target: LayerTarget,
        choice: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiBodyTab {
    Request,
    Response,
}

impl ApiBodyTab {
    pub fn toggle(self) -> Self {
        match self {
            ApiBodyTab::Request => ApiBodyTab::Response,
            ApiBodyTab::Response => ApiBodyTab::Request,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            ApiBodyTab::Request => "Запрос",
            ApiBodyTab::Response => "Ответ",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiFocus {
    List,
    Body,
}

impl ApiFocus {
    pub fn toggle(self) -> Self {
        match self {
            ApiFocus::List => ApiFocus::Body,
            ApiFocus::Body => ApiFocus::List,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyMode {
    /// Ablation памяти Дня 11 (7 конфигураций + очистка).
    Ablation,
    /// Проверка персонализации: 3 профиля и цепочка из 10 вопросов.
    Profiles,
}

impl VerifyMode {
    pub fn toggle(self) -> Self {
        match self {
            VerifyMode::Ablation => VerifyMode::Profiles,
            VerifyMode::Profiles => VerifyMode::Ablation,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            VerifyMode::Ablation => "Ablation памяти",
            VerifyMode::Profiles => "Профили",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormStep {
    Key,
    Value,
    Kind,
}

#[derive(Debug, Clone)]
pub struct FormDraft {
    pub layer: MemoryLayer,
    pub original_key: Option<String>,
    pub pending_id: Option<String>,
    pub key: String,
    pub value: String,
    pub kind: String,
    pub step: FormStep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiFilter {
    All,
    Chat,
    Memory,
    Error,
}

impl ApiFilter {
    pub fn next(self) -> Self {
        match self {
            ApiFilter::All => ApiFilter::Chat,
            ApiFilter::Chat => ApiFilter::Memory,
            ApiFilter::Memory => ApiFilter::Error,
            ApiFilter::Error => ApiFilter::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ApiFilter::All => "all",
            ApiFilter::Chat => "chat",
            ApiFilter::Memory => "memory",
            ApiFilter::Error => "error",
        }
    }

    pub fn matches(self, exchange: &LlmExchange) -> bool {
        match self {
            ApiFilter::All => true,
            ApiFilter::Chat => exchange.kind == ExchangeKind::Chat,
            ApiFilter::Memory => {
                exchange.kind == ExchangeKind::Memory || exchange.kind == ExchangeKind::Summary
            }
            ApiFilter::Error => exchange.status != "ok",
        }
    }
}

pub struct AppState {
    pub screen: Screen,
    pub focus: usize,
    pub memory_selection: [usize; 3],
    pub pending_selection: usize,
    pub experiment_selection: usize,
    pub api_selection: usize,
    pub settings_selection: usize,
    pub profile_selection: usize,
    pub bundle: MemoryBundle,
    pub profiles: ProfileState,
    pub settings: Settings,
    pub last_turn: Option<TurnOutcome>,
    pub last_breakdown: MemoryBreakdown,
    pub exchanges: Vec<LlmExchange>,
    pub experiment: Option<ExperimentReport>,
    pub experiment_progress: Option<ExperimentProgress>,
    experiment_exchange_id: Option<String>,
    pub experiment_quick: bool,
    pub experiment_cleanup: bool,
    /// Режим вкладки «Проверка»: ablation памяти или полярные профили.
    pub verify_mode: VerifyMode,
    /// Отчёт проверки профилей: цепочки по 10 вопросов для каждого профиля.
    pub personalization: Option<PersonalizationReport>,
    pub personalization_progress: Option<PersonalizationProgress>,
    /// Первый вопрос цепочки; по умолчанию — SAME_QUESTION.
    pub personalization_question: String,
    /// Запускать цепочку из 10 вопросов для каждого профиля (30 запросов).
    pub personalization_chain: bool,
    pub migration: Option<MigrationReport>,
    pub audit: Vec<MemoryWriteEvent>,
    pub busy: Option<&'static str>,
    pub banner: Option<(String, bool)>,
    pub modal: Option<Modal>,
    pub input: Option<InputState>,
    pub form: Option<FormDraft>,
    pub ltm_search: Option<String>,
    pub api_filter: ApiFilter,
    pub api_body_tab: ApiBodyTab,
    pub api_focus: ApiFocus,
    pub api_json_scroll: u16,
    pub help_scroll: u16,
    /// Черновик сообщения на вкладке «1 Чат».
    pub chat_input: InputState,
    /// true, когда клавиатура захвачена строкой ввода чата.
    pub chat_focused: bool,
    /// Прокрутка истории чата: число строк от последнего сообщения вверх.
    pub chat_scroll: u16,
    /// Реплика, отправленная в job и ещё не подтверждённая STM.
    pub chat_pending: Option<String>,
    /// Задача на вкладке «8 Состояния» и её ввод.
    pub task: TaskState,
    pub task_input: InputState,
    pub task_focused: bool,
    pub task_paused: bool,
    pub task_scroll: u16,
    pub should_quit: bool,
    pub data_dir: PathBuf,
    pub status: String,
}

impl AppState {
    pub fn new(controller: &AppController) -> Self {
        Self {
            screen: Screen::Memory,
            focus: 0,
            memory_selection: [0, 0, 0],
            pending_selection: 0,
            experiment_selection: 0,
            api_selection: 0,
            settings_selection: 0,
            profile_selection: 0,
            bundle: controller.bundle().clone(),
            profiles: controller.profiles().clone(),
            settings: controller.settings().clone(),
            last_turn: None,
            last_breakdown: controller
                .bundle()
                .stm
                .summary
                .as_ref()
                .map_or(MemoryBreakdown::default(), |_| MemoryBreakdown::default()),
            exchanges: Vec::new(),
            experiment: None,
            experiment_progress: None,
            experiment_exchange_id: None,
            experiment_quick: false,
            experiment_cleanup: false,
            verify_mode: VerifyMode::Ablation,
            personalization: None,
            personalization_progress: None,
            personalization_question: SAME_QUESTION.to_string(),
            personalization_chain: true,
            migration: None,
            audit: controller.audit.events.clone(),
            busy: None,
            banner: None,
            modal: None,
            input: None,
            form: None,
            ltm_search: None,
            api_filter: ApiFilter::All,
            api_body_tab: ApiBodyTab::Request,
            api_focus: ApiFocus::List,
            api_json_scroll: 0,
            help_scroll: 0,
            chat_input: InputState::new(InputPurpose::Chat, "Сообщение", ""),
            chat_focused: false,
            chat_scroll: 0,
            chat_pending: None,
            task: TaskState::idle(chrono::Utc::now()),
            task_input: InputState::new(InputPurpose::TaskGoal, "Задача", ""),
            task_focused: false,
            task_paused: false,
            task_scroll: 0,
            should_quit: false,
            data_dir: controller.runtime().paths.root.clone(),
            status: "Готово".into(),
        }
    }

    pub fn refresh(&mut self, controller: &AppController) {
        self.bundle = controller.bundle().clone();
        self.profiles = controller.profiles().clone();
        self.settings = controller.settings().clone();
        self.audit = controller.audit.events.clone();
        self.clamp_selections();
    }

    pub fn record_turn(&mut self, outcome: &TurnOutcome) {
        self.last_breakdown = outcome.breakdown.clone();
        self.chat_pending = None;
        self.chat_scroll = 0;
    }

    /// Оптимистично показывает новую задачу до ответа job-потока.
    pub fn begin_task(&mut self, goal: &str) {
        self.task = TaskState::start(goal, chrono::Utc::now());
        self.task_focused = false;
        self.task_paused = false;
        self.task_scroll = 0;
    }

    /// Переключает паузу задачи. Возвращает новое состояние паузы.
    pub fn toggle_task_pause(&mut self) -> bool {
        if !self.task.is_active() {
            return self.task_paused;
        }
        self.task_paused = !self.task_paused;
        let now = chrono::Utc::now();
        if self.task_paused {
            self.task.mark_paused(now);
        } else {
            self.task.mark_running(now);
        }
        self.task_paused
    }

    pub fn task_active(&self) -> bool {
        self.task.is_active() || self.task_paused
    }

    /// Добавляет записи API-журнала job-потока. Записи с тем же id
    /// обновляются, новые добавляются в конец.
    pub fn merge_exchanges(&mut self, incoming: Vec<LlmExchange>) {
        let follow_tail = self.api_selection + 1 >= self.filtered_exchanges().len();
        // Для эксперимента до конца прогона показывается одна сводная строка.
        // Как только приходят детальные HTTP-запросы, сводку убираем, чтобы
        // не смешивать её с реальными запросами.
        if incoming.iter().any(|exchange| {
            exchange.kind == ExchangeKind::Experiment && exchange.request_body.is_some()
        }) {
            self.experiment_exchange_id = None;
            self.exchanges.retain(|exchange| {
                !(exchange.kind == ExchangeKind::Experiment && exchange.request_body.is_none())
            });
        }
        for exchange in incoming {
            if let Some(existing) = self
                .exchanges
                .iter_mut()
                .find(|candidate| candidate.id == exchange.id)
            {
                *existing = exchange;
            } else {
                self.exchanges.push(exchange);
            }
        }
        while self.exchanges.len() > 200 {
            self.exchanges.remove(0);
        }
        self.clamp_selections();
        if follow_tail && self.screen == Screen::ApiLog {
            self.api_selection = self.filtered_exchanges().len().saturating_sub(1);
        }
    }

    /// Живая запись эксперимента в журнале API/Токенов: обновляется на каждом
    /// завершённом запросе, чтобы вкладки 4 и 7 не ждали конца прогона.
    pub fn record_experiment_progress(&mut self, progress: &ExperimentProgress) {
        if let Some(breakdown) = &progress.breakdown {
            self.last_breakdown = breakdown.clone();
        }
        let id = match &self.experiment_exchange_id {
            Some(id) => id.clone(),
            None => {
                let exchange = LlmExchange::new(
                    ExchangeKind::Experiment,
                    &self.settings.model,
                    &self.settings.base_url,
                );
                let id = exchange.id.clone();
                self.exchanges.push(exchange);
                self.experiment_exchange_id = Some(id.clone());
                self.trim_exchanges();
                id
            }
        };
        if let Some(exchange) = self.exchanges.iter_mut().find(|exchange| exchange.id == id) {
            exchange.prompt_tokens = progress.prompt_tokens;
            exchange.completion_tokens = progress.completion_tokens;
            exchange.total_tokens = progress.total_tokens();
            exchange.cost_usd = progress.cost_usd;
            exchange.memory = progress.breakdown.clone();
            exchange.status = if progress.errors > 0 {
                "partial".into()
            } else {
                "running".into()
            };
            exchange.response_preview =
                Some(format!("{} из {} запросов", progress.done, progress.total));
        }
    }

    /// Отмена/ошибка прогона: закрываем живую запись, чтобы следующий запуск
    /// начал новую, а не перезаписывал отменённую.
    pub fn fail_experiment_progress(&mut self) {
        if let Some(id) = self.experiment_exchange_id.take() {
            if let Some(exchange) = self.exchanges.iter_mut().find(|exchange| exchange.id == id) {
                exchange.status = "error".into();
            }
        }
    }

    pub fn record_experiment(&mut self, report: &ExperimentReport) {
        let (prompt_tokens, completion_tokens, cost_usd) = report.usage_totals();
        let status = if report.errors.is_empty() {
            "ok"
        } else {
            "partial"
        };
        let preview = format!(
            "{} запросов, {}/{}",
            report.total_requests,
            report
                .config("stm+wm+ltm")
                .map(|config| config.total)
                .unwrap_or(0),
            18
        );
        // Детальные HTTP-записи уже пришли через AppEvent::ApiLogged —
        // отдельная сводная строка больше не нужна.
        if self.exchanges.iter().any(|exchange| {
            exchange.kind == ExchangeKind::Experiment && exchange.request_body.is_some()
        }) {
            self.experiment_exchange_id = None;
            self.exchanges.retain(|exchange| {
                !(exchange.kind == ExchangeKind::Experiment && exchange.request_body.is_none())
            });
            self.clamp_selections();
            return;
        }
        if let Some(id) = self.experiment_exchange_id.take() {
            if let Some(exchange) = self.exchanges.iter_mut().find(|exchange| exchange.id == id) {
                exchange.prompt_tokens = prompt_tokens;
                exchange.completion_tokens = completion_tokens;
                exchange.total_tokens = prompt_tokens + completion_tokens;
                exchange.cost_usd = cost_usd;
                exchange.status = status.into();
                exchange.response_preview = Some(preview);
                return;
            }
        }
        let mut exchange = LlmExchange::new(
            ExchangeKind::Experiment,
            &self.settings.model,
            &self.settings.base_url,
        );
        exchange.prompt_tokens = prompt_tokens;
        exchange.completion_tokens = completion_tokens;
        exchange.total_tokens = prompt_tokens + completion_tokens;
        exchange.cost_usd = cost_usd;
        exchange.status = status.into();
        exchange.response_preview = Some(preview);
        self.exchanges.push(exchange);
        self.trim_exchanges();
    }

    fn trim_exchanges(&mut self) {
        if self.exchanges.len() > 200 {
            self.exchanges.remove(0);
        }
    }

    pub fn set_banner(&mut self, message: impl Into<String>) {
        self.banner = Some((message.into(), true));
    }

    pub fn clear_banner(&mut self) {
        self.banner = None;
    }

    pub fn memory_column(&self) -> MemoryColumn {
        match self.focus {
            1 => MemoryColumn::Wm,
            2 => MemoryColumn::Ltm,
            _ => MemoryColumn::Stm,
        }
    }

    pub fn toggle_focus(&mut self, back: bool) {
        if back {
            self.focus = (self.focus + 2) % 3;
        } else {
            self.focus = (self.focus + 1) % 3;
        }
    }

    pub fn visible_ltm(&self) -> Vec<&LtmRecord> {
        let mut records: Vec<&LtmRecord> = self
            .bundle
            .ltm
            .records
            .iter()
            .filter(|record| record.is_active())
            .collect();
        if let Some(query) = &self.ltm_search {
            let query = query.to_lowercase();
            records.retain(|record| {
                record.key.to_lowercase().contains(&query)
                    || record.value.to_lowercase().contains(&query)
            });
        }
        records.sort_by(|left, right| {
            left.kind
                .profile_order()
                .cmp(&right.kind.profile_order())
                .then(right.updated_at.cmp(&left.updated_at))
                .then(left.key.cmp(&right.key))
        });
        records
    }

    pub fn selected_ltm(&self) -> Option<&LtmRecord> {
        self.visible_ltm().into_iter().nth(self.memory_selection[2])
    }

    pub fn selected_wm(&self) -> Option<&WmItem> {
        self.bundle.wm.items.get(self.memory_selection[1])
    }

    pub fn selected_pending(&self) -> Option<&crate::domain::memory::PendingWrite> {
        self.bundle.pending.get(self.pending_selection)
    }

    pub fn clamp_selections(&mut self) {
        let ltm_len = self.visible_ltm().len();
        let wm_len = self.bundle.wm.items.len();
        let stm_len = self.bundle.stm.messages.len();
        self.memory_selection[0] = self.memory_selection[0].min(stm_len.saturating_sub(1));
        self.memory_selection[1] = self.memory_selection[1].min(wm_len.saturating_sub(1));
        self.memory_selection[2] = self.memory_selection[2].min(ltm_len.saturating_sub(1));
        self.pending_selection = self
            .pending_selection
            .min(self.bundle.pending.len().saturating_sub(1));
        self.profile_selection = self
            .profile_selection
            .min(self.profiles.profiles.len().saturating_sub(1));
        self.api_selection = self
            .api_selection
            .min(self.filtered_exchanges().len().saturating_sub(1));
    }

    pub fn filtered_exchanges(&self) -> Vec<&LlmExchange> {
        self.exchanges
            .iter()
            .filter(|exchange| self.api_filter.matches(exchange))
            .collect()
    }

    pub fn selected_exchange(&self) -> Option<&LlmExchange> {
        self.filtered_exchanges()
            .into_iter()
            .nth(self.api_selection)
    }

    pub fn confirm(
        &mut self,
        title: impl Into<String>,
        message: impl Into<String>,
        action: ConfirmAction,
    ) {
        self.modal = Some(Modal::Confirm {
            title: title.into(),
            message: message.into(),
            action,
        });
    }

    pub fn error_modal(&mut self, message: impl Into<String>) {
        self.modal = Some(Modal::Error(message.into()));
    }

    pub fn open_input(
        &mut self,
        purpose: InputPurpose,
        label: impl Into<String>,
        value: impl Into<String>,
    ) {
        self.input = Some(InputState::new(purpose, label, value));
    }

    pub fn start_add(&mut self, layer: MemoryLayer) {
        if layer == MemoryLayer::Stm {
            self.set_banner("STM пополняется сообщениями, а не ручными записями");
            return;
        }
        self.form = Some(FormDraft {
            layer,
            original_key: None,
            pending_id: None,
            key: String::new(),
            value: String::new(),
            kind: if layer == MemoryLayer::Ltm {
                "knowledge".into()
            } else {
                "entity".into()
            },
            step: FormStep::Key,
        });
        self.open_input(InputPurpose::FormKey, "Новый key", "");
    }

    pub fn start_edit(&mut self) {
        let layer = self.memory_column().layer();
        match layer {
            MemoryLayer::Stm => {
                self.open_card_for_stm();
            }
            MemoryLayer::Wm => {
                if let Some(item) = self.selected_wm().cloned() {
                    self.form = Some(FormDraft {
                        layer,
                        original_key: Some(item.key.clone()),
                        pending_id: None,
                        key: item.key.clone(),
                        value: item.value.clone(),
                        kind: item.kind.as_str().into(),
                        step: FormStep::Key,
                    });
                    self.open_input(InputPurpose::FormKey, "Key", item.key);
                }
            }
            MemoryLayer::Ltm => {
                if let Some(record) = self.selected_ltm().cloned() {
                    self.form = Some(FormDraft {
                        layer,
                        original_key: Some(record.key.clone()),
                        pending_id: None,
                        key: record.key.clone(),
                        value: record.value.clone(),
                        kind: record.kind.as_str().into(),
                        step: FormStep::Key,
                    });
                    self.open_input(InputPurpose::FormKey, "Key", record.key);
                }
            }
        }
    }

    pub fn start_edit_pending(&mut self) {
        if let Some(pending) = self.selected_pending().cloned() {
            self.form = Some(FormDraft {
                layer: MemoryLayer::Ltm,
                original_key: Some(pending.candidate.key.clone()),
                pending_id: Some(pending.id.clone()),
                key: pending.candidate.key.clone(),
                value: pending.candidate.value.clone(),
                kind: pending.target_kind.as_str().into(),
                step: FormStep::Key,
            });
            self.open_input(InputPurpose::FormKey, "Key pending", pending.candidate.key);
        }
    }

    pub fn open_card_for_stm(&mut self) {
        if let Some(message) = self.bundle.stm.messages.get(self.memory_selection[0]) {
            self.modal = Some(Modal::Card {
                title: format!(
                    "Сообщение #{} · {:?}",
                    self.memory_selection[0] + 1,
                    message.role
                ),
                lines: message
                    .text()
                    .lines()
                    .map(str::to_string)
                    .chain(std::iter::once(format!(
                        "tool_calls: {} · tool_call_id: {}",
                        message.tool_calls.len(),
                        message.tool_call_id.as_deref().unwrap_or("—")
                    )))
                    .collect(),
            });
        }
    }

    pub fn open_card_for_record(&mut self) {
        match self.memory_column() {
            MemoryColumn::Wm => {
                if let Some(item) = self.selected_wm() {
                    self.modal = Some(Modal::Card {
                        title: format!("WM · {}", item.key),
                        lines: vec![
                            format!("value: {}", item.value),
                            format!(
                                "kind: {} · status: {}",
                                item.kind.as_str(),
                                item.status.as_str()
                            ),
                            format!("order: {:?}", item.order),
                            format!("источник: сообщение #{:?}", item.source_message_index),
                            format!("создано: {}", item.created_at.to_rfc3339()),
                            format!("обновлено: {}", item.updated_at.to_rfc3339()),
                            format!("ref_ltm_id: {}", item.ref_ltm_id.as_deref().unwrap_or("—")),
                        ],
                    });
                }
            }
            MemoryColumn::Ltm => {
                if let Some(record) = self.selected_ltm() {
                    let reason = self
                        .audit
                        .iter()
                        .rev()
                        .find(|event| event.key == record.key)
                        .map(|event| {
                            format!(
                                "{} · {}",
                                event.rule_id.as_deref().unwrap_or("—"),
                                event.reason
                            )
                        })
                        .unwrap_or_else(|| "—".into());
                    self.modal = Some(Modal::Card {
                        title: format!("LTM · {}", record.key),
                        lines: vec![
                            format!("value: {}", record.value),
                            format!(
                                "kind: {} · confidence: {:.2}",
                                record.kind.as_str(),
                                record.confidence
                            ),
                            format!(
                                "source: {} · pinned: {}",
                                record.source.as_str(),
                                record.pinned
                            ),
                            format!(
                                "tags: {}",
                                if record.tags.is_empty() {
                                    "—".into()
                                } else {
                                    record.tags.join(", ")
                                }
                            ),
                            format!("создано: {}", record.created_at.to_rfc3339()),
                            format!("обновлено: {}", record.updated_at.to_rfc3339()),
                            format!(
                                "последнее использование: {}",
                                record
                                    .last_used_at
                                    .map(|at| at.to_rfc3339())
                                    .unwrap_or_else(|| "—".into())
                            ),
                            format!(
                                "версия: superseded_by={}",
                                record.superseded_by.as_deref().unwrap_or("—")
                            ),
                            format!("причина: {reason}"),
                        ],
                    });
                }
            }
            MemoryColumn::Stm => self.open_card_for_stm(),
        }
    }

    pub fn settings_rows(&self) -> Vec<(String, String)> {
        let memory = &self.settings.memory;
        vec![
            ("Base URL".into(), self.settings.base_url.clone()),
            ("Model".into(), self.settings.model.clone()),
            (
                "Temperature".into(),
                format!("{:.2}", self.settings.temperature),
            ),
            (
                "Max tokens".into(),
                self.settings
                    .max_tokens
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "—".into()),
            ),
            (
                "Max iterations".into(),
                self.settings.max_iterations.to_string(),
            ),
            (
                "Extraction".into(),
                match memory.extraction_mode {
                    ExtractionMode::Local => "local".into(),
                    ExtractionMode::Llm => "llm".into(),
                },
            ),
            (
                "Approval".into(),
                match memory.approval {
                    Approval::Confirm => "confirm".into(),
                    Approval::Auto => "auto".into(),
                },
            ),
            ("STM window".into(), memory.stm.window.to_string()),
            ("STM max tokens".into(), memory.stm.max_tokens.to_string()),
            (
                "STM summary".into(),
                if memory.stm.summary_enabled {
                    "on".into()
                } else {
                    "off".into()
                },
            ),
            ("WM max tokens".into(), memory.wm.max_tokens.to_string()),
            ("LTM top-K".into(), memory.ltm.top_k.to_string()),
            (
                "LTM relevant budget".into(),
                memory.ltm.relevant_max_tokens.to_string(),
            ),
        ]
    }

    /// Применяет значение поля настроек. Возвращает ошибку валидации.
    pub fn apply_setting(&mut self, index: usize, raw: &str) -> Result<(), String> {
        let value = raw.trim();
        match index {
            0 => {
                if value.is_empty() {
                    return Err("Base URL не может быть пустым".into());
                }
                self.settings.base_url = value.to_string();
            }
            1 => {
                if value.is_empty() {
                    return Err("Model не может быть пустой".into());
                }
                self.settings.model = value.to_string();
            }
            2 => {
                self.settings.temperature = value
                    .parse()
                    .map_err(|_| "Temperature: число".to_string())?
            }
            3 => {
                self.settings.max_tokens = if value.is_empty() || value == "—" {
                    None
                } else {
                    Some(value.parse().map_err(|_| "Max tokens: число".to_string())?)
                };
            }
            4 => {
                self.settings.max_iterations = value
                    .parse()
                    .map_err(|_| "Max iterations: число".to_string())?
            }
            5 => {
                self.settings.memory.extraction_mode = match value.to_lowercase().as_str() {
                    "local" => ExtractionMode::Local,
                    "llm" => ExtractionMode::Llm,
                    _ => return Err("Extraction: local или llm".into()),
                };
            }
            6 => {
                self.settings.memory.approval = match value.to_lowercase().as_str() {
                    "confirm" => Approval::Confirm,
                    "auto" => Approval::Auto,
                    _ => return Err("Approval: confirm или auto".into()),
                };
            }
            7 => {
                self.settings.memory.stm.window =
                    value.parse().map_err(|_| "STM window: число".to_string())?
            }
            8 => {
                self.settings.memory.stm.max_tokens = value
                    .parse()
                    .map_err(|_| "STM max tokens: число".to_string())?
            }
            9 => {
                self.settings.memory.stm.summary_enabled = match value.to_lowercase().as_str() {
                    "on" | "true" | "1" => true,
                    "off" | "false" | "0" => false,
                    _ => return Err("STM summary: on или off".into()),
                };
            }
            10 => {
                self.settings.memory.wm.max_tokens = value
                    .parse()
                    .map_err(|_| "WM max tokens: число".to_string())?
            }
            11 => {
                self.settings.memory.ltm.top_k =
                    value.parse().map_err(|_| "LTM top-K: число".to_string())?
            }
            12 => {
                self.settings.memory.ltm.relevant_max_tokens =
                    value.parse().map_err(|_| "LTM budget: число".to_string())?
            }
            _ => return Err("неизвестное поле".into()),
        }
        let errors = self.settings.validate();
        if let Some(error) = errors.first() {
            return Err(error.clone());
        }
        Ok(())
    }

    pub fn pending_selected_id(&self) -> Option<String> {
        self.selected_pending().map(|pending| pending.id.clone())
    }

    pub fn init_input_for_setting(&mut self) {
        let rows = self.settings_rows();
        if let Some((label, value)) = rows.get(self.settings_selection) {
            self.open_input(
                InputPurpose::Setting(self.settings_selection),
                label.clone(),
                value.clone(),
            );
        }
    }

    pub fn selected_profile_id(&self) -> Option<String> {
        self.profiles
            .profiles
            .get(self.profile_selection)
            .map(|profile| profile.id.clone())
    }

    pub fn active_profile(&self) -> Option<&crate::domain::profile::UserProfile> {
        self.profiles.active_profile()
    }

    pub fn report_paths(&self) -> (PathBuf, PathBuf) {
        let date = chrono::Utc::now().format("%Y-%m-%d");
        let md = PathBuf::from(format!("docs/results/experiment-{date}.md"));
        let json = PathBuf::from(format!("docs/results/experiment-{date}.json"));
        (md, json)
    }

    pub fn personalization_report_paths(&self) -> (PathBuf, PathBuf) {
        let date = chrono::Utc::now().format("%Y-%m-%d");
        let md = PathBuf::from(format!("docs/results/personalization-{date}.md"));
        let json = PathBuf::from(format!("docs/results/personalization-{date}.json"));
        (md, json)
    }

    /// Вопрос для сравнения профилей: пустой ввод возвращает дефолтный.
    pub fn personalization_question(&self) -> String {
        let question = self.personalization_question.trim();
        if question.is_empty() {
            SAME_QUESTION.to_string()
        } else {
            question.to_string()
        }
    }
}
