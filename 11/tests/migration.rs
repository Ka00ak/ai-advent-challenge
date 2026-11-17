mod common;

use chrono::Utc;
use lab11::domain::memory::{LtmKind, LtmState};
use lab11::memory::migration::{map_category, parse_facts, run_import};

fn fixture_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/day10-facts.json")
}

#[test]
fn import_fixture_adds_all_facts_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut controller = common::controller_at(dir.path());
    let report = controller.import_facts(&fixture_path()).unwrap();
    assert!(report.ok);
    assert_eq!(report.found, 15);
    assert_eq!(report.added, 15);
    assert_eq!(report.updated, 0);
    assert_eq!(report.skipped, 0);
    let ltm = &controller.bundle().ltm;
    assert_eq!(lab11::memory::ltm::active_ltm(ltm).count(), 15);
}

#[test]
fn import_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let mut controller = common::controller_at(dir.path());
    controller.import_facts(&fixture_path()).unwrap();
    let report = controller.import_facts(&fixture_path()).unwrap();
    assert_eq!(report.added, 0);
    assert_eq!(report.updated, 0);
    assert_eq!(report.skipped, 15);
    assert_eq!(
        lab11::memory::ltm::active_ltm(&controller.bundle().ltm).count(),
        15
    );
}

#[test]
fn mapping_categories_matches_spec() {
    assert_eq!(map_category("цель", "goal"), LtmKind::Project);
    assert_eq!(map_category("срок", "project.deadline"), LtmKind::Knowledge);
    assert_eq!(map_category("бюджет", "budget.total"), LtmKind::Knowledge);
    assert_eq!(map_category("решение", "decision.api"), LtmKind::Decision);
    assert_eq!(map_category("договорённость", "meeting"), LtmKind::Decision);
    assert_eq!(
        map_category("предпочтение", "preference.style"),
        LtmKind::Preference
    );
    assert_eq!(map_category("имя", "user.name"), LtmKind::Profile);
    assert_eq!(map_category("имя", "project.name"), LtmKind::Project);
    assert_eq!(map_category("имя", "team.lead"), LtmKind::Project);
    assert_eq!(map_category("имя", "неизвестно"), LtmKind::Knowledge);
    assert_eq!(map_category("риск", "risk.x"), LtmKind::Knowledge);
    assert_eq!(map_category("неизвестная", "x"), LtmKind::Knowledge);
}

#[test]
fn plain_array_is_accepted() {
    let facts = parse_facts(r#"[{"key":"a","value":"1","category":"срок"}]"#).unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].category, "срок");
}

#[test]
fn broken_input_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.json");
    let mut ltm = LtmState::empty(Utc::now());
    let report = run_import(&mut ltm, &missing, Utc::now()).unwrap();
    assert!(!report.ok);
    assert!(!report.errors.is_empty());

    let broken = dir.path().join("broken.json");
    std::fs::write(&broken, "{not json").unwrap();
    let report = run_import(&mut ltm, &broken, Utc::now()).unwrap();
    assert!(!report.ok);
    assert!(report.errors.iter().any(|error| error.contains("JSON")));
}

#[test]
fn values_are_verbatim_and_legacy_key_saved() {
    let dir = tempfile::tempdir().unwrap();
    let mut controller = common::controller_at(dir.path());
    controller.import_facts(&fixture_path()).unwrap();
    let ltm = &controller.bundle().ltm;
    let budget = lab11::memory::ltm::find_active_ltm(ltm, "budget.total").unwrap();
    assert_eq!(budget.value, "480 000 рублей");
    assert_eq!(budget.legacy_key.as_deref(), Some("budget.total"));
    assert!(budget.tags.iter().any(|tag| tag == "migrated:day10"));
    let deadline = lab11::memory::ltm::find_active_ltm(ltm, "project.deadline").unwrap();
    assert_eq!(deadline.value, "15 декабря 2026");
}
