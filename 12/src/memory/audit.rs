//! Санитайзер значений и маскирование секретов.
//!
//! Правила из `docs/04-memory-policy.md`: управляющие символы вырезаются,
//! переводы строк схлопываются, key ≤ 64, value ≤ 500, tags ≤ 8. Секреты
//! заменяются на `***` до записи в слой, pending и аудит.

use chrono::{DateTime, Utc};

use crate::domain::memory::{MemoryLayer, MemorySource, MemoryWriteEvent, WriteOp};

const MAX_KEY_CHARS: usize = 64;
const MAX_VALUE_CHARS: usize = 500;
const MAX_TAGS: usize = 8;

/// Убирает управляющие символы, схлопывает пробелы и обрезает строку.
pub fn sanitize_value(value: &str) -> String {
    sanitize_text(value, MAX_VALUE_CHARS)
}

pub fn sanitize_key(key: &str) -> String {
    sanitize_text(key, MAX_KEY_CHARS)
}

pub fn sanitize_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let clean = sanitize_text(tag, 32);
        if !clean.is_empty() && !out.iter().any(|existing| existing == &clean) {
            out.push(clean);
        }
        if out.len() == MAX_TAGS {
            break;
        }
    }
    out
}

fn sanitize_text(value: &str, limit: usize) -> String {
    let mut out = String::with_capacity(value.len().min(limit * 2));
    let mut pending_space = false;
    for ch in value.chars() {
        if ch.is_control() || ch == '\u{200b}' || ch == '\u{feff}' {
            pending_space = !out.is_empty();
            continue;
        }
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
        if out.chars().count() >= limit {
            break;
        }
    }
    out.trim().to_string()
}

/// Проверяет, похожа ли строка на секрет (R1).
pub fn contains_secret(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    const MARKERS: [&str; 9] = [
        "sk-", "pk-", "api_key", "api-key", "apikey", "token", "secret", "password", "passwd",
    ];
    if MARKERS.iter().any(|marker| lower.contains(marker)) {
        return true;
    }
    if lower.contains("bearer ") {
        return true;
    }
    if lower.contains("-----begin") && lower.contains("private key") {
        return true;
    }
    text.split(|ch: char| ch.is_whitespace() || ch == '"' || ch == '\'' || ch == ',')
        .any(is_long_token)
}

fn is_long_token(token: &str) -> bool {
    let token =
        token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-');
    token.len() >= 32
        && token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        && token.chars().any(|ch| ch.is_ascii_digit())
}

/// Маскирует секретные фрагменты, сохраняя ключ/метку для читаемости.
pub fn mask_secret(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut mask_next = false;
    for raw in text.split_whitespace() {
        if mask_next {
            out.push("***".to_string());
            mask_next = false;
            continue;
        }
        let lower = raw.to_ascii_lowercase();
        let masked = if is_long_token(raw) || lower.starts_with("sk-") || lower.starts_with("pk-") {
            "***".to_string()
        } else if lower.contains("bearer") {
            mask_next = true;
            raw.to_string()
        } else if lower.contains("password")
            || lower.contains("passwd")
            || lower.contains("secret")
            || lower.contains("token")
            || lower.contains("api_key")
            || lower.contains("api-key")
            || lower.contains("apikey")
        {
            let value = mask_after_separator(raw);
            if value.ends_with("=***") && !raw.ends_with("=***") {
                mask_next = true;
            }
            value
        } else {
            raw.to_string()
        };
        out.push(masked);
    }
    out.join(" ")
}

fn mask_after_separator(raw: &str) -> String {
    if let Some(pos) = raw.find([':', '=']).map(|byte| byte + 1) {
        if pos < raw.len() {
            return format!("{}***", &raw[..pos]);
        }
    }
    format!("{raw}=***")
}

/// Событие аудита с уже замаскированным значением.
#[allow(clippy::too_many_arguments)]
pub fn make_event(
    layer: MemoryLayer,
    op: WriteOp,
    key: &str,
    value: Option<&str>,
    source: MemorySource,
    rule_id: Option<&str>,
    reason: &str,
    message_index: Option<usize>,
    now: DateTime<Utc>,
) -> MemoryWriteEvent {
    MemoryWriteEvent {
        id: format!("evt_{}", uuid::Uuid::new_v4().simple()),
        at: now,
        layer,
        op,
        key: key.to_string(),
        value: value.map(|v| mask_secret(&sanitize_value(v))),
        source,
        rule_id: rule_id.map(str::to_string),
        reason: reason.to_string(),
        message_index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizer_collapses_control_chars() {
        assert_eq!(sanitize_value("a\n\r\tb\u{0}c"), "a b c");
        assert_eq!(sanitize_value("  много   пробелов  "), "много пробелов");
    }

    #[test]
    fn sanitizer_limits_value_and_key() {
        let long = "а".repeat(600);
        assert_eq!(sanitize_value(&long).chars().count(), 500);
        assert_eq!(sanitize_key(&long).chars().count(), 64);
    }

    #[test]
    fn tags_are_limited_and_deduplicated() {
        let tags: Vec<String> = (0..12).map(|index| format!("tag{index}")).collect();
        let clean = sanitize_tags(&tags);
        assert_eq!(clean.len(), 8);
        let dupes = vec!["a".to_string(), "a".to_string(), "b".to_string()];
        assert_eq!(sanitize_tags(&dupes), vec!["a", "b"]);
    }

    #[test]
    fn detects_secrets() {
        assert!(contains_secret("api_key = sk-1234567890"));
        assert!(contains_secret("Bearer abcdef"));
        assert!(contains_secret(&format!(
            "prefix {}",
            "A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6"
        )));
        assert!(contains_secret("-----BEGIN RSA PRIVATE KEY-----"));
        assert!(!contains_secret("Дедлайн — 15 декабря 2026"));
        assert!(!contains_secret("PostgreSQL и REST"));
    }

    #[test]
    fn masks_secret_values() {
        let masked = mask_secret("api_key: abcdef");
        assert!(masked.contains("***"));
        assert!(!masked.contains("abcdef"));
        let masked = mask_secret("Bearer super-secret-value");
        assert_eq!(masked, "Bearer ***");
    }

    #[test]
    fn event_value_is_masked() {
        let event = make_event(
            MemoryLayer::Ltm,
            WriteOp::Reject,
            "secret.api",
            Some("api_key=abcdef"),
            MemorySource::Router,
            Some("R1"),
            "секрет",
            Some(1),
            Utc::now(),
        );
        assert_eq!(event.value.as_deref(), Some("api_key=***"));
    }
}
