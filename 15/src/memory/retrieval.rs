//! Лексический retrieval LTM: score по key/tags/value + pinned + свежесть.
//! Без embeddings: детерминированно и бесплатно (ADR-7).

use chrono::{DateTime, Duration, Utc};

use crate::domain::memory::{Id, LtmKind, LtmRecord, WorkingMemoryState};
use crate::memory::tokens::estimate_text_tokens;

const STOP_WORDS: &[&str] = &[
    "и",
    "в",
    "во",
    "не",
    "на",
    "с",
    "со",
    "а",
    "но",
    "что",
    "это",
    "как",
    "по",
    "к",
    "из",
    "у",
    "за",
    "для",
    "от",
    "до",
    "или",
    "же",
    "бы",
    "то",
    "мы",
    "вы",
    "я",
    "он",
    "она",
    "они",
    "все",
    "еще",
    "уже",
    "только",
    "если",
    "когда",
    "где",
    "какой",
    "какая",
    "какие",
    "так",
    "там",
    "тут",
];

pub struct RetrievalInput<'a> {
    pub records: &'a [LtmRecord],
    pub query: &'a str,
    pub wm: &'a WorkingMemoryState,
    pub max_tokens: usize,
    pub top_k: usize,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct RetrievalResult {
    pub selected: Vec<LtmRecord>,
    pub dropped: Vec<LtmRecord>,
    pub scores: Vec<(Id, f64)>,
    pub notes: Vec<String>,
}

pub fn select_relevant_ltm(input: RetrievalInput<'_>) -> RetrievalResult {
    let mut query_text = input.query.to_lowercase();
    for item in input.wm.items.iter().filter(|item| {
        item.status == crate::domain::memory::WmStatus::Active && item.ref_ltm_id.is_none()
    }) {
        query_text.push(' ');
        query_text.push_str(&item.key);
        query_text.push(' ');
        query_text.push_str(&item.value);
    }
    let tokens = tokenize(&query_text);
    let now = input.now;

    let mut candidates: Vec<(LtmRecord, f64)> = Vec::new();
    let mut dropped: Vec<LtmRecord> = Vec::new();
    let mut notes = Vec::new();

    for record in input.records.iter().filter(|record| record.is_active()) {
        if !record.kind.is_relevant_block() {
            continue;
        }
        // shadowed-by-wm: WM с тем же ключом и другим значением важнее.
        if let Some(wm_item) = input.wm.items.iter().find(|item| {
            item.key == record.key && item.status == crate::domain::memory::WmStatus::Active
        }) {
            if wm_item.value != record.value {
                dropped.push(record.clone());
                notes.push(format!("ltm: {} shadowed-by-wm", record.key));
                continue;
            }
        }
        let score = score_record(record, &tokens, now);
        if score > 0.0 {
            candidates.push((record.clone(), score));
        } else {
            dropped.push(record.clone());
        }
    }

    candidates.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .partial_cmp(left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(right.updated_at.cmp(&left.updated_at))
            .then(left.key.cmp(&right.key))
    });

    let mut selected: Vec<LtmRecord> = Vec::new();
    let mut used_tokens = 0usize;
    let mut scores = Vec::new();
    for (record, score) in candidates {
        if selected.len() >= input.top_k {
            dropped.push(record);
            continue;
        }
        let line = format!(
            "- [{}, {}] {}: {}",
            record.kind.as_str(),
            record.updated_at.format("%Y-%m-%d"),
            record.key,
            record.value
        );
        let cost = estimate_text_tokens(&line);
        if !selected.is_empty() && used_tokens + cost > input.max_tokens {
            dropped.push(record);
            continue;
        }
        used_tokens += cost;
        scores.push((record.id.clone(), score));
        selected.push(record);
    }

    if selected.len() < input.top_k && !dropped.is_empty() {
        notes.push(format!(
            "ltm: selected {} of {} candidates",
            selected.len(),
            selected.len() + dropped.len()
        ));
    }

    RetrievalResult {
        selected,
        dropped,
        scores,
        notes,
    }
}

fn score_record(record: &LtmRecord, tokens: &[String], now: DateTime<Utc>) -> f64 {
    let key_text = normalize(&record.key);
    let value_text = normalize(&record.value);
    let tags_text: String = if record.tags.is_empty() {
        crate::memory::lifecycle::key_tags(&record.key).join(" ")
    } else {
        record
            .tags
            .iter()
            .map(|tag| normalize(tag))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut key_hits = 0;
    let mut value_hits = 0;
    let mut tag_hits = 0;
    for token in tokens {
        if key_text.split(' ').any(|part| part == token) {
            key_hits += 1;
        }
        if tags_text.split(' ').any(|part| part == token) {
            tag_hits += 1;
        }
        if value_text.split(' ').any(|part| part == token) {
            value_hits += 1;
        }
    }
    let base = 3.0 * key_hits as f64 + 2.0 * tag_hits as f64 + value_hits as f64;
    if base == 0.0 && !record.pinned {
        return 0.0;
    }
    let weight = match record.kind {
        LtmKind::Decision => 1.2,
        LtmKind::Knowledge => 1.0,
        LtmKind::Glossary => 0.8,
        _ => 0.0,
    };
    let confidence = 0.5 + f64::from(record.confidence.clamp(0.0, 1.0)) / 2.0;
    let pinned_bonus = if record.pinned { 100.0 } else { 0.0 };
    let fresh_bonus = if now - record.updated_at <= Duration::days(7) {
        0.5
    } else {
        0.0
    };
    base * weight * confidence + pinned_bonus + fresh_bonus
}

/// Нормализация: lower-case, ё → е, пунктуация → пробел.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.to_lowercase().chars() {
        let ch = if ch == 'ё' { 'е' } else { ch };
        if ch.is_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(ch);
        } else {
            space = true;
        }
    }
    out
}

pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for token in normalize(text).split_whitespace() {
        if token.chars().count() < 3 || STOP_WORDS.contains(&token) {
            continue;
        }
        if !tokens.iter().any(|existing| existing == token) {
            tokens.push(token.to_string());
        }
        for synonym in synonyms(token) {
            if !tokens.iter().any(|existing| existing == synonym) {
                tokens.push(synonym.to_string());
            }
        }
    }
    tokens
}

/// Простые лексические соответствия русских слов ключам на латинице.
/// Это не embeddings: список фиксирован и проверяем.
fn synonyms(token: &str) -> &'static [&'static str] {
    let stem = token.trim_end_matches(['а', 'я', 'ы', 'и', 'е', 'у', 'ю', 'о', 'ь']);
    match stem {
        "дедлайн" | "срок" => &["deadline", "milestone"],
        "бюджет" => &["budget"],
        "ограничени" | "инфраструктур" | "цод" | "облак" => {
            &["constraint", "infra"]
        }
        "баз" | "данн" | "database" => &["database"],
        "проект" => &["project"],
        "заказчик" => &["customer"],
        "тимлид" => &["team"],
        "команд" => &["team"],
        "релиз" => &["release", "milestone"],
        "прототип" => &["prototype", "milestone"],
        "предпочтени" => &["preference"],
        "решени" => &["decision"],
        "риск" => &["risk"],
        "им" | "называ" | "зов" => &["name", "user"],
        "маркетинг" => &["marketing"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::{LtmRecord, LtmState, MemorySource, WmItem, WmKind, WmStatus};
    use chrono::TimeZone;

    fn record(key: &str, value: &str, kind: LtmKind) -> LtmRecord {
        let now = Utc.with_ymd_and_hms(2026, 11, 17, 10, 0, 0).unwrap();
        LtmRecord {
            id: format!("ltm_{key}"),
            kind,
            key: key.into(),
            value: value.into(),
            tags: Vec::new(),
            source: MemorySource::User,
            confidence: 1.0,
            pinned: false,
            policy: crate::domain::memory::FactPolicy::Replace,
            scope: crate::domain::memory::CandidateScope::Project,
            revision: 1,
            created_at: now,
            updated_at: now,
            recorded_at: Some(now),
            last_used_at: None,
            superseded_by: None,
            superseded_at: None,
            legacy_key: None,
        }
    }

    fn wm_empty() -> WorkingMemoryState {
        WorkingMemoryState::empty(Utc::now())
    }

    fn run(records: &[LtmRecord], query: &str, top_k: usize, max_tokens: usize) -> RetrievalResult {
        select_relevant_ltm(RetrievalInput {
            records,
            query,
            wm: &wm_empty(),
            max_tokens,
            top_k,
            now: Utc.with_ymd_and_hms(2026, 11, 17, 12, 0, 0).unwrap(),
        })
    }

    #[test]
    fn key_match_weighs_more_than_value() {
        let records = vec![
            record("decision.database", "PostgreSQL", LtmKind::Decision),
            record("note.random", "database", LtmKind::Knowledge),
        ];
        let result = run(&records, "database", 8, 1000);
        assert_eq!(result.selected[0].key, "decision.database");
    }

    #[test]
    fn pinned_is_first() {
        let mut records = vec![
            record("decision.database", "PostgreSQL", LtmKind::Decision),
            record("knowledge.infra", "ЦОД", LtmKind::Knowledge),
        ];
        records[1].pinned = true;
        let result = run(&records, "postgresql", 8, 1000);
        assert_eq!(result.selected[0].key, "knowledge.infra");
    }

    #[test]
    fn top_k_is_respected() {
        let records: Vec<LtmRecord> = (0..5)
            .map(|index| {
                record(
                    &format!("knowledge.item{index}"),
                    "проект атлас",
                    LtmKind::Knowledge,
                )
            })
            .collect();
        let result = run(&records, "проект атлас", 2, 1000);
        assert_eq!(result.selected.len(), 2);
        assert_eq!(result.dropped.len(), 3);
    }

    #[test]
    fn budget_is_respected() {
        let records = vec![
            record(
                "knowledge.a",
                &"длинное значение ".repeat(40),
                LtmKind::Knowledge,
            ),
            record(
                "knowledge.b",
                &"длинное значение ".repeat(40),
                LtmKind::Knowledge,
            ),
        ];
        let result = run(&records, "значение", 8, 20);
        assert!(result.selected.len() <= 1);
    }

    #[test]
    fn superseded_is_never_selected() {
        let mut superseded = record("decision.database", "MongoDB", LtmKind::Decision);
        superseded.superseded_by = Some("ltm_new".into());
        let records = vec![superseded];
        let result = run(&records, "mongodb", 8, 1000);
        assert!(result.selected.is_empty());
    }

    #[test]
    fn shadowed_by_wm_is_dropped() {
        let records = vec![record(
            "budget.marketing",
            "120 000 рублей",
            LtmKind::Knowledge,
        )];
        let now = Utc::now();
        let mut wm = WorkingMemoryState::empty(now);
        wm.items.push(WmItem {
            id: "wm_1".into(),
            kind: WmKind::Entity,
            key: "budget.marketing".into(),
            value: "150 000 рублей".into(),
            status: WmStatus::Active,
            order: None,
            ref_ltm_id: None,
            created_at: now,
            updated_at: now,
            source_message_index: None,
        });
        let result = select_relevant_ltm(RetrievalInput {
            records: &records,
            query: "бюджет",
            wm: &wm,
            max_tokens: 1000,
            top_k: 8,
            now,
        });
        // Значение совпало по ключу, но разное — LTM исключается.
        assert!(result.selected.is_empty());
        assert_eq!(result.dropped.len(), 1);
    }

    #[test]
    fn empty_query_does_not_panic() {
        let records = vec![record("decision.database", "PostgreSQL", LtmKind::Decision)];
        let result = run(&records, "", 8, 1000);
        assert!(result.selected.is_empty());
    }

    #[test]
    fn equal_score_is_deterministic() {
        let records = vec![
            record("knowledge.b", "атлас", LtmKind::Knowledge),
            record("knowledge.a", "атлас", LtmKind::Knowledge),
        ];
        let first = run(&records, "атлас", 8, 1000);
        let second = run(&records, "атлас", 8, 1000);
        let keys1: Vec<&str> = first.selected.iter().map(|r| r.key.as_str()).collect();
        let keys2: Vec<&str> = second.selected.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys1, keys2);
        assert_eq!(keys1, vec!["knowledge.a", "knowledge.b"]);
    }

    #[test]
    fn normalize_handles_yo_and_punctuation() {
        assert_eq!(normalize("Ёлка, тест!"), "елка тест");
        assert_eq!(tokenize("и в на проект"), vec!["проект", "project"]);
    }

    #[test]
    fn only_relevant_kinds_are_selected() {
        let records = vec![
            record("user.name", "Алексей", LtmKind::Profile),
            record("project.name", "«Атлас»", LtmKind::Project),
        ];
        let result = run(&records, "алексей атлас", 8, 1000);
        assert!(result.selected.is_empty());
    }

    #[test]
    fn search_does_not_include_superseded_records_state() {
        let mut state = LtmState::empty(Utc::now());
        state.records.push(record("a", "1", LtmKind::Knowledge));
        let active: Vec<LtmRecord> = crate::memory::ltm::active_ltm(&state).cloned().collect();
        assert_eq!(active.len(), 1);
    }
}
