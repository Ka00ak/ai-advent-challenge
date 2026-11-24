//! Параметры модели и MCP. Значения по умолчанию можно переопределить
//! переменными окружения и флагами CLI.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

fn env_or(name: &str, fallback: &str) -> String {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub max_iterations: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            base_url: env_or("LAB16_BASE_URL", "https://api.deepseek.com"),
            model: env_or("LAB16_MODEL", "deepseek-chat"),
            temperature: 0.2,
            max_tokens: None,
            max_iterations: 8,
        }
    }
}

impl Settings {
    pub fn from_overrides(base_url: Option<String>, model: Option<String>) -> Self {
        let mut settings = Self::default();
        if let Some(base_url) = base_url.filter(|value| !value.trim().is_empty()) {
            settings.base_url = base_url.trim().to_string();
        }
        if let Some(model) = model.filter(|value| !value.trim().is_empty()) {
            settings.model = model.trim().to_string();
        }
        settings
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.base_url.trim().is_empty() {
            return Err("Base URL не может быть пустым".into());
        }
        if self.model.trim().is_empty() {
            return Err("Модель не может быть пустой".into());
        }
        if !(0.0..=2.0).contains(&self.temperature) {
            return Err("Temperature должна быть в диапазоне 0..2".into());
        }
        if !(1..=16).contains(&self.max_iterations) {
            return Err("Max iterations должна быть в диапазоне 1..16".into());
        }
        Ok(())
    }

    pub fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

/// Ключ читается из LAB16_API_KEY, DEEPSEEK_API_KEY или KIMI_API_KEY.
/// `--api-key-file` имеет приоритет.
pub fn load_api_key(file: Option<&PathBuf>) -> Option<String> {
    let from_file = file
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(value) = from_file {
        return Some(value);
    }
    ["LAB16_API_KEY", "DEEPSEEK_API_KEY", "KIMI_API_KEY"]
        .iter()
        .find_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_has_single_slash() {
        let settings = Settings {
            base_url: "https://api.deepseek.com/".into(),
            ..Settings::default()
        };
        assert_eq!(
            settings.endpoint(),
            "https://api.deepseek.com/chat/completions"
        );
    }

    #[test]
    fn validation_catches_empty_model() {
        let settings = Settings {
            model: " ".into(),
            ..Settings::default()
        };
        assert!(settings.validate().is_err());
    }
}
