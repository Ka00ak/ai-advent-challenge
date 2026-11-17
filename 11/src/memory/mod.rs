//! Логика памяти: слои, извлечение, router, retrieval, prompt, миграция.

use crate::domain::memory::MemoryWriteEvent;

pub mod audit;
pub mod candidates;
pub mod extractor;
pub mod ltm;
pub mod migration;
pub mod prompt;
pub mod retrieval;
pub mod router;
pub mod stm;
pub mod tokens;
pub mod tool_write;
pub mod working;

/// Сессионный журнал аудита: лимит 200 событий, экспорт JSON.
#[derive(Debug, Clone)]
pub struct SessionAudit {
    pub events: Vec<MemoryWriteEvent>,
    pub limit: usize,
}

impl Default for SessionAudit {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            limit: 200,
        }
    }
}

impl SessionAudit {
    pub fn push(&mut self, event: MemoryWriteEvent) {
        self.events.push(event);
        if self.events.len() > self.limit {
            let excess = self.events.len() - self.limit;
            self.events.drain(0..excess);
        }
    }

    pub fn extend(&mut self, events: impl IntoIterator<Item = MemoryWriteEvent>) {
        for event in events {
            self.push(event);
        }
    }

    pub fn recent(&self, count: usize) -> &[MemoryWriteEvent] {
        let start = self.events.len().saturating_sub(count);
        &self.events[start..]
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::{MemoryLayer, MemorySource, WriteOp};

    fn event() -> MemoryWriteEvent {
        crate::memory::audit::make_event(
            MemoryLayer::Ltm,
            WriteOp::Create,
            "k",
            Some("v"),
            MemorySource::Router,
            Some("R4"),
            "reason",
            Some(1),
            chrono::Utc::now(),
        )
    }

    #[test]
    fn audit_is_limited() {
        let mut audit = SessionAudit::default();
        for _ in 0..250 {
            audit.push(event());
        }
        assert_eq!(audit.events.len(), 200);
    }

    #[test]
    fn audit_exports_json() {
        let mut audit = SessionAudit::default();
        audit.push(event());
        let json = audit.to_json().unwrap();
        let parsed: Vec<MemoryWriteEvent> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
    }
}
