//! Импорт facts Дня 10 (`lab10.facts.v1`, `{"facts":[...]}` или массив).
//!
//! Миграция идемпотентна: совпадающие записи пропускаются, изменённые создают
//! новую версию, значения сохраняются дословно.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::memory::{LtmKind, LtmState, MemorySource};
use crate::memory::ltm::{UpsertLtm, find_active_ltm, replace_ltm, upsert_ltm};
use crate::storage::atomic::SaveError;
use crate::storage::paths::DataPaths;

pub const MIGRATED_TAG: &str = "migrated:day10";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    pub at: DateTime<Utc>,
    pub source: String,
    pub found: usize,
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
}

impl MigrationReport {
    pub fn empty(source: &str, ok: bool, error: Option<String>) -> Self {
        Self {
            at: Utc::now(),
            source: source.to_string(),
            found: 0,
            added: 0,
            updated: 0,
            skipped: 0,
            errors: Vec::new(),
            ok,
            error,
        }
    }

    pub fn total_changes(&self) -> usize {
        self.added + self.updated
    }
}

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("не удалось прочитать {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("некорректный JSON facts: {0}")]
    Json(String),
    #[error("в файле нет массива facts")]
    NoFacts,
    #[error("не удалось сохранить отчёт: {0}")]
    Save(String),
}

impl From<SaveError> for MigrationError {
    fn from(error: SaveError) -> Self {
        MigrationError::Save(error.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyFact {
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub category: String,
}

/// Читает файл и возвращает отчёт; ошибки входа не паникуют.
pub fn run_import(
    ltm: &mut LtmState,
    path: &Path,
    now: DateTime<Utc>,
) -> Result<MigrationReport, MigrationError> {
    let source = path.display().to_string();
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) => {
            let mut report =
                MigrationReport::empty(&source, false, Some(format!("чтение: {error}")));
            report
                .errors
                .push(format!("не удалось прочитать файл: {error}"));
            return Ok(report);
        }
    };
    let facts = match parse_facts(&raw) {
        Ok(facts) => facts,
        Err(error) => {
            let mut report = MigrationReport::empty(&source, false, Some(error.to_string()));
            report.errors.push(error.to_string());
            return Ok(report);
        }
    };
    Ok(migrate_facts(ltm, &facts, &source, now))
}

pub fn parse_facts(raw: &str) -> Result<Vec<LegacyFact>, MigrationError> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| MigrationError::Json(error.to_string()))?;
    let array = if value.is_array() {
        value
    } else if let Some(facts) = value.get("facts") {
        facts.clone()
    } else {
        return Err(MigrationError::NoFacts);
    };
    let array = array.as_array().ok_or(MigrationError::NoFacts)?;
    let mut facts = Vec::new();
    for item in array {
        let key = item
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let value = item
            .get("value")
            .map(|value| match value {
                serde_json::Value::String(text) => text.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        let category = item
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        facts.push(LegacyFact {
            key,
            value,
            category,
        });
    }
    Ok(facts)
}

pub fn migrate_facts(
    ltm: &mut LtmState,
    facts: &[LegacyFact],
    source: &str,
    now: DateTime<Utc>,
) -> MigrationReport {
    let mut report = MigrationReport::empty(source, true, None);
    report.found = facts.len();
    for fact in facts {
        let key = fact.key.trim();
        if key.is_empty() || fact.value.trim().is_empty() {
            report
                .errors
                .push("пропущен факт без key/value".to_string());
            continue;
        }
        let kind = map_category(&fact.category, key);
        let mut tags = vec![MIGRATED_TAG.to_string()];
        if !fact.category.trim().is_empty() {
            tags.push(fact.category.trim().to_string());
        }
        if kind == LtmKind::Knowledge && fact.category.trim() == "риск" {
            tags.push("risk".to_string());
        }
        let input = UpsertLtm {
            kind,
            key: key.to_string(),
            value: fact.value.clone(),
            tags,
            source: MemorySource::MigrationFacts,
            confidence: 0.9,
            pinned: false,
            legacy_key: Some(key.to_string()),
            policy: None,
            scope: crate::domain::memory::CandidateScope::Project,
            recorded_at: Some(now),
        };
        if let Some(existing) = find_active_ltm(ltm, key) {
            if existing.value == fact.value {
                report.skipped += 1;
                continue;
            }
            replace_ltm(ltm, input, now);
            report.updated += 1;
        } else {
            upsert_ltm(ltm, input, now);
            report.added += 1;
        }
    }
    report.ok = report.errors.is_empty();
    if !report.ok {
        report.error = Some(format!("{} ошибок", report.errors.len()));
    }
    report
}

pub fn map_category(category: &str, key: &str) -> LtmKind {
    match category.trim().to_lowercase().as_str() {
        "цель" => LtmKind::Project,
        "срок" | "бюджет" | "ограничение" | "риск" | "другое" => {
            LtmKind::Knowledge
        }
        "решение" | "договорённость" | "договоренность" => {
            LtmKind::Decision
        }
        "предпочтение" => LtmKind::Preference,
        "имя" => {
            let lower = key.to_lowercase();
            if lower.starts_with("user.") || lower.starts_with("profile.") {
                LtmKind::Profile
            } else if lower.starts_with("project.")
                || lower == "customer"
                || lower.starts_with("team.")
                || lower == "goal"
            {
                LtmKind::Project
            } else {
                LtmKind::Knowledge
            }
        }
        _ => LtmKind::Knowledge,
    }
}

pub fn save_report(paths: &DataPaths, report: &MigrationReport) -> Result<(), SaveError> {
    crate::storage::atomic::atomic_write_json(&paths.migration_report(), report)
}

pub fn load_report(paths: &DataPaths) -> Option<MigrationReport> {
    let raw = std::fs::read_to_string(paths.migration_report()).ok()?;
    serde_json::from_str(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> String {
        r#"{
          "version": "lab10.facts.v1",
          "facts": [
            {"key":"project.name","value":"«Атлас»","category":"имя"},
            {"key":"customer","value":"Марина Ковалёва","category":"имя"},
            {"key":"user.name","value":"Алексей","category":"имя"},
            {"key":"team.lead","value":"Артём Соколов","category":"имя"},
            {"key":"goal","value":"Запускаем проект","category":"цель"},
            {"key":"project.deadline","value":"15 декабря 2026","category":"срок"},
            {"key":"budget.total","value":"480 000 рублей","category":"бюджет"},
            {"key":"constraint.infra","value":"Только российские ЦОД","category":"ограничение"},
            {"key":"team.size","value":"4 разработчика","category":"ограничение"},
            {"key":"milestone.release","value":"20 декабря 2026","category":"срок"},
            {"key":"decision.database","value":"PostgreSQL","category":"решение"},
            {"key":"preference.style","value":"Отвечать коротко","category":"предпочтение"}
          ]
        }"#
        .to_string()
    }

    #[test]
    fn twelve_facts_map_to_kinds() {
        let facts = parse_facts(&sample_json()).unwrap();
        let mut ltm = LtmState::empty(Utc::now());
        let report = migrate_facts(&mut ltm, &facts, "test", Utc::now());
        assert_eq!(report.found, 12);
        assert_eq!(report.added, 12);
        assert!(report.ok);
        assert_eq!(ltm.records.len(), 12);
        assert_eq!(
            ltm.records
                .iter()
                .find(|r| r.key == "project.name")
                .unwrap()
                .kind,
            LtmKind::Project
        );
        assert_eq!(
            ltm.records
                .iter()
                .find(|r| r.key == "user.name")
                .unwrap()
                .kind,
            LtmKind::Profile
        );
        assert_eq!(
            ltm.records
                .iter()
                .find(|r| r.key == "decision.database")
                .unwrap()
                .kind,
            LtmKind::Decision
        );
    }

    #[test]
    fn values_are_verbatim() {
        let facts = parse_facts(&sample_json()).unwrap();
        let mut ltm = LtmState::empty(Utc::now());
        migrate_facts(&mut ltm, &facts, "test", Utc::now());
        let deadline = ltm
            .records
            .iter()
            .find(|r| r.key == "project.deadline")
            .unwrap();
        assert_eq!(deadline.value, "15 декабря 2026");
        let budget = ltm
            .records
            .iter()
            .find(|r| r.key == "budget.total")
            .unwrap();
        assert_eq!(budget.value, "480 000 рублей");
    }

    #[test]
    fn second_import_is_idempotent() {
        let facts = parse_facts(&sample_json()).unwrap();
        let mut ltm = LtmState::empty(Utc::now());
        migrate_facts(&mut ltm, &facts, "test", Utc::now());
        let report = migrate_facts(&mut ltm, &facts, "test", Utc::now());
        assert_eq!(report.found, 12);
        assert_eq!(report.added, 0);
        assert_eq!(report.updated, 0);
        assert_eq!(report.skipped, 12);
    }

    #[test]
    fn changed_fact_creates_version() {
        let facts = parse_facts(&sample_json()).unwrap();
        let mut ltm = LtmState::empty(Utc::now());
        migrate_facts(&mut ltm, &facts, "test", Utc::now());
        let mut changed = facts.clone();
        changed[0].value = "«Атлас-2»".into();
        let report = migrate_facts(&mut ltm, &changed, "test", Utc::now());
        assert_eq!(report.updated, 1);
        assert_eq!(
            find_active_ltm(&ltm, "project.name").unwrap().value,
            "«Атлас-2»"
        );
        assert_eq!(
            ltm.records
                .iter()
                .filter(|r| r.key == "project.name")
                .count(),
            2
        );
    }

    #[test]
    fn broken_json_does_not_panic() {
        let mut ltm = LtmState::empty(Utc::now());
        let report =
            run_import(&mut ltm, Path::new("/definitely/missing.json"), Utc::now()).unwrap();
        assert!(!report.ok);
        assert!(!report.errors.is_empty());
    }

    #[test]
    fn empty_or_unknown_input_fails_cleanly() {
        assert!(parse_facts("").is_err());
        assert!(parse_facts("{}").is_err());
        assert!(parse_facts("[]").unwrap().is_empty());
        assert_eq!(map_category("неведомая", "x"), LtmKind::Knowledge);
    }

    #[test]
    fn legacy_key_and_tag_are_set() {
        let facts = parse_facts(&sample_json()).unwrap();
        let mut ltm = LtmState::empty(Utc::now());
        migrate_facts(&mut ltm, &facts, "test", Utc::now());
        let record = ltm
            .records
            .iter()
            .find(|r| r.key == "project.name")
            .unwrap();
        assert_eq!(record.legacy_key.as_deref(), Some("project.name"));
        assert!(record.tags.contains(&MIGRATED_TAG.to_string()));
        assert!((record.confidence - 0.9).abs() < 1e-6);
        assert_eq!(record.source, MemorySource::MigrationFacts);
    }

    #[test]
    fn accepts_plain_array_and_skips_bad_items() {
        let raw = r#"[{"key":"a","value":"1","category":"срок"},{"key":"","value":"x"}]"#;
        let facts = parse_facts(raw).unwrap();
        let mut ltm = LtmState::empty(Utc::now());
        let report = migrate_facts(&mut ltm, &facts, "test", Utc::now());
        assert_eq!(report.found, 2);
        assert_eq!(report.added, 1);
        assert!(!report.ok);
        assert_eq!(ltm.records.len(), 1);
    }

    #[test]
    fn report_serializes_and_roundtrips() {
        let facts = parse_facts(&sample_json()).unwrap();
        let mut ltm = LtmState::empty(Utc::now());
        let report = migrate_facts(&mut ltm, &facts, "test", Utc::now());
        let json = serde_json::to_string(&report).unwrap();
        let parsed: MigrationReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.found, 12);
        assert_eq!(parsed.added, report.added);
    }
}
