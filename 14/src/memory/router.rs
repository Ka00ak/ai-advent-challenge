//! Router R1–R6: детерминированно выбирает слой и операцию, пишет аудит.
//!
//! Порядок важен: R1 (секрет) → R2 (задача) → R3 (обновление LTM) →
//! R4 (новый устойчивый факт) → R5 (состояние задачи) → R6 (не сохранять).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::memory::{
    CandidateScope, ChatMessage, LtmState, MemoryCandidate, MemoryLayer, MemorySource,
    PendingWrite, WmKind, WmStatus, WorkingMemoryState, WriteOp,
};
use crate::domain::settings::{Approval, MemoryConfig};
use crate::memory::audit::{
    contains_secret, make_event, mask_secret, sanitize_key, sanitize_value,
};
use crate::memory::extractor::{CandidateExtractor, ExtractError};
use crate::memory::ltm::{LtmChange, UpsertLtm, find_active_ltm, replace_ltm, upsert_ltm};
use crate::memory::working::{WmChange, WmUpsert, upsert_wm_item};

#[derive(Debug, Error)]
pub enum RouteError {
    #[error("извлечение кандидатов не удалось: {0}")]
    Extract(#[from] ExtractError),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RouteStats {
    pub candidates: usize,
    pub wm_created: usize,
    pub wm_updated: usize,
    pub ltm_created: usize,
    pub ltm_updated: usize,
    pub pending: usize,
    pub same: usize,
    pub rejected: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RejectedCandidate {
    pub key: String,
    pub value: String,
    pub rule_id: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct RouteOutcome {
    pub wm: WorkingMemoryState,
    pub ltm: LtmState,
    pub pending: Vec<PendingWrite>,
    pub audit: Vec<crate::domain::memory::MemoryWriteEvent>,
    pub rejected: Vec<RejectedCandidate>,
    pub stats: RouteStats,
}

pub struct RouteInput<'a> {
    pub message: &'a ChatMessage,
    pub message_index: usize,
    pub wm: &'a WorkingMemoryState,
    pub ltm: &'a LtmState,
    pub pending: &'a [PendingWrite],
    pub config: &'a MemoryConfig,
    pub now: DateTime<Utc>,
}

pub fn route_candidates(
    input: RouteInput<'_>,
    extractor: &dyn CandidateExtractor,
) -> Result<RouteOutcome, RouteError> {
    let extraction = extractor.extract(input.message, input.ltm)?;
    let mut wm = input.wm.clone();
    let mut ltm = input.ltm.clone();
    let mut pending = input.pending.to_vec();
    let mut audit = Vec::new();
    let mut rejected = Vec::new();
    let mut stats = RouteStats {
        candidates: extraction.candidates.len(),
        ..RouteStats::default()
    };
    let now = input.now;
    let message_lower = input.message.text().to_lowercase();

    // R1: секрет отклоняется целиком до любой записи.
    if contains_secret(&input.message.text()) {
        stats.rejected += 1;
        let event = make_event(
            MemoryLayer::Ltm,
            WriteOp::Reject,
            "secret.message",
            Some(&input.message.text()),
            MemorySource::Router,
            Some("R1"),
            "похоже на секрет",
            Some(input.message_index),
            now,
        );
        audit.push(event);
        rejected.push(RejectedCandidate {
            key: "secret.message".into(),
            value: mask_secret(&input.message.text()),
            rule_id: "R1".into(),
            reason: "похоже на секрет".into(),
        });
        stats.error = None;
        return Ok(RouteOutcome {
            wm,
            ltm,
            pending,
            audit,
            rejected,
            stats,
        });
    }

    for candidate in extraction.candidates {
        let key = sanitize_key(&candidate.key);
        let value = sanitize_value(&candidate.value);
        if key.is_empty() || value.is_empty() {
            continue;
        }
        if contains_secret(&value) || contains_secret(&key) {
            stats.rejected += 1;
            audit.push(make_event(
                MemoryLayer::Ltm,
                WriteOp::Reject,
                &key,
                Some(&value),
                MemorySource::Router,
                Some("R1"),
                "похоже на секрет",
                Some(input.message_index),
                now,
            ));
            rejected.push(RejectedCandidate {
                key,
                value: mask_secret(&value),
                rule_id: "R1".into(),
                reason: "похоже на секрет".into(),
            });
            continue;
        }

        let task_scope = candidate.scope == CandidateScope::Task
            || is_task_key(&key)
            || is_task_marker(&message_lower);
        if task_scope {
            let kind = infer_wm_kind(&key);
            let rule = if is_task_marker(&message_lower) {
                "R2"
            } else {
                "R5"
            };
            let reason = if rule == "R2" {
                "маркер задачи/временное состояние"
            } else {
                "состояние задачи по умолчанию"
            };
            let order = plan_order(&key);
            let upsert = WmUpsert {
                kind,
                key: key.clone(),
                value: value.clone(),
                status: WmStatus::Active,
                order,
                ref_ltm_id: None,
                source_message_index: Some(input.message_index),
            };
            let change = upsert_wm_item(&mut wm, upsert, now);
            match change {
                WmChange::Created(_) => {
                    stats.wm_created += 1;
                    audit.push(make_event(
                        MemoryLayer::Wm,
                        WriteOp::Create,
                        &key,
                        Some(&value),
                        MemorySource::Router,
                        Some(rule),
                        reason,
                        Some(input.message_index),
                        now,
                    ));
                }
                WmChange::Updated(_) => {
                    stats.wm_updated += 1;
                    audit.push(make_event(
                        MemoryLayer::Wm,
                        WriteOp::Update,
                        &key,
                        Some(&value),
                        MemorySource::Router,
                        Some(rule),
                        reason,
                        Some(input.message_index),
                        now,
                    ));
                }
                WmChange::Same(_) => stats.same += 1,
            }
            continue;
        }

        if candidate.scope == CandidateScope::Turn {
            stats.rejected += 1;
            rejected.push(RejectedCandidate {
                key,
                value,
                rule_id: "R6".into(),
                reason: "нет новых данных".into(),
            });
            continue;
        }

        let existing = find_active_ltm(&ltm, &key).cloned();
        if let Some(existing) = existing {
            if existing.value == value {
                stats.same += 1;
                continue;
            }
            if input.config.approval == Approval::Auto {
                let change = replace_ltm(
                    &mut ltm,
                    UpsertLtm {
                        kind: candidate.kind,
                        key: key.clone(),
                        value: value.clone(),
                        tags: candidate.tags.clone(),
                        source: MemorySource::Router,
                        confidence: candidate.confidence,
                        pinned: false,
                        legacy_key: candidate.legacy_category.clone(),
                    },
                    now,
                );
                if matches!(change, LtmChange::Replaced { .. } | LtmChange::Created(_)) {
                    stats.ltm_updated += 1;
                }
                audit.push(make_event(
                    MemoryLayer::Ltm,
                    WriteOp::Update,
                    &key,
                    Some(&value),
                    MemorySource::Router,
                    Some("R3"),
                    "обновление ключа (auto)",
                    Some(input.message_index),
                    now,
                ));
            } else {
                let old_value = Some(existing.value.clone());
                queue_pending(
                    &mut pending,
                    &candidate,
                    &key,
                    &value,
                    old_value,
                    "R3",
                    "обновление существующего ключа",
                    input.message_index,
                    now,
                );
                stats.pending += 1;
                audit.push(make_event(
                    MemoryLayer::Ltm,
                    WriteOp::Update,
                    &key,
                    Some(&value),
                    MemorySource::Router,
                    Some("R3"),
                    "ожидает подтверждения",
                    Some(input.message_index),
                    now,
                ));
            }
            continue;
        }

        // R4: новый устойчивый факт.
        if input.config.approval == Approval::Auto {
            let change = upsert_ltm(
                &mut ltm,
                UpsertLtm {
                    kind: candidate.kind,
                    key: key.clone(),
                    value: value.clone(),
                    tags: candidate.tags.clone(),
                    source: MemorySource::Router,
                    confidence: candidate.confidence,
                    pinned: false,
                    legacy_key: candidate.legacy_category.clone(),
                },
                now,
            );
            match change {
                LtmChange::Created(_) | LtmChange::Replaced { .. } => {
                    stats.ltm_created += 1;
                    audit.push(make_event(
                        MemoryLayer::Ltm,
                        WriteOp::Create,
                        &key,
                        Some(&value),
                        MemorySource::Router,
                        Some("R4"),
                        "устойчивый факт (auto)",
                        Some(input.message_index),
                        now,
                    ));
                }
                LtmChange::Same(_) => stats.same += 1,
                LtmChange::Updated(_) => stats.ltm_updated += 1,
            }
        } else {
            queue_pending(
                &mut pending,
                &candidate,
                &key,
                &value,
                None,
                "R4",
                "устойчивый факт проекта",
                input.message_index,
                now,
            );
            stats.pending += 1;
            audit.push(make_event(
                MemoryLayer::Ltm,
                WriteOp::Create,
                &key,
                Some(&value),
                MemorySource::Router,
                Some("R4"),
                "ожидает подтверждения",
                Some(input.message_index),
                now,
            ));
        }
    }

    Ok(RouteOutcome {
        wm,
        ltm,
        pending,
        audit,
        rejected,
        stats,
    })
}

/// Один pending на key: новое значение заменяет предыдущее, старый `old_value`
/// сохраняется для диффа.
#[allow(clippy::too_many_arguments)]
pub fn queue_pending(
    pending: &mut Vec<PendingWrite>,
    candidate: &MemoryCandidate,
    key: &str,
    value: &str,
    old_value: Option<String>,
    rule_id: &str,
    reason: &str,
    message_index: usize,
    now: DateTime<Utc>,
) {
    let preserved_old = pending
        .iter()
        .position(|item| item.candidate.key == key)
        .map(|index| {
            let existing = pending.remove(index);
            existing.old_value
        })
        .unwrap_or(old_value);
    let mut candidate = candidate.clone();
    candidate.key = key.to_string();
    candidate.value = value.to_string();
    let target_kind = candidate.kind;
    pending.push(PendingWrite {
        id: format!("pend_{}", uuid::Uuid::new_v4().simple()),
        candidate,
        target_layer: MemoryLayer::Ltm,
        target_kind,
        rule_id: rule_id.to_string(),
        reason: reason.to_string(),
        old_value: preserved_old,
        message_index,
        created_at: now,
    });
}

pub fn is_task_marker(lower: &str) -> bool {
    const MARKERS: [&str; 12] = [
        "текущая задача",
        "на эту задачу",
        "для этой задачи",
        "на этом шаге",
        "рабочий вариант",
        "открытый вопрос",
        "черновик",
        "статус",
        "план",
        "пока",
        "сейчас",
        "смету",
    ];
    MARKERS.iter().any(|marker| lower.contains(marker))
}

pub fn is_task_key(key: &str) -> bool {
    const PREFIXES: [&str; 8] = [
        "task.",
        "plan.",
        "status.",
        "question.",
        "note.",
        "estimate.",
        "file.",
        "goal",
    ];
    PREFIXES.iter().any(|prefix| key.starts_with(prefix))
}

fn infer_wm_kind(key: &str) -> WmKind {
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

fn plan_order(key: &str) -> Option<u32> {
    key.strip_prefix("plan.")
        .and_then(|rest| rest.parse::<u32>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::settings::MemoryConfig;
    use crate::memory::extractor::LocalCandidateExtractor;

    fn routed(text: &str, config: &MemoryConfig) -> RouteOutcome {
        let message = ChatMessage::user(text);
        let empty_wm = WorkingMemoryState::empty(Utc::now());
        let empty_ltm = LtmState::empty(Utc::now());
        route_candidates(
            RouteInput {
                message: &message,
                message_index: 0,
                wm: &empty_wm,
                ltm: &empty_ltm,
                pending: &[],
                config,
                now: Utc::now(),
            },
            &LocalCandidateExtractor,
        )
        .unwrap()
    }

    #[test]
    fn r1_rejects_secrets_with_mask() {
        let outcome = routed(
            "Мой API-ключ: sk-abcdef1234567890",
            &MemoryConfig::default(),
        );
        assert!(outcome.wm.items.is_empty());
        assert!(outcome.pending.is_empty());
        assert_eq!(outcome.stats.rejected, 1);
        let json = serde_json::to_string(&outcome.audit).unwrap();
        assert!(!json.contains("abcdef1234567890"));
        assert!(json.contains("***"));
    }

    #[test]
    fn r2_task_marker_goes_to_wm_even_if_key_in_ltm() {
        let mut ltm = LtmState::empty(Utc::now());
        upsert_ltm(
            &mut ltm,
            UpsertLtm::new(
                crate::domain::memory::LtmKind::Knowledge,
                "task.rate",
                "80",
                MemorySource::User,
            ),
            Utc::now(),
        );
        let message = ChatMessage::user("Для этой задачи курс 90 ₽");
        let outcome = route_candidates(
            RouteInput {
                message: &message,
                message_index: 1,
                wm: &WorkingMemoryState::empty(Utc::now()),
                ltm: &ltm,
                pending: &[],
                config: &MemoryConfig::default(),
                now: Utc::now(),
            },
            &LocalCandidateExtractor,
        )
        .unwrap();
        assert!(outcome.wm.items.iter().any(|item| item.key == "task.rate"));
    }

    #[test]
    fn r3_confirm_creates_pending_update() {
        let now = Utc::now();
        let mut ltm = LtmState::empty(now);
        upsert_ltm(
            &mut ltm,
            UpsertLtm::new(
                crate::domain::memory::LtmKind::Knowledge,
                "budget.marketing",
                "120 000 рублей",
                MemorySource::User,
            ),
            now,
        );
        let message = ChatMessage::user("Бюджет маркетинга подняли до 150 000 рублей.");
        let outcome = route_candidates(
            RouteInput {
                message: &message,
                message_index: 2,
                wm: &WorkingMemoryState::empty(now),
                ltm: &ltm,
                pending: &[],
                config: &MemoryConfig::default(),
                now,
            },
            &LocalCandidateExtractor,
        )
        .unwrap();
        assert_eq!(outcome.pending.len(), 1);
        assert_eq!(outcome.pending[0].rule_id, "R3");
        assert_eq!(
            outcome.pending[0].old_value.as_deref(),
            Some("120 000 рублей")
        );
        assert_eq!(
            find_active_ltm(&outcome.ltm, "budget.marketing")
                .unwrap()
                .value,
            "120 000 рублей"
        );
    }

    #[test]
    fn r3_auto_supersedes_without_pending() {
        let now = Utc::now();
        let mut ltm = LtmState::empty(now);
        upsert_ltm(
            &mut ltm,
            UpsertLtm::new(
                crate::domain::memory::LtmKind::Knowledge,
                "budget.marketing",
                "120 000 рублей",
                MemorySource::User,
            ),
            now,
        );
        let config = MemoryConfig {
            approval: Approval::Auto,
            ..MemoryConfig::default()
        };
        let message = ChatMessage::user("Бюджет маркетинга подняли до 150 000 рублей.");
        let outcome = route_candidates(
            RouteInput {
                message: &message,
                message_index: 2,
                wm: &WorkingMemoryState::empty(now),
                ltm: &ltm,
                pending: &[],
                config: &config,
                now,
            },
            &LocalCandidateExtractor,
        )
        .unwrap();
        assert!(outcome.pending.is_empty());
        assert!(
            find_active_ltm(&outcome.ltm, "budget.marketing")
                .unwrap()
                .value
                .contains("150 000")
        );
    }

    #[test]
    fn r4_project_fact_creates_pending() {
        let outcome = routed("Запускаем проект «Атлас».", &MemoryConfig::default());
        assert_eq!(outcome.pending.len(), 1);
        assert_eq!(outcome.pending[0].candidate.key, "project.name");
        assert_eq!(outcome.pending[0].rule_id, "R4");
    }

    #[test]
    fn r5_variant_goes_to_wm() {
        let outcome = routed("Выбери вариант B рабочим.", &MemoryConfig::default());
        assert!(
            outcome
                .wm
                .items
                .iter()
                .any(|item| item.key == "estimate.variant")
        );
        assert!(outcome.pending.is_empty());
    }

    #[test]
    fn r6_small_talk_writes_nothing() {
        for text in ["Привет", "Спасибо, это всё", "Наверное, успеем к декабрю"]
        {
            let outcome = routed(text, &MemoryConfig::default());
            assert!(outcome.wm.items.is_empty());
            assert!(outcome.pending.is_empty());
        }
    }

    #[test]
    fn multiple_candidates_are_independent() {
        let outcome = routed(
            "Заказчик — Марина. Тимлид — Артём.",
            &MemoryConfig::default(),
        );
        let keys: Vec<&str> = outcome
            .pending
            .iter()
            .map(|item| item.candidate.key.as_str())
            .collect();
        assert!(keys.contains(&"customer"));
        assert!(keys.contains(&"team.lead"));
    }

    #[test]
    fn audit_has_layer_rule_source() {
        let outcome = routed("Запускаем проект «Атлас».", &MemoryConfig::default());
        let event = outcome.audit.first().unwrap();
        assert_eq!(event.layer, MemoryLayer::Ltm);
        assert_eq!(event.rule_id.as_deref(), Some("R4"));
        assert_eq!(event.source, MemorySource::Router);
        assert!(!event.reason.is_empty());
    }

    #[test]
    fn pending_per_key_is_replaced_and_old_value_preserved() {
        let now = Utc::now();
        let mut ltm = LtmState::empty(now);
        upsert_ltm(
            &mut ltm,
            UpsertLtm::new(
                crate::domain::memory::LtmKind::Knowledge,
                "budget.marketing",
                "120 000",
                MemorySource::User,
            ),
            now,
        );
        let first = route_candidates(
            RouteInput {
                message: &ChatMessage::user("Бюджет маркетинга подняли до 150 000 рублей."),
                message_index: 1,
                wm: &WorkingMemoryState::empty(now),
                ltm: &ltm,
                pending: &[],
                config: &MemoryConfig::default(),
                now,
            },
            &LocalCandidateExtractor,
        )
        .unwrap();
        let second = route_candidates(
            RouteInput {
                message: &ChatMessage::user("Бюджет маркетинга теперь 160 000 рублей."),
                message_index: 2,
                wm: &WorkingMemoryState::empty(now),
                ltm: &ltm,
                pending: &first.pending,
                config: &MemoryConfig::default(),
                now,
            },
            &LocalCandidateExtractor,
        )
        .unwrap();
        assert_eq!(second.pending.len(), 1);
        assert_eq!(second.pending[0].old_value.as_deref(), Some("120 000"));
    }
}
