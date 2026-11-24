//! Интеграция: настоящий stdio MCP-протокол через rmcp, но с локальным
//! fixture-сервером. Проверяются соединение, tools/list, tools/call и
//! смешанный tool-loop «локальный День 6 + MCP».

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;

use lab16::agent::{Agent, AgentEvent};
use lab16::catalog::ToolCatalog;
use lab16::domain::ToolCall;
use lab16::llm::{ChatClient, ChatRequest, ChatResponse, LlmError, ToolSource};
use lab16::mcp::{McpClient, McpConfig};
use lab16::settings::Settings;
use lab16::tools::LocalToolRegistry;
use serde_json::json;

fn fixture_config() -> McpConfig {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fake_mcp_server.py");
    McpConfig::command("fixture", "python3", vec![script.display().to_string()])
}

struct Scripted {
    responses: Mutex<VecDeque<ChatResponse>>,
    requests: Mutex<Vec<ChatRequest>>,
}

impl Scripted {
    fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl ChatClient for Scripted {
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| ChatResponse {
                content: Some("готово".into()),
                ..ChatResponse::default()
            }))
    }
}

#[test]
fn mcp_connection_lists_tools_and_calls_them() {
    let client = McpClient::connect(fixture_config()).expect("MCP должен подключиться");
    assert_eq!(client.server().name, "fixture-mcp");
    assert_eq!(client.server().version, "1.0.0");
    assert_eq!(client.server().protocol, "2024-11-05");
    let names: Vec<&str> = client
        .tools()
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(names, vec!["repo_stars", "repo_languages"]);
    assert!(client.has_tool("repo_stars"));

    let value = client
        .call_tool("repo_stars", &json!({"repo": "rust-lang/rust"}))
        .expect("tools/call");
    assert_eq!(value["repo"], "rust-lang/rust");
    assert_eq!(value["stars"], 37);
}

#[test]
fn mixed_local_and_mcp_tool_loop() {
    let client = McpClient::connect(fixture_config()).expect("MCP должен подключиться");
    let catalog = ToolCatalog::with_mcp(LocalToolRegistry::day6(), std::sync::Arc::new(client));
    let names: Vec<String> = catalog
        .schemas()
        .into_iter()
        .map(|schema| schema.name)
        .collect();
    assert!(names.contains(&"calculate".to_string()));
    assert!(names.contains(&"get_current_time".to_string()));
    assert!(names.contains(&"convert_currency".to_string()));
    assert!(names.contains(&"repo_stars".to_string()));
    assert_eq!(catalog.local_count(), 3);
    assert_eq!(catalog.mcp_count(), 2);

    let scripted = Scripted::new(vec![
        ChatResponse {
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_mcp".into(),
                name: "repo_stars".into(),
                arguments: json!({"repo": "rust-lang/rust"}).to_string(),
            }],
            usage: None,
        },
        ChatResponse {
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_local".into(),
                name: "calculate".into(),
                arguments: json!({"expression": "37 * 2"}).to_string(),
            }],
            usage: None,
        },
        ChatResponse {
            content: Some("У репозитория 37 звёзд, удвоенное значение — 74.".into()),
            tool_calls: Vec::new(),
            usage: None,
        },
    ]);
    let settings = Settings {
        max_iterations: 4,
        ..Settings::default()
    };
    let mut events = Vec::new();
    let result = Agent {
        settings: &settings,
        client: &scripted,
        catalog: &catalog,
    }
    .run(
        &[],
        "Сколько звёзд у rust-lang/rust и сколько будет, если удвоить это число?",
        &mut |event| events.push(event),
    )
    .expect("tool-loop должен завершиться");

    assert_eq!(
        result.final_text,
        "У репозитория 37 звёзд, удвоенное значение — 74."
    );
    assert_eq!(result.steps, 3);

    let requests = scripted.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    let first_tools: Vec<&str> = requests[0]
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert!(first_tools.contains(&"calculate"));
    assert!(first_tools.contains(&"repo_stars"));
    // Модель увидела результат MCP до вызова локального калькулятора.
    let tool_messages: Vec<&str> = requests[1]
        .messages
        .iter()
        .filter(|message| message.role == lab16::domain::Role::Tool)
        .map(|message| message.text())
        .collect();
    assert_eq!(tool_messages.len(), 1);
    assert!(tool_messages[0].contains("37"));

    assert_eq!(result.exchanges.len(), 3);
    for exchange in &result.exchanges {
        let tools = exchange.request["tools"].as_array().unwrap();
        let names: Vec<&str> = tools
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"calculate"));
        assert!(names.contains(&"repo_stars"));
        assert_eq!(exchange.request["tool_choice"], "auto");
    }

    let calls: Vec<(String, ToolSource)> = events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::ToolCall { name, source, .. } => Some((name.clone(), *source)),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        vec![
            ("repo_stars".to_string(), ToolSource::Mcp),
            ("calculate".to_string(), ToolSource::Local),
        ]
    );
}
