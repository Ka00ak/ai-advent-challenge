//! LTM: активные записи, версии, pin и рендер блоков профиля.

use chrono::{DateTime, Utc};

use crate::domain::memory::{Id, LtmKind, LtmRecord, LtmState, MemorySource, RenderedBlock};
use crate::domain::settings::LtmConfig;
use crate::memory::tokens::estimate_text_tokens;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LtmChange {
    Created(Id),
    Updated(Id),
    Same(Id),
    Replaced { old: Id, new: Id },
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
}

impl UpsertLtm {
    pub fn new(
        kind: LtmKind,
        key: impl Into<String>,
        value: impl Into<String>,
        source: MemorySource,
    ) -> Self {
        Self {
            kind,
            key: key.into(),
            value: value.into(),
            tags: Vec::new(),
            source,
            confidence: 0.8,
            pinned: false,
            legacy_key: None,
        }
    }
}

pub fn active_ltm(state: &LtmState) -> impl Iterator<Item = &LtmRecord> {
    state.records.iter().filter(|record| record.is_active())
}

pub fn find_active_ltm<'a>(state: &'a LtmState, key: &str) -> Option<&'a LtmRecord> {
    state
        .records
        .iter()
        .find(|record| record.is_active() && record.key == key)
}

pub fn find_active_ltm_by_id<'a>(state: &'a LtmState, id: &str) -> Option<&'a LtmRecord> {
    state
        .records
        .iter()
        .find(|record| record.is_active() && record.id == id)
}

fn clean_input(input: &UpsertLtm) -> (String, String, Vec<String>, f32) {
    let key = crate::memory::audit::sanitize_key(&input.key);
    let value = crate::memory::audit::sanitize_value(&input.value);
    let tags = crate::memory::audit::sanitize_tags(&input.tags);
    let confidence = input.confidence.clamp(0.0, 1.0);
    (key, value, tags, confidence)
}

fn new_record(input: &UpsertLtm, now: DateTime<Utc>) -> LtmRecord {
    let (key, value, tags, confidence) = clean_input(input);
    LtmRecord {
        id: format!("ltm_{}", uuid::Uuid::new_v4().simple()),
        kind: input.kind,
        key,
        value,
        tags,
        source: input.source,
        confidence,
        pinned: input.pinned,
        created_at: now,
        updated_at: now,
        last_used_at: None,
        superseded_by: None,
        superseded_at: None,
        legacy_key: input.legacy_key.clone(),
    }
}

pub fn upsert_ltm(state: &mut LtmState, input: UpsertLtm, now: DateTime<Utc>) -> LtmChange {
    let key = crate::memory::audit::sanitize_key(&input.key);
    if let Some(existing) = state
        .records
        .iter()
        .find(|record| record.is_active() && record.key == key)
    {
        let value = crate::memory::audit::sanitize_value(&input.value);
        if existing.value == value {
            return LtmChange::Same(existing.id.clone());
        }
        return replace_ltm(state, input, now);
    }
    let record = new_record(&input, now);
    let id = record.id.clone();
    state.records.push(record);
    state.updated_at = now;
    prune_versions(state);
    LtmChange::Created(id)
}

/// Создаёт новую активную версию, старую помечает `superseded_by`.
pub fn replace_ltm(state: &mut LtmState, input: UpsertLtm, now: DateTime<Utc>) -> LtmChange {
    let key = crate::memory::audit::sanitize_key(&input.key);
    let old_id = state
        .records
        .iter()
        .find(|record| record.is_active() && record.key == key)
        .map(|record| record.id.clone());

    let old_pinned = state
        .records
        .iter()
        .find(|record| record.id == old_id.clone().unwrap_or_default())
        .map(|record| record.pinned)
        .unwrap_or(false);
    let mut record = new_record(&input, now);
    record.pinned = record.pinned || old_pinned;
    let new_id = record.id.clone();

    if let Some(old_id) = old_id {
        if let Some(old) = state.records.iter_mut().find(|record| record.id == old_id) {
            old.superseded_by = Some(new_id.clone());
            old.superseded_at = Some(now);
            old.updated_at = now;
        }
        state.records.push(record);
        state.updated_at = now;
        prune_versions(state);
        LtmChange::Replaced {
            old: old_id,
            new: new_id,
        }
    } else {
        state.records.push(record);
        state.updated_at = now;
        prune_versions(state);
        LtmChange::Created(new_id)
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
    let before = state.records.len();
    state.records.retain(|record| record.key != key);
    let removed = state.records.len() != before;
    if removed {
        state.updated_at = Utc::now();
    }
    removed
}

pub fn set_pinned(state: &mut LtmState, key: &str, pinned: bool, now: DateTime<Utc>) -> bool {
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
            "- [{}] {}: {}",
            record.kind.as_str(),
            record.key,
            truncate_chars(&record.value, 500)
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
            "- [{}, {}] {}: {}",
            record.kind.as_str(),
            record.updated_at.format("%Y-%m-%d"),
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
}
