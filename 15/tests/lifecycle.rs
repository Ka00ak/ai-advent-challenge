//! День 15: сценарий изменения фактов, промпт без устаревших значений,
//! append-списки и очистка после завершения задачи.

mod common;

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use lab15::app::controller::{AppController, AppRuntime};
use lab15::domain::memory::{FactPolicy, LtmRevisionOp, MemoryLayer};
use lab15::domain::settings::Settings;
use lab15::llm::dto::{ChatClient, ChatRequest, ChatResponse, LlmError};
use lab15::storage::paths::DataPaths;

/// Отвечает значениями, которые реально видит в промпте. Если промпт
/// содержит устаревшее значение, это видно по ответу и счётчику.
struct PromptAnswerClient {
    prompts: Mutex<Vec<String>>,
    calls: AtomicUsize,
}

impl PromptAnswerClient {
    fn new() -> Self {
        Self {
            prompts: Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
        }
    }

    fn prompts(&self) -> Vec<String> {
        self.prompts.lock().unwrap().clone()
    }
}

fn prompt_value(prompt: &str, key: &str) -> Option<String> {
    let marker = format!("{key}: ");
    let start = prompt.find(&marker)? + marker.len();
    let rest = &prompt[start..];
    let line = rest.lines().next().unwrap_or(rest);
    let value = line.split(" · записано").next().unwrap_or(line);
    Some(value.trim().to_string())
}

impl ChatClient for PromptAnswerClient {
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let prompt = request
            .messages
            .iter()
            .map(|message| message.text())
            .collect::<Vec<_>>()
            .join("\n");
        self.prompts.lock().unwrap().push(prompt.clone());
        let answer = format!(
            "Срок: {}; требование: {}; подход: {}; версия: {}",
            prompt_value(&prompt, "project.deadline").unwrap_or_else(|| "нет данных".into()),
            prompt_value(&prompt, "requirement.current").unwrap_or_else(|| "нет данных".into()),
            prompt_value(&prompt, "decision.approach").unwrap_or_else(|| "нет данных".into()),
            prompt_value(&prompt, "project.version").unwrap_or_else(|| "нет данных".into()),
        );
        Ok(ChatResponse {
            content: Some(answer),
            tool_calls: Vec::new(),
            usage: None,
        })
    }
}

struct Scripted {
    responses: Mutex<Vec<String>>,
}

impl ChatClient for Scripted {
    fn complete(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let mut responses = self.responses.lock().unwrap();
        let content = if responses.is_empty() {
            "ВЕРДИКТ: OK\nготово".to_string()
        } else {
            responses.remove(0)
        };
        Ok(ChatResponse {
            content: Some(content),
            tool_calls: Vec::new(),
            usage: None,
        })
    }
}

fn runtime(root: &std::path::Path, client: Arc<dyn ChatClient>) -> AppRuntime {
    let paths = DataPaths::new(root);
    paths.ensure().unwrap();
    let mut settings = Settings::default();
    settings.memory.stm.summary_enabled = false;
    settings.memory.approval = lab15::domain::settings::Approval::Auto;
    settings.memory.ltm.profile_max_tokens = 1200;
    settings.memory.ltm.relevant_max_tokens = 800;
    settings.memory.ltm.top_k = 10;
    settings.memory.total_budget = 6000;
    settings.temperature = 0.0;
    AppRuntime::new(paths, settings, client)
}

fn send(controller: &mut AppController, text: &str) -> String {
    controller
        .send_user_message(text, &mut |_| {})
        .unwrap()
        .final_text
}

#[test]
fn dialogue_updates_four_facts_without_stale_values_in_prompts_or_answers() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(PromptAnswerClient::new());
    let mut controller = AppController::load(runtime(dir.path(), client.clone())).unwrap();

    // Исходные факты приходят обычными репликами — они остаются в STM.
    send(&mut controller, "Дедлайн проекта — 15 декабря 2026.");
    send(&mut controller, "Требование: обязательный аудит.");
    send(
        &mut controller,
        "Выбранный подход: подход A — ручной отчёт.",
    );
    send(&mut controller, "Версия проекта: 1.0.");

    let initial_prompt = client.prompts().last().cloned().unwrap();
    assert!(initial_prompt.contains("15 декабря 2026"));
    assert!(initial_prompt.contains("обязательный аудит"));

    let changes = [
        (
            "Срок проекта теперь 20 декабря 2026.",
            "project.deadline",
            "15 декабря 2026",
            "20 декабря 2026",
        ),
        (
            "Требование отменили: аудит больше не обязателен.",
            "requirement.current",
            "обязательный аудит",
            "отменено",
        ),
        (
            "Выбрали другой подход: локальное TUI-приложение.",
            "decision.approach",
            "подход A",
            "TUI",
        ),
        ("Версия проекта: 2.0.", "project.version", "1.0", "2.0"),
    ];

    let mut stale_prompt_count = 0;
    let mut stale_answer_count = 0;
    for (change, key, old_value, new_value) in changes {
        send(&mut controller, change);
        let change_prompt = client.prompts().last().cloned().unwrap();
        if change_prompt.contains(old_value) {
            stale_prompt_count += 1;
        }
        assert!(
            !change_prompt.contains(old_value),
            "после изменения «{old_value}» осталось в промпте"
        );
        let active = lab15::memory::ltm::find_active_ltm(&controller.bundle().ltm, key)
            .expect("активный факт");
        assert!(
            active.value.contains(new_value),
            "{key}: ожидали {new_value}, получили {}",
            active.value
        );
        let revision = controller
            .ltm_revisions()
            .iter()
            .rev()
            .find(|revision| revision.key == key)
            .expect("revision");
        assert_eq!(revision.op, LtmRevisionOp::Replace);
        assert!(
            revision
                .old_value
                .as_deref()
                .is_some_and(|old| old.contains(old_value))
        );

        let question = match key {
            "project.deadline" => "Какой сейчас срок проекта?",
            "requirement.current" => "Какое сейчас требование?",
            "decision.approach" => "Какой сейчас выбранный подход?",
            _ => "Какая сейчас версия проекта?",
        };
        let answer = send(&mut controller, question);
        if answer.contains(old_value) {
            stale_answer_count += 1;
        }
        assert!(answer.contains(new_value), "ответ: {answer}");

        let prompt = client.prompts().last().cloned().unwrap();
        if prompt.contains(old_value) {
            stale_prompt_count += 1;
        }
        assert!(
            !prompt.contains(old_value),
            "старое значение «{old_value}» попало в промпт"
        );
    }

    assert_eq!(stale_prompt_count, 0);
    assert_eq!(stale_answer_count, 0);
    eprintln!(
        "lifecycle check: changes=4 prompts_checked=8 stale_prompts={stale_prompt_count} stale_answers={stale_answer_count}"
    );
    assert_eq!(
        controller
            .bundle()
            .ltm
            .records
            .iter()
            .filter(|record| record.is_active() && record.key == "project.deadline")
            .count(),
        1
    );
    assert!(
        controller
            .bundle()
            .ltm
            .revisions
            .iter()
            .filter(|revision| revision.op == LtmRevisionOp::Replace)
            .count()
            >= 4
    );
}

#[test]
fn append_facts_are_stored_as_a_list() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(PromptAnswerClient::new());
    let mut controller = AppController::load(runtime(dir.path(), client)).unwrap();
    send(&mut controller, "Принятое решение: PostgreSQL основной.");
    send(&mut controller, "Принятое решение: REST для API.");
    let active: Vec<_> = controller
        .bundle()
        .ltm
        .records
        .iter()
        .filter(|record| {
            record.is_active()
                && record.key == "decisions.accepted"
                && record.policy == FactPolicy::Append
        })
        .collect();
    assert_eq!(active.len(), 2);
    assert_eq!(
        controller
            .bundle()
            .ltm
            .revisions
            .iter()
            .filter(|revision| {
                revision.key == "decisions.accepted" && revision.op == LtmRevisionOp::Append
            })
            .count(),
        2
    );
}

#[test]
fn task_completion_clears_wm_and_task_scoped_ltm_but_keeps_project() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(PromptAnswerClient::new());
    let mut controller = AppController::load(runtime(dir.path(), client)).unwrap();
    controller
        .memory_add(
            MemoryLayer::Ltm,
            "project.deadline",
            "20 декабря 2026",
            Some("project"),
            false,
        )
        .unwrap();
    controller
        .memory_add(
            MemoryLayer::Ltm,
            "task.rate",
            "90 ₽",
            Some("knowledge"),
            false,
        )
        .unwrap();
    controller
        .memory_add(
            MemoryLayer::Wm,
            "task.goal",
            "Подготовить смету",
            Some("goal"),
            false,
        )
        .unwrap();
    assert_eq!(
        lab15::memory::ltm::find_active_ltm(&controller.bundle().ltm, "task.rate")
            .unwrap()
            .policy,
        FactPolicy::TaskScoped
    );

    controller.archive_task().unwrap();

    assert!(controller.bundle().wm.items.is_empty());
    assert!(lab15::memory::ltm::find_active_ltm(&controller.bundle().ltm, "task.rate").is_none());
    assert_eq!(
        lab15::memory::ltm::find_active_ltm(&controller.bundle().ltm, "project.deadline")
            .unwrap()
            .value,
        "20 декабря 2026"
    );
    assert!(
        controller
            .bundle()
            .ltm
            .revisions
            .iter()
            .any(|revision| revision.op == LtmRevisionOp::TaskCleanup)
    );
    assert_eq!(controller.last_cleanup().unwrap().wm_items, 1);
    assert_eq!(controller.last_cleanup().unwrap().ltm_records, 1);
}

#[test]
fn successful_task_auto_cleans_memory() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(Scripted {
        responses: Mutex::new(vec![
            "1. Собрать данные\n2. Подготовить отчёт".into(),
            "Отчёт подготовлен.".into(),
            "ВЕРДИКТ: OK\nПлан выполнен.".into(),
        ]),
    });
    let mut controller = AppController::load(runtime(dir.path(), client)).unwrap();
    controller
        .memory_add(
            MemoryLayer::Ltm,
            "task.rate",
            "90 ₽",
            Some("knowledge"),
            false,
        )
        .unwrap();
    controller
        .memory_add(
            MemoryLayer::Wm,
            "task.goal",
            "Подготовить отчёт",
            Some("goal"),
            false,
        )
        .unwrap();
    controller
        .memory_add(
            MemoryLayer::Ltm,
            "project.deadline",
            "20 декабря 2026",
            Some("project"),
            false,
        )
        .unwrap();
    let cancel = AtomicBool::new(false);
    let pause = AtomicBool::new(false);
    let state = controller
        .run_task_cancellable("Подготовить отчёт", &[], &cancel, &pause, &mut |_| {})
        .unwrap();
    assert_eq!(state.status, lab15::domain::task::TaskStatus::Done);
    assert!(controller.bundle().wm.items.is_empty());
    assert!(lab15::memory::ltm::find_active_ltm(&controller.bundle().ltm, "task.rate").is_none());
    assert!(
        lab15::memory::ltm::find_active_ltm(&controller.bundle().ltm, "project.deadline").is_some()
    );
    assert!(controller.last_cleanup().is_some());
}
