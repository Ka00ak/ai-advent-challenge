//! Контрольный диалог (34 сообщения) и seeded-состояние слоёв.
//!
//! Текст фиксирован `docs/07-verification.md`; менять без обновления документа
//! нельзя.

use chrono::{DateTime, Utc};

use crate::domain::memory::{
    CandidateScope, ChatMessage, LtmKind, LtmState, MemoryBundle, MemoryCandidate, MemorySource,
    PendingWrite, WmKind, WmStatus, WorkingMemoryState,
};
use crate::memory::ltm::{UpsertLtm, upsert_ltm};
use crate::memory::working::{WmUpsert, upsert_wm_item};

pub fn fixture_messages() -> Vec<ChatMessage> {
    const DIALOG: [(&str, &str); 17] = [
        (
            "Запускаем проект «Атлас». Заказчик — Марина Ковалёва, она принимает результат.",
            "Принято. Зафиксировал название и заказчика.",
        ),
        (
            "Дедлайн проекта — 15 декабря 2026 года. Бюджет — 480 000 рублей, больше не согласуют.",
            "Учту дедлайн и бюджет при планировании.",
        ),
        (
            "Ограничение: только российские ЦОД, зарубежные облака запрещены. Тимлид — Артём Соколов.",
            "Понял: инфраструктура только в российских ЦОД, вопросы к тимлиду.",
        ),
        (
            "В команде 4 разработчика. Первый прототип покажем 3 ноября 2026 года.",
            "Записал состав команды и дату прототипа.",
        ),
        (
            "По базе данных выбрали PostgreSQL, MongoDB не берём. API делаем REST, GraphQL не нужен.",
            "Согласен: PostgreSQL и REST.",
        ),
        (
            "На маркетинг выделили 120 000 рублей. Публичный релиз — 20 декабря 2026 года.",
            "Принято: маркетинг 120 000 рублей, релиз 20 декабря 2026.",
        ),
        (
            "Меня зовут Алексей. Предпочтение: отвечай коротко, без вступлений и повторов.",
            "Хорошо, буду отвечать коротко.",
        ),
        (
            "Дальше обсуждаем только статусы и организацию встреч.",
            "Принято, переходим к статусам и встречам.",
        ),
        (
            "Текущая задача: подготовить смету маркетинга к релизу. План: 1) собрать каналы, 2) оценить каналы, 3) согласовать с заказчиком.",
            "Задача принята, план из трёх шагов зафиксирован.",
        ),
        (
            "Сейчас на шаге 2. Черновой вариант B — 140 000 рублей, вариант A — 160 000 рублей.",
            "На шаге 2. Вариант B дешевле на 20 000 рублей.",
        ),
        (
            "Выбери вариант B как рабочий, но финальное решение за заказчиком.",
            "Рабочий вариант — B, финальное слово за заказчиком.",
        ),
        (
            "Открытый вопрос: нужен ли отдельный бюджет на PR-подрядчика? Пока оставляем открытым.",
            "Зафиксировал открытый вопрос по PR-подрядчику.",
        ),
        (
            "Статус: каналы собраны, оценка в работе.",
            "Статус понял: сбор каналов завершён, оценка продолжается.",
        ),
        (
            "Уточнение: бюджет маркетинга подняли до 150 000 рублей.",
            "Учту новую сумму в текущей задаче.",
        ),
        (
            "Инфлюенсеров всё-таки включаем в смету отдельной строкой, заложи 25 000 рублей.",
            "Понял: инфлюенсеры отдельной строкой, 25 000 рублей.",
        ),
        (
            "Созвон с заказчиком — в четверг в 11:00.",
            "Записал: четверг, 11:00.",
        ),
        (
            "Черновик сметы лежит в файле smeta-v3.xlsx. Пока не отправляй его заказчику до пятницы.",
            "Понял: файл smeta-v3.xlsx, отправка не раньше пятницы.",
        ),
    ];
    let mut messages = Vec::with_capacity(34);
    for (user, assistant) in DIALOG {
        messages.push(ChatMessage::user(user));
        messages.push(ChatMessage::assistant(assistant));
    }
    messages
}

pub fn seeded_stm(now: DateTime<Utc>) -> crate::domain::memory::StmState {
    let mut stm = crate::domain::memory::StmState::empty(now);
    stm.messages = fixture_messages();
    stm.summary = None;
    stm.covered_messages = 0;
    stm
}

pub fn seeded_wm(now: DateTime<Utc>) -> WorkingMemoryState {
    let mut wm = WorkingMemoryState::empty(now);
    upsert_wm_item(
        &mut wm,
        WmUpsert::active(
            WmKind::Goal,
            "task.goal",
            "Подготовить смету маркетинга к релизу",
        ),
        now,
    );
    let plans = [
        ("plan.1", "Собрать каналы", WmStatus::Done, 1),
        ("plan.2", "Оценить каналы", WmStatus::Active, 2),
        ("plan.3", "Согласовать с заказчиком", WmStatus::Active, 3),
    ];
    for (key, value, status, order) in plans {
        let mut upsert = WmUpsert::active(WmKind::Plan, key, value);
        upsert.status = status;
        upsert.order = Some(order);
        upsert_wm_item(&mut wm, upsert, now);
    }
    upsert_wm_item(
        &mut wm,
        WmUpsert::active(WmKind::Entity, "estimate.variant", "Вариант B — 140 000 ₽"),
        now,
    );
    upsert_wm_item(
        &mut wm,
        WmUpsert::active(
            WmKind::OpenQuestion,
            "question.pr",
            "Нужен ли отдельный бюджет на PR-подрядчика?",
        ),
        now,
    );
    upsert_wm_item(
        &mut wm,
        WmUpsert::active(
            WmKind::Status,
            "status.channels",
            "Каналы собраны, оценка в работе",
        ),
        now,
    );
    wm
}

pub fn seeded_ltm(now: DateTime<Utc>) -> LtmState {
    let mut ltm = LtmState::empty(now);
    let records = [
        (LtmKind::Profile, "user.name", "Алексей", MemorySource::User),
        (
            LtmKind::Project,
            "project.name",
            "«Атлас»",
            MemorySource::MigrationFacts,
        ),
        (
            LtmKind::Project,
            "customer",
            "Марина Ковалёва",
            MemorySource::MigrationFacts,
        ),
        (
            LtmKind::Project,
            "team.lead",
            "Артём Соколов",
            MemorySource::MigrationFacts,
        ),
        (
            LtmKind::Project,
            "goal",
            "Запускаем проект «Атлас»",
            MemorySource::User,
        ),
        (
            LtmKind::Knowledge,
            "project.deadline",
            "15 декабря 2026",
            MemorySource::MigrationFacts,
        ),
        (
            LtmKind::Knowledge,
            "budget.total",
            "480 000 рублей",
            MemorySource::MigrationFacts,
        ),
        (
            LtmKind::Knowledge,
            "budget.marketing",
            "120 000 рублей",
            MemorySource::MigrationFacts,
        ),
        (
            LtmKind::Knowledge,
            "constraint.infra",
            "Только российские ЦОД, зарубежные облака запрещены",
            MemorySource::MigrationFacts,
        ),
        (
            LtmKind::Knowledge,
            "team.size",
            "4 разработчика",
            MemorySource::MigrationFacts,
        ),
        (
            LtmKind::Knowledge,
            "milestone.prototype",
            "3 ноября 2026",
            MemorySource::MigrationFacts,
        ),
        (
            LtmKind::Knowledge,
            "milestone.release",
            "20 декабря 2026",
            MemorySource::MigrationFacts,
        ),
        (
            LtmKind::Decision,
            "decision.database",
            "PostgreSQL, MongoDB не используем",
            MemorySource::MigrationFacts,
        ),
        (
            LtmKind::Decision,
            "decision.api",
            "REST, GraphQL не берём",
            MemorySource::MigrationFacts,
        ),
        (
            LtmKind::Preference,
            "preference.style",
            "Отвечать коротко, без вступлений",
            MemorySource::User,
        ),
    ];
    for (kind, key, value, source) in records {
        let mut input = UpsertLtm::new(kind, key, value, source);
        input.confidence = 0.9;
        input.tags = vec!["migrated:day10".into()];
        upsert_ltm(&mut ltm, input, now);
    }
    ltm
}

pub fn seeded_pending(now: DateTime<Utc>) -> Vec<PendingWrite> {
    vec![PendingWrite {
        id: "pend_seed_marketing".into(),
        candidate: MemoryCandidate {
            key: "budget.marketing".into(),
            value: "150 000 рублей".into(),
            kind: LtmKind::Knowledge,
            tags: Vec::new(),
            confidence: 0.8,
            scope: CandidateScope::Project,
            ttl_turns: None,
            legacy_category: None,
        },
        target_layer: crate::domain::memory::MemoryLayer::Ltm,
        target_kind: LtmKind::Knowledge,
        rule_id: "R3".into(),
        reason: "обновление существующего ключа".into(),
        old_value: Some("120 000 рублей".into()),
        message_index: 26,
        created_at: now,
    }]
}

pub fn seeded_bundle(now: DateTime<Utc>) -> MemoryBundle {
    MemoryBundle {
        stm: seeded_stm(now),
        wm: seeded_wm(now),
        ltm: seeded_ltm(now),
        pending: seeded_pending(now),
        meta: crate::domain::memory::MemoryMetaState {
            version: crate::domain::memory::STATE_VERSION,
            conversation_id: None,
            processed_messages: 34,
            last_route_at: Some(now),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_has_34_messages() {
        let messages = fixture_messages();
        assert_eq!(messages.len(), 34);
        assert!(
            messages
                .iter()
                .all(|m| m.role != crate::domain::memory::ChatRole::System)
        );
    }

    #[test]
    fn seeded_layers_match_spec() {
        let now = Utc::now();
        let wm = seeded_wm(now);
        assert_eq!(wm.items.len(), 7);
        assert_eq!(
            wm.items
                .iter()
                .filter(|item| item.status == WmStatus::Done)
                .count(),
            1
        );
        let ltm = seeded_ltm(now);
        assert_eq!(ltm.records.len(), 15);
        assert_eq!(
            crate::memory::ltm::find_active_ltm(&ltm, "budget.marketing")
                .unwrap()
                .value,
            "120 000 рублей"
        );
        let pending = seeded_pending(now);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].old_value.as_deref(), Some("120 000 рублей"));
    }

    #[test]
    fn fixture_stm_window_is_phase_c() {
        let now = Utc::now();
        let mut stm = seeded_stm(now);
        stm.messages.push(ChatMessage::user("probe"));
        let window = crate::memory::stm::select_stm_window(&stm, 8, 1600);
        assert_eq!(window.indices, (26..34).collect::<Vec<usize>>());
    }
}
