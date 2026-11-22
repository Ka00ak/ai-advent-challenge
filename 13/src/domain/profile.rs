//! Профиль пользователя и его предпочтения (День 12).
//!
//! Профиль — не слой памяти и не часть LTM: это отдельная персонализация,
//! которая применяется к каждому запросу поверх STM/WM/LTM. Профилей может
//! быть несколько; ровно один активен.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const PROFILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verbosity {
    Brief,
    Normal,
    Detailed,
}

impl Verbosity {
    pub fn as_str(self) -> &'static str {
        match self {
            Verbosity::Brief => "brief",
            Verbosity::Normal => "normal",
            Verbosity::Detailed => "detailed",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Verbosity::Brief => "кратко",
            Verbosity::Normal => "обычный объём",
            Verbosity::Detailed => "подробно",
        }
    }

    pub fn from_label(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "brief" | "кратко" | "short" => Some(Verbosity::Brief),
            "normal" | "обычно" | "обычный" => Some(Verbosity::Normal),
            "detailed" | "подробно" | "detail" => Some(Verbosity::Detailed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Formal,
    Neutral,
    Informal,
}

impl Tone {
    pub fn as_str(self) -> &'static str {
        match self {
            Tone::Formal => "formal",
            Tone::Neutral => "neutral",
            Tone::Informal => "informal",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tone::Formal => "формальный, на «вы»",
            Tone::Neutral => "нейтральный",
            Tone::Informal => "неформальный, на «ты»",
        }
    }

    pub fn from_label(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "formal" | "формально" | "официально" => Some(Tone::Formal),
            "neutral" | "нейтрально" => Some(Tone::Neutral),
            "informal" | "неформально" | "по-простому" => Some(Tone::Informal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodePolicy {
    WithCode,
    NoCode,
    Neutral,
}

impl CodePolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            CodePolicy::WithCode => "with-code",
            CodePolicy::NoCode => "no-code",
            CodePolicy::Neutral => "neutral",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CodePolicy::WithCode => "с примерами кода",
            CodePolicy::NoCode => "без кода",
            CodePolicy::Neutral => "по ситуации",
        }
    }

    pub fn from_label(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "with-code" | "with_code" | "code" | "с-кодом" | "с кодом" => {
                Some(CodePolicy::WithCode)
            }
            "no-code" | "no_code" | "без-кода" | "без кода" => {
                Some(CodePolicy::NoCode)
            }
            "neutral" | "нейтрально" | "по-ситуации" => {
                Some(CodePolicy::Neutral)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProfileSource {
    Manual,
    Dialogue,
    Preset,
    Migration,
}

impl ProfileSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileSource::Manual => "manual",
            ProfileSource::Dialogue => "dialogue",
            ProfileSource::Preset => "preset",
            ProfileSource::Migration => "migration",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileIdentity {
    pub name: Option<String>,
    pub role: Option<String>,
    pub language: String,
}

impl Default for ProfileIdentity {
    fn default() -> Self {
        Self {
            name: None,
            role: None,
            language: "ru".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileFormat {
    pub code: CodePolicy,
    pub lists: bool,
    pub tables: bool,
    pub emoji: bool,
}

impl Default for ProfileFormat {
    fn default() -> Self {
        Self {
            code: CodePolicy::Neutral,
            lists: true,
            tables: false,
            emoji: false,
        }
    }
}

/// След изменения профиля: откуда пришло предпочтение и на чём основано.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LearnedPreference {
    pub key: String,
    pub value: String,
    pub source: ProfileSource,
    pub evidence: Option<String>,
    pub confidence: f32,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: String,
    pub title: String,
    pub identity: ProfileIdentity,
    pub verbosity: Verbosity,
    pub tone: Tone,
    pub format: ProfileFormat,
    pub constraints: Vec<String>,
    #[serde(default)]
    pub preferences: Vec<LearnedPreference>,
    pub source: ProfileSource,
    #[serde(default)]
    pub revision: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl UserProfile {
    pub fn empty(id: impl Into<String>, now: DateTime<Utc>) -> Self {
        let id = id.into();
        Self {
            title: id.clone(),
            id,
            identity: ProfileIdentity::default(),
            verbosity: Verbosity::Normal,
            tone: Tone::Neutral,
            format: ProfileFormat::default(),
            constraints: Vec::new(),
            preferences: Vec::new(),
            source: ProfileSource::Manual,
            revision: 0,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn preset(id: &str, now: DateTime<Utc>) -> Option<Self> {
        match id {
            "default" => Some(Self::preset_default(now)),
            "brief" => Some(Self::preset_brief(now)),
            "detailed" => Some(Self::preset_detailed(now)),
            _ => None,
        }
    }

    pub fn preset_default(now: DateTime<Utc>) -> Self {
        Self {
            title: "Универсальный".into(),
            source: ProfileSource::Preset,
            ..Self::empty("default", now)
        }
    }

    pub fn preset_brief(now: DateTime<Utc>) -> Self {
        Self {
            title: "Кратко, без кода, неформально".into(),
            verbosity: Verbosity::Brief,
            tone: Tone::Informal,
            format: ProfileFormat {
                code: CodePolicy::NoCode,
                lists: false,
                tables: false,
                emoji: false,
            },
            constraints: vec!["Без вступлений".into(), "Без эмодзи".into()],
            source: ProfileSource::Preset,
            ..Self::empty("brief", now)
        }
    }

    pub fn preset_detailed(now: DateTime<Utc>) -> Self {
        Self {
            title: "Подробно, с кодом, формально".into(),
            verbosity: Verbosity::Detailed,
            tone: Tone::Formal,
            format: ProfileFormat {
                code: CodePolicy::WithCode,
                lists: false,
                tables: false,
                emoji: false,
            },
            constraints: vec![
                "Структурировать ответ".into(),
                "Указывать ограничения".into(),
            ],
            source: ProfileSource::Preset,
            ..Self::empty("detailed", now)
        }
    }

    /// Человекочитаемая строка предпочтений для CLI, TUI и отчёта.
    pub fn summary(&self) -> String {
        format!(
            "{}; {}; {}; списки={}; таблицы={}; эмодзи={}",
            self.verbosity.label(),
            self.tone.label(),
            self.format.code.label(),
            yes_no(self.format.lists),
            yes_no(self.format.tables),
            yes_no(self.format.emoji),
        )
    }

    pub fn touch(&mut self, now: DateTime<Utc>) {
        self.revision = self.revision.saturating_add(1);
        self.updated_at = now;
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "да" } else { "нет" }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileState {
    pub version: u32,
    pub active: Option<String>,
    pub profiles: Vec<UserProfile>,
    pub updated_at: DateTime<Utc>,
}

impl ProfileState {
    pub fn empty(now: DateTime<Utc>) -> Self {
        Self {
            version: PROFILE_VERSION,
            active: None,
            profiles: Vec::new(),
            updated_at: now,
        }
    }

    pub fn with_presets(now: DateTime<Utc>) -> Self {
        Self {
            version: PROFILE_VERSION,
            active: Some("default".into()),
            profiles: vec![
                UserProfile::preset_default(now),
                UserProfile::preset_brief(now),
                UserProfile::preset_detailed(now),
            ],
            updated_at: now,
        }
    }

    /// Гарантирует, что есть хотя бы один профиль и активный выбран.
    /// Возвращает `true`, если состояние изменилось.
    pub fn ensure_defaults(&mut self, now: DateTime<Utc>) -> bool {
        let mut changed = false;
        if self.profiles.is_empty() {
            *self = Self::with_presets(now);
            return true;
        }
        if self.active.is_none()
            || self
                .find(&self.active.clone().unwrap_or_default())
                .is_none()
        {
            self.active = Some(self.profiles[0].id.clone());
            changed = true;
        }
        if self.version != PROFILE_VERSION {
            self.version = PROFILE_VERSION;
            changed = true;
        }
        if changed {
            self.updated_at = now;
        }
        changed
    }

    pub fn active_profile(&self) -> Option<&UserProfile> {
        self.active.as_deref().and_then(|id| self.find(id))
    }

    pub fn active_profile_mut(&mut self) -> Option<&mut UserProfile> {
        let id = self.active.clone()?;
        self.profiles.iter_mut().find(|profile| profile.id == id)
    }

    pub fn find(&self, id: &str) -> Option<&UserProfile> {
        self.profiles.iter().find(|profile| profile.id == id)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut UserProfile> {
        self.profiles.iter_mut().find(|profile| profile.id == id)
    }

    pub fn set_active(&mut self, id: &str, now: DateTime<Utc>) -> bool {
        if self.find(id).is_none() {
            return false;
        }
        self.active = Some(id.to_string());
        self.updated_at = now;
        true
    }

    pub fn upsert(&mut self, profile: UserProfile, now: DateTime<Utc>) {
        if let Some(existing) = self.find_mut(&profile.id) {
            *existing = profile;
        } else {
            self.profiles.push(profile);
        }
        self.updated_at = now;
    }

    pub fn remove(&mut self, id: &str, now: DateTime<Utc>) -> bool {
        let before = self.profiles.len();
        self.profiles.retain(|profile| profile.id != id);
        if self.profiles.len() == before {
            return false;
        }
        if self.active.as_deref() == Some(id) {
            self.active = self.profiles.first().map(|profile| profile.id.clone());
        }
        self.updated_at = now;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 19, 10, 0, 0).unwrap()
    }

    #[test]
    fn presets_cover_opposite_preferences() {
        let brief = UserProfile::preset_brief(now());
        let detailed = UserProfile::preset_detailed(now());
        assert_eq!(brief.verbosity, Verbosity::Brief);
        assert_eq!(detailed.verbosity, Verbosity::Detailed);
        assert_eq!(brief.tone, Tone::Informal);
        assert_eq!(detailed.tone, Tone::Formal);
        assert_eq!(brief.format.code, CodePolicy::NoCode);
        assert_eq!(detailed.format.code, CodePolicy::WithCode);
        assert_ne!(brief.summary(), detailed.summary());
    }

    #[test]
    fn profile_state_always_has_active_profile() {
        let mut state = ProfileState::empty(now());
        assert!(state.active_profile().is_none());
        assert!(state.ensure_defaults(now()));
        assert_eq!(state.active.as_deref(), Some("default"));
        assert!(state.active_profile().is_some());
        assert!(!state.ensure_defaults(now()));
    }

    #[test]
    fn set_active_and_remove_keep_invariants() {
        let mut state = ProfileState::with_presets(now());
        assert!(state.set_active("brief", now()));
        assert_eq!(state.active_profile().unwrap().id, "brief");
        assert!(state.remove("brief", now()));
        assert!(state.active_profile().is_some());
        assert_ne!(state.active.as_deref(), Some("brief"));
        assert!(!state.remove("missing", now()));
    }

    #[test]
    fn serde_roundtrip_uses_snake_case_and_lowercase() {
        let state = ProfileState::with_presets(now());
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"active\":\"default\""));
        assert!(json.contains("\"verbosity\":\"brief\""));
        assert!(json.contains("\"code\":\"no-code\""));
        let parsed: ProfileState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, state);
    }
}
