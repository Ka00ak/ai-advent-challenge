mod common;

use chrono::Utc;
use lab11::domain::memory::{
    ChatMessage, LtmKind, LtmState, MemoryLayer, MemorySource, WorkingMemoryState,
};
use lab11::domain::settings::{Approval, MemoryConfig};
use lab11::memory::extractor::LocalCandidateExtractor;
use lab11::memory::ltm::UpsertLtm;
use lab11::memory::router::{RouteInput, route_candidates};

fn route(
    text: &str,
    wm: &WorkingMemoryState,
    ltm: &LtmState,
    config: &MemoryConfig,
) -> lab11::memory::router::RouteOutcome {
    let message = ChatMessage::user(text);
    route_candidates(
        RouteInput {
            message: &message,
            message_index: 1,
            wm,
            ltm,
            pending: &[],
            config,
            now: Utc::now(),
        },
        &LocalCandidateExtractor,
    )
    .unwrap()
}

#[test]
fn r1_secret_is_rejected_and_masked() {
    let now = Utc::now();
    let outcome = route(
        "Мой API-ключ: sk-super-secret-1234567890",
        &WorkingMemoryState::empty(now),
        &LtmState::empty(now),
        &MemoryConfig::default(),
    );
    assert_eq!(outcome.stats.rejected, 1);
    assert!(outcome.pending.is_empty());
    let audit = serde_json::to_string(&outcome.audit).unwrap();
    assert!(!audit.contains("super-secret-1234567890"));
    assert!(audit.contains("***"));
}

#[test]
fn r2_task_marker_beats_ltm_key() {
    let now = Utc::now();
    let mut ltm = LtmState::empty(now);
    lab11::memory::ltm::upsert_ltm(
        &mut ltm,
        UpsertLtm::new(LtmKind::Knowledge, "task.rate", "80", MemorySource::User),
        now,
    );
    let outcome = route(
        "Для этой задачи курс 90 ₽",
        &WorkingMemoryState::empty(now),
        &ltm,
        &MemoryConfig::default(),
    );
    assert!(outcome.wm.items.iter().any(|item| item.key == "task.rate"));
    assert!(outcome.pending.is_empty());
}

#[test]
fn r3_confirm_queues_update_with_old_value() {
    let now = Utc::now();
    let mut ltm = LtmState::empty(now);
    lab11::memory::ltm::upsert_ltm(
        &mut ltm,
        UpsertLtm::new(
            LtmKind::Knowledge,
            "budget.marketing",
            "120 000 рублей",
            MemorySource::User,
        ),
        now,
    );
    let outcome = route(
        "Бюджет маркетинга подняли до 150 000 рублей.",
        &WorkingMemoryState::empty(now),
        &ltm,
        &MemoryConfig::default(),
    );
    assert_eq!(outcome.pending.len(), 1);
    assert_eq!(outcome.pending[0].rule_id, "R3");
    assert_eq!(
        outcome.pending[0].old_value.as_deref(),
        Some("120 000 рублей")
    );
}

#[test]
fn r3_auto_updates_ltm_immediately() {
    let now = Utc::now();
    let mut ltm = LtmState::empty(now);
    lab11::memory::ltm::upsert_ltm(
        &mut ltm,
        UpsertLtm::new(
            LtmKind::Knowledge,
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
    let outcome = route(
        "Бюджет маркетинга подняли до 150 000 рублей.",
        &WorkingMemoryState::empty(now),
        &ltm,
        &config,
    );
    assert!(outcome.pending.is_empty());
    let active = lab11::memory::ltm::find_active_ltm(&outcome.ltm, "budget.marketing").unwrap();
    assert!(active.value.contains("150 000"));
}

#[test]
fn r4_stable_project_fact_is_pending() {
    let now = Utc::now();
    let outcome = route(
        "Запускаем проект «Атлас». Заказчик — Марина Ковалёва.",
        &WorkingMemoryState::empty(now),
        &LtmState::empty(now),
        &MemoryConfig::default(),
    );
    let keys: Vec<&str> = outcome
        .pending
        .iter()
        .map(|pending| pending.candidate.key.as_str())
        .collect();
    assert!(keys.contains(&"project.name"));
    assert!(keys.contains(&"customer"));
    assert!(
        outcome
            .pending
            .iter()
            .all(|pending| pending.rule_id == "R4")
    );
}

#[test]
fn r5_variant_is_wm_entity() {
    let now = Utc::now();
    let outcome = route(
        "Выбери вариант B рабочим",
        &WorkingMemoryState::empty(now),
        &LtmState::empty(now),
        &MemoryConfig::default(),
    );
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
    let now = Utc::now();
    for text in ["Привет", "Спасибо, это всё", "Наверное, успеем к декабрю"]
    {
        let outcome = route(
            text,
            &WorkingMemoryState::empty(now),
            &LtmState::empty(now),
            &MemoryConfig::default(),
        );
        assert!(outcome.wm.items.is_empty(), "{text}");
        assert!(outcome.pending.is_empty(), "{text}");
    }
}

#[test]
fn audit_contains_layer_rule_reason_source() {
    let now = Utc::now();
    let outcome = route(
        "Дедлайн проекта — 15 декабря 2026.",
        &WorkingMemoryState::empty(now),
        &LtmState::empty(now),
        &MemoryConfig::default(),
    );
    let event = outcome.audit.first().expect("audit");
    assert_eq!(event.layer, MemoryLayer::Ltm);
    assert_eq!(event.source, MemorySource::Router);
    assert_eq!(event.rule_id.as_deref(), Some("R4"));
    assert!(!event.reason.is_empty());
}

#[test]
fn several_candidates_do_not_interfere() {
    let now = Utc::now();
    let outcome = route(
        "По базе данных выбрали PostgreSQL, MongoDB не берём. API делаем REST.",
        &WorkingMemoryState::empty(now),
        &LtmState::empty(now),
        &MemoryConfig::default(),
    );
    let keys: Vec<&str> = outcome
        .pending
        .iter()
        .map(|pending| pending.candidate.key.as_str())
        .collect();
    assert!(keys.contains(&"decision.database"));
    assert!(keys.contains(&"decision.api"));
}

#[test]
fn local_extraction_does_not_make_requests() {
    // NFR-1: Local — 0 дополнительных запросов. FakeChatClient считает вызовы
    // только в других тестах; здесь проверяем, что route не требует клиента.
    let now = Utc::now();
    let outcome = route(
        "Меня зовут Алексей",
        &WorkingMemoryState::empty(now),
        &LtmState::empty(now),
        &MemoryConfig::default(),
    );
    assert_eq!(outcome.pending.len(), 1);
}
