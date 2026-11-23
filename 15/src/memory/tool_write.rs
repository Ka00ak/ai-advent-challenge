//! Валидация и применение tool-вызова `memory_write`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::memory::{
    LtmKind, MemoryBundle, MemoryLayer, MemorySource, MemoryWriteEvent, WmKind, WmStatus, WriteOp,
};
use crate::domain::settings::Approval;
use crate::memory::audit::{contains_secret, make_event, sanitize_key, sanitize_value};
use crate::memory::ltm::{LtmChange, UpsertLtm, find_active_ltm, replace_ltm, upsert_ltm};
use crate::memory::router::queue_pending;
use crate::memory::working::{WmUpsert, upsert_wm_item};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum MemoryWriteOutcome {
    Created { id: String },
    Updated { id: String },
    Pending { id: String },
    Same { id: String },
    Rejected { reason: String },
}

impl MemoryWriteOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryWriteOutcome::Created { .. } => "created",
            MemoryWriteOutcome::Updated { .. } => "updated",
            MemoryWriteOutcome::Pending { .. } => "pending",
            MemoryWriteOutcome::Same { .. } => "same",
            MemoryWriteOutcome::Rejected { .. } => "rejected",
        }
    }

    pub fn to_tool_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({"status": "rejected"}))
    }
}

#[derive(Debug, Clone)]
pub struct MemoryWriteRequest {
    pub layer: String,
    pub key: String,
    pub value: String,
    pub kind: Option<String>,
    pub pinned: bool,
}

impl MemoryWriteRequest {
    pub fn from_json(value: &serde_json::Value) -> Self {
        Self {
            layer: value
                .get("layer")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            key: value
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            value: value
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            kind: value
                .get("kind")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            pinned: value
                .get("pinned")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }
    }
}

pub fn apply_memory_write(
    bundle: &mut MemoryBundle,
    request: &MemoryWriteRequest,
    source: MemorySource,
    approval: Approval,
    now: DateTime<Utc>,
) -> (MemoryWriteOutcome, Option<MemoryWriteEvent>) {
    let layer = request.layer.trim().to_ascii_lowercase();
    if layer == "stm" || layer.is_empty() {
        return rejected(
            "memory_write не пишет в STM; выберите wm или ltm",
            &request.key,
            source,
            now,
        );
    }
    let key = sanitize_key(&request.key);
    let value = sanitize_value(&request.value);
    if key.is_empty() {
        return rejected("пустой key", &key, source, now);
    }
    if value.is_empty() {
        return rejected("пустое value", &key, source, now);
    }
    if contains_secret(&value) || contains_secret(&key) {
        return rejected("похоже на секрет — не сохраняю", &key, source, now);
    }

    if layer == "wm" {
        let kind = request
            .kind
            .as_deref()
            .and_then(WmKind::from_label)
            .or_else(|| wm_kind_from_key(&key))
            .unwrap_or(WmKind::Entity);
        let change = upsert_wm_item(
            &mut bundle.wm,
            WmUpsert {
                kind,
                key: key.clone(),
                value: value.clone(),
                status: WmStatus::Active,
                order: None,
                ref_ltm_id: None,
                source_message_index: None,
            },
            now,
        );
        let (outcome, op) = match change {
            crate::memory::working::WmChange::Created(id) => {
                (MemoryWriteOutcome::Created { id }, WriteOp::Create)
            }
            crate::memory::working::WmChange::Updated(id) => {
                (MemoryWriteOutcome::Updated { id }, WriteOp::Update)
            }
            crate::memory::working::WmChange::Same(id) => {
                (MemoryWriteOutcome::Same { id }, WriteOp::Update)
            }
        };
        let event = make_event(
            MemoryLayer::Wm,
            op,
            &key,
            Some(&value),
            source,
            None,
            "tool memory_write",
            None,
            now,
        );
        return (outcome, Some(event));
    }

    if layer != "ltm" {
        return rejected("неизвестный слой: ожидается wm или ltm", &key, source, now);
    }

    let kind = request
        .kind
        .as_deref()
        .and_then(LtmKind::from_label)
        .unwrap_or(LtmKind::Knowledge);
    let existing = find_active_ltm(&bundle.ltm, &key).cloned();
    if let Some(existing) = existing {
        if existing.value == value {
            return (
                MemoryWriteOutcome::Same {
                    id: existing.id.clone(),
                },
                None,
            );
        }
        // День 15: конфликт не ждёт подтверждения. Более поздняя запись
        // заменяет старую, иначе в промпте останется устаревшее значение.
        let change = replace_ltm(
            &mut bundle.ltm,
            UpsertLtm {
                kind,
                key: key.clone(),
                value: value.clone(),
                tags: Vec::new(),
                source,
                confidence: 0.9,
                pinned: request.pinned,
                legacy_key: None,
                policy: Some(crate::domain::memory::FactPolicy::Replace),
                scope: existing.scope,
                recorded_at: Some(now),
            },
            now,
        );
        let id = match change {
            LtmChange::Created(id) | LtmChange::Updated(id) | LtmChange::Same(id) => id,
            LtmChange::Replaced { new, .. } => new,
            LtmChange::Stale { kept } => kept,
        };
        let event = make_event(
            MemoryLayer::Ltm,
            WriteOp::Update,
            &key,
            Some(&value),
            source,
            None,
            "tool memory_write: конфликт разрешён последней записью",
            None,
            now,
        );
        return (MemoryWriteOutcome::Updated { id }, Some(event));
    }

    if approval == Approval::Confirm {
        let candidate = crate::domain::memory::MemoryCandidate {
            key: key.clone(),
            value: value.clone(),
            kind,
            tags: Vec::new(),
            confidence: 0.9,
            scope: crate::domain::memory::CandidateScope::Project,
            ttl_turns: None,
            legacy_category: None,
        };
        queue_pending(
            &mut bundle.pending,
            &candidate,
            &key,
            &value,
            None,
            "TOOL",
            "tool memory_write: новая запись",
            0,
            now,
        );
        let id = bundle
            .pending
            .last()
            .map(|item| item.id.clone())
            .unwrap_or_default();
        let event = make_event(
            MemoryLayer::Ltm,
            WriteOp::Create,
            &key,
            Some(&value),
            source,
            Some("TOOL"),
            "ожидает подтверждения",
            None,
            now,
        );
        return (MemoryWriteOutcome::Pending { id }, Some(event));
    }

    let change = upsert_ltm(
        &mut bundle.ltm,
        UpsertLtm {
            kind,
            key: key.clone(),
            value: value.clone(),
            tags: Vec::new(),
            source,
            confidence: 0.9,
            pinned: request.pinned,
            legacy_key: None,
            policy: Some(crate::domain::memory::FactPolicy::Replace),
            scope: crate::domain::memory::CandidateScope::Project,
            recorded_at: Some(now),
        },
        now,
    );
    let id = match change {
        LtmChange::Created(id) | LtmChange::Updated(id) | LtmChange::Same(id) => id,
        LtmChange::Replaced { new, .. } => new,
        LtmChange::Stale { kept } => kept,
    };
    let event = make_event(
        MemoryLayer::Ltm,
        WriteOp::Create,
        &key,
        Some(&value),
        source,
        None,
        "tool memory_write",
        None,
        now,
    );
    (MemoryWriteOutcome::Created { id }, Some(event))
}

fn wm_kind_from_key(key: &str) -> Option<WmKind> {
    if key == "task.goal" {
        Some(WmKind::Goal)
    } else if key.starts_with("plan.") {
        Some(WmKind::Plan)
    } else if key.starts_with("status.") {
        Some(WmKind::Status)
    } else if key.starts_with("question.") {
        Some(WmKind::OpenQuestion)
    } else if key.starts_with("note.") {
        Some(WmKind::Note)
    } else {
        None
    }
}

fn rejected(
    reason: &str,
    key: &str,
    source: MemorySource,
    now: DateTime<Utc>,
) -> (MemoryWriteOutcome, Option<MemoryWriteEvent>) {
    let event = make_event(
        MemoryLayer::Ltm,
        WriteOp::Reject,
        key,
        None,
        source,
        None,
        reason,
        None,
        now,
    );
    (
        MemoryWriteOutcome::Rejected {
            reason: reason.to_string(),
        },
        Some(event),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle() -> MemoryBundle {
        MemoryBundle::empty(Utc::now())
    }

    #[test]
    fn wrong_layer_is_rejected() {
        let mut bundle = bundle();
        let request = MemoryWriteRequest {
            layer: "stm".into(),
            key: "x".into(),
            value: "y".into(),
            kind: None,
            pinned: false,
        };
        let (outcome, event) = apply_memory_write(
            &mut bundle,
            &request,
            MemorySource::Agent,
            Approval::Confirm,
            Utc::now(),
        );
        assert!(matches!(outcome, MemoryWriteOutcome::Rejected { .. }));
        assert_eq!(event.unwrap().op, WriteOp::Reject);
        assert!(bundle.wm.items.is_empty());
    }

    #[test]
    fn wm_write_is_created_immediately() {
        let mut bundle = bundle();
        let request = MemoryWriteRequest {
            layer: "wm".into(),
            key: "estimate.variant".into(),
            value: "Вариант B".into(),
            kind: Some("entity".into()),
            pinned: false,
        };
        let (outcome, _) = apply_memory_write(
            &mut bundle,
            &request,
            MemorySource::Agent,
            Approval::Confirm,
            Utc::now(),
        );
        assert!(matches!(outcome, MemoryWriteOutcome::Created { .. }));
        assert_eq!(bundle.wm.items.len(), 1);
    }

    #[test]
    fn ltm_write_requires_confirmation() {
        let mut bundle = bundle();
        let request = MemoryWriteRequest {
            layer: "ltm".into(),
            key: "project.name".into(),
            value: "«Атлас»".into(),
            kind: Some("project".into()),
            pinned: false,
        };
        let (outcome, _) = apply_memory_write(
            &mut bundle,
            &request,
            MemorySource::Agent,
            Approval::Confirm,
            Utc::now(),
        );
        assert!(matches!(outcome, MemoryWriteOutcome::Pending { .. }));
        assert!(bundle.ltm.records.is_empty());
        assert_eq!(bundle.pending.len(), 1);
    }

    #[test]
    fn ltm_write_auto_creates() {
        let mut bundle = bundle();
        let request = MemoryWriteRequest {
            layer: "ltm".into(),
            key: "project.name".into(),
            value: "«Атлас»".into(),
            kind: Some("project".into()),
            pinned: false,
        };
        let (outcome, _) = apply_memory_write(
            &mut bundle,
            &request,
            MemorySource::Agent,
            Approval::Auto,
            Utc::now(),
        );
        assert!(matches!(outcome, MemoryWriteOutcome::Created { .. }));
        assert_eq!(bundle.ltm.records.len(), 1);
    }

    #[test]
    fn secret_tool_write_is_rejected() {
        let mut bundle = bundle();
        let request = MemoryWriteRequest {
            layer: "ltm".into(),
            key: "secret.token".into(),
            value: "sk-abcdef1234567890".into(),
            kind: None,
            pinned: false,
        };
        let (outcome, _) = apply_memory_write(
            &mut bundle,
            &request,
            MemorySource::Agent,
            Approval::Auto,
            Utc::now(),
        );
        assert!(matches!(outcome, MemoryWriteOutcome::Rejected { .. }));
        assert!(bundle.ltm.records.is_empty());
    }
}
