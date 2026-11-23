//! День 15: правила жизни фактов, разрешение конфликтов и очистка.
//!
//! Модуль намеренно не зависит от TUI/CLI: те же правила применяют router,
//! ручные команды, импорт, инструмент `memory_write` и сборка промпта.

use chrono::{DateTime, Utc};

use crate::domain::memory::{
    CandidateScope, ChatMessage, ChatRole, FactPolicy, Id, LtmKind, LtmRecord, LtmRevision,
    LtmRevisionOp, LtmState, MemorySource, PendingWrite, WorkingMemoryState,
};

/// Ключи, которые накапливаются списком, а не перезаписываются.
const APPEND_PREFIXES: [&str; 6] = [
    "history.",
    "log.",
    "decisions.",
    "accepted.",
    "changes.",
    "revision.",
];

/// Ключи, принадлежащие текущей задаче.
const TASK_PREFIXES: [&str; 7] = [
    "task.",
    "plan.",
    "status.",
    "question.",
    "note.",
    "estimate.",
    "file.",
];

/// Канонический ключ. Нужен, чтобы «срок», «дедлайн» и `project.deadline`
/// считались одним и тем же фактом, а не тремя записями рядом.
pub fn canonical_key(raw: &str) -> String {
    let key = crate::memory::audit::sanitize_key(raw).to_lowercase();
    let trimmed = key.trim();
    match trimmed {
        "дедлайн" | "срок" | "срок проекта" | "дедлайн проекта" | "project.deadline" => {
            "project.deadline".to_string()
        }
        "статус проекта" | "статус" | "project.status" => {
            "project.status".to_string()
        }
        "версия" | "версия проекта" | "версия продукта" | "project.version" => {
            "project.version".to_string()
        }
        "требование" | "требования" | "requirement" | "requirement.current" => {
            "requirement.current".to_string()
        }
        "подход" | "выбранный подход" | "approach" | "decision.approach" => {
            "decision.approach".to_string()
        }
        "" => String::new(),
        other => other.to_string(),
    }
}

/// Правило жизни факта по ключу, типу и области.
pub fn policy_for(key: &str, _kind: LtmKind, scope: CandidateScope) -> FactPolicy {
    let key = canonical_key(key);
    if APPEND_PREFIXES.iter().any(|prefix| key.starts_with(prefix)) {
        return FactPolicy::Append;
    }
    if scope == CandidateScope::Task || TASK_PREFIXES.iter().any(|prefix| key.starts_with(prefix)) {
        return FactPolicy::TaskScoped;
    }
    FactPolicy::Replace
}

pub fn scope_for(key: &str, kind: LtmKind) -> CandidateScope {
    let scope = match kind {
        LtmKind::Profile | LtmKind::Preference => CandidateScope::Global,
        LtmKind::Project | LtmKind::Decision | LtmKind::Knowledge | LtmKind::Glossary => {
            CandidateScope::Project
        }
    };
    if is_task_scoped_key(key) {
        CandidateScope::Task
    } else {
        scope
    }
}

pub fn is_task_scoped_key(key: &str) -> bool {
    let key = canonical_key(key);
    TASK_PREFIXES.iter().any(|prefix| key.starts_with(prefix))
}

/// Русские синонимы ключа для retrieval: вопрос «какой сейчас подход?»
/// находит `decision.approach`, даже если значение не содержит слова «подход».
pub fn key_tags(key: &str) -> Vec<String> {
    match canonical_key(key).as_str() {
        "project.deadline" => ["срок", "дедлайн"].iter().map(|s| s.to_string()).collect(),
        "project.status" => ["статус", "состояние"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        "project.version" => ["версия", "релиз"].iter().map(|s| s.to_string()).collect(),
        "requirement.current" => ["требование", "ограничение"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        "decision.approach" => ["подход", "решение"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        "decisions.accepted" => ["решение", "история"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

pub fn is_append_key(key: &str) -> bool {
    let key = canonical_key(key);
    APPEND_PREFIXES.iter().any(|prefix| key.starts_with(prefix))
}

/// Добавляет запись в журнал «что было → что стало». Журнал append-only:
/// старые значения не перезаписываются, только добавляются новые редакции.
#[allow(clippy::too_many_arguments)]
pub fn record_revision(
    state: &mut LtmState,
    op: LtmRevisionOp,
    key: &str,
    old: Option<&LtmRecord>,
    new: Option<&LtmRecord>,
    source: MemorySource,
    reason: &str,
    now: DateTime<Utc>,
) {
    state.revisions.push(LtmRevision {
        id: format!("rev_{}", uuid::Uuid::new_v4().simple()),
        at: now,
        op,
        key: key.to_string(),
        old_value: old.map(|record| record.value.clone()),
        new_value: new.map(|record| record.value.clone()),
        old_record_id: old.map(|record| record.id.clone()),
        new_record_id: new.map(|record| record.id.clone()),
        source,
        reason: reason.to_string(),
    });
    trim_revisions(state, 500);
}

pub fn trim_revisions(state: &mut LtmState, limit: usize) {
    if state.revisions.len() > limit {
        let excess = state.revisions.len() - limit;
        state.revisions.drain(0..excess);
    }
}

/// Последняя замена конкретного ключа.
pub fn latest_revision<'a>(state: &'a LtmState, key: &str) -> Option<&'a LtmRevision> {
    state
        .revisions
        .iter()
        .rev()
        .find(|revision| revision.key == key)
}

/// Значения, которые больше не являются актуальными. Используются для
/// страховки промпта: историческая реплика со старым сроком не должна
/// подсказывать модели устаревший ответ.
pub fn stale_values(state: &LtmState) -> Vec<String> {
    let active: Vec<String> = state
        .records
        .iter()
        .filter(|record| record.is_active())
        .map(|record| record.value.to_lowercase())
        .collect();
    let mut values: Vec<String> = state
        .records
        .iter()
        .filter(|record| !record.is_active() && !record.value.trim().is_empty())
        .map(|record| record.value.clone())
        .chain(state.revisions.iter().filter_map(|revision| {
            if revision.op == LtmRevisionOp::Replace || revision.op == LtmRevisionOp::TaskCleanup {
                revision.old_value.clone()
            } else {
                None
            }
        }))
        .collect();
    values.sort_by_key(|value| std::cmp::Reverse(value.chars().count()));
    let mut seen: Vec<String> = Vec::new();
    values.retain(|value| {
        let lower = value.to_lowercase();
        if seen.contains(&lower) {
            false
        } else {
            seen.push(lower);
            true
        }
    });
    values.retain(|value| {
        let lower = value.to_lowercase();
        if lower.chars().count() < 3 {
            return false;
        }
        // Не маскируем старую строку, если она является частью актуального
        // значения: «PostgreSQL» не должно портить «PostgreSQL 16».
        !active
            .iter()
            .any(|current| current != &lower && current.contains(&lower))
    });
    values
}

/// Заменяет устаревшие значения в истории на маркер. Текущая (последняя)
/// реплика пользователя не трогается: в ней может быть явное «было → стало».
pub fn mask_stale_values(messages: &mut [ChatMessage], stale: &[String]) -> usize {
    if stale.is_empty() {
        return 0;
    }
    let keep_last = messages
        .last()
        .is_some_and(|message| message.role == ChatRole::User);
    let limit = messages.len().saturating_sub(usize::from(keep_last));
    let mut masked = 0;
    for message in messages.iter_mut().take(limit) {
        if message.role == ChatRole::System && message.name.is_some() {
            continue;
        }
        let Some(content) = message.content.as_mut() else {
            continue;
        };
        for value in stale {
            if replace_case_insensitive(content, value, "«устарело»") {
                masked += 1;
            }
        }
    }
    masked
}

fn replace_case_insensitive(text: &mut String, needle: &str, replacement: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let needle_lower = needle.to_lowercase();
    let mut changed = false;
    loop {
        let lower = text.to_lowercase();
        let Some(position) = lower.find(&needle_lower) else {
            break;
        };
        let end = position + needle_lower.len();
        if !text.is_char_boundary(position) || !text.is_char_boundary(end) {
            break;
        }
        text.replace_range(position..end, replacement);
        changed = true;
    }
    changed
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskCleanupReport {
    pub wm_items: usize,
    pub ltm_records: usize,
    pub pending: usize,
}

/// Завершение задачи: рабочая память полностью очищается, task-scoped факты
/// LTM удаляются, проектные и глобальные факты остаются. Каждое удаление
/// фиксируется в журнале LTM.
pub fn cleanup_after_task(
    ltm: &mut LtmState,
    wm: &mut WorkingMemoryState,
    pending: &mut Vec<PendingWrite>,
    now: DateTime<Utc>,
) -> TaskCleanupReport {
    let wm_items = wm.items.len();
    let removed: Vec<LtmRecord> = ltm
        .records
        .iter()
        .filter(|record| record.policy == FactPolicy::TaskScoped)
        .cloned()
        .collect();
    for record in &removed {
        if record.is_active() {
            record_revision(
                ltm,
                LtmRevisionOp::TaskCleanup,
                &record.key,
                Some(record),
                None,
                MemorySource::User,
                "задача завершена: task-scoped факт удалён",
                now,
            );
        }
    }
    let task_keys: Vec<Id> = removed.iter().map(|record| record.id.clone()).collect();
    ltm.records.retain(|record| !task_keys.contains(&record.id));
    let pending_before = pending.len();
    pending.retain(|item| !is_task_scoped_key(&item.candidate.key));
    let pending_removed = pending_before.saturating_sub(pending.len());

    *wm = WorkingMemoryState::empty(now);
    ltm.updated_at = now;

    TaskCleanupReport {
        wm_items,
        ltm_records: removed.iter().filter(|record| record.is_active()).count(),
        pending: pending_removed,
    }
}

/// Журнал в формате для вкладки `10 Обновления` и CLI.
pub fn recent_replacements(state: &LtmState, limit: usize) -> Vec<&LtmRevision> {
    state
        .revisions
        .iter()
        .rev()
        .filter(|revision| {
            matches!(
                revision.op,
                LtmRevisionOp::Replace
                    | LtmRevisionOp::TaskCleanup
                    | LtmRevisionOp::Append
                    | LtmRevisionOp::Delete
            )
        })
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::{LtmKind, LtmRecord};
    use crate::memory::ltm::{UpsertLtm, upsert_ltm};

    fn ltm_with(key: &str, value: &str, now: DateTime<Utc>) -> LtmState {
        let mut state = LtmState::empty(now);
        upsert_ltm(
            &mut state,
            UpsertLtm::new(LtmKind::Knowledge, key, value, MemorySource::User),
            now,
        );
        state
    }

    #[test]
    fn aliases_map_to_one_key() {
        assert_eq!(canonical_key("срок"), "project.deadline");
        assert_eq!(canonical_key("Дедлайн проекта"), "project.deadline");
        assert_eq!(canonical_key("PROJECT.DEADLINE"), "project.deadline");
    }

    #[test]
    fn policies_are_explicit() {
        assert_eq!(
            policy_for(
                "project.deadline",
                LtmKind::Knowledge,
                CandidateScope::Project
            ),
            FactPolicy::Replace
        );
        assert_eq!(
            policy_for(
                "decisions.accepted",
                LtmKind::Decision,
                CandidateScope::Project
            ),
            FactPolicy::Append
        );
        assert_eq!(
            policy_for("task.rate", LtmKind::Knowledge, CandidateScope::Project),
            FactPolicy::TaskScoped
        );
    }

    #[test]
    fn stale_values_skip_actual_substring() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        let mut old = UpsertLtm::new(
            LtmKind::Knowledge,
            "project.version",
            "PostgreSQL",
            MemorySource::User,
        );
        old.recorded_at = Some(now);
        upsert_ltm(&mut state, old, now);
        let mut new = UpsertLtm::new(
            LtmKind::Knowledge,
            "project.version",
            "PostgreSQL 16",
            MemorySource::User,
        );
        new.recorded_at = Some(now + chrono::Duration::seconds(1));
        upsert_ltm(&mut state, new, now + chrono::Duration::seconds(1));
        assert!(stale_values(&state).is_empty());
    }

    #[test]
    fn stale_old_value_is_masked_but_current_message_kept() {
        let now = Utc::now();
        let mut state = ltm_with("project.deadline", "15 декабря 2026", now);
        upsert_ltm(
            &mut state,
            UpsertLtm::new(
                LtmKind::Knowledge,
                "project.deadline",
                "20 декабря 2026",
                MemorySource::User,
            ),
            now + chrono::Duration::seconds(1),
        );
        let stale = stale_values(&state);
        assert_eq!(stale, vec!["15 декабря 2026".to_string()]);
        let mut messages = vec![
            ChatMessage::user("Дедлайн — 15 декабря 2026."),
            ChatMessage::assistant("Запомнил срок 15 декабря 2026."),
            ChatMessage::user("Какой срок? Старый был 15 декабря 2026."),
        ];
        let count = mask_stale_values(&mut messages, &stale);
        assert_eq!(count, 2);
        assert!(!messages[0].text().contains("15 декабря"));
        assert!(!messages[1].text().contains("15 декабря"));
        assert!(messages[2].text().contains("15 декабря"));
    }

    #[test]
    fn cleanup_removes_task_scope_and_keeps_project() {
        let now = Utc::now();
        let mut ltm = ltm_with("project.deadline", "20 декабря 2026", now);
        let mut task = UpsertLtm::new(LtmKind::Knowledge, "task.rate", "90", MemorySource::User);
        task.scope = CandidateScope::Task;
        upsert_ltm(&mut ltm, task, now);
        let mut wm = WorkingMemoryState::empty(now);
        wm.items.push(crate::domain::memory::WmItem {
            id: "wm_1".into(),
            kind: crate::domain::memory::WmKind::Goal,
            key: "task.goal".into(),
            value: "смета".into(),
            status: crate::domain::memory::WmStatus::Active,
            order: None,
            ref_ltm_id: None,
            created_at: now,
            updated_at: now,
            source_message_index: None,
        });
        let mut pending = Vec::new();
        let report = cleanup_after_task(&mut ltm, &mut wm, &mut pending, now);
        assert_eq!(report.wm_items, 1);
        assert_eq!(report.ltm_records, 1);
        assert!(wm.items.is_empty());
        assert_eq!(ltm.records.len(), 1);
        assert_eq!(ltm.records[0].key, "project.deadline");
        assert!(
            ltm.revisions
                .iter()
                .any(|revision| revision.op == LtmRevisionOp::TaskCleanup)
        );
    }

    #[test]
    fn record_revision_keeps_old_and_new() {
        let now = Utc::now();
        let old = LtmRecord {
            id: "ltm_old".into(),
            kind: LtmKind::Knowledge,
            key: "project.deadline".into(),
            value: "15 декабря".into(),
            tags: Vec::new(),
            source: MemorySource::User,
            confidence: 0.9,
            pinned: false,
            policy: FactPolicy::Replace,
            scope: CandidateScope::Project,
            revision: 1,
            created_at: now,
            updated_at: now,
            recorded_at: Some(now),
            last_used_at: None,
            superseded_by: None,
            superseded_at: None,
            legacy_key: None,
        };
        let mut new = old.clone();
        new.id = "ltm_new".into();
        new.value = "20 декабря".into();
        let mut state = LtmState::empty(now);
        record_revision(
            &mut state,
            LtmRevisionOp::Replace,
            "project.deadline",
            Some(&old),
            Some(&new),
            MemorySource::User,
            "срок сдвинулся",
            now,
        );
        assert_eq!(state.revisions[0].old_value.as_deref(), Some("15 декабря"));
        assert_eq!(state.revisions[0].new_value.as_deref(), Some("20 декабря"));
    }
}
