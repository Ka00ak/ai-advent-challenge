//! Подсветка JSON для панели деталей API.

use ratatui::text::{Line, Span};

use crate::tui::theme;

/// Разбивает JSON-строку на цветные токены: ключи, строки, числа, bool/null.
pub fn highlight_line(line: &str) -> Line<'static> {
    let chars: Vec<char> = line.chars().collect();
    let mut spans = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            let start = index;
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
            spans.push(Span::raw(chars[start..index].iter().collect::<String>()));
            continue;
        }
        if ch == '"' {
            let start = index;
            index += 1;
            while index < chars.len() {
                if chars[index] == '\\' {
                    index = (index + 2).min(chars.len());
                    continue;
                }
                let closing = chars[index] == '"';
                index += 1;
                if closing {
                    break;
                }
            }
            let text: String = chars[start..index].iter().collect();
            let mut lookahead = index;
            while lookahead < chars.len() && chars[lookahead].is_whitespace() {
                lookahead += 1;
            }
            let is_key = lookahead < chars.len() && chars[lookahead] == ':';
            spans.push(Span::styled(
                text,
                if is_key {
                    theme::json_key()
                } else {
                    theme::json_string()
                },
            ));
            continue;
        }
        if ch.is_ascii_digit() || ch == '-' {
            let start = index;
            index += 1;
            while index < chars.len()
                && (chars[index].is_ascii_digit()
                    || matches!(chars[index], '.' | 'e' | 'E' | '+' | '-'))
            {
                index += 1;
            }
            spans.push(Span::styled(
                chars[start..index].iter().collect::<String>(),
                theme::json_number(),
            ));
            continue;
        }
        if ch.is_alphabetic() {
            let start = index;
            index += 1;
            while index < chars.len() && chars[index].is_alphanumeric() {
                index += 1;
            }
            let text: String = chars[start..index].iter().collect();
            let style = if matches!(text.as_str(), "true" | "false" | "null") {
                theme::json_bool()
            } else {
                theme::muted()
            };
            spans.push(Span::styled(text, style));
            continue;
        }
        spans.push(Span::styled(ch.to_string(), theme::muted()));
        index += 1;
    }
    Line::from(spans)
}

pub fn body_lines(body: Option<&str>, empty: &str) -> Vec<Line<'static>> {
    match body.filter(|text| !text.trim().is_empty()) {
        Some(text) => text.lines().map(highlight_line).collect(),
        None => vec![Line::from(Span::styled(empty.to_string(), theme::muted()))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_keys_strings_numbers_and_bools() {
        let line = highlight_line("  \"key\": \"value\", \"n\": 12, \"ok\": true");
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text, "  \"key\": \"value\", \"n\": 12, \"ok\": true");

        let key = line
            .spans
            .iter()
            .find(|span| span.content == "\"key\"")
            .unwrap();
        let value = line
            .spans
            .iter()
            .find(|span| span.content == "\"value\"")
            .unwrap();
        let number = line.spans.iter().find(|span| span.content == "12").unwrap();
        let boolean = line
            .spans
            .iter()
            .find(|span| span.content == "true")
            .unwrap();
        assert_eq!(key.style, theme::json_key());
        assert_eq!(value.style, theme::json_string());
        assert_eq!(number.style, theme::json_number());
        assert_eq!(boolean.style, theme::json_bool());
    }

    #[test]
    fn escaped_quotes_do_not_break_highlighting() {
        let line = highlight_line("{\"text\": \"a \\\"quoted\\\" value\"}");
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text, "{\"text\": \"a \\\"quoted\\\" value\"}");
    }
}
