mod common;

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use lab11::app::controller::{AppController, AppRuntime};
use lab11::domain::memory::MemoryLayer;
use lab11::domain::settings::ExtractionMode;
use lab11::llm::dto::{ChatClient, ChatRequest, ChatResponse, LlmError};
use lab11::storage::paths::DataPaths;

fn runtime_with(
    root: &std::path::Path,
    client: Arc<dyn ChatClient>,
    settings: lab11::domain::settings::Settings,
) -> AppRuntime {
    let paths = DataPaths::new(root);
    paths.ensure().unwrap();
    AppRuntime::new(paths, settings, client)
}

struct Scripted {
    responses: Mutex<Vec<ChatResponse>>,
    calls: AtomicUsize,
}

impl Scripted {
    fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
            calls: AtomicUsize::new(0),
        }
    }
}

impl ChatClient for Scripted {
    fn complete(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(ChatResponse {
                content: Some("ответ".into()),
                tool_calls: Vec::new(),
                usage: None,
            })
        } else {
            Ok(responses.remove(0))
        }
    }
}

struct Failing;

impl ChatClient for Failing {
    fn complete(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        Err(LlmError::Http("сеть недоступна".into()))
    }
}

struct Counting {
    calls: AtomicUsize,
}

impl ChatClient for Counting {
    fn complete(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ChatResponse {
            content: Some("ответ".into()),
            tool_calls: Vec::new(),
            usage: None,
        })
    }
}

struct ExtractionClient {
    calls: AtomicUsize,
}

impl ChatClient for ExtractionClient {
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let content = if request.tools.is_empty() {
            "[{\"key\":\"project.name\",\"value\":\"«Атлас»\",\"kind\":\"project\",\"scope\":\"project\"}]"
        } else {
            "Готово"
        };
        Ok(ChatResponse {
            content: Some(content.into()),
            tool_calls: Vec::new(),
            usage: None,
        })
    }
}

#[test]
fn send_user_message_routes_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(Scripted::new(Vec::new()));
    let mut controller =
        AppController::load(runtime_with(dir.path(), client, common::test_settings())).unwrap();
    let outcome = controller
        .send_user_message("Запускаем проект «Атлас».", &mut |_| {})
        .unwrap();
    assert_eq!(outcome.final_text, "ответ");
    assert!(!controller.bundle().pending.is_empty());
    assert!(
        controller
            .bundle()
            .stm
            .messages
            .iter()
            .any(|message| message.content.as_deref() == Some("Запускаем проект «Атлас»."))
    );

    let reloaded = common::controller_at(dir.path());
    assert!(!reloaded.bundle().pending.is_empty());
    assert!(
        reloaded
            .bundle()
            .stm
            .messages
            .iter()
            .any(|message| message.content.as_deref() == Some("ответ"))
    );
}

#[test]
fn auto_approval_writes_ltm_immediately() {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = common::test_settings();
    settings.memory.approval = lab11::domain::settings::Approval::Auto;
    let client = Arc::new(Scripted::new(Vec::new()));
    let mut controller = AppController::load(runtime_with(dir.path(), client, settings)).unwrap();
    controller
        .send_user_message("Дедлайн проекта — 15 декабря 2026.", &mut |_| {})
        .unwrap();
    assert!(controller.bundle().pending.is_empty());
    assert!(
        controller
            .bundle()
            .ltm
            .records
            .iter()
            .any(|record| record.key == "project.deadline" && record.is_active())
    );
}

#[test]
fn memory_write_tool_creates_pending_and_can_be_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(Scripted::new(vec![
        ChatResponse {
            content: None,
            tool_calls: vec![lab11::domain::memory::ToolCall {
                id: "call_1".into(),
                name: "memory_write".into(),
                arguments: "{\"layer\":\"ltm\",\"key\":\"project.name\",\"value\":\"«Атлас»\",\"kind\":\"project\"}".into(),
            }],
            usage: None,
        },
        ChatResponse {
            content: Some("Запомнил.".into()),
            tool_calls: Vec::new(),
            usage: None,
        },
    ]));
    let mut controller = AppController::load(runtime_with(
        dir.path(),
        client.clone(),
        common::test_settings(),
    ))
    .unwrap();
    controller
        .send_user_message("Запомни это", &mut |_| {})
        .unwrap();
    assert_eq!(client.calls.load(Ordering::SeqCst), 2);
    let pending = controller.pending_list().to_vec();
    assert!(
        pending
            .iter()
            .any(|item| item.candidate.key == "project.name")
    );
    let id = pending
        .iter()
        .find(|item| item.candidate.key == "project.name")
        .unwrap()
        .id
        .clone();
    controller.accept_pending(&id).unwrap();
    assert!(
        controller
            .bundle()
            .ltm
            .records
            .iter()
            .any(|record| record.key == "project.name" && record.value == "«Атлас»")
    );
    assert!(controller.bundle().pending.iter().all(|item| item.id != id));
}

#[test]
fn reject_pending_leaves_no_ltm_record() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(Scripted::new(Vec::new()));
    let mut controller =
        AppController::load(runtime_with(dir.path(), client, common::test_settings())).unwrap();
    controller
        .send_user_message("Запускаем проект «Атлас».", &mut |_| {})
        .unwrap();
    let id = controller.pending_list()[0].id.clone();
    controller.reject_pending(&id).unwrap();
    assert!(controller.bundle().pending.is_empty());
    assert!(
        controller
            .bundle()
            .ltm
            .records
            .iter()
            .all(|record| record.key != "project.name")
    );
}

#[test]
fn model_error_does_not_lose_routed_changes() {
    let dir = tempfile::tempdir().unwrap();
    let mut controller = AppController::load(runtime_with(
        dir.path(),
        Arc::new(Failing),
        common::test_settings(),
    ))
    .unwrap();
    let result = controller.send_user_message("Дедлайн проекта — 15 декабря 2026.", &mut |_| {});
    assert!(result.is_err());
    let reloaded = common::controller_at(dir.path());
    assert!(
        reloaded
            .bundle()
            .pending
            .iter()
            .any(|pending| pending.candidate.key == "project.deadline")
    );
}

#[test]
fn local_extraction_makes_exactly_one_llm_request() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(Counting {
        calls: AtomicUsize::new(0),
    });
    let mut controller = AppController::load(runtime_with(
        dir.path(),
        client.clone(),
        common::test_settings(),
    ))
    .unwrap();
    controller
        .send_user_message("Дедлайн проекта — 15 декабря 2026.", &mut |_| {})
        .unwrap();
    assert_eq!(client.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn llm_extraction_adds_one_request() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(ExtractionClient {
        calls: AtomicUsize::new(0),
    });
    let mut settings = common::test_settings();
    settings.memory.extraction_mode = ExtractionMode::Llm;
    let mut controller =
        AppController::load(runtime_with(dir.path(), client.clone(), settings)).unwrap();
    controller
        .send_user_message("Запускаем проект «Атлас».", &mut |_| {})
        .unwrap();
    assert_eq!(client.calls.load(Ordering::SeqCst), 2);
    assert!(
        controller
            .bundle()
            .pending
            .iter()
            .any(|pending| pending.candidate.key == "project.name")
    );
}

#[test]
fn clear_stm_before_turn_keeps_other_layers() {
    let dir = tempfile::tempdir().unwrap();
    let mut controller = common::seeded_controller(dir.path());
    let wm_len = controller.bundle().wm.items.len();
    let ltm_len = controller.bundle().ltm.records.len();
    controller.clear_layer(MemoryLayer::Stm).unwrap();
    assert!(controller.bundle().stm.messages.is_empty());
    assert_eq!(controller.bundle().wm.items.len(), wm_len);
    assert_eq!(controller.bundle().ltm.records.len(), ltm_len);
    let client = Arc::new(Scripted::new(Vec::new()));
    let mut controller =
        AppController::load(runtime_with(dir.path(), client, common::test_settings())).unwrap();
    controller.send_user_message("Привет", &mut |_| {}).unwrap();
    assert_eq!(controller.bundle().wm.items.len(), wm_len);
    assert_eq!(controller.bundle().ltm.records.len(), ltm_len);
}

#[test]
fn manual_add_update_delete_and_pin() {
    let dir = tempfile::tempdir().unwrap();
    let mut controller = common::controller_at(dir.path());
    assert_eq!(
        controller
            .memory_add(
                MemoryLayer::Ltm,
                "project.name",
                "«Атлас»",
                Some("project"),
                false
            )
            .unwrap(),
        "created"
    );
    controller
        .memory_update(
            MemoryLayer::Ltm,
            "project.name",
            "«Атлас-2»",
            Some("project"),
        )
        .unwrap();
    assert_eq!(
        lab11::memory::ltm::find_active_ltm(&controller.bundle().ltm, "project.name")
            .unwrap()
            .value,
        "«Атлас-2»"
    );
    controller.memory_pin("project.name", true).unwrap();
    assert!(
        lab11::memory::ltm::find_active_ltm(&controller.bundle().ltm, "project.name")
            .unwrap()
            .pinned
    );
    assert_eq!(
        controller
            .memory_delete(MemoryLayer::Ltm, "project.name")
            .unwrap(),
        1
    );
    assert!(
        lab11::memory::ltm::find_active_ltm(&controller.bundle().ltm, "project.name").is_none()
    );
}

#[test]
fn branches_restore_all_layers() {
    let dir = tempfile::tempdir().unwrap();
    let mut controller = common::seeded_controller(dir.path());
    let id = controller.checkpoint_branch("baseline").unwrap();
    controller.clear_layer(MemoryLayer::Ltm).unwrap();
    controller.checkpoint_branch("empty ltm").unwrap();
    assert_eq!(
        lab11::memory::ltm::active_ltm(&controller.bundle().ltm).count(),
        0
    );
    controller.switch_branch(&id).unwrap();
    assert_eq!(
        lab11::memory::ltm::active_ltm(&controller.bundle().ltm).count(),
        15
    );
    assert!(!controller.bundle().stm.messages.is_empty());
    let reloaded = common::controller_at(dir.path());
    assert_eq!(
        lab11::memory::ltm::active_ltm(&reloaded.bundle().ltm).count(),
        15
    );
}

#[test]
fn wm_to_ltm_move_respects_confirm() {
    let dir = tempfile::tempdir().unwrap();
    let mut controller = common::seeded_controller(dir.path());
    let status = controller
        .memory_move(MemoryLayer::Wm, "estimate.variant", MemoryLayer::Ltm)
        .unwrap();
    assert_eq!(status, "pending");
    assert!(
        controller
            .bundle()
            .pending
            .iter()
            .any(|pending| pending.candidate.key == "estimate.variant")
    );
}

#[test]
fn secrets_never_reach_layer_files_or_audit() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(Scripted::new(Vec::new()));
    let mut controller =
        AppController::load(runtime_with(dir.path(), client, common::test_settings())).unwrap();
    controller
        .send_user_message("Мой API-ключ: sk-super-secret-1234567890", &mut |_| {})
        .unwrap();
    fn walk(path: &std::path::Path, out: &mut String) {
        if path.is_file() {
            out.push_str(&std::fs::read_to_string(path).unwrap_or_default());
        } else if path.is_dir() {
            for entry in std::fs::read_dir(path).unwrap() {
                walk(&entry.unwrap().path(), out);
            }
        }
    }
    let mut text = String::new();
    walk(dir.path(), &mut text);
    assert!(!text.contains("super-secret-1234567890"));
    assert!(text.contains("***"));
}

#[test]
fn breakdown_is_recorded_in_api_log() {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(Scripted::new(Vec::new()));
    let mut controller =
        AppController::load(runtime_with(dir.path(), client, common::test_settings())).unwrap();
    controller
        .send_user_message("Дедлайн проекта — 15 декабря 2026.", &mut |_| {})
        .unwrap();
    let exchange = controller.exchanges.last().unwrap();
    assert!(exchange.memory.is_some());
    assert!(exchange.prompt_tokens > 0);
    assert!(exchange.memory.as_ref().unwrap().total_tokens > 0);
}

#[test]
fn llm_extraction_failure_falls_back_to_local() {
    struct BadExtraction {
        calls: AtomicUsize,
    }
    impl ChatClient for BadExtraction {
        fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let content = if request.tools.is_empty() {
                "это не json"
            } else {
                "ответ"
            };
            Ok(ChatResponse {
                content: Some(content.into()),
                tool_calls: Vec::new(),
                usage: None,
            })
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(BadExtraction {
        calls: AtomicUsize::new(0),
    });
    let mut settings = common::test_settings();
    settings.memory.extraction_mode = ExtractionMode::Llm;
    let mut controller =
        AppController::load(runtime_with(dir.path(), client.clone(), settings)).unwrap();
    controller
        .send_user_message("Дедлайн проекта — 15 декабря 2026.", &mut |_| {})
        .unwrap();
    assert_eq!(client.calls.load(Ordering::SeqCst), 2);
    assert!(
        controller
            .bundle()
            .pending
            .iter()
            .any(|pending| pending.candidate.key == "project.deadline")
    );
}

#[test]
fn controller_has_no_process_exit() {
    let source = include_str!("../src/app/controller.rs");
    assert!(!source.contains("std::process::exit("));
}
