//! Блокирующий HTTP-клиент Chat Completions (reqwest + rustls).
//!
//! Вызывается только из job-потока TUI или CLI, никогда в главном потоке TUI.

use std::time::Duration;

use serde_json::{Value, json};

use crate::domain::memory::{ChatRole, ToolCall};
use crate::llm::dto::{ChatClient, ChatRequest, ChatResponse, LlmError, Usage};

pub struct ReqwestClient {
    base_url: String,
    api_key: Option<String>,
    http: reqwest::blocking::Client,
}

impl ReqwestClient {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Result<Self, LlmError> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|error| LlmError::Http(error.to_string()))?;
        Ok(Self {
            base_url: base_url.into(),
            api_key,
            http,
        })
    }

    pub fn endpoint(&self) -> String {
        chat_endpoint(&self.base_url)
    }

    fn send(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let api_key = self
            .api_key
            .as_deref()
            .filter(|key| !key.trim().is_empty())
            .ok_or(LlmError::MissingKey)?;
        let body = build_body(request);
        let mut last_error = None;
        for attempt in 0..2 {
            let response = self
                .http
                .post(self.endpoint())
                .bearer_auth(api_key)
                .json(&body)
                .send();
            match response {
                Ok(response) => {
                    let status = response.status();
                    let text = response.text().unwrap_or_default();
                    if status.is_success() {
                        return parse_response(&text);
                    }
                    if (status.as_u16() == 429 || status.is_server_error()) && attempt == 0 {
                        std::thread::sleep(Duration::from_millis(300));
                        last_error = Some(LlmError::Status {
                            status: status.as_u16(),
                            body: truncate(&text, 400),
                        });
                        continue;
                    }
                    return Err(LlmError::Status {
                        status: status.as_u16(),
                        body: truncate(&text, 400),
                    });
                }
                Err(error) => {
                    if attempt == 0 {
                        std::thread::sleep(Duration::from_millis(300));
                        last_error = Some(LlmError::Http(error.to_string()));
                        continue;
                    }
                    return Err(LlmError::Http(error.to_string()));
                }
            }
        }
        Err(last_error.unwrap_or_else(|| LlmError::Http("неизвестная ошибка".into())))
    }
}

impl ChatClient for ReqwestClient {
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        self.send(request)
    }
}

/// Адрес Chat Completions для заданного base URL.
pub fn chat_endpoint(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    format!("{base}/chat/completions")
}

pub fn build_body(request: &ChatRequest) -> Value {
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|message| {
            let mut value = json!({
                "role": role_str(message.role),
                "content": message.content,
            });
            if !message.tool_calls.is_empty() {
                let calls: Vec<Value> = message
                    .tool_calls
                    .iter()
                    .map(|call| {
                        json!({
                            "id": call.id,
                            "type": "function",
                            "function": {"name": call.name, "arguments": call.arguments}
                        })
                    })
                    .collect();
                value["tool_calls"] = Value::Array(calls);
            }
            if let Some(call_id) = &message.tool_call_id {
                value["tool_call_id"] = json!(call_id);
            }
            if let Some(name) = &message.name {
                value["name"] = json!(name);
            }
            value
        })
        .collect();

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "temperature": request.temperature,
        "stream": false,
    });
    if let Some(max_tokens) = request.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    }
                })
            })
            .collect();
        body["tools"] = Value::Array(tools);
    }
    body
}

fn role_str(role: ChatRole) -> &'static str {
    match role {
        ChatRole::System => "system",
        ChatRole::User => "user",
        ChatRole::Assistant => "assistant",
        ChatRole::Tool => "tool",
    }
}

fn parse_response(text: &str) -> Result<ChatResponse, LlmError> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| LlmError::Json(error.to_string()))?;
    let message = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| LlmError::Json("в ответе нет choices[0].message".into()))?;
    let content = message
        .get("content")
        .and_then(|content| content.as_str())
        .map(str::to_string);
    let mut tool_calls = Vec::new();
    if let Some(calls) = message.get("tool_calls").and_then(|calls| calls.as_array()) {
        for call in calls {
            let id = call
                .get("id")
                .and_then(|id| id.as_str())
                .unwrap_or("call_unknown")
                .to_string();
            let function = call.get("function");
            let name = function
                .and_then(|f| f.get("name"))
                .and_then(|name| name.as_str())
                .unwrap_or_default()
                .to_string();
            let arguments = function
                .and_then(|f| f.get("arguments"))
                .and_then(|arguments| arguments.as_str())
                .unwrap_or("{}")
                .to_string();
            tool_calls.push(ToolCall {
                id,
                name,
                arguments,
            });
        }
    }
    let usage = value.get("usage").and_then(|usage| {
        Some(Usage {
            prompt_tokens: usage.get("prompt_tokens")?.as_u64()? as usize,
            completion_tokens: usage.get("completion_tokens")?.as_u64()? as usize,
            total_tokens: usage.get("total_tokens")?.as_u64().unwrap_or(0) as usize,
        })
    });
    Ok(ChatResponse {
        content,
        tool_calls,
        usage,
    })
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        text.chars().take(limit).collect::<String>() + "…"
    }
}

/// Детерминированный fake-клиент для тестов и `LAB11_FAKE_LLM=1`.
///
/// Если в промпте (кроме последней реплики) есть ключевая деталь контрольного
/// диалога, возвращается ожидаемый ответ; иначе «не знаю». Это проверяет
/// механику блоков, а не качество модели.
pub struct FakeChatClient;

impl ChatClient for FakeChatClient {
    fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let current = request
            .messages
            .last()
            .and_then(|message| message.content.clone())
            .unwrap_or_default();
        let context: String = request
            .messages
            .iter()
            .take(request.messages.len().saturating_sub(1))
            .map(|message| message.text())
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(stage) = detect_task_stage(&context) {
            return Ok(ChatResponse {
                content: Some(fake_task_answer(stage)),
                tool_calls: Vec::new(),
                usage: None,
            });
        }
        if let Some(style) = detect_profile_style(&context) {
            return Ok(ChatResponse {
                content: Some(fake_personalized_answer(&style)),
                tool_calls: Vec::new(),
                usage: None,
            });
        }
        let answer = fake_experiment_answer(&current, &context).unwrap_or_else(|| {
            if request.tools.is_empty() {
                "Не знаю.".to_string()
            } else {
                "Готово.".to_string()
            }
        });
        Ok(ChatResponse {
            content: Some(answer),
            tool_calls: Vec::new(),
            usage: None,
        })
    }
}

#[derive(Debug, Clone, Default)]
struct DetectedProfileStyle {
    verbosity: String,
    tone: String,
    code: String,
    lists: bool,
    tables: bool,
    emoji: bool,
}

fn detect_task_stage(context: &str) -> Option<&'static str> {
    let lower = context.to_lowercase();
    for stage in ["planning", "execution", "validation"] {
        if lower.contains(&format!("task_stage={stage}")) {
            return Some(stage);
        }
    }
    None
}

fn fake_task_answer(stage: &str) -> String {
    match stage {
        "planning" => {
            "1. Разобрать задачу пользователя\n2. Подготовить результат\n3. Проверить результат по плану"
                .to_string()
        }
        "execution" => {
            "Результат (fake): план выполнен, итоговый ответ подготовлен.".to_string()
        }
        "validation" => {
            "ВЕРДИКТ: OK\nПлан выполнен, результат соответствует задаче.".to_string()
        }
        _ => "Готово.".to_string(),
    }
}

fn detect_profile_style(context: &str) -> Option<DetectedProfileStyle> {
    if !context.contains("## Персонализация") {
        return None;
    }
    let mut style = DetectedProfileStyle::default();
    for line in context.lines() {
        let line = line.trim();
        let lower = line.to_lowercase();
        let value = lower
            .split_once(':')
            .map(|(_, value)| value.trim().to_string())
            .unwrap_or_default();
        if lower.starts_with("- объём:") {
            style.verbosity = value;
        } else if lower.starts_with("- тон:") {
            style.tone = value;
        } else if lower.starts_with("- код:") {
            style.code = value;
        } else if lower.starts_with("- списки:") {
            style.lists = value.starts_with("да") || value.starts_with("использовать");
        } else if lower.starts_with("- таблицы:") {
            style.tables = value.starts_with("да") || value.starts_with("использовать");
        } else if lower.starts_with("- эмодзи:") {
            style.emoji = value.starts_with("да") || value.starts_with("использовать");
        }
    }
    Some(style)
}

/// Детерминированный ответ, соблюдающий профиль. Нужен только для
/// offline-проверки механики персонализации; в отчёте такой прогон помечен
/// как fake.
fn fake_personalized_answer(style: &DetectedProfileStyle) -> String {
    let brief = style.verbosity.contains("кратко");
    let detailed = style.verbosity.contains("подробно");
    let formal = style.tone.contains("формальный");
    let informal = style.tone.contains("неформальный");
    let with_code = style.code.contains("с примерами") || style.code.contains("с кодом");
    let no_code = style.code.contains("без кода");

    if brief && no_code && informal && !style.lists && !style.tables {
        return "Слушай, просто прочитай файл через std::fs::read_to_string, потом разбери через serde_json::from_str::<Value>. Ошибки возвращай через Result, не паникуй. Коротко и по делу.".to_string();
    }

    if detailed && with_code && formal {
        let mut answer = "Для безопасного чтения JSON в Rust рекомендуется разделить ввод-вывод и разбор данных, а ошибки возвращать вызывающему коду через Result. Такой подход позволяет избежать паники и корректно обработать отсутствующий файл или некорректный JSON. Сначала файл читается функцией std::fs::read_to_string, затем содержимое разбирается через serde_json::from_str::<serde_json::Value>. Если файла нет или JSON повреждён, функция возвращает ошибку, а вызывающий код сам решает, как её показать пользователю.\n\n```rust\nuse std::fs;\nuse serde_json::Value;\n\nfn read_json(path: &str) -> Result<Value, Box<dyn std::error::Error>> {\n    let data = fs::read_to_string(path)?;\n    let value = serde_json::from_str(&data)?;\n    Ok(value)\n}\n```\n\nОграничения: для больших файлов используйте потоковый парсер, а для сетевых источников — таймауты. Такой вариант подходит для учебного примера и небольшого сервиса.".to_string();
        if style.tables {
            answer.push_str("\n\n| Шаг | Что проверяем |\n|---|---|\n| 1 | файл существует и доступен |\n| 2 | JSON синтаксически корректен |\n| 3 | ошибка обработана без паники |");
        }
        return answer;
    }

    let mut parts = Vec::new();
    if informal {
        parts.push("Слушай, тут всё просто: читай файл и возвращай Result.".to_string());
    } else if formal {
        parts.push(
            "Для решения этой задачи рекомендуется вернуть ошибку вызывающему коду.".to_string(),
        );
    } else {
        parts.push("Прочитайте файл и обработайте ошибку через Result.".to_string());
    }
    if detailed {
        parts.push("Сначала отделите ввод-вывод от разбора JSON, затем проверьте корректность данных и только после этого используйте результат. Явная обработка ошибок делает поведение программы предсказуемым и упрощает тестирование.".to_string());
    }
    if with_code {
        parts.push("```rust\nuse std::fs;\nfn read(path: &str) -> std::io::Result<String> {\n    fs::read_to_string(path)\n}\n```".to_string());
    } else if no_code {
        parts.push(
            "Код не привожу: используйте std::fs::read_to_string и serde_json::from_str."
                .to_string(),
        );
    }
    if style.lists {
        parts.push("1. Прочитайте файл.\n2. Разберите JSON.\n3. Обработайте ошибку.".to_string());
    }
    if style.tables {
        parts.push("| Шаг | Действие |\n|---|---|\n| 1 | чтение |\n| 2 | разбор |".to_string());
    }
    if style.emoji {
        parts.push("🙂".to_string());
    }
    parts.join("\n\n")
}

/// Ответы по 18 probe из `docs/07-verification.md`.
pub fn fake_experiment_answer(question: &str, context: &str) -> Option<String> {
    let q = question.to_lowercase();
    let evidence: &[(&str, &str, &str)] = &[
        ("до какой суммы подняли бюджет", "150 000", "150 000 рублей"),
        (
            "что решили насчёт инфлюенсеров",
            "инфлюенсер",
            "отдельной строкой, 25 000 рублей",
        ),
        ("когда созвон с заказчиком", "четверг", "в четверг в 11:00"),
        ("где лежит черновик сметы", "smeta-v3", "smeta-v3.xlsx"),
        (
            "когда можно отправлять смету",
            "пятниц",
            "не раньше пятницы",
        ),
        (
            "сколько заложить на инфлюенсеров",
            "25 000",
            "25 000 рублей",
        ),
        (
            "какая задача сейчас в работе",
            "смету маркетинга",
            "смета маркетинга к релизу",
        ),
        ("на каком шаге плана", "оценить каналы", "на шаге 2"),
        ("какой вариант сметы выбран", "вариант b", "вариант B"),
        ("сколько стоит рабочий вариант", "140 000", "140 000 рублей"),
        (
            "какой вопрос остался открытым",
            "pr-подрядчик",
            "вопрос про PR-подрядчика",
        ),
        (
            "что уже сделано по задаче",
            "каналы собраны",
            "каналы собраны",
        ),
        ("как называется проект", "атлас", "«Атлас»"),
        ("как зовут заказчика", "марина", "Марина Ковалёва"),
        ("какой дедлайн проекта", "15 декабря", "15 декабря 2026"),
        ("какой общий бюджет проекта", "480 000", "480 000 рублей"),
        (
            "какое ограничение по инфраструктуре",
            "российские цод",
            "только российские ЦОД",
        ),
        ("какую базу данных выбрали", "postgresql", "PostgreSQL"),
    ];
    let context_lower = context.to_lowercase();
    let mut known_probe = false;
    for (needle, marker, answer) in evidence {
        if q.contains(needle) {
            known_probe = true;
            if context_lower.contains(marker) {
                return Some((*answer).to_string());
            }
        }
    }
    if known_probe {
        Some("не знаю".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::memory::ChatMessage;

    #[test]
    fn endpoint_appends_chat_completions() {
        let client = ReqwestClient::new("https://api.deepseek.com/", None).unwrap();
        assert_eq!(
            client.endpoint(),
            "https://api.deepseek.com/chat/completions"
        );
    }

    #[test]
    fn missing_key_is_reported_without_network() {
        let client = ReqwestClient::new("https://api.deepseek.com", None).unwrap();
        let error = client
            .complete(&ChatRequest {
                model: "m".into(),
                messages: vec![ChatMessage::user("привет")],
                temperature: 0.0,
                max_tokens: None,
                tools: Vec::new(),
            })
            .unwrap_err();
        assert!(matches!(error, LlmError::MissingKey));
    }

    #[test]
    fn body_uses_openai_tool_format() {
        let request = ChatRequest {
            model: "deepseek-chat".into(),
            messages: vec![ChatMessage::user("2+2")],
            temperature: 0.2,
            max_tokens: Some(80),
            tools: vec![crate::llm::dto::ToolSchema {
                name: "calculate".into(),
                description: "считает".into(),
                parameters: serde_json::json!({"type": "object"}),
            }],
        };
        let body = build_body(&request);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "calculate");
        assert_eq!(body["max_tokens"], 80);
    }

    #[test]
    fn response_parses_content_and_usage() {
        let raw = r#"{"choices":[{"message":{"content":"ok","tool_calls":[{"id":"c1","function":{"name":"calculate","arguments":"{}"}}]}}],"usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}}"#;
        let parsed = parse_response(raw).unwrap();
        assert_eq!(parsed.content.as_deref(), Some("ok"));
        assert_eq!(parsed.tool_calls[0].name, "calculate");
        assert_eq!(parsed.usage.unwrap().total_tokens, 12);
    }

    #[test]
    fn fake_answers_only_with_evidence() {
        assert_eq!(
            fake_experiment_answer("Как называется проект?", "Данные: «Атлас»").as_deref(),
            Some("«Атлас»")
        );
        assert_eq!(
            fake_experiment_answer("Как называется проект?", "привет").as_deref(),
            Some("не знаю")
        );
    }
}
