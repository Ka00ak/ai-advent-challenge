//! WM: пункты текущей задачи, их статусы, отображение и GC.

use chrono::{DateTime, Duration, Utc};

use crate::domain::memory::{
    Id, MemoryMetaState, RenderedBlock, WmItem, WmKind, WmStatus, WorkingMemoryState,
};
use crate::domain::settings::WmConfig;
use crate::memory::tokens::estimate_text_tokens;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WmChange {
    Created(Id),
    Updated(Id),
    Same(Id),
}

#[derive(Debug, Clone)]
pub struct WmUpsert {
    pub kind: WmKind,
    pub key: String,
    pub value: String,
    pub status: WmStatus,
    pub order: Option<u32>,
    pub ref_ltm_id: Option<Id>,
    pub source_message_index: Option<usize>,
}

impl WmUpsert {
    pub fn active(kind: WmKind, key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            kind,
            key: key.into(),
            value: value.into(),
            status: WmStatus::Active,
            order: None,
            ref_ltm_id: None,
            source_message_index: None,
        }
    }
}

pub fn empty_working(now: DateTime<Utc>) -> WorkingMemoryState {
    WorkingMemoryState::empty(now)
}

pub fn upsert_wm_item(
    state: &mut WorkingMemoryState,
    input: WmUpsert,
    now: DateTime<Utc>,
) -> WmChange {
    let key = crate::memory::audit::sanitize_key(&input.key);
    let value = crate::memory::audit::sanitize_value(&input.value);
    if let Some(item) = state.items.iter_mut().find(|item| item.key == key) {
        if item.value == value && item.kind == input.kind && item.status == input.status {
            return WmChange::Same(item.id.clone());
        }
        item.value = value;
        item.kind = input.kind;
        item.status = input.status;
        if input.order.is_some() {
            item.order = input.order;
        }
        if input.ref_ltm_id.is_some() {
            item.ref_ltm_id = input.ref_ltm_id;
        }
        if input.source_message_index.is_some() {
            item.source_message_index = input.source_message_index;
        }
        item.updated_at = now;
        state.updated_at = now;
        if input.kind == WmKind::Goal {
            state.task_title = item.value.clone();
        }
        return WmChange::Updated(item.id.clone());
    }

    let id = format!("wm_{}", uuid::Uuid::new_v4().simple());
    if input.kind == WmKind::Goal {
        state.task_title = value.clone();
        state.started_at = Some(now);
        if state.task_id.is_none() {
            state.task_id = Some(format!("task_{}", uuid::Uuid::new_v4().simple()));
        }
    }
    state.items.push(WmItem {
        id: id.clone(),
        kind: input.kind,
        key,
        value,
        status: input.status,
        order: input.order,
        ref_ltm_id: input.ref_ltm_id,
        created_at: now,
        updated_at: now,
        source_message_index: input.source_message_index,
    });
    state.updated_at = now;
    WmChange::Created(id)
}

pub fn set_wm_status(
    state: &mut WorkingMemoryState,
    id: &str,
    status: WmStatus,
    now: DateTime<Utc>,
) -> bool {
    if let Some(item) = state.items.iter_mut().find(|item| item.id == id) {
        item.status = status;
        item.updated_at = now;
        state.updated_at = now;
        true
    } else {
        false
    }
}

pub fn remove_wm_item(state: &mut WorkingMemoryState, id: &str) -> bool {
    let before = state.items.len();
    state.items.retain(|item| item.id != id);
    let removed = state.items.len() != before;
    if removed {
        state.updated_at = Utc::now();
    }
    removed
}

/// «Завершить задачу»: active → dropped, done остаются для истории.
pub fn archive_task(state: &mut WorkingMemoryState, now: DateTime<Utc>) {
    for item in &mut state.items {
        if item.status == WmStatus::Active {
            item.status = WmStatus::Dropped;
            item.updated_at = now;
        }
    }
    state.updated_at = now;
}

/// Удаляет старые done/dropped и вытесняет лишние пункты.
pub fn prune_wm(state: &mut WorkingMemoryState, config: &WmConfig, now: DateTime<Utc>) {
    let cutoff = now - Duration::days(config.ttl_days.max(0));
    state.items.retain(|item| {
        let finished = item.status == WmStatus::Done || item.status == WmStatus::Dropped;
        !(finished && item.updated_at < cutoff)
    });

    while state.items.len() > config.max_items {
        let victim = state
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| item.status != WmStatus::Active || item.kind == WmKind::Note)
            .min_by_key(|(index, item)| {
                let priority = match item.kind {
                    WmKind::Note => 0,
                    WmKind::Status => 1,
                    WmKind::Entity => 2,
                    WmKind::OpenQuestion => 3,
                    WmKind::Plan => 4,
                    WmKind::Goal => 5,
                };
                (priority, (*index, item.updated_at))
            })
            .map(|(index, _)| index);
        match victim {
            Some(index) => {
                state.items.remove(index);
            }
            None => break,
        }
    }
    state.updated_at = now;
}

/// Дополнительные статусы, которые имеет смысл хранить в meta после GC.
pub fn note_gc(meta: &mut MemoryMetaState, now: DateTime<Utc>) {
    meta.last_route_at = Some(now);
}

/// Рендер WM-блока. `included_ltm` — id LTM-записей, уже попавших в промпт.
pub fn render_wm(
    state: &WorkingMemoryState,
    max_tokens: usize,
    included_ltm: &[Id],
) -> RenderedBlock {
    let mut candidates: Vec<&WmItem> = state.items.iter().filter(|item| renderable(item)).collect();
    if candidates.is_empty() && state.task_title.trim().is_empty() {
        return RenderedBlock::default();
    }
    candidates.sort_by(|a, b| {
        a.kind
            .sort_order()
            .cmp(&b.kind.sort_order())
            .then(
                a.order
                    .unwrap_or(u32::MAX)
                    .cmp(&b.order.unwrap_or(u32::MAX)),
            )
            .then(a.created_at.cmp(&b.created_at))
    });

    let mut dropped: Vec<Id> = Vec::new();
    let mut notes = Vec::new();
    loop {
        let mut block = build_wm_text(state, &candidates, included_ltm);
        let mut tokens = estimate_text_tokens(&block.text);
        if tokens <= max_tokens || candidates.is_empty() {
            block.tokens = tokens;
            block.dropped = dropped;
            block.notes = notes;
            return block;
        }

        // Урезаем note → старые status → entity; goal/plan последними.
        let victim = candidates
            .iter()
            .enumerate()
            .filter(|(_, item)| matches!(item.kind, WmKind::Note | WmKind::Status | WmKind::Entity))
            .max_by_key(|(index, item)| {
                let priority = match item.kind {
                    WmKind::Note => 3,
                    WmKind::Status => 2,
                    _ => 1,
                };
                (priority, *index)
            })
            .map(|(index, _)| index);
        if let Some(index) = victim {
            let removed = candidates.remove(index);
            dropped.push(removed.id.clone());
            notes.push(format!("wm: dropped {}", removed.key));
            tokens = estimate_text_tokens(&block.text);
            continue;
        }

        // Если остались только goal/plan — обрезаем текст построчно.
        let text = truncate_block(&mut block.text, max_tokens);
        block.tokens = text;
        block.dropped = dropped;
        block.notes = notes;
        block.notes.push("wm: truncated".into());
        return block;
    }
}

fn renderable(item: &WmItem) -> bool {
    match item.status {
        WmStatus::Active => true,
        WmStatus::Done => item.kind == WmKind::Plan,
        WmStatus::Dropped => false,
    }
}

fn build_wm_text(
    state: &WorkingMemoryState,
    items: &[&WmItem],
    included_ltm: &[Id],
) -> RenderedBlock {
    let mut block = RenderedBlock::default();
    let mut lines: Vec<String> = Vec::new();
    lines.push("## Рабочая память: текущая задача".to_string());
    lines.push(
        "Это состояние текущей задачи. Используй его при ответе и не переспрашивай.".to_string(),
    );
    if !state.task_title.trim().is_empty() {
        lines.push(format!("Задача: {}", state.task_title));
    }

    let plans: Vec<&WmItem> = items
        .iter()
        .copied()
        .filter(|item| item.kind == WmKind::Plan)
        .collect();
    if !plans.is_empty() {
        lines.push("План:".to_string());
        for item in plans {
            let mark = if item.status == WmStatus::Done {
                "[x]"
            } else {
                "[ ]"
            };
            let number = item.order.unwrap_or(0);
            lines.push(format!("  {number}. {mark} {}", item.value));
        }
    }

    for item in items {
        if item.kind == WmKind::Plan || item.kind == WmKind::Goal {
            continue;
        }
        if let Some(ltm_id) = &item.ref_ltm_id {
            if included_ltm.iter().any(|id| id == ltm_id) {
                block.deduped.push(item.id.clone());
                continue;
            }
            if item.value.trim().is_empty() {
                lines.push(format!("- см. долговременную память: {}", item.key));
                block.included.push(item.id.clone());
                continue;
            }
        }
        let label = match item.kind {
            WmKind::Status => "Статус",
            WmKind::Entity => "Данные",
            WmKind::OpenQuestion => "Открытые вопросы",
            WmKind::Note => "Заметка",
            _ => "Пункт",
        };
        lines.push(format!("{label}: {}", item.value));
        block.included.push(item.id.clone());
    }

    for item in items {
        if item.kind == WmKind::Goal {
            block.included.push(item.id.clone());
        }
    }

    block.text = lines.join("\n");
    block.tokens = estimate_text_tokens(&block.text);
    block
}

fn truncate_block(text: &mut String, max_tokens: usize) -> usize {
    while estimate_text_tokens(text) > max_tokens && !text.is_empty() {
        if let Some(pos) = text.rfind('\n') {
            text.truncate(pos);
        } else {
            let mut chars: Vec<char> = text.chars().collect();
            chars.pop();
            *text = chars.into_iter().collect();
        }
    }
    estimate_text_tokens(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::WmKind;

    fn state_with(items: Vec<WmUpsert>) -> WorkingMemoryState {
        let mut state = WorkingMemoryState::empty(Utc::now());
        for item in items {
            upsert_wm_item(&mut state, item, Utc::now());
        }
        state
    }

    #[test]
    fn upsert_creates_id_and_timestamps() {
        let mut state = WorkingMemoryState::empty(Utc::now());
        let change = upsert_wm_item(
            &mut state,
            WmUpsert::active(WmKind::Goal, "task.goal", "Смета"),
            Utc::now(),
        );
        assert!(matches!(change, WmChange::Created(_)));
        assert_eq!(state.items.len(), 1);
        assert_eq!(state.task_title, "Смета");
        assert!(state.task_id.is_some());
        assert!(state.started_at.is_some());
    }

    #[test]
    fn repeated_key_updates_not_duplicates() {
        let mut state = WorkingMemoryState::empty(Utc::now());
        upsert_wm_item(
            &mut state,
            WmUpsert::active(WmKind::Entity, "estimate.variant", "A"),
            Utc::now(),
        );
        let change = upsert_wm_item(
            &mut state,
            WmUpsert::active(WmKind::Entity, "estimate.variant", "B"),
            Utc::now(),
        );
        assert!(matches!(change, WmChange::Updated(_)));
        assert_eq!(state.items.len(), 1);
        assert_eq!(state.items[0].value, "B");
    }

    #[test]
    fn done_plan_renders_x_but_done_status_does_not() {
        let mut state = WorkingMemoryState::empty(Utc::now());
        let mut plan = WmUpsert::active(WmKind::Plan, "plan.1", "Собрать каналы");
        plan.status = WmStatus::Done;
        plan.order = Some(1);
        upsert_wm_item(&mut state, plan, Utc::now());
        let mut status = WmUpsert::active(WmKind::Status, "status.x", "старый статус");
        status.status = WmStatus::Done;
        upsert_wm_item(&mut state, status, Utc::now());
        let block = render_wm(&state, 1000, &[]);
        assert!(block.text.contains("[x] Собрать каналы"));
        assert!(!block.text.contains("старый статус"));
    }

    #[test]
    fn order_is_goal_plan_status_entity_question_note() {
        let state = state_with(vec![
            WmUpsert::active(WmKind::Note, "note", "заметка"),
            WmUpsert::active(WmKind::OpenQuestion, "q", "вопрос"),
            WmUpsert::active(WmKind::Entity, "e", "данные"),
            WmUpsert::active(WmKind::Status, "s", "статус"),
            WmUpsert::active(WmKind::Plan, "plan.1", "шаг"),
            WmUpsert::active(WmKind::Goal, "goal", "цель"),
        ]);
        let text = render_wm(&state, 1000, &[]).text;
        let goal = text.find("Задача: цель").unwrap();
        let plan = text.find("План:").unwrap();
        let status = text.find("Статус: статус").unwrap();
        let entity = text.find("Данные: данные").unwrap();
        let question = text.find("Открытые вопросы: вопрос").unwrap();
        let note = text.find("Заметка: заметка").unwrap();
        assert!(goal < plan && plan < status && status < entity && entity < question);
        assert!(question < note);
    }

    #[test]
    fn truncation_drops_note_first() {
        let state = state_with(vec![
            WmUpsert::active(WmKind::Goal, "goal", "цель"),
            WmUpsert::active(WmKind::Plan, "plan.1", "шаг"),
            WmUpsert::active(WmKind::Note, "note", "очень длинная заметка ".repeat(200)),
        ]);
        let block = render_wm(&state, 120, &[]);
        assert!(!block.text.contains("Заметка"));
        assert!(block.text.contains("План:"));
        assert!(!block.dropped.is_empty());
    }

    #[test]
    fn ref_ltm_is_deduped_or_rendered_as_link() {
        let mut state = WorkingMemoryState::empty(Utc::now());
        let mut upsert = WmUpsert::active(WmKind::Entity, "entity.ref", "");
        upsert.ref_ltm_id = Some("ltm_1".into());
        upsert_wm_item(&mut state, upsert, Utc::now());
        let linked = render_wm(&state, 1000, &[]);
        assert!(linked.text.contains("см. долговременную память"));
        let deduped = render_wm(&state, 1000, &["ltm_1".into()]);
        assert!(deduped.text.contains("Рабочая память"));
        assert!(deduped.deduped.iter().any(|id| id.starts_with("wm_")));
        assert_eq!(deduped.deduped.len(), 1);
    }

    #[test]
    fn prune_removes_old_done_and_keeps_active() {
        let now = Utc::now();
        let mut state = WorkingMemoryState::empty(now);
        let mut old = WmUpsert::active(WmKind::Plan, "plan.old", "старый");
        old.status = WmStatus::Done;
        upsert_wm_item(&mut state, old, now - Duration::days(30));
        upsert_wm_item(
            &mut state,
            WmUpsert::active(WmKind::Goal, "goal", "цель"),
            now,
        );
        prune_wm(&mut state, &WmConfig::default(), now);
        assert_eq!(state.items.len(), 1);
        assert_eq!(state.items[0].kind, WmKind::Goal);
    }

    #[test]
    fn archive_moves_active_to_dropped() {
        let now = Utc::now();
        let mut state = WorkingMemoryState::empty(now);
        upsert_wm_item(
            &mut state,
            WmUpsert::active(WmKind::Goal, "goal", "цель"),
            now,
        );
        archive_task(&mut state, now);
        assert_eq!(state.items[0].status, WmStatus::Dropped);
    }
}
