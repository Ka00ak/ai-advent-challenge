//! Локальные инструменты из Дня 6: калькулятор, время и конвертер валют.
//! Их схемы уходят в модель вместе со схемами GitHub MCP.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{Datelike, Local, Timelike, Utc};
use chrono_tz::Tz;
use serde_json::{Value, json};

use crate::llm::{ToolSchema, ToolSource};

type Handler = Arc<dyn Fn(&Value) -> Result<Value, String> + Send + Sync>;

#[derive(Clone)]
struct ToolDefinition {
    schema: ToolSchema,
    handler: Handler,
}

#[derive(Clone, Default)]
pub struct LocalToolRegistry {
    tools: HashMap<String, ToolDefinition>,
}

impl LocalToolRegistry {
    pub fn day6() -> Self {
        let mut registry = Self::default();
        registry.register(
            "calculate",
            "Вычисляет арифметическое выражение. Поддерживает + - * / % ^, скобки, \
             функции sqrt, abs, round, floor, ceil, min, max и константы pi, e. \
             Примеры: \"2 + 2 * 2\", \"sqrt(16)\", \"round(1234.567, 2)\".",
            json!({
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "Арифметическое выражение, например (250 * 1.15) / 0.92"
                    }
                },
                "required": ["expression"],
                "additionalProperties": false
            }),
            |args| {
                let expression = args
                    .get("expression")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                if expression.is_empty() {
                    return Err("параметр expression не может быть пустым".into());
                }
                let result = evaluate_expression(expression)?;
                Ok(json!({"expression": expression, "result": result}))
            },
        );
        registry.register(
            "get_current_time",
            "Возвращает текущие дату и время, unix timestamp и день недели. \
             Можно указать IANA-таймзону: Europe/Moscow, Asia/Tokyo, UTC и т.п. \
             Без параметра используется локальная таймзона.",
            json!({
                "type": "object",
                "properties": {
                    "timezone": {
                        "type": "string",
                        "description": "IANA-таймзона, например Europe/Moscow. Необязательно."
                    }
                },
                "additionalProperties": false
            }),
            |args| {
                let requested = args
                    .get("timezone")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let now = Utc::now();
                if let Some(name) = requested {
                    let timezone: Tz = name.parse().map_err(|_| {
                        format!(
                            "неизвестная таймзона «{name}»; используйте IANA, например Europe/Moscow"
                        )
                    })?;
                    let local = now.with_timezone(&timezone);
                    return Ok(json!({
                        "timezone": name,
                        "formatted": local.format("%Y-%m-%d %H:%M:%S %Z").to_string(),
                        "iso": local.to_rfc3339(),
                        "unix": now.timestamp(),
                        "weekday": weekday_ru(local.weekday()),
                        "year": local.year(),
                        "month": local.month(),
                        "day": local.day(),
                        "hour": local.hour(),
                        "minute": local.minute()
                    }));
                }
                let local = now.with_timezone(&Local);
                Ok(json!({
                    "timezone": local.offset().to_string(),
                    "formatted": local.format("%Y-%m-%d %H:%M:%S %Z").to_string(),
                    "iso": local.to_rfc3339(),
                    "unix": now.timestamp(),
                    "weekday": weekday_ru(local.weekday()),
                    "year": local.year(),
                    "month": local.month(),
                    "day": local.day(),
                    "hour": local.hour(),
                    "minute": local.minute()
                }))
            },
        );
        registry.register(
            "convert_currency",
            "Конвертирует сумму между валютами по зашитому в код курсу. \
             Поддерживаются USD, EUR, GBP, RUB, CNY, JPY, KZT, TRY, INR, BYN. \
             Курс фиксированный и не обновляется в реальном времени.",
            json!({
                "type": "object",
                "properties": {
                    "amount": {"type": "number", "description": "Сумма для конвертации"},
                    "from": {"type": "string", "description": "Код исходной валюты"},
                    "to": {"type": "string", "description": "Код целевой валюты"}
                },
                "required": ["amount", "from", "to"],
                "additionalProperties": false
            }),
            |args| {
                let amount = args
                    .get("amount")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| "параметр amount должен быть числом".to_string())?;
                if !amount.is_finite() {
                    return Err("параметр amount должен быть конечным числом".into());
                }
                let from = args
                    .get("from")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_uppercase();
                let to = args
                    .get("to")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_uppercase();
                let rates = rates_per_usd();
                let from_rate = *rates
                    .get(from.as_str())
                    .ok_or_else(|| format!("валюта «{from}» не поддерживается"))?;
                let to_rate = *rates
                    .get(to.as_str())
                    .ok_or_else(|| format!("валюта «{to}» не поддерживается"))?;
                let rate = to_rate / from_rate;
                Ok(json!({
                    "amount": amount,
                    "from": from,
                    "to": to,
                    "rate": round(rate, 6),
                    "result": round(amount * rate, 4),
                    "note": "Курс зашит в код и не является актуальным."
                }))
            },
        );
        registry
    }

    fn register<F>(&mut self, name: &str, description: &str, parameters: Value, handler: F)
    where
        F: Fn(&Value) -> Result<Value, String> + Send + Sync + 'static,
    {
        self.tools.insert(
            name.to_string(),
            ToolDefinition {
                schema: ToolSchema {
                    name: name.to_string(),
                    description: description.to_string(),
                    parameters,
                    source: ToolSource::Local,
                },
                handler: Arc::new(handler),
            },
        );
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        let mut schemas: Vec<ToolSchema> = self
            .tools
            .values()
            .map(|definition| definition.schema.clone())
            .collect();
        schemas.sort_by(|left, right| left.name.cmp(&right.name));
        schemas
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn execute(&self, name: &str, arguments: &Value) -> Result<Value, String> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| format!("неизвестный локальный инструмент «{name}»"))?;
        (tool.handler)(arguments)
    }
}

fn rates_per_usd() -> HashMap<&'static str, f64> {
    HashMap::from([
        ("USD", 1.0),
        ("EUR", 0.92),
        ("GBP", 0.79),
        ("RUB", 92.5),
        ("CNY", 7.1),
        ("JPY", 155.0),
        ("KZT", 480.0),
        ("TRY", 34.0),
        ("INR", 84.0),
        ("BYN", 3.27),
    ])
}

fn round(value: f64, digits: i32) -> f64 {
    let factor = 10f64.powi(digits);
    (value * factor).round() / factor
}

fn weekday_ru(weekday: chrono::Weekday) -> &'static str {
    match weekday {
        chrono::Weekday::Mon => "понедельник",
        chrono::Weekday::Tue => "вторник",
        chrono::Weekday::Wed => "среда",
        chrono::Weekday::Thu => "четверг",
        chrono::Weekday::Fri => "пятница",
        chrono::Weekday::Sat => "суббота",
        chrono::Weekday::Sun => "воскресенье",
    }
}

/// Рекурсивный парсер арифметики без eval.
pub fn evaluate_expression(expression: &str) -> Result<f64, String> {
    if expression.trim().is_empty() {
        return Err("пустое выражение".into());
    }
    let mut parser = Parser {
        source: expression.chars().collect(),
        position: 0,
    };
    let value = parser.parse_expression()?;
    parser.skip_spaces();
    if parser.position < parser.source.len() {
        return Err(format!(
            "неожиданный символ «{}» в позиции {}",
            parser.source[parser.position],
            parser.position + 1
        ));
    }
    if !value.is_finite() {
        return Err("результат не является конечным числом".into());
    }
    Ok((value * 1e12).round() / 1e12)
}

struct Parser {
    source: Vec<char>,
    position: usize,
}

impl Parser {
    fn parse_expression(&mut self) -> Result<f64, String> {
        let mut value = self.parse_term()?;
        loop {
            self.skip_spaces();
            match self.peek() {
                Some('+') => {
                    self.position += 1;
                    value += self.parse_term()?;
                }
                Some('-') => {
                    self.position += 1;
                    value -= self.parse_term()?;
                }
                _ => return Ok(value),
            }
        }
    }

    fn parse_term(&mut self) -> Result<f64, String> {
        let mut value = self.parse_unary()?;
        loop {
            self.skip_spaces();
            match self.peek() {
                Some('*') => {
                    self.position += 1;
                    value *= self.parse_unary()?;
                }
                Some('/') => {
                    self.position += 1;
                    let right = self.parse_unary()?;
                    if right == 0.0 {
                        return Err("деление на ноль".into());
                    }
                    value /= right;
                }
                Some('%') => {
                    self.position += 1;
                    let right = self.parse_unary()?;
                    if right == 0.0 {
                        return Err("деление на ноль".into());
                    }
                    value %= right;
                }
                _ => return Ok(value),
            }
        }
    }

    fn parse_unary(&mut self) -> Result<f64, String> {
        self.skip_spaces();
        match self.peek() {
            Some('+') => {
                self.position += 1;
                self.parse_unary()
            }
            Some('-') => {
                self.position += 1;
                Ok(-self.parse_unary()?)
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Result<f64, String> {
        let base = self.parse_primary()?;
        self.skip_spaces();
        if self.peek() == Some('^') {
            self.position += 1;
            let exponent = self.parse_unary()?;
            Ok(base.powf(exponent))
        } else {
            Ok(base)
        }
    }

    fn parse_primary(&mut self) -> Result<f64, String> {
        self.skip_spaces();
        match self.peek() {
            Some('(') => {
                self.position += 1;
                let value = self.parse_expression()?;
                self.skip_spaces();
                if self.peek() != Some(')') {
                    return Err("не закрыта скобка".into());
                }
                self.position += 1;
                Ok(value)
            }
            Some(ch) if ch.is_ascii_digit() || ch == '.' => self.parse_number(),
            Some(ch) if ch.is_ascii_alphabetic() => self.parse_identifier(),
            Some(ch) => Err(format!(
                "ожидалось число, скобка или функция, получено «{ch}»"
            )),
            None => Err("неожиданный конец выражения".into()),
        }
    }

    fn parse_number(&mut self) -> Result<f64, String> {
        let start = self.position;
        let mut dots = 0;
        while let Some(ch) = self.peek() {
            if ch == '.' {
                dots += 1;
                self.position += 1;
            } else if ch.is_ascii_digit() {
                self.position += 1;
            } else {
                break;
            }
        }
        let raw: String = self.source[start..self.position].iter().collect();
        if dots > 1 || raw == "." || raw.is_empty() {
            return Err(format!("некорректное число «{raw}»"));
        }
        raw.parse::<f64>()
            .map_err(|_| format!("некорректное число «{raw}»"))
    }

    fn parse_identifier(&mut self) -> Result<f64, String> {
        let start = self.position;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                self.position += 1;
            } else {
                break;
            }
        }
        let name: String = self.source[start..self.position]
            .iter()
            .collect::<String>()
            .to_lowercase();
        self.skip_spaces();
        if self.peek() != Some('(') {
            return match name.as_str() {
                "pi" => Ok(std::f64::consts::PI),
                "e" => Ok(std::f64::consts::E),
                _ => Err(format!("неизвестная константа «{name}»")),
            };
        }
        self.position += 1;
        let mut args = Vec::new();
        self.skip_spaces();
        if self.peek() == Some(')') {
            self.position += 1;
        } else {
            loop {
                args.push(self.parse_expression()?);
                self.skip_spaces();
                match self.peek() {
                    Some(',') => {
                        self.position += 1;
                    }
                    Some(')') => {
                        self.position += 1;
                        break;
                    }
                    _ => return Err("ожидалась запятая или закрывающая скобка".into()),
                }
            }
        }
        apply_function(&name, &args)
    }

    fn skip_spaces(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.position += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<char> {
        self.source.get(self.position).copied()
    }
}

fn apply_function(name: &str, args: &[f64]) -> Result<f64, String> {
    let check = |min: usize, max: usize| -> Result<(), String> {
        if args.len() < min || args.len() > max {
            Err(format!(
                "функция «{name}» ожидает от {min} до {max} аргументов, получено {}",
                args.len()
            ))
        } else {
            Ok(())
        }
    };
    match name {
        "sqrt" => {
            check(1, 1)?;
            Ok(args[0].sqrt())
        }
        "abs" => {
            check(1, 1)?;
            Ok(args[0].abs())
        }
        "floor" => {
            check(1, 1)?;
            Ok(args[0].floor())
        }
        "ceil" => {
            check(1, 1)?;
            Ok(args[0].ceil())
        }
        "min" => {
            check(1, usize::MAX)?;
            Ok(args.iter().copied().fold(f64::INFINITY, f64::min))
        }
        "max" => {
            check(1, usize::MAX)?;
            Ok(args.iter().copied().fold(f64::NEG_INFINITY, f64::max))
        }
        "round" => {
            check(1, 2)?;
            let digits = args.get(1).copied().unwrap_or(0.0).trunc() as i32;
            Ok(round(args[0], digits))
        }
        _ => Err(format!("неизвестная функция «{name}»")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculator_precedence_and_functions() {
        assert_eq!(evaluate_expression("2 + 2 * 2").unwrap(), 6.0);
        assert_eq!(evaluate_expression("(2 + 2) * 2").unwrap(), 8.0);
        assert_eq!(evaluate_expression("sqrt(16)").unwrap(), 4.0);
        assert_eq!(evaluate_expression("round(1234.567, 2)").unwrap(), 1234.57);
        assert_eq!(evaluate_expression("2 ^ 3 ^ 2").unwrap(), 512.0);
        assert!((evaluate_expression("pi").unwrap() - std::f64::consts::PI).abs() < 1e-9);
    }

    #[test]
    fn calculator_errors_are_clean() {
        assert!(evaluate_expression("1 / 0").is_err());
        assert!(evaluate_expression("").is_err());
        assert!(evaluate_expression("2 +").is_err());
        assert!(evaluate_expression("foo(1)").is_err());
    }

    #[test]
    fn day6_registry_has_exactly_three_tools() {
        let registry = LocalToolRegistry::day6();
        let names: Vec<String> = registry
            .schemas()
            .into_iter()
            .map(|schema| schema.name)
            .collect();
        assert_eq!(
            names,
            vec!["calculate", "convert_currency", "get_current_time"]
        );
        assert!(
            registry
                .schemas()
                .iter()
                .all(|tool| tool.source == ToolSource::Local)
        );
    }

    #[test]
    fn currency_conversion_works() {
        let registry = LocalToolRegistry::day6();
        let result = registry
            .execute(
                "convert_currency",
                &json!({"amount": 100, "from": "USD", "to": "RUB"}),
            )
            .unwrap();
        assert_eq!(result["result"], 9250.0);
        assert!(
            registry
                .execute(
                    "convert_currency",
                    &json!({"amount": 1, "from": "XXX", "to": "RUB"})
                )
                .is_err()
        );
    }

    #[test]
    fn time_tool_supports_iana_timezone() {
        let registry = LocalToolRegistry::day6();
        let result = registry
            .execute("get_current_time", &json!({"timezone": "Europe/Moscow"}))
            .unwrap();
        assert_eq!(result["timezone"], "Europe/Moscow");
        assert!(result["iso"].as_str().unwrap().contains("+03:00"));
        assert!(
            registry
                .execute("get_current_time", &json!({"timezone": "Nowhere/Zone"}))
                .is_err()
        );
    }

    #[test]
    fn unknown_tool_is_error() {
        assert!(
            LocalToolRegistry::day6()
                .execute("nope", &json!({}))
                .is_err()
        );
    }
}
