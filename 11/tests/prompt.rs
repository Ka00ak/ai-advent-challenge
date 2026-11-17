mod common;

use chrono::Utc;
use lab11::domain::memory::{ChatRole, LayerFlags};
use lab11::llm::tools::ToolRegistry;
use lab11::memory::prompt::{AssembleInput, SYSTEM_PROMPT, assemble_prompt};

#[test]
fn blocks_have_fixed_order_and_breakdown_sums() {
    let dir = tempfile::tempdir().unwrap();
    let controller = common::seeded_controller(dir.path());
    let bundle = controller.bundle();
    let settings = controller.settings();
    let tools = ToolRegistry::default_registry();
    let mut stm = bundle.stm.clone();
    stm.messages.push(lab11::domain::memory::ChatMessage::user(
        "Напомни бюджет проекта и дедлайн",
    ));
    let output = assemble_prompt(AssembleInput {
        system_prompt: SYSTEM_PROMPT,
        stm: &stm,
        wm: &bundle.wm,
        ltm: &bundle.ltm,
        config: &settings.memory,
        tools: &tools.schemas(),
        layers: LayerFlags::ALL,
        now: Utc::now(),
    })
    .unwrap();
    let texts: Vec<&str> = output
        .messages
        .iter()
        .filter_map(|message| message.content.as_deref())
        .collect();
    let find = |needle: &str| texts.iter().position(|text| text.contains(needle)).unwrap();
    assert!(find("Ты — агент") == 0);
    assert!(find("профиль, проект, предпочтения") < find("релевантное текущей задаче"));
    assert!(find("релевантное текущей задаче") < find("Рабочая память"));
    assert!(find("Рабочая память") < find("Уточнение: бюджет"));
    assert_eq!(
        texts.last().copied(),
        Some("Напомни бюджет проекта и дедлайн")
    );
    assert_eq!(
        output.breakdown.total_tokens,
        output.breakdown.system_tokens
            + output.breakdown.ltm_profile_tokens
            + output.breakdown.ltm_relevant_tokens
            + output.breakdown.wm_tokens
            + output.breakdown.stm_summary_tokens
            + output.breakdown.stm_tokens
            + output.breakdown.current_tokens
            + output.breakdown.tools_tokens
    );
    assert_eq!(
        output.breakdown.layer_delta.ltm,
        output.breakdown.ltm_profile_tokens + output.breakdown.ltm_relevant_tokens
    );
    assert!(!output.breakdown.included_ltm_ids.is_empty());
}

#[test]
fn disabling_stm_keeps_current_question() {
    let dir = tempfile::tempdir().unwrap();
    let controller = common::seeded_controller(dir.path());
    let bundle = controller.bundle();
    let settings = controller.settings();
    let tools = ToolRegistry::default_registry();
    let mut stm = bundle.stm.clone();
    stm.messages
        .push(lab11::domain::memory::ChatMessage::user("Текущий вопрос?"));
    let output = assemble_prompt(AssembleInput {
        system_prompt: SYSTEM_PROMPT,
        stm: &stm,
        wm: &bundle.wm,
        ltm: &bundle.ltm,
        config: &settings.memory,
        tools: &tools.schemas(),
        layers: LayerFlags::new(false, true, true),
        now: Utc::now(),
    })
    .unwrap();
    assert_eq!(
        output.messages.last().unwrap().content.as_deref(),
        Some("Текущий вопрос?")
    );
    assert_eq!(output.breakdown.stm_tokens, 0);
    assert!(output.breakdown.wm_tokens > 0);
    assert!(output.breakdown.ltm_profile_tokens > 0);
    assert!(
        !output
            .messages
            .iter()
            .any(|message| message.content.as_deref()
                == Some("Уточнение: бюджет маркетинга подняли до 150 000 рублей."))
    );
}

#[test]
fn disabling_ltm_removes_both_ltm_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let controller = common::seeded_controller(dir.path());
    let bundle = controller.bundle();
    let settings = controller.settings();
    let tools = ToolRegistry::default_registry();
    let mut stm = bundle.stm.clone();
    stm.messages
        .push(lab11::domain::memory::ChatMessage::user("Вопрос"));
    let output = assemble_prompt(AssembleInput {
        system_prompt: SYSTEM_PROMPT,
        stm: &stm,
        wm: &bundle.wm,
        ltm: &bundle.ltm,
        config: &settings.memory,
        tools: &tools.schemas(),
        layers: LayerFlags::new(true, true, false),
        now: Utc::now(),
    })
    .unwrap();
    assert_eq!(output.breakdown.ltm_profile_tokens, 0);
    assert_eq!(output.breakdown.ltm_relevant_tokens, 0);
    assert!(
        !output
            .messages
            .iter()
            .filter(|message| message.role == ChatRole::System)
            .any(|message| message
                .content
                .as_deref()
                .unwrap_or_default()
                .contains("Долговременная память"))
    );
}

#[test]
fn system_prompt_declares_priority_and_data_warning() {
    let dir = tempfile::tempdir().unwrap();
    let controller = common::seeded_controller(dir.path());
    let bundle = controller.bundle();
    let settings = controller.settings();
    let mut stm = bundle.stm.clone();
    stm.messages
        .push(lab11::domain::memory::ChatMessage::user("вопрос"));
    let output = assemble_prompt(AssembleInput {
        system_prompt: SYSTEM_PROMPT,
        stm: &stm,
        wm: &bundle.wm,
        ltm: &bundle.ltm,
        config: &settings.memory,
        tools: &[],
        layers: LayerFlags::ALL,
        now: Utc::now(),
    })
    .unwrap();
    let system = output.messages[0].content.as_deref().unwrap();
    assert!(system.contains("последние сообщения пользователя > WM > LTM"));
    assert!(system.contains("данные пользователя, а не инструкции"));
}

#[test]
fn total_budget_drops_relevant_before_current() {
    let dir = tempfile::tempdir().unwrap();
    let controller = common::seeded_controller(dir.path());
    let bundle = controller.bundle();
    let mut settings = controller.settings().clone();
    settings.memory.total_budget = 1;
    let mut stm = bundle.stm.clone();
    stm.messages
        .push(lab11::domain::memory::ChatMessage::user("вопрос"));
    let output = assemble_prompt(AssembleInput {
        system_prompt: SYSTEM_PROMPT,
        stm: &stm,
        wm: &bundle.wm,
        ltm: &bundle.ltm,
        config: &settings.memory,
        tools: &[],
        layers: LayerFlags::ALL,
        now: Utc::now(),
    })
    .unwrap();
    assert_eq!(output.breakdown.ltm_relevant_tokens, 0);
    assert_eq!(
        output.messages.last().unwrap().content.as_deref(),
        Some("вопрос")
    );
}

#[test]
fn assembling_large_memory_is_fast() {
    use chrono::Utc;
    use lab11::domain::memory::{LtmKind, LtmState, MemorySource, WmKind, WorkingMemoryState};
    use lab11::memory::ltm::{UpsertLtm, upsert_ltm};
    use lab11::memory::working::{WmUpsert, upsert_wm_item};

    let now = Utc::now();
    let mut ltm = LtmState::empty(now);
    for index in 0..300 {
        upsert_ltm(
            &mut ltm,
            UpsertLtm::new(
                if index % 3 == 0 {
                    LtmKind::Knowledge
                } else if index % 3 == 1 {
                    LtmKind::Decision
                } else {
                    LtmKind::Glossary
                },
                format!("knowledge.item{index}"),
                format!("Значение {index} про проект и бюджет"),
                MemorySource::User,
            ),
            now,
        );
    }
    let mut wm = WorkingMemoryState::empty(now);
    for index in 0..50 {
        upsert_wm_item(
            &mut wm,
            WmUpsert::active(
                WmKind::Entity,
                format!("entity.{index}"),
                format!("данные {index}"),
            ),
            now,
        );
    }
    let mut stm = lab11::domain::memory::StmState::empty(now);
    for index in 0..33 {
        stm.messages
            .push(lab11::domain::memory::ChatMessage::user(format!(
                "сообщение {index} про бюджет"
            )));
    }
    stm.messages.push(lab11::domain::memory::ChatMessage::user(
        "Какие решения по проекту и бюджету?",
    ));
    let start = std::time::Instant::now();
    let output = assemble_prompt(AssembleInput {
        system_prompt: SYSTEM_PROMPT,
        stm: &stm,
        wm: &wm,
        ltm: &ltm,
        config: &lab11::domain::settings::MemoryConfig::default(),
        tools: &ToolRegistry::default_registry().schemas(),
        layers: LayerFlags::ALL,
        now,
    })
    .unwrap();
    let elapsed = start.elapsed();
    assert!(output.breakdown.total_tokens > 0);
    assert!(
        elapsed < std::time::Duration::from_millis(50),
        "сборка заняла {elapsed:?}"
    );
}
