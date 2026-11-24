//! MCP-клиент поверх официального Rust SDK `rmcp`.
//!
//! Транспорт — stdio: приложение запускает дочерний процесс MCP-сервера
//! (по умолчанию `npx -y @modelcontextprotocol/server-github`) и обменивается
//! с ним JSON-RPC. Соединение и список инструментов создаются один раз,
//! вызовы инструментов синхронно пробрасываются в tokio-runtime.

use std::path::PathBuf;
use std::sync::Mutex;

use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock, Tool};
use rmcp::service::{Peer, RoleClient, RunningService};
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::llm::{ToolSchema, ToolSource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpConfig {
    pub label: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
}

impl McpConfig {
    /// GitHub MCP по умолчанию. Пакет `@modelcontextprotocol/server-github`
    /// публичный и умеет работать без токена (только чтение публичных
    /// репозиториев); токен расширяет доступ и лимиты GitHub API.
    pub fn github() -> Self {
        let command = non_empty_env("LAB16_MCP_COMMAND").unwrap_or_else(|| "npx".into());
        let args = match non_empty_env("LAB16_MCP_ARGS") {
            Some(raw) => raw.split_whitespace().map(str::to_string).collect(),
            None => vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
        };
        let mut env = Vec::new();
        if let Some(token) = non_empty_env("GITHUB_PERSONAL_ACCESS_TOKEN")
            .or_else(|| non_empty_env("GITHUB_TOKEN"))
            .or_else(|| non_empty_env("GH_TOKEN"))
        {
            env.push(("GITHUB_PERSONAL_ACCESS_TOKEN".into(), token));
        }
        Self {
            label: non_empty_env("LAB16_MCP_LABEL").unwrap_or_else(|| "github".into()),
            command,
            args,
            env,
            cwd: None,
        }
    }

    /// Локальный stdio-сервер для тестов: например, python3 fixtures/server.py.
    pub fn command(
        label: impl Into<String>,
        command: impl Into<String>,
        args: Vec<String>,
    ) -> Self {
        Self {
            label: label.into(),
            command: command.into(),
            args,
            env: Vec::new(),
            cwd: None,
        }
    }

    pub fn disabled() -> bool {
        matches!(
            std::env::var("LAB16_MCP_DISABLE")
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
                .as_str(),
            "1" | "true" | "yes" | "on"
        )
    }

    pub fn command_line(&self) -> String {
        let mut parts = vec![self.command.clone()];
        parts.extend(self.args.iter().cloned());
        parts.join(" ")
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    pub name: String,
    pub version: String,
    pub protocol: String,
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

impl McpTool {
    pub fn to_schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: self.input_schema.clone(),
            source: ToolSource::Mcp,
        }
    }
}

impl From<&Tool> for McpTool {
    fn from(tool: &Tool) -> Self {
        Self {
            name: tool.name.to_string(),
            description: tool
                .description
                .as_deref()
                .or(tool.title.as_deref())
                .unwrap_or("Инструмент MCP")
                .to_string(),
            input_schema: Value::Object((*tool.input_schema).clone()),
        }
    }
}

#[derive(Debug, Error)]
pub enum McpError {
    #[error("не удалось запустить MCP-сервер {command}: {source}")]
    Spawn {
        command: String,
        source: std::io::Error,
    },
    #[error("ошибка инициализации MCP: {0}")]
    Init(String),
    #[error("ошибка MCP-инструмента: {0}")]
    Tool(String),
    #[error("MCP-соединение закрыто")]
    Closed,
}

pub struct McpClient {
    config: McpConfig,
    runtime: tokio::runtime::Runtime,
    /// Сервис нужен, чтобы соединение жило и корректно закрывалось.
    service: Mutex<Option<RunningService<RoleClient, ()>>>,
    peer: Peer<RoleClient>,
    server: McpServerInfo,
    tools: Vec<McpTool>,
}

impl McpClient {
    /// Устанавливает MCP-соединение и получает список инструментов.
    pub fn connect(config: McpConfig) -> Result<Self, McpError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("lab16-mcp")
            .build()
            .map_err(|error| McpError::Init(error.to_string()))?;
        let command_line = config.command_line();
        let (service, tools, server) = runtime.block_on(async {
            let command = tokio::process::Command::new(&config.command).configure(|command| {
                command.args(&config.args);
                for (key, value) in &config.env {
                    command.env(key, value);
                }
                if let Some(cwd) = &config.cwd {
                    command.current_dir(cwd);
                }
            });
            let transport = TokioChildProcess::new(command).map_err(|source| McpError::Spawn {
                command: command_line.clone(),
                source,
            })?;
            let service = ()
                .serve(transport)
                .await
                .map_err(|error| McpError::Init(format!("{error}; команда: {command_line}")))?;
            let tools = service
                .list_all_tools()
                .await
                .map_err(|error| McpError::Init(format!("tools/list: {error}")))?
                .iter()
                .map(McpTool::from)
                .collect::<Vec<_>>();
            let info = service.peer_info();
            let server = McpServerInfo {
                name: info
                    .as_ref()
                    .and_then(|peer| peer.server_info.as_ref())
                    .map(|implementation| implementation.name.clone())
                    .unwrap_or_else(|| config.label.clone()),
                version: info
                    .as_ref()
                    .and_then(|peer| peer.server_info.as_ref())
                    .map(|implementation| implementation.version.clone())
                    .unwrap_or_else(|| "—".into()),
                protocol: info
                    .as_ref()
                    .map(|peer| peer.protocol_version.to_string())
                    .unwrap_or_else(|| "—".into()),
                instructions: info.as_ref().and_then(|peer| peer.instructions.clone()),
            };
            Ok::<_, McpError>((service, tools, server))
        })?;
        let peer = service.peer().clone();
        Ok(Self {
            config,
            runtime,
            service: Mutex::new(Some(service)),
            peer,
            server,
            tools,
        })
    }

    pub fn config(&self) -> &McpConfig {
        &self.config
    }

    pub fn server(&self) -> &McpServerInfo {
        &self.server
    }

    pub fn tools(&self) -> &[McpTool] {
        &self.tools
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.iter().map(McpTool::to_schema).collect()
    }

    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.iter().any(|tool| tool.name == name)
    }

    /// Вызов инструмента через MCP. Возвращает значение, которое агенту
    /// останется только сериализовать в `tool`-сообщение.
    pub fn call_tool(&self, name: &str, arguments: &Value) -> Result<Value, McpError> {
        if !self.has_tool(name) {
            return Err(McpError::Tool(format!(
                "инструмент «{name}» не объявлен MCP-сервером"
            )));
        }
        let arguments = match arguments {
            Value::Object(object) => object.clone(),
            Value::Null => serde_json::Map::new(),
            other => {
                return Err(McpError::Tool(format!(
                    "аргументы должны быть JSON-объектом, получено {other}"
                )));
            }
        };
        let params = CallToolRequestParams::new(name.to_string()).with_arguments(arguments);
        let result = self
            .runtime
            .block_on(self.peer.call_tool(params))
            .map_err(|error| McpError::Tool(error.to_string()))?;
        Ok(call_result_to_value(result))
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.service.lock()
            && let Some(service) = guard.take()
        {
            let _ = self.runtime.block_on(service.cancel());
        }
    }
}

fn call_result_to_value(result: CallToolResult) -> Value {
    let mut texts = Vec::new();
    for block in &result.content {
        match block {
            ContentBlock::Text(text) => texts.push(text.text.clone()),
            ContentBlock::Image(_) => texts.push("[image content omitted]".into()),
            ContentBlock::Audio(_) => texts.push("[audio content omitted]".into()),
            ContentBlock::Resource(resource) => {
                texts.push(format!("[resource {:?}]", resource.resource));
            }
            ContentBlock::ResourceLink(link) => {
                texts.push(format!("[resource link {}]", link.uri));
            }
            _ => texts.push("[unsupported MCP content]".into()),
        }
    }
    let text = texts.join("\n");
    let content = if let Some(structured) = result.structured_content {
        structured
    } else if texts.len() == 1 {
        serde_json::from_str(&text).unwrap_or(Value::String(text.clone()))
    } else if texts.is_empty() {
        json!({})
    } else {
        Value::String(text.clone())
    };
    if result.is_error == Some(true) {
        json!({"is_error": true, "content": content, "text": text})
    } else {
        content
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_config_keeps_label_and_args() {
        let config = McpConfig::command(
            "github",
            "npx",
            vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
        );
        assert_eq!(config.label, "github");
        assert_eq!(
            config.command_line(),
            "npx -y @modelcontextprotocol/server-github"
        );
    }

    #[test]
    fn tool_conversion_keeps_schema_and_source() {
        let tool = McpTool {
            name: "search_repositories".into(),
            description: "Search".into(),
            input_schema: json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        };
        let schema = tool.to_schema();
        assert_eq!(schema.name, "search_repositories");
        assert_eq!(schema.source, ToolSource::Mcp);
        assert_eq!(schema.parameters["properties"]["query"]["type"], "string");
    }

    #[test]
    fn call_result_parses_json_text() {
        let result =
            CallToolResult::success(vec![ContentBlock::text(json!({"stars": 42}).to_string())]);
        let value = call_result_to_value(result);
        assert_eq!(value["stars"], 42);
    }

    #[test]
    fn call_result_marks_tool_error() {
        let result = CallToolResult::error(vec![ContentBlock::text("boom")]);
        let value = call_result_to_value(result);
        assert_eq!(value["is_error"], true);
        assert_eq!(value["text"], "boom");
    }
}
