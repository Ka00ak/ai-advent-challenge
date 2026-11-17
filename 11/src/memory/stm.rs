//! STM: окно истории для промпта и сводка вытесненной части.
//!
//! Полная история в файле не мутирует: окно — только параметр запроса.

use crate::domain::memory::{ChatRole, StmState, StmWindow};
use crate::memory::tokens::{estimate_message_tokens, estimate_text_tokens};

/// Выбирает последние `window` сообщений **до текущей реплики**.
///
/// Граница сдвигается к ближайшему `User`, чтобы блок не начинался с
/// `Assistant`/`Tool`; пара `Assistant(tool_calls) → Tool` не рвётся.
pub fn select_stm_window(stm: &StmState, window: usize, max_tokens: usize) -> StmWindow {
    let total = stm.messages.len();
    if total <= 1 || window == 0 {
        return StmWindow::default();
    }
    let end = total - 1; // текущая реплика не входит в окно
    let mut start = end.saturating_sub(window);
    while start < end && stm.messages[start].role != ChatRole::User {
        start += 1;
    }

    let mut tokens: usize = stm.messages[start..end]
        .iter()
        .map(estimate_message_tokens)
        .sum();
    let mut dropped_pairs = 0;
    while tokens > max_tokens && start < end {
        start += 1;
        while start < end && stm.messages[start].role != ChatRole::User {
            start += 1;
        }
        dropped_pairs += 1;
        tokens = stm.messages[start..end]
            .iter()
            .map(estimate_message_tokens)
            .sum();
    }

    StmWindow {
        indices: (start..end).collect(),
        tokens,
        dropped_pairs,
    }
}

pub fn stm_summary_tokens(stm: &StmState) -> usize {
    stm.summary
        .as_deref()
        .map(estimate_text_tokens)
        .unwrap_or(0)
}

/// Полная история для режима `full`: все сообщения до текущей реплики.
pub fn full_history_window(stm: &StmState) -> StmWindow {
    let end = stm.messages.len().saturating_sub(1);
    let indices: Vec<usize> = (0..end).collect();
    let tokens = indices
        .iter()
        .map(|index| estimate_message_tokens(&stm.messages[*index]))
        .sum();
    StmWindow {
        indices,
        tokens,
        dropped_pairs: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::{ChatMessage, ToolCall};

    fn stm_with(roles: &[(ChatRole, &str)]) -> StmState {
        let mut stm = StmState::empty(chrono::Utc::now());
        for (role, text) in roles {
            stm.messages.push(ChatMessage {
                role: *role,
                content: Some((*text).into()),
                tool_calls: Vec::new(),
                tool_call_id: None,
                name: None,
            });
        }
        stm
    }

    #[test]
    fn takes_last_window_of_history() {
        let stm = stm_with(&[
            (ChatRole::User, "1"),
            (ChatRole::Assistant, "2"),
            (ChatRole::User, "3"),
            (ChatRole::Assistant, "4"),
            (ChatRole::User, "5"),
            (ChatRole::Assistant, "6"),
            (ChatRole::User, "текущий"),
        ]);
        let window = select_stm_window(&stm, 4, 10_000);
        assert_eq!(window.indices, vec![2, 3, 4, 5]);
        assert_eq!(window.dropped_pairs, 0);
    }

    #[test]
    fn boundary_moves_to_user() {
        let stm = stm_with(&[
            (ChatRole::User, "1"),
            (ChatRole::Assistant, "2"),
            (ChatRole::User, "3"),
            (ChatRole::Assistant, "4"),
            (ChatRole::User, "5"),
            (ChatRole::Assistant, "6"),
            (ChatRole::User, "текущий"),
        ]);
        let window = select_stm_window(&stm, 3, 10_000);
        assert_eq!(stm.messages[window.indices[0]].role, ChatRole::User);
    }

    #[test]
    fn tool_pair_is_not_broken() {
        let mut stm = stm_with(&[
            (ChatRole::User, "1"),
            (ChatRole::Assistant, "2"),
            (ChatRole::User, "3"),
        ]);
        let mut assistant = ChatMessage {
            role: ChatRole::Assistant,
            content: None,
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "calculate".into(),
                arguments: "{}".into(),
            }],
            tool_call_id: None,
            name: None,
        };
        assistant.content = None;
        stm.messages.insert(2, assistant);
        stm.messages.insert(
            3,
            ChatMessage::tool_result("call_1", "calculate", "4".into()),
        );
        stm.messages.push(ChatMessage::user("текущий"));

        let window = select_stm_window(&stm, 2, 10_000);
        assert!(!window.indices.is_empty());
        let first = &stm.messages[window.indices[0]];
        assert_ne!(first.role, ChatRole::Tool);
    }

    #[test]
    fn long_window_drops_old_pairs_with_budget() {
        let mut stm = StmState::empty(chrono::Utc::now());
        for index in 0..20 {
            stm.messages.push(ChatMessage::user(format!(
                "длинная реплика номер {index} {}",
                "текст ".repeat(20)
            )));
            stm.messages.push(ChatMessage::assistant("ответ"));
        }
        stm.messages.push(ChatMessage::user("текущий"));
        let window = select_stm_window(&stm, 20, 100);
        assert!(window.tokens <= 100 || window.indices.is_empty());
        assert!(window.dropped_pairs > 0);
    }

    #[test]
    fn summary_tokens_only_for_non_empty() {
        let mut stm = StmState::empty(chrono::Utc::now());
        assert_eq!(stm_summary_tokens(&stm), 0);
        stm.summary = Some("сводка диалога".into());
        assert!(stm_summary_tokens(&stm) > 0);
    }

    #[test]
    fn full_history_excludes_current() {
        let stm = stm_with(&[
            (ChatRole::User, "1"),
            (ChatRole::Assistant, "2"),
            (ChatRole::User, "текущий"),
        ]);
        let window = full_history_window(&stm);
        assert_eq!(window.indices, vec![0, 1]);
    }
}
