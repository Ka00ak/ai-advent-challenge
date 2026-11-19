//! Эвристическая оценка токенов (порт Дня 10).
//!
//! Точные значения приходят из `usage`; здесь — оценка, чтобы видеть порядок
//! величин и заранее ловить переполнение. Кириллица в BPE дороже латиницы.

use crate::domain::memory::ChatMessage;
use serde::Serialize;

/// Оценка токенов строки. Для пустой строки — 0.
pub fn estimate_text_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }

    let mut cyrillic = 0.0f64;
    let mut latin = 0.0f64;
    let mut punctuation = 0.0f64;
    let mut whitespace = 0.0f64;
    let mut wide = 0.0f64;

    for ch in text.chars() {
        if ch.is_whitespace() {
            whitespace += 1.0;
        } else if ('\u{0400}'..='\u{052f}').contains(&ch) {
            cyrillic += 1.0;
        } else if ch.is_ascii_alphanumeric() {
            latin += 1.0;
        } else if (ch as u32) > 0xffff {
            wide += 1.0;
        } else {
            punctuation += 1.0;
        }
    }

    let tokens = cyrillic / 2.0 + latin / 3.8 + punctuation / 2.5 + whitespace / 14.0 + wide * 2.0;
    tokens.ceil().max(1.0) as usize
}

/// Оценка сообщения вместе со служебными полями формата API.
pub fn estimate_message_tokens(message: &ChatMessage) -> usize {
    let content = message.content.as_deref().unwrap_or("");
    let mut tokens = 4 + estimate_text_tokens(content);

    if let Some(name) = &message.name {
        tokens += estimate_text_tokens(name) + 1;
    }
    if let Some(call_id) = &message.tool_call_id {
        tokens += estimate_text_tokens(call_id) + 3;
    }
    for call in &message.tool_calls {
        tokens += 8;
        tokens += estimate_text_tokens(&call.id);
        tokens += estimate_text_tokens(&call.name);
        tokens += estimate_text_tokens(&call.arguments);
    }
    tokens
}

pub fn estimate_messages_tokens(messages: &[ChatMessage]) -> usize {
    messages.iter().map(estimate_message_tokens).sum()
}

/// Схемы инструментов тоже занимают контекст.
pub fn estimate_tools_tokens<T: Serialize>(tools: &T) -> usize {
    match serde_json::to_string(tools) {
        Ok(json) if !json.is_empty() && json != "[]" => estimate_text_tokens(&json),
        _ => 0,
    }
}

pub fn format_tokens(tokens: usize) -> String {
    let raw = tokens.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    for (index, ch) in raw.chars().enumerate() {
        if index > 0 && (raw.len() - index) % 3 == 0 {
            out.push('\u{202f}');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::ChatRole;

    #[test]
    fn empty_is_zero() {
        assert_eq!(estimate_text_tokens(""), 0);
        assert_eq!(estimate_text_tokens("a"), 1);
    }

    #[test]
    fn cyrillic_costs_more_than_latin() {
        let ru = estimate_text_tokens("привет мир");
        let en = estimate_text_tokens("hello world");
        assert!(ru > en, "ru={ru}, en={en}");
    }

    #[test]
    fn whitespace_is_cheap() {
        let dense = estimate_text_tokens("абвгдежзий");
        let spaced = estimate_text_tokens("абвг дежзий");
        assert!(spaced >= dense);
    }

    #[test]
    fn message_overhead_is_counted() {
        let plain = ChatMessage {
            role: ChatRole::User,
            content: Some("привет".into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
            name: None,
        };
        let with_call = ChatMessage {
            role: ChatRole::Assistant,
            content: None,
            tool_calls: vec![crate::domain::memory::ToolCall {
                id: "call_1".into(),
                name: "calculate".into(),
                arguments: "{\"expression\":\"2+2\"}".into(),
            }],
            tool_call_id: None,
            name: None,
        };
        assert!(estimate_message_tokens(&plain) >= 4);
        assert!(estimate_message_tokens(&with_call) > 8);
    }

    #[test]
    fn messages_sum() {
        let messages = vec![ChatMessage::user("раз"), ChatMessage::assistant("два")];
        let total = estimate_messages_tokens(&messages);
        assert_eq!(
            total,
            estimate_message_tokens(&messages[0]) + estimate_message_tokens(&messages[1])
        );
    }

    #[test]
    fn empty_tools_are_zero() {
        let empty: Vec<serde_json::Value> = Vec::new();
        assert_eq!(estimate_tools_tokens(&empty), 0);
        let tools = vec![serde_json::json!({"name": "calculator"})];
        assert!(estimate_tools_tokens(&tools) > 0);
    }

    #[test]
    fn formatting_uses_thin_spaces() {
        assert_eq!(format_tokens(1234567), "1\u{202f}234\u{202f}567");
        assert_eq!(format_tokens(12), "12");
    }
}
