//! LTM: активные записи, версии, pin и рендер блоков профиля.

use chrono::{DateTime, Utc};

use crate::domain::memory::{
    CandidateScope, FactPolicy, Id, LtmKind, LtmRecord, LtmRevisionOp, LtmState, MemorySource,
    RenderedBlock,
};
use crate::domain::settings::LtmConfig;
use crate::memory::lifecycle::{canonical_key, key_tags, policy_for, record_revision, scope_for};
use crate::memory::tokens::estimate_text_tokens;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LtmChange {
    Created(Id),
    Updated(Id),
    Same(Id),
    Replaced {
        old: Id,
        new: Id,
    },
    /// Новая запись старше уже сохранённой: приоритет у более поздней.
    Stale {
        kept: Id,
    },
}

#[derive(Debug, Clone)]
pub struct UpsertLtm {
    pub kind: LtmKind,
    pub key: String,
    pub value: String,
    pub tags: Vec<String>,
    pub source: MemorySource,
    pub confidence: f32,
    pub pinned: bool,
    pub legacy_key: Option<String>,
    /// Правило жизни факта. `None` — вывести из ключа и scope.
    pub policy: Option<FactPolicy>,
    pub scope: CandidateScope,
    /// Время записи факта. `None` — текущее время операции.
    pub recorded_at: Option<DateTime<Utc>>,
}

impl UpsertLtm {
    pub fn new(
        kind: LtmKind,
        key: impl Into<String>,
        value: impl Into<String>,
        source: MemorySource,
    ) -> Self {
        let key = key.into();
        let scope = scope_for(&key, kind);
        let policy = policy_for(&key, kind, scope);
        Self {
            kind,
            key,
            value: value.into(),
            tags: Vec::new(),
            source,
            confidence: 0.8,
            pinned: false,
            legacy_key: None,
            policy: Some(policy),
            scope,
            recorded_at: None,
        }
    }
}

pub fn active_ltm(state: &LtmState) -> impl Iterator<Item = &LtmRecord> {
    state.records.iter().filter(|record| record.is_active())
}

pub fn find_active_ltm<'a>(state: &'a LtmState, key: &str) -> Option<&'a LtmRecord> {
    let key = canonical_key(key);
    state
        .records
        .iter()
        .filter(|record| record.is_active() && record.key == key)
        .max_by_key(|record| record.write_time())
}

pub fn find_active_ltm_by_id<'a>(state: &'a LtmState, id: &str) -> Option<&'a LtmRecord> {
    state
        .records
        .iter()
        .find(|record| record.is_active() && record.id == id)
}

fn clean_input(input: &UpsertLtm) -> (String, String, Vec<String>, f32) {
    let key = canonical_key(&input.key);
    let value = crate::memory::audit::sanitize_value(&input.value);
    let tags = crate::memory::audit::sanitize_tags(&input.tags);
    let confidence = input.confidence.clamp(0.0, 1.0);
    (key, value, tags, confidence)
}

fn new_record(input: &UpsertLtm, key: String, revision: u32, now: DateTime<Utc>) -> LtmRecord {
    let (_, value, mut tags, confidence) = clean_input(input);
    if tags.is_empty() {
        tags = key_tags(&key);
    }
    let policy = input
        .policy
        .unwrap_or_else(|| policy_for(&key, input.kind, input.scope));
    LtmRecord {
        id: format!("ltm_{}", uuid::Uuid::new_v4().simple()),
        kind: input.kind,
        key,
        value,
        tags,
        source: input.source,
        confidence,
        pinned: input.pinned,
        policy,
        scope: input.scope,
        revision,
        created_at: now,
        updated_at: now,
        recorded_at: Some(input.recorded_at.unwrap_or(now)),
        last_used_at: None,
        superseded_by: None,
        superseded_at: None,
        legacy_key: input.legacy_key.clone(),
    }
}

fn active_indexes(state: &LtmState, key: &str) -> Vec<usize> {
    state
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.is_active() && record.key == key)
        .map(|(index, _)| index)
        .collect()
}

fn latest_active_index(state: &LtmState, key: &str) -> Option<usize> {
    active_indexes(state, key)
        .into_iter()
        .max_by_key(|index| state.records[*index].write_time())
}

/// Создаёт или обновляет факт с учётом правил Дня 15.
///
/// - `Replace`: у ключа остаётся одно активное значение, старое помечается
///   `superseded_by`; запись с более ранним `recorded_at` отклоняется.
/// - `Append`: новое уникальное значение добавляется рядом, старые остаются.
/// - `TaskScoped`: как `Replace`, но будет удалён после завершения задачи.
pub fn upsert_ltm(state: &mut LtmState, input: UpsertLtm, now: DateTime<Utc>) -> LtmChange {
    let (key, value, _, _) = clean_input(&input);
    if key.is_empty() {
        return LtmChange::Stale {
            kept: String::new(),
        };
    }
    let policy = input
        .policy
        .unwrap_or_else(|| policy_for(&key, input.kind, input.scope));
    let incoming_at = input.recorded_at.unwrap_or(now);

    if policy == FactPolicy::Append {
        if let Some(existing) = state
            .records
            .iter()
            .find(|record| record.is_active() && record.key == key && record.value == value)
        {
            return LtmChange::Same(existing.id.clone());
        }
        let revision = state
            .records
            .iter()
            .filter(|record| record.key == key)
            .map(|record| record.revision)
            .max()
            .unwrap_or(0)
            + 1;
        let record = new_record(&input, key.clone(), revision, now);
        let id = record.id.clone();
        state.records.push(record);
        let new = state.records.last().cloned();
        record_revision(
            state,
            LtmRevisionOp::Append,
            &key,
            None,
            new.as_ref(),
            input.source,
            "факт накапливается списком",
            now,
        );
        state.updated_at = now;
        prune_versions(state);
        return LtmChange::Created(id);
    }

    if let Some(index) = latest_active_index(state, &key) {
        let existing = state.records[index].clone();
        if existing.value == value {
            return LtmChange::Same(existing.id.clone());
        }
        if incoming_at < existing.write_time() {
            return LtmChange::Stale { kept: existing.id };
        }
        return replace_ltm(state, input, now);
    }

    let record = new_record(&input, key.clone(), 1, now);
    let id = record.id.clone();
    state.records.push(record);
    let new = state.records.last().cloned();
    record_revision(
        state,
        LtmRevisionOp::Create,
        &key,
        None,
        new.as_ref(),
        input.source,
        "новый факт",
        now,
    );
    state.updated_at = now;
    prune_versions(state);
    LtmChange::Created(id)
}

/// Создаёт новую активную версию, старую помечает `superseded_by`.
/// Журнал сохраняет «было → стало»; более поздняя запись всегда выигрывает.
pub fn replace_ltm(state: &mut LtmState, input: UpsertLtm, now: DateTime<Utc>) -> LtmChange {
    let (key, value, _, _) = clean_input(&input);
    let incoming_at = input.recorded_at.unwrap_or(now);
    let indexes = active_indexes(state, &key);
    if indexes.is_empty() {
        return upsert_ltm(state, input, now);
    }
    let latest_index = *indexes
        .iter()
        .max_by_key(|index| state.records[**index].write_time())
        .unwrap_or(&indexes[0]);
    let latest = state.records[latest_index].clone();
    if incoming_at < latest.write_time() {
        return LtmChange::Stale { kept: latest.id };
    }
    if latest.value == value {
        return LtmChange::Same(latest.id);
    }

    let old_pinned = indexes.iter().any(|index| state.records[*index].pinned);
    let revision = latest.revision.saturating_add(1);
    let mut record = new_record(&input, key.clone(), revision, now);
    record.pinned = record.pinned || old_pinned;
    let new_id = record.id.clone();
    let old_ids: Vec<Id> = indexes
        .iter()
        .map(|index| state.records[*index].id.clone())
        .collect();

    for index in &indexes {
        if let Some(old) = state.records.get_mut(*index) {
            old.superseded_by = Some(new_id.clone());
            old.superseded_at = Some(now);
            old.updated_at = now;
        }
    }
    state.records.push(record);
    let new = state.records.last().cloned();
    record_revision(
        state,
        LtmRevisionOp::Replace,
        &key,
        Some(&latest),
        new.as_ref(),
        input.source,
        "конфликт: более поздняя запись заменила старую",
        now,
    );
    state.updated_at = now;
    prune_versions(state);

    let old_id = old_ids
        .first()
        .cloned()
        .unwrap_or_else(|| latest.id.clone());
    LtmChange::Replaced {
        old: old_id,
        new: new_id,
    }
}

pub fn remove_ltm(state: &mut LtmState, id: &str) -> bool {
    let before = state.records.len();
    state.records.retain(|record| record.id != id);
    let removed = state.records.len() != before;
    if removed {
        state.updated_at = Utc::now();
    }
    removed
}

/// Удаляет все версии ключа (кнопка «удалить» в UI).
pub fn remove_ltm_key(state: &mut LtmState, key: &str) -> bool {
    let key = canonical_key(key);
    let before = state.records.len();
    state.records.retain(|record| record.key != key);
    let removed = state.records.len() != before;
    if removed {
        state.updated_at = Utc::now();
    }
    removed
}

pub fn set_pinned(state: &mut LtmState, key: &str, pinned: bool, now: DateTime<Utc>) -> bool {
    let key = canonical_key(key);
    let mut found = false;
    for record in &mut state.records {
        if record.is_active() && record.key == key {
            record.pinned = pinned;
            record.updated_at = now;
            found = true;
        }
    }
    if found {
        state.updated_at = now;
    }
    found
}

pub fn touch_used(state: &mut LtmState, ids: &[Id], now: DateTime<Utc>) {
    for record in &mut state.records {
        if ids.iter().any(|id| id == &record.id) {
            record.last_used_at = Some(now);
        }
    }
}

/// Не более активной + 2 superseded на ключ.
fn prune_versions(state: &mut LtmState) {
    let mut keys: Vec<String> = state
        .records
        .iter()
        .map(|record| record.key.clone())
        .collect();
    keys.sort();
    keys.dedup();
    for key in keys {
        let mut superseded: Vec<(DateTime<Utc>, usize)> = state
            .records
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                record.key == key
                    && record.superseded_by.is_some()
                    && record.superseded_at.is_some()
            })
            .map(|(index, record)| (record.superseded_at.unwrap_or(record.updated_at), index))
            .collect();
        superseded.sort_by_key(|a| std::cmp::Reverse(a.0));
        let to_remove: Vec<usize> = superseded.into_iter().skip(2).map(|(_, i)| i).collect();
        for index in to_remove.into_iter().rev() {
            state.records.remove(index);
        }
    }
}

/// Ограничение количества записей: pinned/profile не вытесняются.
pub fn prune_ltm(state: &mut LtmState, config: &LtmConfig) {
    let active_count = state.records.iter().filter(|r| r.is_active()).count();
    if active_count <= config.max_records {
        return;
    }
    let mut victims: Vec<(usize, DateTime<Utc>)> = state
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| {
            record.is_active()
                && !record.pinned
                && record.kind != LtmKind::Profile
                && record.kind != LtmKind::Project
        })
        .map(|(index, record)| (index, record.last_used_at.unwrap_or(record.updated_at)))
        .collect();
    victims.sort_by_key(|(_, at)| *at);
    let excess = active_count - config.max_records;
    for (index, _) in victims.into_iter().take(excess).rev() {
        state.records.remove(index);
    }
}

pub fn render_ltm_profile(state: &LtmState, max_tokens: usize) -> RenderedBlock {
    let mut records: Vec<&LtmRecord> = state
        .records
        .iter()
        .filter(|record| record.is_active() && record.kind.is_profile_block())
        .collect();
    records.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(a.kind.profile_order().cmp(&b.kind.profile_order()))
            .then(b.updated_at.cmp(&a.updated_at))
            .then(a.key.cmp(&b.key))
    });

    let mut block = RenderedBlock::default();
    let mut lines = vec![
        "## Долговременная память: профиль, проект, предпочтения".to_string(),
        "Это устойчивые данные пользователя. Считай их известными и не переспрашивай.".to_string(),
        String::new(),
    ];
    let mut kept: Vec<&LtmRecord> = Vec::new();
    let mut dropped: Vec<Id> = Vec::new();
    for record in records {
        let line = format!(
            "- [{}] {}: {} · записано {}",
            record.kind.as_str(),
            record.key,
            truncate_chars(&record.value, 500),
            record.write_time().format("%Y-%m-%d %H:%M")
        );
        let candidate_text = format!("{}\n{}", lines.join("\n"), line);
        if estimate_text_tokens(&candidate_text) > max_tokens && !record.pinned {
            dropped.push(record.id.clone());
            continue;
        }
        let line = if estimate_text_tokens(&line) > 80 && record.pinned {
            let mut text = line;
            while estimate_text_tokens(&text) > 70 && !text.is_empty() {
                text = truncate_chars(&text, text.chars().count().saturating_sub(1));
            }
            format!("{text}…")
        } else {
            line
        };
        lines.push(line);
        kept.push(record);
    }

    if kept.is_empty() {
        return block;
    }
    block.text = lines.join("\n").trim_end().to_string();
    block.tokens = estimate_text_tokens(&block.text);
    block.included = kept.iter().map(|record| record.id.clone()).collect();
    block.dropped = dropped;
    if block.tokens > max_tokens {
        block.text = truncate_block(&block.text, max_tokens);
        block.tokens = estimate_text_tokens(&block.text);
        block.notes.push("ltm: profile truncated".into());
    }
    block
}

pub fn render_ltm_relevant(records: &[&LtmRecord], max_tokens: usize) -> RenderedBlock {
    let mut block = RenderedBlock::default();
    if records.is_empty() {
        return block;
    }
    let mut lines = vec![
        "## Долговременная память: релевантное текущей задаче".to_string(),
        "Это данные из прошлых обсуждений. Учитывай их, но свежие реплики важнее.".to_string(),
        String::new(),
    ];
    let mut kept = Vec::new();
    for record in records {
        let line = format!(
            "- [{}, {}, записано {}] {}: {}",
            record.kind.as_str(),
            record.policy.as_str(),
            record.write_time().format("%Y-%m-%d %H:%M"),
            record.key,
            truncate_chars(&record.value, 500)
        );
        let candidate_text = format!("{}\n{}", lines.join("\n"), line);
        if estimate_text_tokens(&candidate_text) > max_tokens && !record.pinned {
            break;
        }
        lines.push(line);
        kept.push((*record).clone());
    }
    if kept.is_empty() {
        block.dropped = records.iter().map(|record| record.id.clone()).collect();
        return block;
    }
    block.text = lines.join("\n").trim_end().to_string();
    block.tokens = estimate_text_tokens(&block.text);
    block.included = kept.iter().map(|record| record.id.clone()).collect();
    block.dropped = records
        .iter()
        .filter(|record| !kept.iter().any(|kept| kept.id == record.id))
        .map(|record| record.id.clone())
        .collect();
    block
}

fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.to_string()
    } else {
        let mut text: String = value.chars().take(limit.saturating_sub(1)).collect();
        text.push('…');
        text
    }
}

fn truncate_block(text: &str, max_tokens: usize) -> String {
    let mut text = text.to_string();
    while estimate_text_tokens(&text) > max_tokens && !text.is_empty() {
        match text.rfind('\n') {
            Some(pos) => text.truncate(pos),
            None => {
                text = truncate_chars(&text, text.chars().count().saturating_sub(1));
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::MemorySource;

    fn input(key: &str, value: &str, kind: LtmKind) -> UpsertLtm {
        UpsertLtm::new(kind, key, value, MemorySource::User)
    }

    #[test]
    fn create_and_unique_key() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        let change = upsert_ltm(
            &mut state,
            input("project.name", "«Атлас»", LtmKind::Project),
            now,
        );
        assert!(matches!(change, LtmChange::Created(_)));
        assert_eq!(active_ltm(&state).count(), 1);
        assert!(find_active_ltm(&state, "project.name").is_some());
    }

    #[test]
    fn same_value_is_same() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        upsert_ltm(&mut state, input("a", "1", LtmKind::Knowledge), now);
        let change = upsert_ltm(&mut state, input("a", "1", LtmKind::Knowledge), now);
        assert!(matches!(change, LtmChange::Same(_)));
        assert_eq!(state.records.len(), 1);
    }

    #[test]
    fn replace_sets_superseded_by() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        upsert_ltm(&mut state, input("budget", "120", LtmKind::Knowledge), now);
        let change = upsert_ltm(&mut state, input("budget", "150", LtmKind::Knowledge), now);
        assert!(matches!(change, LtmChange::Replaced { .. }));
        let active: Vec<&LtmRecord> = active_ltm(&state).collect();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].value, "150");
        assert!(active[0].superseded_by.is_none());
        assert_eq!(state.records.len(), 2);
        assert!(state.records[0].superseded_by.is_some());
    }

    #[test]
    fn active_ltm_excludes_superseded() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        upsert_ltm(&mut state, input("a", "1", LtmKind::Knowledge), now);
        upsert_ltm(&mut state, input("a", "2", LtmKind::Knowledge), now);
        assert_eq!(state.records.len(), 2);
        assert_eq!(active_ltm(&state).count(), 1);
    }

    #[test]
    fn profile_renders_pinned_first() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        upsert_ltm(
            &mut state,
            input("project.name", "«Атлас»", LtmKind::Project),
            now,
        );
        let mut pinned = input("user.name", "Алексей", LtmKind::Profile);
        pinned.pinned = true;
        upsert_ltm(&mut state, pinned, now);
        upsert_ltm(
            &mut state,
            input("preference.style", "коротко", LtmKind::Preference),
            now,
        );
        upsert_ltm(
            &mut state,
            input("decision.db", "PostgreSQL", LtmKind::Decision),
            now,
        );
        let block = render_ltm_profile(&state, 1000);
        assert!(block.text.contains("user.name"));
        assert!(!block.text.contains("decision.db"));
        let user = block.text.find("user.name").unwrap();
        let project = block.text.find("project.name").unwrap();
        assert!(user < project);
    }

    #[test]
    fn profile_budget_drops_unpinned_but_keeps_pinned() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        for index in 0..10 {
            upsert_ltm(
                &mut state,
                input(
                    &format!("knowledge.empty.{index}"),
                    &"очень длинное значение ".repeat(10),
                    LtmKind::Knowledge,
                ),
                now,
            );
        }
        let mut pinned = input("user.name", "Алексей", LtmKind::Profile);
        pinned.pinned = true;
        upsert_ltm(&mut state, pinned, now);
        let block = render_ltm_profile(&state, 80);
        assert!(block.text.contains("user.name"));
        assert!(block.tokens <= 80 || block.text.contains("…"));
    }

    #[test]
    fn limits_and_sanitizer_apply() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        let mut input = input(
            &format!("key.{}", "a".repeat(100)),
            &format!("значение\n{}", "x".repeat(600)),
            LtmKind::Knowledge,
        );
        input.tags = (0..12).map(|index| format!("tag{index}")).collect();
        upsert_ltm(&mut state, input, now);
        let record = &state.records[0];
        assert_eq!(record.key.chars().count(), 64);
        assert_eq!(record.value.chars().count(), 500);
        assert_eq!(record.tags.len(), 8);
        assert!(!record.value.contains('\n'));
    }

    #[test]
    fn versions_pruned_to_active_plus_two() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        for value in ["1", "2", "3", "4", "5"] {
            upsert_ltm(&mut state, input("a", value, LtmKind::Knowledge), now);
        }
        assert_eq!(state.records.len(), 3);
        assert_eq!(active_ltm(&state).count(), 1);
    }

    #[test]
    fn pin_toggles_and_remove_works() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        upsert_ltm(&mut state, input("a", "1", LtmKind::Knowledge), now);
        assert!(set_pinned(&mut state, "a", true, now));
        assert!(find_active_ltm(&state, "a").unwrap().pinned);
        assert!(set_pinned(&mut state, "a", false, now));
        assert!(!find_active_ltm(&state, "a").unwrap().pinned);
        assert!(remove_ltm_key(&mut state, "a"));
        assert!(state.records.is_empty());
    }

    #[test]
    fn prune_ltm_keeps_profile() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        for index in 0..10 {
            upsert_ltm(
                &mut state,
                input(&format!("k{index}"), "v", LtmKind::Knowledge),
                now,
            );
        }
        upsert_ltm(
            &mut state,
            input("user.name", "Алексей", LtmKind::Profile),
            now,
        );
        let config = LtmConfig {
            max_records: 3,
            ..LtmConfig::default()
        };
        prune_ltm(&mut state, &config);
        assert!(find_active_ltm(&state, "user.name").is_some());
        assert!(active_ltm(&state).count() <= 8);
    }

    #[test]
    fn relevant_block_obeys_budget() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        for index in 0..5 {
            upsert_ltm(
                &mut state,
                input(
                    &format!("k{index}"),
                    &"длинное значение ".repeat(20),
                    LtmKind::Knowledge,
                ),
                now,
            );
        }
        let records: Vec<&LtmRecord> = active_ltm(&state).collect();
        let block = render_ltm_relevant(&records, 60);
        assert!(block.tokens <= 60 || block.included.len() <= 1);
    }

    #[test]
    fn earlier_recorded_at_cannot_override_later_value() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        let mut later = input("project.deadline", "20 декабря 2026", LtmKind::Knowledge);
        later.recorded_at = Some(now);
        upsert_ltm(&mut state, later, now);
        let mut earlier = input("project.deadline", "15 декабря 2026", LtmKind::Knowledge);
        earlier.recorded_at = Some(now - chrono::Duration::hours(1));
        let change = upsert_ltm(&mut state, earlier, now);
        assert!(matches!(change, LtmChange::Stale { .. }));
        assert_eq!(
            find_active_ltm(&state, "project.deadline").unwrap().value,
            "20 декабря 2026"
        );
        assert_eq!(active_ltm(&state).count(), 1);
    }

    #[test]
    fn append_policy_accumulates_without_superseding() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        upsert_ltm(
            &mut state,
            input(
                "decisions.accepted",
                "выбрали PostgreSQL",
                LtmKind::Decision,
            ),
            now,
        );
        upsert_ltm(
            &mut state,
            input("decisions.accepted", "выбрали REST", LtmKind::Decision),
            now,
        );
        let active: Vec<&LtmRecord> = active_ltm(&state).collect();
        assert_eq!(active.len(), 2);
        assert!(
            active
                .iter()
                .all(|record| record.key == "decisions.accepted")
        );
        assert_eq!(
            state
                .revisions
                .iter()
                .filter(|revision| revision.op == LtmRevisionOp::Append)
                .count(),
            2
        );
    }

    #[test]
    fn alias_key_replaces_existing_record() {
        let now = Utc::now();
        let mut state = LtmState::empty(now);
        upsert_ltm(
            &mut state,
            input("project.deadline", "15 декабря", LtmKind::Knowledge),
            now,
        );
        let change = upsert_ltm(
            &mut state,
            input("срок", "20 декабря", LtmKind::Knowledge),
            now + chrono::Duration::seconds(1),
        );
        assert!(matches!(change, LtmChange::Replaced { .. }));
        assert_eq!(
            find_active_ltm(&state, "project.deadline").unwrap().value,
            "20 декабря"
        );
        assert_eq!(active_ltm(&state).count(), 1);
    }

    #[test]
    fn legacy_json_without_day15_fields_loads_with_defaults() {
        let raw = r#"{
          "version": 1,
          "conversation_id": null,
          "records": [{
            "id": "ltm_old",
            "kind": "knowledge",
            "key": "project.deadline",
            "value": "15 декабря 2026",
            "source": "user",
            "confidence": 0.8,
            "pinned": false,
            "created_at": "2026-11-17T10:00:00Z",
            "updated_at": "2026-11-17T10:00:00Z",
            "last_used_at": null,
            "superseded_by": null,
            "superseded_at": null,
            "legacy_key": null
          }],
          "updated_at": "2026-11-17T10:00:00Z"
        }"#;
        let state: LtmState = serde_json::from_str(raw).unwrap();
        assert!(state.revisions.is_empty());
        let record = &state.records[0];
        assert_eq!(record.policy, FactPolicy::Replace);
        assert_eq!(record.revision, 1);
        assert_eq!(record.write_time(), record.updated_at);
    }
}
