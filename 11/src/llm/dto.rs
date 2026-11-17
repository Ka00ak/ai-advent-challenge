//! OpenAI-совместимые DTO для Chat Completions.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::memory::{ChatMessage, ToolCall};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("API-ключ не задан (нужен DEEPSEEK_API_KEY или KIMI_API_KEY)")]
    MissingKey,
    #[error("HTTP-ошибка: {0}")]
    Http(String),
    #[error("API вернул статус {status}: {body}")]
    Status { status: u16, body: String },
    #[error("некорректный ответ API: {0}")]
    Json(String),
    #[error("запрос отменён")]
    Cancelled,
}

pub trait ChatClient: Send + Sync {
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError>;
}

/// Идентификатор провайдера по модели — для оценки стоимости.
pub fn model_profile(model: &str) -> (f64, f64) {
    let lower = model.to_ascii_lowercase();
    if lower.contains("deepseek") {
        (0.28, 0.42)
    } else if lower.contains("kimi") || lower.contains("moonshot") {
        (0.6, 2.5)
    } else {
        (0.5, 1.5)
    }
}
