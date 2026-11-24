//! OpenAI-совместимые DTO и блокирующий HTTP-клиент Chat Completions.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::domain::{ChatMessage, Role, ToolCall, Usage};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    /// Откуда инструмент: local (День 6) или mcp (GitHub MCP).
    #[serde(default)]
    pub source: ToolSource,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolSource {
    #[default]
    Local,
    Mcp,
}

impl ToolSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolSource::Local => "local",
            ToolSource::Mcp => "mcp",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub tools: Vec<ToolSchema>,
}

#[derive(Debug, Clone, Default)]
pub struct ChatResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("API-ключ не задан (нужен LAB16_API_KEY, DEEPSEEK_API_KEY или KIMI_API_KEY)")]
    MissingKey,
    #[error("HTTP-ошибка: {0}")]
    Http(String),
    #[error("API вернул статус {status}: {body}")]
    Status { status: u16, body: String },
    #[error("некорректный ответ API: {0}")]
    Json(String),
}

pub trait ChatClient: Send + Sync {
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError>;
}

pub struct ReqwestClient {
    endpoint: String,
    api_key: Option<String>,
    http: reqwest::blocking::Client,
}

impl ReqwestClient {
    pub fn new(base_url: &str, api_key: Option<String>) -> Result<Self, LlmError> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|error| LlmError::Http(error.to_string()))?;
        Ok(Self {
            endpoint: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            api_key,
            http,
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

impl ChatClient for ReqwestClient {
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let api_key = self
            .api_key
            .as_deref()
            .filter(|key| !key.trim().is_empty())
            .ok_or(LlmError::MissingKey)?;
        let body = build_body(request);
        let mut last_error = None;
        for attempt in 0..2 {
            let response = self
                .http
                .post(&self.endpoint)
                .bearer_auth(api_key)
                .json(&body)
                .send();
            match response {
                Ok(response) => {
                    let status = response.status();
                    let text = response.text().unwrap_or_default();
                    if status.is_success() {
                        return parse_response(&text);
                    }
                    if (status.as_u16() == 429 || status.is_server_error()) && attempt == 0 {
                        std::thread::sleep(Duration::from_millis(400));
                        last_error = Some(LlmError::Status {
                            status: status.as_u16(),
                            body: truncate(&text, 600),
                        });
                        continue;
                    }
                    return Err(LlmError::Status {
                        status: status.as_u16(),
                        body: truncate(&text, 600),
                    });
                }
                Err(error) => {
                    if attempt == 0 {
                        std::thread::sleep(Duration::from_millis(400));
                        last_error = Some(LlmError::Http(error.to_string()));
                        continue;
                    }
                    return Err(LlmError::Http(error.to_string()));
                }
            }
        }
        Err(last_error.unwrap_or_else(|| LlmError::Http("неизвестная ошибка".into())))
    }
}

/// Точное тело запроса. Схемы всех инструментов (локальных и MCP) попадают
/// сюда же — это тело печатает CLI и показывает вкладка API.
pub fn build_body(request: &ChatRequest) -> Value {
    let messages: Vec<Value> = request.messages.iter().map(message_body).collect();
    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "temperature": request.temperature,
        "stream": false,
    });
    if let Some(max_tokens) = request.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
        body["tool_choice"] = json!("auto");
    }
    body
}

fn message_body(message: &ChatMessage) -> Value {
    let mut value = json!({
        "role": message.role.as_str(),
        "content": message.content,
    });
    if !message.tool_calls.is_empty() {
        let calls: Vec<Value> = message
            .tool_calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": {"name": call.name, "arguments": call.arguments}
                })
            })
            .collect();
        value["tool_calls"] = Value::Array(calls);
    }
    if let Some(call_id) = &message.tool_call_id {
        value["tool_call_id"] = json!(call_id);
    }
    if let Some(name) = &message.name {
        value["name"] = json!(name);
    }
    value
}

pub fn response_body(response: &ChatResponse) -> Value {
    let calls: Vec<Value> = response
        .tool_calls
        .iter()
        .map(|call| {
            json!({
                "id": call.id,
                "type": "function",
                "function": {"name": call.name, "arguments": call.arguments}
            })
        })
        .collect();
    let mut message = json!({"role": Role::Assistant.as_str(), "content": response.content});
    if !calls.is_empty() {
        message["tool_calls"] = Value::Array(calls);
    }
    let mut body = json!({"choices": [{"message": message}]});
    if let Some(usage) = &response.usage {
        body["usage"] = json!({
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
            "total_tokens": usage.total_tokens,
        });
    }
    body
}

fn parse_response(text: &str) -> Result<ChatResponse, LlmError> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| LlmError::Json(error.to_string()))?;
    let message = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| LlmError::Json("в ответе нет choices[0].message".into()))?;
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut tool_calls = Vec::new();
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for (index, call) in calls.iter().enumerate() {
            let id = call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("call_unknown")
                .to_string();
            let function = call.get("function");
            let name = function
                .and_then(|value| value.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let arguments = function
                .and_then(|value| value.get("arguments"))
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_string();
            tool_calls.push(ToolCall {
                id: if id == "call_unknown" {
                    format!("call_{index}")
                } else {
                    id
                },
                name,
                arguments,
            });
        }
    }
    let usage = value.get("usage").and_then(|usage| {
        Some(Usage {
            prompt_tokens: usage.get("prompt_tokens")?.as_u64()? as usize,
            completion_tokens: usage.get("completion_tokens")?.as_u64()? as usize,
            total_tokens: usage.get("total_tokens")?.as_u64().unwrap_or(0) as usize,
        })
    });
    Ok(ChatResponse {
        content,
        tool_calls,
        usage,
    })
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        text.chars().take(limit).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(name: &str, source: ToolSource) -> ToolSchema {
        ToolSchema {
            name: name.into(),
            description: "desc".into(),
            parameters: json!({"type": "object", "properties": {}}),
            source,
        }
    }

    #[test]
    fn request_body_contains_both_tool_sources() {
        let request = ChatRequest {
            model: "test".into(),
            messages: vec![ChatMessage::user("привет")],
            temperature: 0.0,
            max_tokens: Some(10),
            tools: vec![
                schema("calculate", ToolSource::Local),
                schema("search_repositories", ToolSource::Mcp),
            ],
        };
        let body = build_body(&request);
        assert_eq!(body["tools"].as_array().unwrap().len(), 2);
        assert_eq!(body["tools"][0]["function"]["name"], "calculate");
        assert_eq!(body["tools"][1]["function"]["name"], "search_repositories");
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["max_tokens"], 10);
    }

    #[test]
    fn response_parser_reads_tool_calls() {
        let raw = r#"{
            "choices": [{"message": {
                "content": null,
                "tool_calls": [{"id":"call_1","type":"function",
                    "function":{"name":"calculate","arguments":"{\"expression\":\"2+2\"}"}}]
            }}],
            "usage": {"prompt_tokens": 5, "completion_tokens": 6, "total_tokens": 11}
        }"#;
        let response = parse_response(raw).unwrap();
        assert_eq!(response.tool_calls[0].name, "calculate");
        assert_eq!(response.usage.unwrap().total_tokens, 11);
    }
}
