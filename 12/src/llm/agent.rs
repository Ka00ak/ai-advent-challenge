//! Tool-loop агента. Промпт собирается один раз за turn (ADR-6);
//! `memory_write` перехватывается до registry.

use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Utc;
use serde_json::{Value, json};
use thiserror::Error;

use crate::domain::chat::{TokenStats, TurnOutcome};
use crate::domain::memory::{
    ChatMessage, LayerFlags, MemoryBreakdown, MemoryBundle, MemorySource, ToolCall,
};
use crate::domain::profile::UserProfile;
use crate::domain::settings::Settings;
use crate::llm::dto::{ChatClient, ChatRequest, model_profile};
use crate::llm::tools::ToolRegistry;
use crate::memory::prompt::{AssembleInput, PromptError, SYSTEM_PROMPT, assemble_prompt};
use crate::memory::tokens::{
    estimate_messages_tokens, estimate_text_tokens, estimate_tools_tokens,
};
use crate::memory::tool_write::{MemoryWriteRequest, apply_memory_write};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    Assembling,
    Request(usize),
    ToolCall(String),
    Finished,
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("ошибка модели: {0}")]
    Llm(String),
    #[error(transparent)]
    Prompt(#[from] PromptError),
    #[error("достигнут лимит итераций tool-loop")]
    Iterations,
    #[error("запрос отменён")]
    Cancelled,
}

pub struct TurnInput<'a> {
    pub user_text: &'a str,
    pub memory: MemoryBundle,
    pub settings: &'a Settings,
    pub tools: &'a ToolRegistry,
    pub client: &'a dyn ChatClient,
    pub cancel: &'a AtomicBool,
    pub profile: Option<&'a UserProfile>,
}

pub fn run_turn(
    input: TurnInput<'_>,
    progress: &mut dyn FnMut(Progress),
) -> Result<TurnOutcome, AgentError> {
    if input.cancel.load(Ordering::Relaxed) {
        return Err(AgentError::Cancelled);
    }
    let mut memory = input.memory.clone();
    let now = Utc::now();
    progress(Progress::Assembling);
    let schemas = input.tools.schemas();
    let assembly = assemble_prompt(AssembleInput {
        system_prompt: SYSTEM_PROMPT,
        stm: &memory.stm,
        wm: &memory.wm,
        ltm: &memory.ltm,
        config: &input.settings.memory,
        tools: &schemas,
        layers: LayerFlags::ALL,
        profile: input.profile,
        now,
    })?;
    let breakdown = assembly.breakdown.clone();
    let mut messages = assembly.messages;
    let mut audit = Vec::new();
    let (input_cost, output_cost) = model_profile(&input.settings.model);
    let mut token_stats = TokenStats::default();
    let mut usage_reported = false;

    for iteration in 0..input.settings.max_iterations.max(1) {
        if input.cancel.load(Ordering::Relaxed) {
            return Err(AgentError::Cancelled);
        }
        progress(Progress::Request(iteration + 1));
        let request = ChatRequest {
            model: input.settings.model.clone(),
            messages: messages.clone(),
            temperature: input.settings.temperature,
            max_tokens: input.settings.max_tokens,
            tools: schemas.clone(),
        };
        let estimated_prompt =
            estimate_messages_tokens(&request.messages) + estimate_tools_tokens(&request.tools);
        let response = input
            .client
            .complete(&request)
            .map_err(|error| AgentError::Llm(error.to_string()))?;

        if let Some(usage) = response.usage {
            token_stats.prompt_tokens += usage.prompt_tokens;
            token_stats.completion_tokens += usage.completion_tokens;
            usage_reported = true;
        } else {
            token_stats.prompt_tokens += estimated_prompt;
            token_stats.completion_tokens +=
                estimate_text_tokens(response.content.as_deref().unwrap_or(""));
        }

        if response.tool_calls.is_empty() {
            let final_text = response.content.unwrap_or_default();
            let assistant = ChatMessage::assistant(final_text.clone());
            messages.push(assistant.clone());
            memory.stm.messages.push(assistant);
            memory.stm.updated_at = Utc::now();
            memory.meta.processed_messages = memory.stm.messages.len();
            memory.meta.last_route_at = Some(Utc::now());
            token_stats.reported = usage_reported;
            token_stats.finish_cost(input_cost, output_cost);
            progress(Progress::Finished);
            return Ok(TurnOutcome {
                final_text,
                memory,
                messages,
                breakdown,
                token_stats,
                audit,
                profile_id: input.profile.map(|profile| profile.id.clone()),
                profile_changes: Vec::new(),
            });
        }

        let assistant = ChatMessage {
            role: crate::domain::memory::ChatRole::Assistant,
            content: response.content.clone(),
            tool_calls: response.tool_calls.clone(),
            tool_call_id: None,
            name: None,
        };
        messages.push(assistant.clone());
        memory.stm.messages.push(assistant);
        for call in response.tool_calls {
            progress(Progress::ToolCall(call.name.clone()));
            let result = execute_tool(&mut memory, &call, &input, now, &mut audit);
            let tool_message =
                ChatMessage::tool_result(call.id.clone(), call.name.clone(), result.to_string());
            messages.push(tool_message.clone());
            memory.stm.messages.push(tool_message);
        }
    }

    memory.stm.updated_at = Utc::now();
    memory.meta.processed_messages = memory.stm.messages.len();
    token_stats.reported = usage_reported;
    token_stats.finish_cost(input_cost, output_cost);
    let _ = input.user_text;
    Err(AgentError::Iterations)
}

fn execute_tool(
    memory: &mut MemoryBundle,
    call: &ToolCall,
    input: &TurnInput<'_>,
    now: chrono::DateTime<Utc>,
    audit: &mut Vec<crate::domain::memory::MemoryWriteEvent>,
) -> Value {
    if call.name == "memory_write" {
        let args: Value = serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!({}));
        let request = MemoryWriteRequest::from_json(&args);
        let (outcome, event) = apply_memory_write(
            memory,
            &request,
            MemorySource::Agent,
            input.settings.memory.approval,
            now,
        );
        if let Some(event) = event {
            audit.push(event);
        }
        return outcome.to_tool_json();
    }
    let args: Value = serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!({}));
    match input.tools.execute(&call.name, &args) {
        Ok(value) => value,
        Err(error) => json!({"error": error}),
    }
}

/// Разбивка для CLI/TUI без выполнения запроса.
pub fn estimate_breakdown(
    memory: &MemoryBundle,
    settings: &Settings,
    tools: &ToolRegistry,
    profile: Option<&UserProfile>,
) -> MemoryBreakdown {
    let schemas = tools.schemas();
    let result = assemble_prompt(AssembleInput {
        system_prompt: SYSTEM_PROMPT,
        stm: &memory.stm,
        wm: &memory.wm,
        ltm: &memory.ltm,
        config: &settings.memory,
        tools: &schemas,
        layers: LayerFlags::ALL,
        profile,
        now: Utc::now(),
    });
    match result {
        Ok(output) => output.breakdown,
        Err(_) => MemoryBreakdown::default(),
    }
}

pub fn estimate_message_cost(messages: &[ChatMessage], content: &str) -> TokenStats {
    let mut stats = TokenStats {
        prompt_tokens: estimate_messages_tokens(messages),
        completion_tokens: estimate_text_tokens(content),
        ..TokenStats::default()
    };
    stats.total_tokens = stats.prompt_tokens + stats.completion_tokens;
    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::MemoryLayer;
    use crate::llm::dto::{ChatResponse, LlmError};
    use std::sync::Mutex;

    struct Scripted {
        responses: Mutex<Vec<ChatResponse>>,
        calls: Mutex<Vec<ChatRequest>>,
    }

    impl ChatClient for Scripted {
        fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
            self.calls.lock().unwrap().push(request.clone());
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Ok(ChatResponse {
                    content: Some("готово".into()),
                    tool_calls: Vec::new(),
                    usage: None,
                })
            } else {
                Ok(responses.remove(0))
            }
        }
    }

    fn memory_with_question() -> MemoryBundle {
        let now = Utc::now();
        let mut memory = MemoryBundle::empty(now);
        memory
            .stm
            .messages
            .push(ChatMessage::user("Запомни: проект «Атлас»"));
        memory
    }

    fn settings() -> Settings {
        Settings {
            max_iterations: 4,
            ..Settings::default()
        }
    }

    #[test]
    fn tool_loop_executes_tool_then_final() {
        let scripted = Scripted {
            responses: Mutex::new(vec![
                ChatResponse {
                    content: None,
                    tool_calls: vec![ToolCall {
                        id: "call_1".into(),
                        name: "calculate".into(),
                        arguments: "{\"expression\":\"2+2\"}".into(),
                    }],
                    usage: None,
                },
                ChatResponse {
                    content: Some("Ответ: 4".into()),
                    tool_calls: Vec::new(),
                    usage: None,
                },
            ]),
            calls: Mutex::new(Vec::new()),
        };
        let tools = ToolRegistry::default_registry();
        let cancel = AtomicBool::new(false);
        let mut progress = Vec::new();
        let outcome = run_turn(
            TurnInput {
                user_text: "Сколько 2+2?",
                memory: memory_with_question(),
                settings: &settings(),
                tools: &tools,
                client: &scripted,
                cancel: &cancel,
                profile: None,
            },
            &mut |event| progress.push(event),
        )
        .unwrap();
        assert_eq!(outcome.final_text, "Ответ: 4");
        assert_eq!(scripted.calls.lock().unwrap().len(), 2);
        assert!(
            outcome
                .messages
                .iter()
                .any(|m| m.role == crate::domain::memory::ChatRole::Tool)
        );
        assert!(progress.contains(&Progress::ToolCall("calculate".into())));
    }

    #[test]
    fn memory_write_in_tool_loop_creates_pending() {
        let scripted = Scripted {
            responses: Mutex::new(vec![
                ChatResponse {
                    content: None,
                    tool_calls: vec![ToolCall {
                        id: "call_1".into(),
                        name: "memory_write".into(),
                        arguments: "{\"layer\":\"ltm\",\"key\":\"project.name\",\"value\":\"«Атлас»\",\"kind\":\"project\"}".into(),
                    }],
                    usage: None,
                },
                ChatResponse {
                    content: Some("Запомнил.".into()),
                    tool_calls: Vec::new(),
                    usage: None,
                },
            ]),
            calls: Mutex::new(Vec::new()),
        };
        let tools = ToolRegistry::default_registry();
        let cancel = AtomicBool::new(false);
        let outcome = run_turn(
            TurnInput {
                user_text: "Запомни",
                memory: memory_with_question(),
                settings: &settings(),
                tools: &tools,
                client: &scripted,
                cancel: &cancel,
                profile: None,
            },
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(outcome.memory.pending.len(), 1);
        assert_eq!(outcome.audit[0].layer, MemoryLayer::Ltm);
        assert!(outcome.memory.ltm.records.is_empty());
    }

    #[test]
    fn cancel_stops_before_request() {
        let scripted = Scripted {
            responses: Mutex::new(Vec::new()),
            calls: Mutex::new(Vec::new()),
        };
        let tools = ToolRegistry::default_registry();
        let cancel = AtomicBool::new(true);
        let error = run_turn(
            TurnInput {
                user_text: "привет",
                memory: memory_with_question(),
                settings: &settings(),
                tools: &tools,
                client: &scripted,
                cancel: &cancel,
                profile: None,
            },
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(error, AgentError::Cancelled));
        assert!(scripted.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn token_stats_are_filled() {
        let mut memory = memory_with_question();
        memory.stm.messages.push(ChatMessage::assistant("ответ"));
        memory.stm.messages.push(ChatMessage::user("вопрос"));
        let scripted = Scripted {
            responses: Mutex::new(vec![ChatResponse {
                content: Some("ответ".into()),
                tool_calls: Vec::new(),
                usage: Some(crate::llm::dto::Usage {
                    prompt_tokens: 100,
                    completion_tokens: 10,
                    total_tokens: 110,
                }),
            }]),
            calls: Mutex::new(Vec::new()),
        };
        let tools = ToolRegistry::default_registry();
        let cancel = AtomicBool::new(false);
        let outcome = run_turn(
            TurnInput {
                user_text: "вопрос",
                memory,
                settings: &settings(),
                tools: &tools,
                client: &scripted,
                cancel: &cancel,
                profile: None,
            },
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(outcome.token_stats.prompt_tokens, 100);
        assert_eq!(outcome.token_stats.completion_tokens, 10);
        assert!(outcome.token_stats.reported);
        assert!(outcome.token_stats.cost_usd > 0.0);
    }
}
