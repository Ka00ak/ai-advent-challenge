//! Сообщения диалога, вызовы инструментов и журнал запросов к модели.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Аргументы приходят от модели строкой JSON — ровно как в OpenAI API.
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self::plain(Role::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::plain(Role::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::plain(Role::Assistant, content)
    }

    fn plain(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
        }
    }

    pub fn assistant_tool_calls(content: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content,
            tool_calls,
            tool_call_id: None,
            name: None,
        }
    }

    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: Some(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
            name: Some(name.into()),
        }
    }

    /// Текст сообщения; у assistant с tool_calls контент может быть пустым.
    pub fn text(&self) -> &str {
        self.content.as_deref().unwrap_or("")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

/// Один HTTP-обмен с Chat Completions. `request` — точное тело запроса,
/// включая все схемы инструментов; его можно распечатать или открыть в TUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Exchange {
    pub id: String,
    pub iteration: usize,
    pub started_at: String,
    pub duration_ms: u128,
    pub request: serde_json::Value,
    pub response: Option<serde_json::Value>,
    pub error: Option<String>,
    pub usage: Usage,
}

impl Exchange {
    pub fn status(&self) -> &'static str {
        if self.error.is_some() { "error" } else { "ok" }
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.request
            .get("tools")
            .and_then(|tools| tools.as_array())
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| {
                        tool.get("function")
                            .and_then(|function| function.get("name"))
                            .and_then(|name| name.as_str())
                            .map(str::to_string)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_message_serializes_openai_shape() {
        let message = ChatMessage::tool_result("call_1", "calculate", "{\"result\":4}");
        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["role"], "tool");
        assert_eq!(json["tool_call_id"], "call_1");
        assert_eq!(json["name"], "calculate");
        assert_eq!(json["content"], "{\"result\":4}");
    }

    #[test]
    fn assistant_with_calls_can_have_no_content() {
        let message = ChatMessage::assistant_tool_calls(
            None,
            vec![ToolCall {
                id: "call_1".into(),
                name: "calculate".into(),
                arguments: "{}".into(),
            }],
        );
        assert!(message.content.is_none());
        assert_eq!(message.tool_calls.len(), 1);
    }
}
