//! Единый контроллер для TUI и CLI. Никаких `process::exit` — коды только в CLI.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use chrono::Utc;
use thiserror::Error;

use crate::domain::chat::{ExchangeKind, LlmExchange, TokenStats, TurnOutcome};
use crate::domain::memory::{
    ChatMessage, LtmKind, LtmState, MemoryBreakdown, MemoryBundle, MemoryLayer, MemorySource,
    PendingWrite, StmState, WmKind, WmStatus, WorkingMemoryState, WriteOp,
};
use crate::domain::profile::{ProfileState, UserProfile};
use crate::domain::settings::{Approval, ExtractionMode, Settings};
use crate::experiment::personalization::{
    PersonalizationError, PersonalizationOptions, PersonalizationProgress, PersonalizationReport,
    run_personalization, save_personalization_report,
};
use crate::experiment::report::save_report;
use crate::experiment::runner::{
    ExperimentError, ExperimentOptions, ExperimentProgress, ExperimentReport, run_experiment,
};
use crate::llm::agent::{AgentError, Progress, TurnInput, estimate_breakdown, run_turn};
use crate::llm::client::ReqwestClient;
use crate::llm::dto::ChatClient;
use crate::llm::tools::ToolRegistry;
use crate::memory::SessionAudit;
use crate::memory::audit::{contains_secret, make_event, mask_secret};
use crate::memory::extractor::{
    CandidateExtractor, LlmCandidateExtractor, LocalCandidateExtractor,
};
use crate::memory::ltm::{
    UpsertLtm, active_ltm, find_active_ltm, render_ltm_profile, replace_ltm, set_pinned,
};
use crate::memory::migration::{
    MigrationError, MigrationReport, run_import, save_report as save_migration_report,
};
use crate::memory::personalization::{ProfileChange, learn_from_message};
use crate::memory::prompt::PromptError;
use crate::memory::router::{RouteError, RouteInput, queue_pending, route_candidates};
use crate::memory::stm::select_stm_window;
use crate::memory::tokens::{
    estimate_message_tokens, estimate_messages_tokens, estimate_tools_tokens,
};
use crate::memory::tool_write::{MemoryWriteOutcome, MemoryWriteRequest, apply_memory_write};
use crate::memory::working::{
    WmUpsert, archive_task, empty_working, prune_wm, remove_wm_item, upsert_wm_item,
};
use crate::storage::atomic::{SaveError, atomic_write_json};
use crate::storage::branches::{BranchFile, BranchSnapshot};
use crate::storage::paths::DataPaths;
use crate::storage::{
    LoadError, acquire_lock, load_branch_file, load_bundle, load_profiles, save_branch_file,
    save_bundle, save_profiles, save_settings, save_stm,
};

#[derive(Clone)]
pub struct AppRuntime {
    pub paths: DataPaths,
    pub settings: Settings,
    pub client: Arc<dyn ChatClient>,
}

impl AppRuntime {
    pub fn new(paths: DataPaths, settings: Settings, client: Arc<dyn ChatClient>) -> Self {
        Self {
            paths,
            settings,
            client,
        }
    }
}

impl std::fmt::Debug for AppRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AppRuntime")
            .field("paths", &self.paths)
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("не удалось загрузить слой: {0}")]
    Load(#[from] LoadError),
    #[error("не удалось сохранить слой: {0}")]
    Save(#[from] SaveError),
    #[error("ошибка маршрутизации: {0}")]
    Route(#[from] RouteError),
    #[error("ошибка промпта: {0}")]
    Prompt(#[from] PromptError),
    #[error("ошибка агента: {0}")]
    Agent(#[from] AgentError),
    #[error("ошибка импорта: {0}")]
    Migration(#[from] MigrationError),
    #[error("ошибка эксперимента: {0}")]
    Experiment(#[from] ExperimentError),
    #[error("ошибка персонализации: {0}")]
    Personalization(#[from] PersonalizationError),
    #[error("неверный ввод: {0}")]
    Invalid(String),
    #[error("не найдено: {0}")]
    NotFound(String),
    #[error("ошибка ввода-вывода: {0}")]
    Io(String),
}

pub struct AppController {
    runtime: AppRuntime,
    bundle: MemoryBundle,
    profiles: ProfileState,
    pub audit: SessionAudit,
    pub exchanges: Vec<LlmExchange>,
    pub last_breakdown: MemoryBreakdown,
    pub last_profile_changes: Vec<ProfileChange>,
}

impl AppController {
    pub fn load(runtime: AppRuntime) -> Result<Self, AppError> {
        runtime
            .paths
            .ensure()
            .map_err(|error| AppError::Io(error.to_string()))?;
        let now = Utc::now();
        let mut bundle = load_bundle(&runtime.paths)?;
        prune_wm(&mut bundle.wm, &runtime.settings.memory.wm, now);
        crate::memory::ltm::prune_ltm(&mut bundle.ltm, &runtime.settings.memory.ltm);
        let (mut profiles, profiles_missing) = match load_profiles(&runtime.paths) {
            Ok(profiles) => (profiles, false),
            Err(error) if error.is_not_found() => (ProfileState::with_presets(now), true),
            Err(error) => return Err(AppError::Load(error)),
        };
        let profiles_changed = profiles.ensure_defaults(now);
        if profiles_changed || profiles_missing {
            save_profiles(&runtime.paths, &profiles)?;
        }
        Ok(Self {
            runtime,
            bundle,
            profiles,
            audit: SessionAudit::default(),
            exchanges: Vec::new(),
            last_breakdown: MemoryBreakdown::default(),
            last_profile_changes: Vec::new(),
        })
    }

    pub fn runtime(&self) -> &AppRuntime {
        &self.runtime
    }

    pub fn bundle(&self) -> &MemoryBundle {
        &self.bundle
    }

    pub fn settings(&self) -> &Settings {
        &self.runtime.settings
    }

    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.runtime.settings
    }

    pub fn profiles(&self) -> &ProfileState {
        &self.profiles
    }

    pub fn active_profile(&self) -> Option<&UserProfile> {
        self.profiles.active_profile()
    }

    pub fn profile(&self, id: Option<&str>) -> Option<&UserProfile> {
        match id {
            Some(id) => self.profiles.find(id),
            None => self.profiles.active_profile(),
        }
    }

    pub fn save_profiles_state(&self) -> Result<(), AppError> {
        save_profiles(&self.runtime.paths, &self.profiles)?;
        Ok(())
    }

    pub fn set_active_profile(&mut self, id: &str) -> Result<(), AppError> {
        if !self.profiles.set_active(id, Utc::now()) {
            return Err(AppError::NotFound(format!("профиль {id}")));
        }
        self.save_profiles_state()
    }

    pub fn upsert_profile(&mut self, mut profile: UserProfile) -> Result<(), AppError> {
        if profile.id.trim().is_empty() {
            return Err(AppError::Invalid("id профиля не может быть пустым".into()));
        }
        let mut fields = vec![
            profile.id.clone(),
            profile.title.clone(),
            profile.identity.name.clone().unwrap_or_default(),
            profile.identity.role.clone().unwrap_or_default(),
            profile.identity.language.clone(),
        ];
        fields.extend(profile.constraints.iter().cloned());
        if fields.iter().any(|value| contains_secret(value)) {
            return Err(AppError::Invalid(
                "секрет нельзя сохранить в профиль".into(),
            ));
        }
        let existing = self.profiles.find(&profile.id).is_some();
        if existing {
            profile.revision = profile.revision.saturating_add(1);
        }
        profile.updated_at = Utc::now();
        if profile.created_at > profile.updated_at {
            profile.created_at = profile.updated_at;
        }
        self.profiles.upsert(profile, Utc::now());
        self.save_profiles_state()
    }

    pub fn delete_profile(&mut self, id: &str) -> Result<(), AppError> {
        if self.profiles.profiles.len() <= 1 {
            return Err(AppError::Invalid(
                "нельзя удалить единственный профиль".into(),
            ));
        }
        if !self.profiles.remove(id, Utc::now()) {
            return Err(AppError::NotFound(format!("профиль {id}")));
        }
        self.save_profiles_state()
    }

    pub fn learn_profile_from_text(&mut self, text: &str) -> Result<Vec<ProfileChange>, AppError> {
        let now = Utc::now();
        let Some(profile) = self.profiles.active_profile_mut() else {
            return Err(AppError::Invalid("нет активного профиля".into()));
        };
        let changes = learn_from_message(profile, text, now);
        if !changes.is_empty() {
            self.save_profiles_state()?;
        }
        Ok(changes)
    }

    pub fn save_settings(&self) -> Result<(), AppError> {
        save_settings(&self.runtime.paths, &self.runtime.settings)?;
        Ok(())
    }

    pub fn acquire_lock(&self) -> Result<crate::storage::LockGuard, crate::storage::LockError> {
        acquire_lock(&self.runtime.paths)
    }

    pub fn save_all(&self) -> Result<(), AppError> {
        save_bundle(&self.runtime.paths, &self.bundle)?;
        Ok(())
    }

    fn save_ltm(&self) -> Result<(), AppError> {
        crate::storage::save_ltm(&self.runtime.paths, &self.bundle.ltm)?;
        Ok(())
    }

    fn save_wm(&self) -> Result<(), AppError> {
        crate::storage::save_working(&self.runtime.paths, &self.bundle.wm)?;
        Ok(())
    }

    fn save_pending(&self) -> Result<(), AppError> {
        crate::storage::save_pending(&self.runtime.paths, &self.bundle.pending)?;
        Ok(())
    }

    fn save_stm_state(&self) -> Result<(), AppError> {
        save_stm(&self.runtime.paths, &self.bundle.stm)?;
        crate::storage::save_meta(&self.runtime.paths, &self.bundle.meta)?;
        Ok(())
    }

    fn push_audit(&mut self, event: crate::domain::memory::MemoryWriteEvent) {
        self.audit.push(event);
    }

    pub fn send_user_message(
        &mut self,
        text: &str,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<TurnOutcome, AppError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AppError::Invalid("пустое сообщение".into()));
        }
        let now = Utc::now();
        let safe_text = if contains_secret(text) {
            mask_secret(text)
        } else {
            text.to_string()
        };
        let user_message = ChatMessage::user(safe_text.clone());
        self.bundle.stm.messages.push(user_message.clone());
        self.bundle.stm.updated_at = now;

        // Профиль пополняется из диалога до сборки промпта: новое
        // предпочтение действует уже на этот ответ.
        let profile_changes = match self.profiles.active_profile_mut() {
            Some(profile) => learn_from_message(profile, &safe_text, now),
            None => Vec::new(),
        };
        if !profile_changes.is_empty() {
            self.save_profiles_state()?;
        }
        self.last_profile_changes = profile_changes.clone();
        let profile_snapshot = self.profiles.active_profile().cloned();

        let mode = self.runtime.settings.memory.extraction_mode;
        let result = match mode {
            ExtractionMode::Local => self.route_with(&LocalCandidateExtractor, now),
            ExtractionMode::Llm => {
                let extractor = LlmCandidateExtractor::new(
                    self.runtime.client.clone(),
                    &self.runtime.settings.model,
                );
                match self.route_with(&extractor, now) {
                    Ok(outcome) => Ok(outcome),
                    Err(error) => {
                        let mut outcome = self.route_with(&LocalCandidateExtractor, now)?;
                        outcome.stats.error = Some(error.to_string());
                        Ok(outcome)
                    }
                }
            }
        };
        let outcome = result?;
        self.bundle.wm = outcome.wm;
        self.bundle.ltm = outcome.ltm;
        self.bundle.pending = outcome.pending;
        self.bundle.meta.last_route_at = Some(now);
        self.bundle.meta.processed_messages = self.bundle.stm.messages.len();
        self.push_audit_events(outcome.audit);
        // Сначала сохраняем уже принятые решения: ошибка модели их не теряет.
        self.save_all()?;

        let tools = ToolRegistry::default_registry();
        let cancel = AtomicBool::new(false);
        let turn = run_turn(
            TurnInput {
                user_text: &safe_text,
                memory: self.bundle.clone(),
                settings: &self.runtime.settings,
                tools: &tools,
                client: self.runtime.client.as_ref(),
                cancel: &cancel,
                profile: profile_snapshot.as_ref(),
            },
            progress,
        );
        match turn {
            Ok(mut outcome) => {
                outcome.profile_id = profile_snapshot.as_ref().map(|profile| profile.id.clone());
                outcome.profile_changes = profile_changes.clone();
                self.bundle = outcome.memory.clone();
                self.push_audit_events(outcome.audit.clone());
                self.last_breakdown = outcome.breakdown.clone();
                self.save_all()?;
                self.record_exchange(
                    ExchangeKind::Chat,
                    &outcome.token_stats,
                    Some(outcome.breakdown.clone()),
                    Some(outcome.final_text.clone()),
                    "ok",
                );
                Ok(outcome)
            }
            Err(error) => {
                self.save_all()?;
                self.record_exchange(
                    ExchangeKind::Chat,
                    &TokenStats::default(),
                    None,
                    None,
                    "error",
                );
                Err(AppError::Agent(error))
            }
        }
    }

    fn route_with(
        &self,
        extractor: &dyn CandidateExtractor,
        now: chrono::DateTime<Utc>,
    ) -> Result<crate::memory::router::RouteOutcome, AppError> {
        let Some(message) = self.bundle.stm.messages.last() else {
            return Err(AppError::Invalid("нет пользовательского сообщения".into()));
        };
        let message_index = self.bundle.stm.messages.len().saturating_sub(1);
        let outcome = route_candidates(
            RouteInput {
                message,
                message_index,
                wm: &self.bundle.wm,
                ltm: &self.bundle.ltm,
                pending: &self.bundle.pending,
                config: &self.runtime.settings.memory,
                now,
            },
            extractor,
        )?;
        Ok(outcome)
    }

    fn push_audit_events(
        &mut self,
        events: impl IntoIterator<Item = crate::domain::memory::MemoryWriteEvent>,
    ) {
        for event in events {
            self.push_audit(event);
        }
    }

    fn record_exchange(
        &mut self,
        kind: ExchangeKind,
        stats: &TokenStats,
        breakdown: Option<MemoryBreakdown>,
        response: Option<String>,
        status: &str,
    ) {
        let mut exchange = LlmExchange::new(
            kind,
            &self.runtime.settings.model,
            &self.runtime.settings.base_url,
        );
        exchange.prompt_tokens = stats.prompt_tokens;
        exchange.completion_tokens = stats.completion_tokens;
        exchange.total_tokens = stats.total_tokens;
        exchange.cost_usd = stats.cost_usd;
        exchange.memory = breakdown;
        exchange.status = status.to_string();
        exchange.response_preview = response.map(|text| text.chars().take(200).collect());
        self.exchanges.push(exchange);
        if self.exchanges.len() > 200 {
            self.exchanges.remove(0);
        }
    }

    pub fn accept_pending(&mut self, id: &str) -> Result<(), AppError> {
        let now = Utc::now();
        let position = self
            .bundle
            .pending
            .iter()
            .position(|item| item.id == id)
            .ok_or_else(|| AppError::NotFound(format!("pending {id}")))?;
        let pending = self.bundle.pending.remove(position);
        let existing = find_active_ltm(&self.bundle.ltm, &pending.candidate.key).is_some();
        let input = UpsertLtm {
            kind: pending.target_kind,
            key: pending.candidate.key.clone(),
            value: pending.candidate.value.clone(),
            tags: pending.candidate.tags.clone(),
            source: pending
                .candidate
                .legacy_category
                .as_ref()
                .map_or(MemorySource::Router, |_| MemorySource::MigrationFacts),
            confidence: pending.candidate.confidence,
            pinned: false,
            legacy_key: pending.candidate.legacy_category.clone(),
        };
        if existing {
            replace_ltm(&mut self.bundle.ltm, input, now);
        } else {
            crate::memory::ltm::upsert_ltm(&mut self.bundle.ltm, input, now);
        }
        let event = make_event(
            MemoryLayer::Ltm,
            WriteOp::Accept,
            &pending.candidate.key,
            Some(&pending.candidate.value),
            MemorySource::User,
            Some(&pending.rule_id),
            "pending подтверждён",
            Some(pending.message_index),
            now,
        );
        self.push_audit(event);
        self.save_ltm()?;
        self.save_pending()?;
        Ok(())
    }

    pub fn reject_pending(&mut self, id: &str) -> Result<(), AppError> {
        let now = Utc::now();
        let position = self
            .bundle
            .pending
            .iter()
            .position(|item| item.id == id)
            .ok_or_else(|| AppError::NotFound(format!("pending {id}")))?;
        let pending = self.bundle.pending.remove(position);
        let event = make_event(
            MemoryLayer::Ltm,
            WriteOp::Reject,
            &pending.candidate.key,
            Some(&pending.candidate.value),
            MemorySource::User,
            Some(&pending.rule_id),
            "pending отклонён",
            Some(pending.message_index),
            now,
        );
        self.push_audit(event);
        self.save_pending()?;
        Ok(())
    }

    pub fn edit_pending_candidate(
        &mut self,
        id: &str,
        key: &str,
        value: &str,
        kind: &str,
    ) -> Result<(), AppError> {
        let now = Utc::now();
        let parsed = LtmKind::from_label(kind).unwrap_or(LtmKind::Knowledge);
        let pending = self
            .bundle
            .pending
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| AppError::NotFound(format!("pending {id}")))?;
        if key.trim().is_empty() || value.trim().is_empty() {
            return Err(AppError::Invalid(
                "key и value не могут быть пустыми".into(),
            ));
        }
        let old_key = pending.candidate.key.clone();
        pending.candidate.key = crate::memory::audit::sanitize_key(key);
        pending.candidate.value = crate::memory::audit::sanitize_value(value);
        pending.candidate.kind = parsed;
        pending.target_kind = parsed;
        let event = make_event(
            MemoryLayer::Ltm,
            WriteOp::Update,
            &pending.candidate.key,
            Some(&pending.candidate.value),
            MemorySource::User,
            Some(&pending.rule_id),
            &format!("pending изменён: {old_key}"),
            Some(pending.message_index),
            now,
        );
        self.push_audit(event);
        self.save_pending()?;
        Ok(())
    }

    pub fn change_pending_layer(&mut self, id: &str, layer: MemoryLayer) -> Result<(), AppError> {
        let now = Utc::now();
        let position = self
            .bundle
            .pending
            .iter()
            .position(|item| item.id == id)
            .ok_or_else(|| AppError::NotFound(format!("pending {id}")))?;
        match layer {
            MemoryLayer::Ltm => {
                self.accept_pending(id)?;
            }
            MemoryLayer::Wm => {
                let pending = self.bundle.pending.remove(position);
                let kind = wm_kind_for_key(&pending.candidate.key);
                let order = pending
                    .candidate
                    .key
                    .strip_prefix("plan.")
                    .and_then(|rest| rest.parse::<u32>().ok());
                upsert_wm_item(
                    &mut self.bundle.wm,
                    WmUpsert {
                        kind,
                        key: pending.candidate.key.clone(),
                        value: pending.candidate.value.clone(),
                        status: WmStatus::Active,
                        order,
                        ref_ltm_id: None,
                        source_message_index: Some(pending.message_index),
                    },
                    now,
                );
                self.push_audit(make_event(
                    MemoryLayer::Wm,
                    WriteOp::Demote,
                    &pending.candidate.key,
                    Some(&pending.candidate.value),
                    MemorySource::User,
                    Some(&pending.rule_id),
                    "pending перенесён в WM",
                    Some(pending.message_index),
                    now,
                ));
                self.save_wm()?;
                self.save_pending()?;
            }
            MemoryLayer::Stm => {
                return Err(AppError::Invalid("pending нельзя перенести в STM".into()));
            }
        }
        Ok(())
    }

    pub fn clear_layer(&mut self, layer: MemoryLayer) -> Result<(), AppError> {
        let now = Utc::now();
        let (_count, reason) = match layer {
            MemoryLayer::Stm => {
                let count = self.bundle.stm.messages.len();
                self.bundle.stm = StmState::empty(now);
                self.bundle.meta.processed_messages = 0;
                self.save_stm_state()?;
                (count, format!("очищено сообщений: {count}"))
            }
            MemoryLayer::Wm => {
                let count = self.bundle.wm.items.len();
                self.bundle.wm = empty_working(now);
                self.save_wm()?;
                (count, format!("очищено пунктов: {count}"))
            }
            MemoryLayer::Ltm => {
                let count = self.bundle.ltm.records.len();
                self.bundle.ltm = LtmState::empty(now);
                self.bundle.pending.clear();
                self.save_ltm()?;
                self.save_pending()?;
                (count, format!("очищено записей: {count}, pending очищен"))
            }
        };
        self.push_audit(make_event(
            layer,
            WriteOp::Clear,
            "*",
            None,
            MemorySource::User,
            None,
            &reason,
            None,
            now,
        ));
        Ok(())
    }

    pub fn archive_task(&mut self) -> Result<(), AppError> {
        let now = Utc::now();
        archive_task(&mut self.bundle.wm, now);
        self.push_audit(make_event(
            MemoryLayer::Wm,
            WriteOp::Update,
            "*",
            None,
            MemorySource::User,
            None,
            "задача завершена",
            None,
            now,
        ));
        self.save_wm()?;
        Ok(())
    }

    pub fn import_facts(&mut self, path: &Path) -> Result<MigrationReport, AppError> {
        let now = Utc::now();
        let report = run_import(&mut self.bundle.ltm, path, now)?;
        self.save_ltm()?;
        save_migration_report(&self.runtime.paths, &report)?;
        self.push_audit(make_event(
            MemoryLayer::Ltm,
            WriteOp::Create,
            "*",
            None,
            MemorySource::MigrationFacts,
            None,
            &format!(
                "import-facts: added={}, updated={}, skipped={}",
                report.added, report.updated, report.skipped
            ),
            None,
            now,
        ));
        Ok(report)
    }

    pub fn run_experiment(
        &mut self,
        options: ExperimentOptions,
        progress: &mut dyn FnMut(ExperimentProgress),
    ) -> Result<ExperimentReport, AppError> {
        self.run_experiment_cancellable(options, &AtomicBool::new(false), progress)
    }

    pub fn run_experiment_cancellable(
        &mut self,
        options: ExperimentOptions,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(ExperimentProgress),
    ) -> Result<ExperimentReport, AppError> {
        // Эксперимент воспроизводим: если в data-dir нет контрольной фикстуры,
        // используем её детерминированную копию (07, раздел 2).
        let fixture = crate::experiment::fixtures::seeded_bundle(Utc::now());
        let use_fixture = self.bundle.stm.messages.len() < 34 || self.bundle.wm.items.is_empty();
        let bundle = if use_fixture { &fixture } else { &self.bundle };
        let mut report = run_experiment(
            bundle,
            options,
            &self.runtime.settings,
            self.runtime.client.as_ref(),
            cancel,
            progress,
        )?;
        report.data_dir = Some(self.runtime.paths.root.display().to_string());
        if std::env::var("LAB11_FAKE_LLM").as_deref() == Ok("1") {
            report.source = "fake (LAB11_FAKE_LLM=1)".into();
        }
        let (prompt_tokens, completion_tokens, cost_usd) = report.usage_totals();
        self.record_exchange(
            ExchangeKind::Experiment,
            &TokenStats {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
                cost_usd,
                reported: true,
            },
            None,
            None,
            if report.errors.is_empty() {
                "ok"
            } else {
                "partial"
            },
        );
        Ok(report)
    }

    pub fn save_report(
        &self,
        report: &ExperimentReport,
        out: &Path,
        json: &Path,
    ) -> Result<(), AppError> {
        save_report(report, out, json)?;
        Ok(())
    }

    pub fn run_personalization(
        &mut self,
        options: PersonalizationOptions,
        progress: &mut dyn FnMut(PersonalizationProgress),
    ) -> Result<PersonalizationReport, AppError> {
        self.run_personalization_cancellable(options, &AtomicBool::new(false), progress)
    }

    pub fn run_personalization_cancellable(
        &mut self,
        options: PersonalizationOptions,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(PersonalizationProgress),
    ) -> Result<PersonalizationReport, AppError> {
        let mut report = run_personalization(
            &self.profiles,
            &self.runtime.settings,
            self.runtime.client.as_ref(),
            &options,
            cancel,
            progress,
        )?;
        report.data_dir = Some(self.runtime.paths.root.display().to_string());
        if fake_llm_enabled() {
            report.source = "fake (LAB12_FAKE_LLM=1)".into();
        }
        let (prompt_tokens, completion_tokens, cost_usd) = report.usage_totals();
        self.record_exchange(
            ExchangeKind::Experiment,
            &TokenStats {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
                cost_usd,
                reported: true,
            },
            None,
            None,
            if report.errors.is_empty() {
                "ok"
            } else {
                "partial"
            },
        );
        Ok(report)
    }

    pub fn save_personalization_report(
        &self,
        report: &PersonalizationReport,
        out: &Path,
        json: &Path,
    ) -> Result<(), AppError> {
        save_personalization_report(report, out, json)?;
        Ok(())
    }

    pub fn preview_breakdown(&self) -> MemoryBreakdown {
        let tools = ToolRegistry::default_registry();
        estimate_breakdown(
            &self.bundle,
            &self.runtime.settings,
            &tools,
            self.profiles.active_profile(),
        )
    }

    pub fn branch_file(&self) -> Result<BranchFile, AppError> {
        match load_branch_file(&self.runtime.paths) {
            Ok(file) => Ok(file),
            Err(error) if error.is_not_found() => Ok(BranchFile::default()),
            Err(error) => Err(AppError::Load(error)),
        }
    }

    pub fn checkpoint_branch(&mut self, label: &str) -> Result<String, AppError> {
        let now = Utc::now();
        let mut file = self.branch_file()?;
        let id = format!("branch_{}", uuid::Uuid::new_v4().simple());
        file.branches.push(BranchSnapshot {
            id: id.clone(),
            label: label.to_string(),
            created_at: now,
            stm: self.bundle.stm.clone(),
            wm: self.bundle.wm.clone(),
            ltm: self.bundle.ltm.clone(),
            pending: self.bundle.pending.clone(),
            meta: self.bundle.meta.clone(),
        });
        file.active_branch = Some(id.clone());
        save_branch_file(&self.runtime.paths, &file)?;
        Ok(id)
    }

    pub fn list_branches(&self) -> Result<BranchFile, AppError> {
        self.branch_file()
    }

    pub fn switch_branch(&mut self, id: &str) -> Result<(), AppError> {
        let mut file = self.branch_file()?;
        let snapshot = file
            .branches
            .iter()
            .find(|branch| branch.id == id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("ветка {id}")))?;
        self.bundle.stm = snapshot.stm;
        self.bundle.wm = snapshot.wm;
        self.bundle.ltm = snapshot.ltm;
        self.bundle.pending = snapshot.pending;
        self.bundle.meta = snapshot.meta;
        self.save_all()?;
        file.active_branch = Some(id.to_string());
        save_branch_file(&self.runtime.paths, &file)?;
        Ok(())
    }

    pub fn delete_branch(&mut self, id: &str) -> Result<(), AppError> {
        let mut file = self.branch_file()?;
        file.branches.retain(|branch| branch.id != id);
        if file.active_branch.as_deref() == Some(id) {
            file.active_branch = None;
        }
        save_branch_file(&self.runtime.paths, &file)?;
        Ok(())
    }

    // ----- Ручные операции (CLI/TUI) -----

    pub fn memory_add(
        &mut self,
        layer: MemoryLayer,
        key: &str,
        value: &str,
        kind: Option<&str>,
        pinned: bool,
    ) -> Result<String, AppError> {
        self.memory_upsert(layer, key, value, kind, pinned, false)
    }

    pub fn memory_update(
        &mut self,
        layer: MemoryLayer,
        key: &str,
        value: &str,
        kind: Option<&str>,
    ) -> Result<String, AppError> {
        self.memory_upsert(layer, key, value, kind, false, true)
    }

    fn memory_upsert(
        &mut self,
        layer: MemoryLayer,
        key: &str,
        value: &str,
        kind: Option<&str>,
        pinned: bool,
        update: bool,
    ) -> Result<String, AppError> {
        if key.trim().is_empty() {
            return Err(AppError::Invalid("пустой key".into()));
        }
        if value.trim().is_empty() {
            return Err(AppError::Invalid("пустое value".into()));
        }
        let now = Utc::now();
        match layer {
            MemoryLayer::Ltm => {
                let kind = kind
                    .and_then(LtmKind::from_label)
                    .unwrap_or(LtmKind::Knowledge);
                if update && find_active_ltm(&self.bundle.ltm, key).is_none() {
                    return Err(AppError::NotFound(format!("ключ {key}")));
                }
                let change = crate::memory::ltm::upsert_ltm(
                    &mut self.bundle.ltm,
                    UpsertLtm {
                        kind,
                        key: key.to_string(),
                        value: value.to_string(),
                        tags: Vec::new(),
                        source: MemorySource::User,
                        confidence: 0.9,
                        pinned,
                        legacy_key: None,
                    },
                    now,
                );
                if pinned {
                    set_pinned(&mut self.bundle.ltm, key, true, now);
                }
                let label = match change {
                    crate::memory::ltm::LtmChange::Created(_) => "created",
                    crate::memory::ltm::LtmChange::Same(_) => "same",
                    _ => "updated",
                };
                self.push_audit(make_event(
                    MemoryLayer::Ltm,
                    if label == "created" {
                        WriteOp::Create
                    } else {
                        WriteOp::Update
                    },
                    key,
                    Some(value),
                    MemorySource::User,
                    None,
                    "ручная запись",
                    None,
                    now,
                ));
                self.save_ltm()?;
                Ok(label.to_string())
            }
            MemoryLayer::Wm => {
                let kind = kind
                    .and_then(WmKind::from_label)
                    .unwrap_or_else(|| wm_kind_for_key(key));
                if update {
                    let exists = self.bundle.wm.items.iter().any(|item| item.key == key);
                    if !exists {
                        return Err(AppError::NotFound(format!("ключ {key}")));
                    }
                }
                let change = upsert_wm_item(
                    &mut self.bundle.wm,
                    WmUpsert {
                        kind,
                        key: key.to_string(),
                        value: value.to_string(),
                        status: WmStatus::Active,
                        order: key
                            .strip_prefix("plan.")
                            .and_then(|rest| rest.parse::<u32>().ok()),
                        ref_ltm_id: None,
                        source_message_index: None,
                    },
                    now,
                );
                self.push_audit(make_event(
                    MemoryLayer::Wm,
                    WriteOp::Create,
                    key,
                    Some(value),
                    MemorySource::User,
                    None,
                    "ручная запись",
                    None,
                    now,
                ));
                self.save_wm()?;
                Ok(match change {
                    crate::memory::working::WmChange::Created(_) => "created",
                    crate::memory::working::WmChange::Same(_) => "same",
                    _ => "updated",
                }
                .to_string())
            }
            MemoryLayer::Stm => Err(AppError::Invalid(
                "STM заполняется сообщениями, а не ручными записями".into(),
            )),
        }
    }

    pub fn memory_delete(&mut self, layer: MemoryLayer, key: &str) -> Result<usize, AppError> {
        let now = Utc::now();
        let removed = match layer {
            MemoryLayer::Ltm => {
                let before = active_ltm(&self.bundle.ltm).count();
                crate::memory::ltm::remove_ltm_key(&mut self.bundle.ltm, key);
                let after = active_ltm(&self.bundle.ltm).count();
                before.saturating_sub(after)
            }
            MemoryLayer::Wm => {
                let ids: Vec<String> = self
                    .bundle
                    .wm
                    .items
                    .iter()
                    .filter(|item| item.key == key || item.id == key)
                    .map(|item| item.id.clone())
                    .collect();
                for id in &ids {
                    remove_wm_item(&mut self.bundle.wm, id);
                }
                ids.len()
            }
            MemoryLayer::Stm => {
                let index = key.parse::<usize>().ok();
                match index
                    .and_then(|index| (index < self.bundle.stm.messages.len()).then_some(index))
                {
                    Some(index) => {
                        self.bundle.stm.messages.remove(index);
                        self.save_stm_state()?;
                        1
                    }
                    None => 0,
                }
            }
        };
        if removed == 0 {
            return Err(AppError::NotFound(format!("запись {key}")));
        }
        self.push_audit(make_event(
            layer,
            WriteOp::Delete,
            key,
            None,
            MemorySource::User,
            None,
            "удаление вручную",
            None,
            now,
        ));
        match layer {
            MemoryLayer::Ltm => self.save_ltm()?,
            MemoryLayer::Wm => self.save_wm()?,
            MemoryLayer::Stm => self.save_stm_state()?,
        }
        Ok(removed)
    }

    pub fn memory_move(
        &mut self,
        from: MemoryLayer,
        key: &str,
        to: MemoryLayer,
    ) -> Result<String, AppError> {
        let now = Utc::now();
        match (from, to) {
            (MemoryLayer::Wm, MemoryLayer::Ltm) => {
                let item = self
                    .bundle
                    .wm
                    .items
                    .iter()
                    .find(|item| item.key == key || item.id == key)
                    .cloned()
                    .ok_or_else(|| AppError::NotFound(format!("WM {key}")))?;
                if self.runtime.settings.memory.approval == Approval::Confirm {
                    let candidate = crate::domain::memory::MemoryCandidate {
                        key: item.key.clone(),
                        value: item.value.clone(),
                        kind: LtmKind::Knowledge,
                        tags: vec!["from-wm".into()],
                        confidence: 0.9,
                        scope: crate::domain::memory::CandidateScope::Project,
                        ttl_turns: None,
                        legacy_category: None,
                    };
                    queue_pending(
                        &mut self.bundle.pending,
                        &candidate,
                        &item.key,
                        &item.value,
                        None,
                        "PROMOTE",
                        "WM → LTM через pending",
                        0,
                        now,
                    );
                    self.save_pending()?;
                    Ok("pending".into())
                } else {
                    crate::memory::ltm::upsert_ltm(
                        &mut self.bundle.ltm,
                        UpsertLtm::new(
                            LtmKind::Knowledge,
                            item.key.clone(),
                            item.value.clone(),
                            MemorySource::User,
                        ),
                        now,
                    );
                    if let Some(record) = find_active_ltm(&self.bundle.ltm, &item.key) {
                        let record_id = record.id.clone();
                        if let Some(wm_item) = self
                            .bundle
                            .wm
                            .items
                            .iter_mut()
                            .find(|candidate| candidate.id == item.id)
                        {
                            wm_item.ref_ltm_id = Some(record_id);
                        }
                    }
                    self.save_wm()?;
                    self.save_ltm()?;
                    Ok("created".into())
                }
            }
            (MemoryLayer::Ltm, MemoryLayer::Wm) => {
                let record = find_active_ltm(&self.bundle.ltm, key)
                    .ok_or_else(|| AppError::NotFound(format!("LTM {key}")))?
                    .clone();
                upsert_wm_item(
                    &mut self.bundle.wm,
                    WmUpsert {
                        kind: WmKind::Entity,
                        key: record.key.clone(),
                        value: record.value.clone(),
                        status: WmStatus::Active,
                        order: None,
                        ref_ltm_id: Some(record.id.clone()),
                        source_message_index: None,
                    },
                    now,
                );
                self.save_wm()?;
                Ok("created".into())
            }
            (from, to) => Err(AppError::Invalid(format!(
                "перенос {} → {} недоступен",
                from.as_str(),
                to.as_str()
            ))),
        }
    }

    pub fn memory_pin(&mut self, key: &str, pinned: bool) -> Result<(), AppError> {
        if !set_pinned(&mut self.bundle.ltm, key, pinned, Utc::now()) {
            return Err(AppError::NotFound(format!("LTM {key}")));
        }
        self.save_ltm()?;
        Ok(())
    }

    pub fn pending_list(&self) -> &[PendingWrite] {
        &self.bundle.pending
    }

    pub fn export_history(&self, path: &Path) -> Result<(), AppError> {
        atomic_write_json(path, &self.bundle.stm)?;
        Ok(())
    }

    pub fn import_history(&mut self, path: &Path) -> Result<(), AppError> {
        let raw = std::fs::read_to_string(path)
            .map_err(|error| AppError::Io(format!("{}: {error}", path.display())))?;
        let mut state: StmState = serde_json::from_str(&raw)
            .map_err(|error| AppError::Invalid(format!("история: {error}")))?;
        if state.version != crate::domain::memory::STATE_VERSION {
            return Err(AppError::Invalid(format!(
                "неподдерживаемая версия истории: {}",
                state.version
            )));
        }
        state.updated_at = Utc::now();
        self.bundle.stm = state;
        self.bundle.meta.processed_messages = 0;
        self.save_stm_state()?;
        Ok(())
    }

    pub fn dump_json(&self) -> Result<String, AppError> {
        serde_json::to_string_pretty(&self.bundle).map_err(|error| AppError::Io(error.to_string()))
    }

    pub fn view_ltm_profile(&self) -> String {
        render_ltm_profile(
            &self.bundle.ltm,
            self.runtime.settings.memory.ltm.profile_max_tokens,
        )
        .text
    }

    pub fn last_exchange(&self) -> Option<&LlmExchange> {
        self.exchanges.last()
    }

    pub fn estimate_history_tokens(&self) -> usize {
        self.bundle
            .stm
            .messages
            .iter()
            .map(estimate_message_tokens)
            .sum()
    }

    pub fn estimate_prompt_tokens(&self) -> usize {
        let tools = ToolRegistry::default_registry();
        let schemas = tools.schemas();
        let profile_tokens = self
            .profiles
            .active_profile()
            .map(|profile| crate::memory::personalization::render_profile_block(profile).tokens)
            .unwrap_or(0);
        estimate_messages_tokens(&self.bundle.stm.messages)
            + estimate_tools_tokens(&schemas)
            + profile_tokens
    }

    pub fn stm_window(&self) -> crate::domain::memory::StmWindow {
        select_stm_window(
            &self.bundle.stm,
            self.runtime.settings.memory.stm.window,
            self.runtime.settings.memory.stm.max_tokens,
        )
    }

    pub fn apply_tool_write(
        &mut self,
        request: &MemoryWriteRequest,
        source: MemorySource,
    ) -> Result<MemoryWriteOutcome, AppError> {
        let (outcome, event) = apply_memory_write(
            &mut self.bundle,
            request,
            source,
            self.runtime.settings.memory.approval,
            Utc::now(),
        );
        if let Some(event) = event {
            self.push_audit(event);
        }
        self.save_all()?;
        Ok(outcome)
    }

    /// Для тестов и CLI: заменить состояние целиком.
    pub fn replace_bundle(&mut self, bundle: MemoryBundle) -> Result<(), AppError> {
        self.bundle = bundle;
        self.save_all()
    }

    pub fn client(&self) -> Arc<dyn ChatClient> {
        self.runtime.client.clone()
    }

    pub fn client_from_env(settings: &Settings) -> Result<Arc<dyn ChatClient>, AppError> {
        let key = std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .or_else(|| std::env::var("KIMI_API_KEY").ok());
        let client = ReqwestClient::new(settings.base_url.clone(), key)
            .map_err(|error| AppError::Invalid(error.to_string()))?;
        Ok(Arc::new(client))
    }

    pub fn wm_kind(key: &str) -> WmKind {
        wm_kind_for_key(key)
    }
}

fn wm_kind_for_key(key: &str) -> WmKind {
    if key == "task.goal" || key == "goal" {
        WmKind::Goal
    } else if key.starts_with("plan.") {
        WmKind::Plan
    } else if key.starts_with("status.") {
        WmKind::Status
    } else if key.starts_with("question.") {
        WmKind::OpenQuestion
    } else if key.starts_with("note.") || key.starts_with("file.") {
        WmKind::Note
    } else {
        WmKind::Entity
    }
}

pub fn working_is_empty(state: &WorkingMemoryState) -> bool {
    state.items.is_empty()
}

pub fn fake_llm_enabled() -> bool {
    ["LAB12_FAKE_LLM", "LAB11_FAKE_LLM"]
        .iter()
        .any(|name| std::env::var(name).as_deref() == Ok("1"))
}
