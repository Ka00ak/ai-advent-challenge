//! Типы ответа хода, токенов и API-журнала.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::memory::{ChatMessage, MemoryBreakdown};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct TokenStats {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub cost_usd: f64,
    /// true, если prompt_tokens получены из `usage`, а не эвристикой.
    pub reported: bool,
}

impl TokenStats {
    pub fn finish_cost(&mut self, input_per_million: f64, output_per_million: f64) {
        self.cost_usd = (self.prompt_tokens as f64 / 1_000_000.0) * input_per_million
            + (self.completion_tokens as f64 / 1_000_000.0) * output_per_million;
        self.total_tokens = self.prompt_tokens + self.completion_tokens;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExchangeKind {
    Chat,
    Memory,
    Summary,
    Experiment,
}

impl ExchangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ExchangeKind::Chat => "chat",
            ExchangeKind::Memory => "memory",
            ExchangeKind::Summary => "summary",
            ExchangeKind::Experiment => "experiment",
        }
    }
}

/// Запись API-журнала сессии. Секреты в журнал не попадают.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmExchange {
    pub id: String,
    pub at: DateTime<Utc>,
    pub kind: ExchangeKind,
    pub model: String,
    pub url: String,
    pub duration_ms: u64,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub cost_usd: f64,
    pub status: String,
    pub error: Option<String>,
    pub memory: Option<MemoryBreakdown>,
    pub request_preview: Option<String>,
    pub response_preview: Option<String>,
}

impl LlmExchange {
    pub fn new(kind: ExchangeKind, model: &str, url: &str) -> Self {
        Self {
            id: format!("ex_{}", uuid::Uuid::new_v4().simple()),
            at: Utc::now(),
            kind,
            model: model.to_string(),
            url: url.to_string(),
            duration_ms: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
            status: "ok".to_string(),
            error: None,
            memory: None,
            request_preview: None,
            response_preview: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageSummary {
    pub requests: usize,
    pub chat_requests: usize,
    pub memory_requests: usize,
    pub experiment_requests: usize,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub cost_usd: f64,
}

pub fn summarize_exchanges(exchanges: &[LlmExchange]) -> UsageSummary {
    let mut summary = UsageSummary::default();
    for exchange in exchanges {
        summary.requests += 1;
        match exchange.kind {
            ExchangeKind::Chat => summary.chat_requests += 1,
            ExchangeKind::Memory | ExchangeKind::Summary => summary.memory_requests += 1,
            ExchangeKind::Experiment => summary.experiment_requests += 1,
        }
        summary.prompt_tokens += exchange.prompt_tokens;
        summary.completion_tokens += exchange.completion_tokens;
        summary.total_tokens += exchange.total_tokens;
        summary.cost_usd += exchange.cost_usd;
    }
    summary
}

/// Результат одного хода агента.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub final_text: String,
    pub memory: crate::domain::memory::MemoryBundle,
    pub messages: Vec<ChatMessage>,
    pub breakdown: MemoryBreakdown,
    pub token_stats: TokenStats,
    pub audit: Vec<crate::domain::memory::MemoryWriteEvent>,
    /// Активный профиль, с которым собран запрос.
    pub profile_id: Option<String>,
    /// Изменения профиля, извлечённые из текущей реплики.
    pub profile_changes: Vec<crate::memory::personalization::ProfileChange>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::{MemorySource, MemoryWriteEvent, WriteOp};

    fn event(kind: ExchangeKind, prompt: usize, completion: usize) -> LlmExchange {
        let mut exchange = LlmExchange::new(kind, "deepseek-chat", "https://api.deepseek.com");
        exchange.prompt_tokens = prompt;
        exchange.completion_tokens = completion;
        exchange.total_tokens = prompt + completion;
        exchange
    }

    #[test]
    fn token_stats_cost() {
        let mut stats = TokenStats {
            prompt_tokens: 1_000_000,
            completion_tokens: 1_000_000,
            ..TokenStats::default()
        };
        stats.finish_cost(0.28, 0.42);
        assert!((stats.cost_usd - 0.7).abs() < 1e-9);
        assert_eq!(stats.total_tokens, 2_000_000);
    }

    #[test]
    fn usage_summary_groups_kinds() {
        let exchanges = vec![
            event(ExchangeKind::Chat, 10, 5),
            event(ExchangeKind::Memory, 20, 3),
            event(ExchangeKind::Experiment, 30, 1),
            event(ExchangeKind::Summary, 8, 2),
        ];
        let summary = summarize_exchanges(&exchanges);
        assert_eq!(summary.requests, 4);
        assert_eq!(summary.chat_requests, 1);
        assert_eq!(summary.memory_requests, 2);
        assert_eq!(summary.experiment_requests, 1);
        assert_eq!(summary.prompt_tokens, 68);
        assert_eq!(summary.completion_tokens, 11);
        assert_eq!(summary.total_tokens, 79);
    }

    #[test]
    fn write_event_serde_roundtrip() {
        let event = MemoryWriteEvent {
            id: "evt_1".into(),
            at: Utc::now(),
            layer: crate::domain::memory::MemoryLayer::Ltm,
            op: WriteOp::Create,
            key: "project.name".into(),
            value: Some("«Атлас»".into()),
            source: MemorySource::Router,
            rule_id: Some("R4".into()),
            reason: "устойчивый факт".into(),
            message_index: Some(2),
        };
        let json = serde_json::to_string(&event).unwrap();
        let parsed: MemoryWriteEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(event, parsed);
        assert!(json.contains("\"layer\":\"ltm\""));
        assert!(json.contains("\"op\":\"create\""));
    }
}
