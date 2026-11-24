//! Агент из Дня 6: цикл «запрос → tool_calls → результаты → запрос».
//! Модель получает схемы локальных инструментов и GitHub MCP одновременно,
//! сама выбирает имя и аргументы, а исполнение MCP уходит в `McpClient`.

use std::time::Instant;

use chrono::Utc;
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

use crate::catalog::ToolCatalog;
use crate::domain::{ChatMessage, Exchange, ToolCall, Usage};
use crate::llm::{ChatClient, ChatRequest, ToolSource, build_body, response_body};
use crate::settings::Settings;

#[derive(Debug, Clone)]
pub enum AgentEvent {
    Exchange(Exchange),
    ToolCall {
        name: String,
        source: ToolSource,
        arguments: String,
    },
    ToolResult {
        name: String,
        source: ToolSource,
        ok: bool,
        preview: String,
    },
}

#[derive(Debug, Clone)]
pub struct TurnResult {
    pub final_text: String,
    pub history: Vec<ChatMessage>,
    pub exchanges: Vec<Exchange>,
    pub usage: Usage,
    pub steps: usize,
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("ошибка модели: {0}")]
    Llm(String),
    #[error("достигнут лимит итераций tool-loop ({0})")]
    Iterations(usize),
}

pub struct Agent<'a> {
    pub settings: &'a Settings,
    pub client: &'a dyn ChatClient,
    pub catalog: &'a ToolCatalog,
}

impl Agent<'_> {
    pub fn run(
        &self,
        history: &[ChatMessage],
        user_text: &str,
        on_event: &mut dyn FnMut(AgentEvent),
    ) -> Result<TurnResult, AgentError> {
        let mut messages = vec![ChatMessage::system(system_prompt())];
        messages.extend(history.iter().cloned());
        messages.push(ChatMessage::user(user_text));

        let tools = self.catalog.schemas();
        let mut exchanges = Vec::new();
        let mut usage = Usage::default();

        for iteration in 1..=self.settings.max_iterations {
            let request = ChatRequest {
                model: self.settings.model.clone(),
                messages: messages.clone(),
                temperature: self.settings.temperature,
                max_tokens: self.settings.max_tokens,
                tools: tools.clone(),
            };
            let request_body = build_body(&request);
            let started = Instant::now();
            let started_at = Utc::now();
            let result = self.client.complete(&request);
            let duration_ms = started.elapsed().as_millis();
            let exchange = match &result {
                Ok(response) => Exchange {
                    id: Uuid::new_v4().to_string(),
                    iteration,
                    started_at: started_at.format("%H:%M:%S").to_string(),
                    duration_ms,
                    request: request_body,
                    response: Some(response_body(response)),
                    error: None,
                    usage: response.usage.clone().unwrap_or_default(),
                },
                Err(error) => Exchange {
                    id: Uuid::new_v4().to_string(),
                    iteration,
                    started_at: started_at.format("%H:%M:%S").to_string(),
                    duration_ms,
                    request: request_body,
                    response: None,
                    error: Some(error.to_string()),
                    usage: Usage::default(),
                },
            };
            on_event(AgentEvent::Exchange(exchange.clone()));
            exchanges.push(exchange);

            let response = result.map_err(|error| AgentError::Llm(error.to_string()))?;
            if let Some(response_usage) = response.usage {
                usage.prompt_tokens += response_usage.prompt_tokens;
                usage.completion_tokens += response_usage.completion_tokens;
                usage.total_tokens += response_usage.total_tokens;
            }

            if response.tool_calls.is_empty() {
                let final_text = response.content.unwrap_or_default();
                messages.push(ChatMessage::assistant(final_text.clone()));
                let steps = exchanges.len();
                return Ok(TurnResult {
                    final_text,
                    history: messages.into_iter().skip(1).collect(),
                    exchanges,
                    usage,
                    steps,
                });
            }

            messages.push(ChatMessage::assistant_tool_calls(
                response.content.clone(),
                response.tool_calls.clone(),
            ));
            for call in response.tool_calls {
                let source = self.catalog.source(&call.name).unwrap_or(ToolSource::Local);
                on_event(AgentEvent::ToolCall {
                    name: call.name.clone(),
                    source,
                    arguments: call.arguments.clone(),
                });
                let arguments = parse_arguments(&call);
                let value = self.catalog.execute(&call.name, &arguments);
                let ok =
                    value.get("error").is_none() && value.get("is_error") != Some(&json!(true));
                let preview = preview(&value);
                on_event(AgentEvent::ToolResult {
                    name: call.name.clone(),
                    source,
                    ok,
                    preview,
                });
                messages.push(ChatMessage::tool_result(
                    call.id.clone(),
                    call.name.clone(),
                    serde_json::to_string(&value).unwrap_or_else(|_| "{}".into()),
                ));
            }
        }

        Err(AgentError::Iterations(self.settings.max_iterations))
    }
}

fn parse_arguments(call: &ToolCall) -> Value {
    if call.arguments.trim().is_empty() {
        return json!({});
    }
    serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!({}))
}

fn preview(value: &Value) -> String {
    let raw = serde_json::to_string(value).unwrap_or_default();
    let mut preview: String = raw.chars().take(220).collect();
    if raw.chars().count() > 220 {
        preview.push('…');
    }
    preview
}

pub fn system_prompt() -> String {
    format!(
        "Ты — ассистент с инструментами. Сегодня {}. \
         В запросе доступны локальные инструменты Дня 6 (calculate, get_current_time, \
         convert_currency) и инструменты подключённого GitHub MCP-сервера.\n\
         Правила:\n\
         1. Данные GitHub (репозитории, файлы, issue, коммиты и т.п.) получай только \
         через MCP-инструменты; не выдумывай числа и названия.\n\
         2. Если задача требует вычисления по данным инструмента, сначала получи данные, \
         затем вызови локальный calculate с явным выражением.\n\
         3. Можно вызывать инструменты последовательно, пока не соберёшь всё нужное.\n\
         4. После получения результатов дай короткий ответ по-русски; не пересказывай \
         весь JSON, если пользователь не просил.",
        Utc::now().format("%Y-%m-%d")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{ChatResponse, LlmError};
    use std::sync::Mutex;

    struct Scripted {
        responses: Mutex<Vec<ChatResponse>>,
        requests: Mutex<Vec<ChatRequest>>,
    }

    impl ChatClient for Scripted {
        fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
            self.requests.lock().unwrap().push(request.clone());
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Ok(ChatResponse {
                    content: Some("готово".into()),
                    ..ChatResponse::default()
                })
            } else {
                Ok(responses.remove(0))
            }
        }
    }

    fn settings() -> Settings {
        Settings {
            max_iterations: 4,
            ..Settings::default()
        }
    }

    #[test]
    fn tool_loop_executes_local_tool_then_final() {
        let client = Scripted {
            responses: Mutex::new(vec![
                ChatResponse {
                    content: None,
                    tool_calls: vec![ToolCall {
                        id: "call_1".into(),
                        name: "calculate".into(),
                        arguments: r#"{"expression":"2+2"}"#.into(),
                    }],
                    ..ChatResponse::default()
                },
                ChatResponse {
                    content: Some("Ответ: 4".into()),
                    ..ChatResponse::default()
                },
            ]),
            requests: Mutex::new(Vec::new()),
        };
        let catalog = ToolCatalog::local_only();
        let mut events = Vec::new();
        let result = Agent {
            settings: &settings(),
            client: &client,
            catalog: &catalog,
        }
        .run(&[], "Сколько 2+2?", &mut |event| events.push(event))
        .unwrap();
        assert_eq!(result.final_text, "Ответ: 4");
        let requests = client.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].tools.len(), 3);
        assert!(
            result
                .history
                .iter()
                .any(|message| message.role == crate::domain::Role::Tool)
        );
        assert!(events.iter().any(|event| matches!(
            event,
            AgentEvent::ToolCall { name, .. } if name == "calculate"
        )));
        assert_eq!(
            result.exchanges[0].request["tools"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn malformed_arguments_become_empty_object() {
        let call = ToolCall {
            id: "1".into(),
            name: "calculate".into(),
            arguments: "not json".into(),
        };
        assert_eq!(parse_arguments(&call), json!({}));
    }
}
