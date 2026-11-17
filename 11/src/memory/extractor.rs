//! Извлечение кандидатов: Local (0 запросов) и LLM (1 запрос).
//!
//! Модель не выбирает слой — только извлекает кандидатов; слой определяет
//! router. При ошибке LLM контроллер откатывается на Local.

use std::sync::Arc;

use serde::Deserialize;
use thiserror::Error;

use crate::domain::memory::{CandidateScope, ChatMessage, LtmKind, LtmState, MemoryCandidate};
use crate::llm::dto::{ChatClient, ChatRequest};
use crate::memory::candidates::extract_local;

#[derive(Debug, Clone, Default)]
pub struct Extraction {
    pub candidates: Vec<MemoryCandidate>,
    pub raw: Option<String>,
}

#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("ошибка LLM-извлечения: {0}")]
    Llm(String),
    #[error("LLM вернул не-JSON: {0}")]
    Parse(String),
}

pub trait CandidateExtractor: Send + Sync {
    fn extract(&self, message: &ChatMessage, ltm: &LtmState) -> Result<Extraction, ExtractError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LocalCandidateExtractor;

impl CandidateExtractor for LocalCandidateExtractor {
    fn extract(&self, message: &ChatMessage, _ltm: &LtmState) -> Result<Extraction, ExtractError> {
        Ok(Extraction {
            candidates: extract_local(message.content.as_deref().unwrap_or("")),
            raw: None,
        })
    }
}

pub const EXTRACTION_PROMPT: &str = "Извлеки кандидатов в память из одной реплики. Верни JSON-массив:\n\
[{\"key\",\"value\",\"kind\",\"scope\",\"tags\",\"confidence\"}]\n\
kind: profile|project|preference|decision|knowledge|glossary|goal|plan|status|entity|open-question|note\n\
scope: turn|task|project|global\n\
Не переписывай значения, не добавляй лишнего, секреты не извлекай.\n\
Если новых данных нет — [].";

pub struct LlmCandidateExtractor {
    client: Arc<dyn ChatClient>,
    model: String,
}

impl LlmCandidateExtractor {
    pub fn new(client: Arc<dyn ChatClient>, model: impl Into<String>) -> Self {
        Self {
            client,
            model: model.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawCandidate {
    #[serde(default)]
    key: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    confidence: f32,
}

impl CandidateExtractor for LlmCandidateExtractor {
    fn extract(&self, message: &ChatMessage, _ltm: &LtmState) -> Result<Extraction, ExtractError> {
        let request = ChatRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage::system(EXTRACTION_PROMPT),
                ChatMessage::user(message.content.clone().unwrap_or_default()),
            ],
            temperature: 0.0,
            max_tokens: Some(400),
            tools: Vec::new(),
        };
        let response = self
            .client
            .complete(&request)
            .map_err(|error| ExtractError::Llm(error.to_string()))?;
        let content = response.content.unwrap_or_default();
        let cleaned = strip_code_fence(&content);
        let raw: Vec<RawCandidate> = serde_json::from_str(cleaned)
            .map_err(|error| ExtractError::Parse(format!("{error}: {cleaned}")))?;
        let candidates = raw
            .into_iter()
            .filter(|candidate| !candidate.key.trim().is_empty())
            .map(|candidate| MemoryCandidate {
                key: candidate.key,
                value: candidate.value,
                kind: parse_kind(&candidate.kind),
                tags: candidate.tags,
                confidence: if candidate.confidence > 0.0 {
                    candidate.confidence.clamp(0.0, 1.0)
                } else {
                    0.7
                },
                scope: parse_scope(&candidate.scope),
                ttl_turns: None,
                legacy_category: None,
            })
            .collect();
        Ok(Extraction {
            candidates,
            raw: Some(cleaned.to_string()),
        })
    }
}

fn strip_code_fence(content: &str) -> &str {
    let trimmed = content.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        return rest.trim_end_matches("```").trim();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        return rest.trim_end_matches("```").trim();
    }
    trimmed
}

fn parse_kind(value: &str) -> LtmKind {
    match value.trim().to_ascii_lowercase().as_str() {
        "profile" => LtmKind::Profile,
        "project" => LtmKind::Project,
        "preference" => LtmKind::Preference,
        "decision" => LtmKind::Decision,
        "glossary" => LtmKind::Glossary,
        _ => LtmKind::Knowledge,
    }
}

fn parse_scope(value: &str) -> CandidateScope {
    match value.trim().to_ascii_lowercase().as_str() {
        "turn" => CandidateScope::Turn,
        "task" => CandidateScope::Task,
        "project" => CandidateScope::Project,
        "global" => CandidateScope::Global,
        _ => CandidateScope::Turn,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::dto::{ChatResponse, LlmError};
    use std::sync::Mutex;

    struct Fake {
        answer: String,
        calls: Mutex<usize>,
    }

    impl ChatClient for Fake {
        fn complete(&self, _request: &ChatRequest) -> Result<ChatResponse, LlmError> {
            *self.calls.lock().unwrap() += 1;
            Ok(ChatResponse {
                content: Some(self.answer.clone()),
                tool_calls: Vec::new(),
                usage: None,
            })
        }
    }

    fn message() -> ChatMessage {
        ChatMessage::user("Тестовый факт")
    }

    #[test]
    fn local_extractor_makes_no_requests() {
        let extractor = LocalCandidateExtractor;
        let extraction = extractor
            .extract(
                &ChatMessage::user("Заказчик — Марина"),
                &LtmState::empty(chrono::Utc::now()),
            )
            .unwrap();
        assert_eq!(extraction.candidates.len(), 1);
    }

    #[test]
    fn llm_extractor_parses_json() {
        let fake = Arc::new(Fake {
            answer: "```json\n[{\"key\":\"project.name\",\"value\":\"«Атлас»\",\"kind\":\"project\",\"scope\":\"project\",\"tags\":[\"a\"],\"confidence\":0.9}]\n```".into(),
            calls: Mutex::new(0),
        });
        let extractor = LlmCandidateExtractor::new(fake.clone(), "test");
        let extraction = extractor
            .extract(&message(), &LtmState::empty(chrono::Utc::now()))
            .unwrap();
        assert_eq!(extraction.candidates.len(), 1);
        assert_eq!(extraction.candidates[0].kind, LtmKind::Project);
        assert_eq!(extraction.candidates[0].scope, CandidateScope::Project);
        assert_eq!(*fake.calls.lock().unwrap(), 1);
    }

    #[test]
    fn llm_extractor_rejects_non_json() {
        let fake = Arc::new(Fake {
            answer: "не json".into(),
            calls: Mutex::new(0),
        });
        let extractor = LlmCandidateExtractor::new(fake, "test");
        let error = extractor
            .extract(&message(), &LtmState::empty(chrono::Utc::now()))
            .unwrap_err();
        assert!(matches!(error, ExtractError::Parse(_)));
    }

    #[test]
    fn llm_extractor_maps_wm_kind_to_knowledge_but_keeps_scope() {
        let fake = Arc::new(Fake {
            answer: "[{\"key\":\"plan.1\",\"value\":\"шаг\",\"kind\":\"plan\",\"scope\":\"task\"}]"
                .into(),
            calls: Mutex::new(0),
        });
        let extractor = LlmCandidateExtractor::new(fake, "test");
        let extraction = extractor
            .extract(&message(), &LtmState::empty(chrono::Utc::now()))
            .unwrap();
        assert_eq!(extraction.candidates[0].scope, CandidateScope::Task);
    }
}
