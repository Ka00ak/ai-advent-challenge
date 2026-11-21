//! Записывающая обёртка ChatClient: один вызов `complete` — одна строка
//! API-журнала с полным (и очищенным от секретов) запросом и ответом.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{Value, json};

use crate::domain::chat::{ExchangeKind, LlmExchange};
use crate::llm::client::{build_body, chat_endpoint};
use crate::llm::dto::{ChatClient, ChatRequest, ChatResponse, LlmError, model_profile};
use crate::memory::audit::redact_secrets_preserve;
use crate::memory::tokens::{
    estimate_messages_tokens, estimate_text_tokens, estimate_tools_tokens,
};

const PREVIEW_CHARS: usize = 200;
const MAX_BODY_CHARS: usize = 200_000;

pub type RequestLog = Arc<Mutex<Vec<LlmExchange>>>;
pub type ExchangeListener = Arc<dyn Fn(LlmExchange) + Send + Sync>;

pub fn new_request_log() -> RequestLog {
    Arc::new(Mutex::new(Vec::new()))
}

pub struct RecordingChatClient {
    inner: Arc<dyn ChatClient>,
    kind: ExchangeKind,
    log: RequestLog,
    listener: Option<ExchangeListener>,
    url: String,
    model: String,
}

impl RecordingChatClient {
    pub fn new(
        inner: Arc<dyn ChatClient>,
        kind: ExchangeKind,
        log: RequestLog,
        base_url: &str,
        model: &str,
    ) -> Self {
        Self {
            inner,
            kind,
            log,
            listener: None,
            url: chat_endpoint(base_url),
            model: model.to_string(),
        }
    }

    pub fn with_listener(mut self, listener: Option<ExchangeListener>) -> Self {
        self.listener = listener;
        self
    }
}

impl ChatClient for RecordingChatClient {
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let started = Instant::now();
        let request_body = pretty(&build_body(request));
        let result = self.inner.complete(request);
        let duration_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;

        let mut exchange = LlmExchange::new(self.kind, &self.model, &self.url);
        exchange.duration_ms = duration_ms;
        exchange.request_body = Some(clean(&request_body));
        exchange.request_preview = Some(preview(&request_body));
        exchange.prompt_tokens =
            estimate_messages_tokens(&request.messages) + estimate_tools_tokens(&request.tools);

        match &result {
            Ok(response) => {
                exchange.status = "ok".into();
                exchange.status_code = Some(200);
                if let Some(usage) = response.usage {
                    exchange.prompt_tokens = usage.prompt_tokens;
                    exchange.completion_tokens = usage.completion_tokens;
                    exchange.total_tokens = if usage.total_tokens > 0 {
                        usage.total_tokens
                    } else {
                        usage.prompt_tokens + usage.completion_tokens
                    };
                } else {
                    exchange.completion_tokens =
                        estimate_text_tokens(response.content.as_deref().unwrap_or_default());
                    exchange.total_tokens = exchange.prompt_tokens + exchange.completion_tokens;
                }
                let response_body = response_json(response);
                exchange.response_preview = Some(preview(&response_body));
                exchange.response_body = Some(clean(&response_body));
            }
            Err(error) => {
                exchange.status = "error".into();
                exchange.error = Some(clean(&error.to_string()));
                exchange.response_preview = Some(preview(&error.to_string()));
                if let LlmError::Status { status, body } = error {
                    exchange.status_code = Some(*status);
                    exchange.response_body = Some(clean(&format!("HTTP {status}: {body}")));
                } else {
                    exchange.response_body = Some(clean(&error.to_string()));
                }
                exchange.total_tokens = exchange.prompt_tokens + exchange.completion_tokens;
            }
        }

        let (input_cost, output_cost) = model_profile(&self.model);
        exchange.cost_usd = (exchange.prompt_tokens as f64 / 1_000_000.0) * input_cost
            + (exchange.completion_tokens as f64 / 1_000_000.0) * output_cost;

        if let Ok(mut log) = self.log.lock() {
            log.push(exchange.clone());
        }
        if let Some(listener) = &self.listener {
            listener(exchange);
        }
        result
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|_| serde_json::to_string(value).unwrap_or_default())
}

fn response_json(response: &ChatResponse) -> String {
    let calls: Vec<Value> = response
        .tool_calls
        .iter()
        .map(|call| {
            json!({
                "id": call.id,
                "type": "function",
                "function": {
                    "name": call.name,
                    "arguments": call.arguments,
                }
            })
        })
        .collect();
    let usage = response.usage.map(|usage| {
        json!({
            "prompt_tokens": usage.prompt_tokens,
            "completion_tokens": usage.completion_tokens,
            "total_tokens": usage.total_tokens,
        })
    });
    pretty(&json!({
        "content": response.content,
        "tool_calls": calls,
        "usage": usage,
    }))
}

fn clean(body: &str) -> String {
    let redacted = redact_secrets_preserve(body);
    if redacted.chars().count() <= MAX_BODY_CHARS {
        redacted
    } else {
        let mut out: String = redacted.chars().take(MAX_BODY_CHARS).collect();
        out.push_str("\n… (тело обрезано)");
        out
    }
}

fn preview(text: &str) -> String {
    let one_line = text.replace('\n', " ");
    if one_line.chars().count() <= PREVIEW_CHARS {
        one_line
    } else {
        let mut out: String = one_line.chars().take(PREVIEW_CHARS - 1).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::ChatMessage;

    struct Fake {
        answer: String,
        fail: bool,
    }

    impl ChatClient for Fake {
        fn complete(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
            if self.fail {
                return Err(LlmError::Status {
                    status: 429,
                    body: "rate limited token=sk-abcdefghijklmnopqrstuvwxyz123456".into(),
                });
            }
            Ok(ChatResponse {
                content: Some(self.answer.clone()),
                tool_calls: Vec::new(),
                usage: None,
            })
        }
    }

    fn request() -> ChatRequest {
        ChatRequest {
            model: "deepseek-chat".into(),
            messages: vec![ChatMessage::user("привет")],
            temperature: 0.2,
            max_tokens: None,
            tools: Vec::new(),
        }
    }

    #[test]
    fn records_one_exchange_with_full_bodies() {
        let log = new_request_log();
        let client = RecordingChatClient::new(
            Arc::new(Fake {
                answer: "ответ".into(),
                fail: false,
            }),
            ExchangeKind::Chat,
            log.clone(),
            "https://api.deepseek.com",
            "deepseek-chat",
        );
        client.complete(&request()).unwrap();
        let entries = log.lock().unwrap();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.status_code, Some(200));
        assert!(entry.request_body.as_deref().unwrap().contains("привет"));
        assert!(entry.response_body.as_deref().unwrap().contains("ответ"));
        assert!(entry.request_size() > 0);
        assert!(entry.response_size() > 0);
    }

    #[test]
    fn records_error_and_redacts_secret() {
        let log = new_request_log();
        let client = RecordingChatClient::new(
            Arc::new(Fake {
                answer: String::new(),
                fail: true,
            }),
            ExchangeKind::Memory,
            log.clone(),
            "https://api.deepseek.com",
            "deepseek-chat",
        );
        assert!(client.complete(&request()).is_err());
        let entries = log.lock().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, "error");
        assert_eq!(entries[0].status_code, Some(429));
        assert!(entries[0].response_body.as_deref().unwrap().contains("429"));
        assert!(
            !entries[0]
                .response_body
                .as_deref()
                .unwrap()
                .contains("abcdefghijklmnopqrstuvwxyz123456")
        );
    }

    #[test]
    fn separate_calls_are_separate_entries() {
        let log = new_request_log();
        let client = RecordingChatClient::new(
            Arc::new(Fake {
                answer: "ok".into(),
                fail: false,
            }),
            ExchangeKind::Chat,
            log.clone(),
            "https://api.deepseek.com",
            "deepseek-chat",
        );
        client.complete(&request()).unwrap();
        client.complete(&request()).unwrap();
        assert_eq!(log.lock().unwrap().len(), 2);
    }
}
