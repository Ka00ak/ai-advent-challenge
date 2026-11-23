//! Настройки приложения и параметры памяти.

use serde::{Deserialize, Serialize};

use crate::domain::memory::STATE_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtractionMode {
    Local,
    Llm,
}

impl ExtractionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ExtractionMode::Local => "local",
            ExtractionMode::Llm => "llm",
        }
    }
}

impl std::str::FromStr for ExtractionMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(ExtractionMode::Local),
            "llm" => Ok(ExtractionMode::Llm),
            other => Err(format!("неизвестный режим извлечения: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Approval {
    Confirm,
    Auto,
}

impl Approval {
    pub fn as_str(self) -> &'static str {
        match self {
            Approval::Confirm => "confirm",
            Approval::Auto => "auto",
        }
    }
}

impl std::str::FromStr for Approval {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "confirm" => Ok(Approval::Confirm),
            "auto" => Ok(Approval::Auto),
            other => Err(format!("неизвестный режим подтверждения: {other}")),
        }
    }
}

fn default_stm_window() -> usize {
    8
}
fn default_stm_max_tokens() -> usize {
    1600
}
fn default_summary_enabled() -> bool {
    true
}
fn default_summarize_every() -> usize {
    20
}
fn default_wm_max_items() -> usize {
    24
}
fn default_wm_max_tokens() -> usize {
    320
}
fn default_wm_ttl_days() -> i64 {
    14
}
fn default_ltm_max_records() -> usize {
    300
}
fn default_profile_tokens() -> usize {
    220
}
fn default_relevant_tokens() -> usize {
    360
}
fn default_top_k() -> usize {
    8
}
fn default_total_budget() -> usize {
    2600
}
fn default_extraction() -> ExtractionMode {
    ExtractionMode::Local
}
fn default_approval() -> Approval {
    Approval::Auto
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StmConfig {
    pub window: usize,
    pub max_tokens: usize,
    pub summary_enabled: bool,
    pub summarize_every_messages: usize,
}

impl Default for StmConfig {
    fn default() -> Self {
        Self {
            window: default_stm_window(),
            max_tokens: default_stm_max_tokens(),
            summary_enabled: default_summary_enabled(),
            summarize_every_messages: default_summarize_every(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WmConfig {
    pub max_items: usize,
    pub max_tokens: usize,
    pub ttl_days: i64,
}

impl Default for WmConfig {
    fn default() -> Self {
        Self {
            max_items: default_wm_max_items(),
            max_tokens: default_wm_max_tokens(),
            ttl_days: default_wm_ttl_days(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LtmConfig {
    pub max_records: usize,
    pub profile_max_tokens: usize,
    pub relevant_max_tokens: usize,
    pub top_k: usize,
}

impl Default for LtmConfig {
    fn default() -> Self {
        Self {
            max_records: default_ltm_max_records(),
            profile_max_tokens: default_profile_tokens(),
            relevant_max_tokens: default_relevant_tokens(),
            top_k: default_top_k(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    pub stm: StmConfig,
    pub wm: WmConfig,
    pub ltm: LtmConfig,
    pub extraction_mode: ExtractionMode,
    pub approval: Approval,
    pub total_budget: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            stm: StmConfig::default(),
            wm: WmConfig::default(),
            ltm: LtmConfig::default(),
            extraction_mode: default_extraction(),
            approval: default_approval(),
            total_budget: default_total_budget(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub version: u32,
    pub base_url: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    pub max_iterations: usize,
    pub memory: MemoryConfig,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-chat".to_string(),
            temperature: 0.2,
            max_tokens: None,
            max_iterations: 6,
            memory: MemoryConfig::default(),
        }
    }
}

impl Settings {
    /// Проверка диапазонов из UI-спецификации. Возвращает список ошибок.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.base_url.trim().is_empty() {
            errors.push("Base URL не может быть пустым".to_string());
        }
        if self.model.trim().is_empty() {
            errors.push("Модель не может быть пустой".to_string());
        }
        if !(0.0..=2.0).contains(&self.temperature) {
            errors.push("Temperature должна быть в диапазоне 0..2".to_string());
        }
        if !(1..=12).contains(&self.max_iterations) {
            errors.push("Max iterations должна быть в диапазоне 1..12".to_string());
        }
        let m = &self.memory;
        if !(4..=20).contains(&m.stm.window) {
            errors.push("Окно STM должно быть в диапазоне 4..20".to_string());
        }
        if !(400..=4000).contains(&m.stm.max_tokens) {
            errors.push("Лимит STM должен быть в диапазоне 400..4000".to_string());
        }
        if !(100..=1000).contains(&m.wm.max_tokens) {
            errors.push("Лимит WM должен быть в диапазоне 100..1000".to_string());
        }
        if !(1..=20).contains(&m.ltm.top_k) {
            errors.push("LTM top-K должен быть в диапазоне 1..20".to_string());
        }
        if !(100..=1200).contains(&m.ltm.relevant_max_tokens) {
            errors.push("Бюджет LTM relevant должен быть в диапазоне 100..1200".to_string());
        }
        errors
    }

    /// Настройки для воспроизводимого эксперимента (07, раздел 5).
    pub fn experiment_defaults() -> Self {
        Self {
            temperature: 0.0,
            max_tokens: Some(80),
            memory: MemoryConfig {
                stm: StmConfig {
                    window: 8,
                    summary_enabled: false,
                    ..StmConfig::default()
                },
                ..MemoryConfig::default()
            },
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let settings = Settings::default();
        assert_eq!(settings.base_url, "https://api.deepseek.com");
        assert_eq!(settings.model, "deepseek-chat");
        assert_eq!(settings.temperature, 0.2);
        assert_eq!(settings.max_iterations, 6);
        assert_eq!(settings.memory.stm.window, 8);
        assert_eq!(settings.memory.stm.max_tokens, 1600);
        assert_eq!(settings.memory.ltm.profile_max_tokens, 220);
        assert_eq!(settings.memory.ltm.relevant_max_tokens, 360);
        assert_eq!(settings.memory.wm.max_tokens, 320);
        assert_eq!(settings.memory.ltm.top_k, 8);
        assert_eq!(settings.memory.total_budget, 2600);
        assert_eq!(settings.memory.extraction_mode, ExtractionMode::Local);
        assert_eq!(settings.memory.approval, Approval::Auto);
        assert!(settings.validate().is_empty());
    }

    #[test]
    fn validation_rejects_out_of_range() {
        let mut settings = Settings::default();
        settings.memory.stm.window = 3;
        settings.memory.ltm.top_k = 0;
        settings.temperature = 3.0;
        assert_eq!(settings.validate().len(), 3);
    }

    #[test]
    fn settings_roundtrip_preserves_unknown_defaults() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let parsed: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(settings, parsed);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let parsed: Settings = serde_json::from_str("{\"version\":1}").unwrap();
        assert_eq!(parsed, Settings::default());
    }

    #[test]
    fn experiment_defaults_disable_summary() {
        let settings = Settings::experiment_defaults();
        assert_eq!(settings.temperature, 0.0);
        assert_eq!(settings.max_tokens, Some(80));
        assert!(!settings.memory.stm.summary_enabled);
    }
}
