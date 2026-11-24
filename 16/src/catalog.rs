//! Единый каталог инструментов: локальные из Дня 6 + инструменты MCP.
//! Именно этот каталог отдаёт схемы модели и исполняет её выбор.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::llm::{ToolSchema, ToolSource};
use crate::mcp::McpClient;
use crate::tools::LocalToolRegistry;

#[derive(Clone)]
pub struct ToolCatalog {
    local: LocalToolRegistry,
    mcp: Option<Arc<McpClient>>,
    schemas: Vec<ToolSchema>,
}

impl ToolCatalog {
    pub fn local_only() -> Self {
        let local = LocalToolRegistry::day6();
        let schemas = local.schemas();
        Self {
            local,
            mcp: None,
            schemas,
        }
    }

    pub fn with_mcp(local: LocalToolRegistry, mcp: Arc<McpClient>) -> Self {
        let mut schemas = local.schemas();
        let local_names: Vec<String> = schemas.iter().map(|tool| tool.name.clone()).collect();
        for schema in mcp.schemas() {
            if !local_names.contains(&schema.name) {
                schemas.push(schema);
            }
        }
        Self {
            local,
            mcp: Some(mcp),
            schemas,
        }
    }

    pub fn from_local(local: LocalToolRegistry) -> Self {
        let schemas = local.schemas();
        Self {
            local,
            mcp: None,
            schemas,
        }
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.schemas.clone()
    }

    pub fn mcp(&self) -> Option<&Arc<McpClient>> {
        self.mcp.as_ref()
    }

    pub fn local_count(&self) -> usize {
        self.schemas
            .iter()
            .filter(|tool| tool.source == ToolSource::Local)
            .count()
    }

    pub fn mcp_count(&self) -> usize {
        self.schemas
            .iter()
            .filter(|tool| tool.source == ToolSource::Mcp)
            .count()
    }

    pub fn source(&self, name: &str) -> Option<ToolSource> {
        self.schemas
            .iter()
            .find(|tool| tool.name == name)
            .map(|tool| tool.source)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.schemas.iter().any(|tool| tool.name == name)
    }

    /// Исполняет инструмент. Ошибки не прерывают tool-loop: они возвращаются
    /// модели как результат инструмента, чтобы она могла исправиться.
    pub fn execute(&self, name: &str, arguments: &Value) -> Value {
        if self.local.contains(name) {
            return match self.local.execute(name, arguments) {
                Ok(value) => value,
                Err(error) => json!({"error": error}),
            };
        }
        if let Some(mcp) = &self.mcp
            && mcp.has_tool(name)
        {
            return match mcp.call_tool(name, arguments) {
                Ok(value) => value,
                Err(error) => json!({"error": error.to_string()}),
            };
        }
        json!({"error": format!("неизвестный инструмент «{name}»")})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_catalog_contains_day6_tools() {
        let catalog = ToolCatalog::local_only();
        let names: Vec<String> = catalog
            .schemas()
            .into_iter()
            .map(|schema| schema.name)
            .collect();
        assert_eq!(
            names,
            vec!["calculate", "convert_currency", "get_current_time"]
        );
        assert_eq!(catalog.local_count(), 3);
        assert_eq!(catalog.mcp_count(), 0);
    }

    #[test]
    fn local_tool_executes_through_catalog() {
        let catalog = ToolCatalog::local_only();
        let result = catalog.execute("calculate", &json!({"expression": "6*7"}));
        assert_eq!(result["result"], 42.0);
        assert_eq!(
            catalog.source("calculate"),
            Some(crate::llm::ToolSource::Local)
        );
    }

    #[test]
    fn unknown_tool_returns_error_json() {
        let catalog = ToolCatalog::local_only();
        let result = catalog.execute("nope", &json!({}));
        assert!(result["error"].is_string());
    }
}
