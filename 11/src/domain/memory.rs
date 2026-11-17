//! Доменные типы трёх слоёв памяти.
//!
//! Контракты повторяют `docs/02-domain-model.md`: snake_case для полей,
//! нижний регистр для значений enum. Менять эти типы можно только вместе
//! со спецификацией.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub type Id = String;

pub const STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryLayer {
    Stm,
    Wm,
    Ltm,
}

impl MemoryLayer {
    pub const ALL: [MemoryLayer; 3] = [MemoryLayer::Stm, MemoryLayer::Wm, MemoryLayer::Ltm];

    pub fn as_str(self) -> &'static str {
        match self {
            MemoryLayer::Stm => "stm",
            MemoryLayer::Wm => "wm",
            MemoryLayer::Ltm => "ltm",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MemoryLayer::Stm => "Краткосрочная (STM)",
            MemoryLayer::Wm => "Рабочая (WM)",
            MemoryLayer::Ltm => "Долговременная (LTM)",
        }
    }
}

impl std::str::FromStr for MemoryLayer {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "stm" => Ok(MemoryLayer::Stm),
            "wm" => Ok(MemoryLayer::Wm),
            "ltm" => Ok(MemoryLayer::Ltm),
            other => Err(format!("неизвестный слой: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LtmKind {
    Profile,
    Project,
    Preference,
    Decision,
    Knowledge,
    Glossary,
}

impl LtmKind {
    pub const ALL: [LtmKind; 6] = [
        LtmKind::Profile,
        LtmKind::Project,
        LtmKind::Preference,
        LtmKind::Decision,
        LtmKind::Knowledge,
        LtmKind::Glossary,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            LtmKind::Profile => "profile",
            LtmKind::Project => "project",
            LtmKind::Preference => "preference",
            LtmKind::Decision => "decision",
            LtmKind::Knowledge => "knowledge",
            LtmKind::Glossary => "glossary",
        }
    }

    pub fn from_label(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "profile" | "профиль" => Some(LtmKind::Profile),
            "project" | "проект" => Some(LtmKind::Project),
            "preference" | "предпочтение" => Some(LtmKind::Preference),
            "decision" | "решение" => Some(LtmKind::Decision),
            "knowledge" | "знание" => Some(LtmKind::Knowledge),
            "glossary" | "глоссарий" => Some(LtmKind::Glossary),
            _ => None,
        }
    }

    /// Порядок в блоке профиля: pinned → kind → updated_at.
    pub fn profile_order(self) -> u8 {
        match self {
            LtmKind::Profile => 0,
            LtmKind::Project => 1,
            LtmKind::Preference => 2,
            _ => 3,
        }
    }

    pub fn is_profile_block(self) -> bool {
        matches!(
            self,
            LtmKind::Profile | LtmKind::Project | LtmKind::Preference
        )
    }

    pub fn is_relevant_block(self) -> bool {
        matches!(
            self,
            LtmKind::Decision | LtmKind::Knowledge | LtmKind::Glossary
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WmKind {
    Goal,
    Plan,
    Status,
    Entity,
    #[serde(rename = "open-question")]
    OpenQuestion,
    Note,
}

impl WmKind {
    pub const ALL: [WmKind; 6] = [
        WmKind::Goal,
        WmKind::Plan,
        WmKind::Status,
        WmKind::Entity,
        WmKind::OpenQuestion,
        WmKind::Note,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            WmKind::Goal => "goal",
            WmKind::Plan => "plan",
            WmKind::Status => "status",
            WmKind::Entity => "entity",
            WmKind::OpenQuestion => "open-question",
            WmKind::Note => "note",
        }
    }

    pub fn from_label(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "goal" | "цель" => Some(WmKind::Goal),
            "plan" | "план" => Some(WmKind::Plan),
            "status" | "статус" => Some(WmKind::Status),
            "entity" | "сущность" => Some(WmKind::Entity),
            "open-question" | "open_question" | "вопрос" => Some(WmKind::OpenQuestion),
            "note" | "заметка" => Some(WmKind::Note),
            _ => None,
        }
    }

    pub fn sort_order(self) -> u8 {
        match self {
            WmKind::Goal => 0,
            WmKind::Plan => 1,
            WmKind::Status => 2,
            WmKind::Entity => 3,
            WmKind::OpenQuestion => 4,
            WmKind::Note => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WmStatus {
    Active,
    Done,
    Dropped,
}

impl WmStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            WmStatus::Active => "active",
            WmStatus::Done => "done",
            WmStatus::Dropped => "dropped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MemorySource {
    MigrationFacts,
    User,
    Agent,
    Router,
}

impl MemorySource {
    pub fn as_str(self) -> &'static str {
        match self {
            MemorySource::MigrationFacts => "migration-facts",
            MemorySource::User => "user",
            MemorySource::Agent => "agent",
            MemorySource::Router => "router",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CandidateScope {
    Turn,
    Task,
    Project,
    Global,
}

impl CandidateScope {
    pub fn as_str(self) -> &'static str {
        match self {
            CandidateScope::Turn => "turn",
            CandidateScope::Task => "task",
            CandidateScope::Project => "project",
            CandidateScope::Global => "global",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LtmRecord {
    pub id: Id,
    pub kind: LtmKind,
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source: MemorySource,
    pub confidence: f32,
    pub pinned: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub superseded_by: Option<Id>,
    pub superseded_at: Option<DateTime<Utc>>,
    pub legacy_key: Option<String>,
}

impl LtmRecord {
    pub fn is_active(&self) -> bool {
        self.superseded_by.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LtmState {
    pub version: u32,
    pub conversation_id: Option<String>,
    pub records: Vec<LtmRecord>,
    pub updated_at: DateTime<Utc>,
}

impl LtmState {
    pub fn empty(now: DateTime<Utc>) -> Self {
        Self {
            version: STATE_VERSION,
            conversation_id: None,
            records: Vec::new(),
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WmItem {
    pub id: Id,
    pub kind: WmKind,
    pub key: String,
    pub value: String,
    pub status: WmStatus,
    pub order: Option<u32>,
    pub ref_ltm_id: Option<Id>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub source_message_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkingMemoryState {
    pub version: u32,
    pub task_id: Option<String>,
    pub task_title: String,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub items: Vec<WmItem>,
}

impl WorkingMemoryState {
    pub fn empty(now: DateTime<Utc>) -> Self {
        Self {
            version: STATE_VERSION,
            task_id: None,
            task_title: String::new(),
            started_at: None,
            updated_at: now,
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool_result(
        call_id: impl Into<String>,
        name: impl Into<String>,
        content: String,
    ) -> Self {
        Self {
            role: ChatRole::Tool,
            content: Some(content),
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id.into()),
            name: Some(name.into()),
        }
    }

    pub fn text(&self) -> String {
        let mut text = self.content.clone().unwrap_or_default();
        for call in &self.tool_calls {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&call.name);
            text.push(' ');
            text.push_str(&call.arguments);
        }
        text
    }

    pub fn preview(&self, limit: usize) -> String {
        let text = self.text().replace('\n', " ");
        if text.chars().count() <= limit {
            text
        } else {
            let mut out: String = text.chars().take(limit.saturating_sub(1)).collect();
            out.push('…');
            out
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StmState {
    pub version: u32,
    pub conversation_id: String,
    pub messages: Vec<ChatMessage>,
    pub summary: Option<String>,
    pub covered_messages: usize,
    pub updated_at: DateTime<Utc>,
}

impl StmState {
    pub fn empty(now: DateTime<Utc>) -> Self {
        Self {
            version: STATE_VERSION,
            conversation_id: format!("conv_{}", uuid::Uuid::new_v4()),
            messages: Vec::new(),
            summary: None,
            covered_messages: 0,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryCandidate {
    pub key: String,
    pub value: String,
    pub kind: LtmKind,
    #[serde(default)]
    pub tags: Vec<String>,
    pub confidence: f32,
    pub scope: CandidateScope,
    pub ttl_turns: Option<u32>,
    pub legacy_category: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingWrite {
    pub id: Id,
    pub candidate: MemoryCandidate,
    pub target_layer: MemoryLayer,
    pub target_kind: LtmKind,
    pub rule_id: String,
    pub reason: String,
    pub old_value: Option<String>,
    pub message_index: usize,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WriteOp {
    Create,
    Update,
    Delete,
    Promote,
    Demote,
    Reject,
    Clear,
    Accept,
}

impl WriteOp {
    pub fn as_str(self) -> &'static str {
        match self {
            WriteOp::Create => "create",
            WriteOp::Update => "update",
            WriteOp::Delete => "delete",
            WriteOp::Promote => "promote",
            WriteOp::Demote => "demote",
            WriteOp::Reject => "reject",
            WriteOp::Clear => "clear",
            WriteOp::Accept => "accept",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryWriteEvent {
    pub id: Id,
    pub at: DateTime<Utc>,
    pub layer: MemoryLayer,
    pub op: WriteOp,
    pub key: String,
    pub value: Option<String>,
    pub source: MemorySource,
    pub rule_id: Option<String>,
    pub reason: String,
    pub message_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryMetaState {
    pub version: u32,
    pub conversation_id: Option<String>,
    pub processed_messages: usize,
    pub last_route_at: Option<DateTime<Utc>>,
}

impl MemoryMetaState {
    pub fn empty() -> Self {
        Self {
            version: STATE_VERSION,
            conversation_id: None,
            processed_messages: 0,
            last_route_at: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LayerDelta {
    pub ltm: usize,
    pub wm: usize,
    pub stm: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryCounts {
    pub ltm_records: usize,
    pub wm_items: usize,
    pub stm_messages: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryBreakdown {
    pub system_tokens: usize,
    pub ltm_profile_tokens: usize,
    pub ltm_relevant_tokens: usize,
    pub wm_tokens: usize,
    pub stm_summary_tokens: usize,
    pub stm_tokens: usize,
    pub current_tokens: usize,
    pub tools_tokens: usize,
    pub total_tokens: usize,
    pub layer_delta: LayerDelta,
    pub counts: MemoryCounts,
    pub included_ltm_ids: Vec<Id>,
    pub dropped_ltm_ids: Vec<Id>,
    pub deduped_wm_ids: Vec<Id>,
}

impl MemoryBreakdown {
    pub fn recompute_total(&mut self) {
        self.total_tokens = self.system_tokens
            + self.ltm_profile_tokens
            + self.ltm_relevant_tokens
            + self.wm_tokens
            + self.stm_summary_tokens
            + self.stm_tokens
            + self.current_tokens
            + self.tools_tokens;
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryBundle {
    pub stm: StmState,
    pub wm: WorkingMemoryState,
    pub ltm: LtmState,
    pub pending: Vec<PendingWrite>,
    pub meta: MemoryMetaState,
}

impl MemoryBundle {
    pub fn empty(now: DateTime<Utc>) -> Self {
        Self {
            stm: StmState::empty(now),
            wm: WorkingMemoryState::empty(now),
            ltm: LtmState::empty(now),
            pending: Vec::new(),
            meta: MemoryMetaState::empty(),
        }
    }
}

/// Флаги слоёв для ablation-эксперимента.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerFlags {
    pub stm: bool,
    pub wm: bool,
    pub ltm: bool,
}

impl LayerFlags {
    pub const NONE: LayerFlags = LayerFlags {
        stm: false,
        wm: false,
        ltm: false,
    };
    pub const ALL: LayerFlags = LayerFlags {
        stm: true,
        wm: true,
        ltm: true,
    };

    pub fn new(stm: bool, wm: bool, ltm: bool) -> Self {
        Self { stm, wm, ltm }
    }
}

/// Отрендеренный блок слоя.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RenderedBlock {
    pub text: String,
    pub tokens: usize,
    pub included: Vec<Id>,
    pub deduped: Vec<Id>,
    pub dropped: Vec<Id>,
    pub notes: Vec<String>,
}

impl RenderedBlock {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// Окно STM, выбранное для промпта.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StmWindow {
    pub indices: Vec<usize>,
    pub tokens: usize,
    pub dropped_pairs: usize,
}
