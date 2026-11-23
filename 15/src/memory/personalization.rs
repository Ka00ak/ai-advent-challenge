//! Персонализация: обучение профиля из реплик и блок профиля в промпте.
//!
//! Профиль пополняется двумя путями: вручную (CLI/TUI) и из диалога. Здесь —
//! детерминированные правила второго пути: они не делают сетевых запросов и
//! сохраняют происхождение каждого изменения.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::memory::RenderedBlock;
use crate::domain::profile::{
    CodePolicy, LearnedPreference, ProfileSource, Tone, UserProfile, Verbosity,
};
use crate::memory::audit::{sanitize_key, sanitize_value};
use crate::memory::tokens::estimate_text_tokens;

const EVIDENCE_LIMIT: usize = 160;
const MAX_LEARNED: usize = 100;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileChange {
    pub key: String,
    pub old: String,
    pub new: String,
    pub evidence: String,
}

impl ProfileChange {
    pub fn describe(&self) -> String {
        format!("{}: {} → {}", self.key, self.old, self.new)
    }
}

/// Извлекает предпочтения из одной user-реплики и сразу применяет их к
/// активному профилю. Конфликт решается в пользу последней реплики.
pub fn learn_from_message(
    profile: &mut UserProfile,
    text: &str,
    now: DateTime<Utc>,
) -> Vec<ProfileChange> {
    let sanitized = sanitize_value(text);
    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let lower = normalize(trimmed);
    let evidence = snippet(trimmed);
    let cue = has_preference_cue(&lower);
    let mut changes = Vec::new();

    if cue {
        if let Some(verbosity) = detect_verbosity(&lower) {
            if profile.verbosity != verbosity {
                changes.push(ProfileChange {
                    key: "style.verbosity".into(),
                    old: profile.verbosity.label().into(),
                    new: verbosity.label().into(),
                    evidence: evidence.clone(),
                });
                profile.verbosity = verbosity;
                record(
                    profile,
                    "style.verbosity",
                    verbosity.as_str(),
                    &evidence,
                    now,
                );
            }
        }
        if let Some(tone) = detect_tone(&lower) {
            if profile.tone != tone {
                changes.push(ProfileChange {
                    key: "style.tone".into(),
                    old: profile.tone.label().into(),
                    new: tone.label().into(),
                    evidence: evidence.clone(),
                });
                profile.tone = tone;
                record(profile, "style.tone", tone.as_str(), &evidence, now);
            }
        }
        if let Some(code) = detect_code(&lower) {
            if profile.format.code != code {
                changes.push(ProfileChange {
                    key: "format.code".into(),
                    old: profile.format.code.label().into(),
                    new: code.label().into(),
                    evidence: evidence.clone(),
                });
                profile.format.code = code;
                record(profile, "format.code", code.as_str(), &evidence, now);
            }
        }
        if let Some(lists) = detect_lists(&lower) {
            if profile.format.lists != lists {
                changes.push(ProfileChange {
                    key: "format.lists".into(),
                    old: yes_no(profile.format.lists).into(),
                    new: yes_no(lists).into(),
                    evidence: evidence.clone(),
                });
                profile.format.lists = lists;
                record(profile, "format.lists", yes_no(lists), &evidence, now);
            }
        }
        if let Some(tables) = detect_tables(&lower) {
            if profile.format.tables != tables {
                changes.push(ProfileChange {
                    key: "format.tables".into(),
                    old: yes_no(profile.format.tables).into(),
                    new: yes_no(tables).into(),
                    evidence: evidence.clone(),
                });
                profile.format.tables = tables;
                record(profile, "format.tables", yes_no(tables), &evidence, now);
            }
        }
        if let Some(emoji) = detect_emoji(&lower) {
            if profile.format.emoji != emoji {
                changes.push(ProfileChange {
                    key: "format.emoji".into(),
                    old: yes_no(profile.format.emoji).into(),
                    new: yes_no(emoji).into(),
                    evidence: evidence.clone(),
                });
                profile.format.emoji = emoji;
                record(profile, "format.emoji", yes_no(emoji), &evidence, now);
            }
        }
    }

    if let Some(name) = after_marker(trimmed, &lower, &["меня зовут ", "моё имя ", "мое имя "])
    {
        let name = trim_name(&name);
        if !name.is_empty() && profile.identity.name.as_deref() != Some(name.as_str()) {
            changes.push(ProfileChange {
                key: "identity.name".into(),
                old: profile.identity.name.clone().unwrap_or_else(|| "—".into()),
                new: name.clone(),
                evidence: evidence.clone(),
            });
            profile.identity.name = Some(name.clone());
            record(profile, "identity.name", &name, &evidence, now);
        }
    }
    if let Some(role) = after_marker(
        trimmed,
        &lower,
        &["я работаю ", "моя роль ", "я — ", "я - "],
    ) {
        let role = stop_at(&role);
        if !role.is_empty() && profile.identity.role.as_deref() != Some(role.as_str()) {
            changes.push(ProfileChange {
                key: "identity.role".into(),
                old: profile.identity.role.clone().unwrap_or_else(|| "—".into()),
                new: role.clone(),
                evidence: evidence.clone(),
            });
            profile.identity.role = Some(role.clone());
            record(profile, "identity.role", &role, &evidence, now);
        }
    }
    if let Some(language) = detect_language(&lower) {
        if profile.identity.language != language {
            changes.push(ProfileChange {
                key: "identity.language".into(),
                old: profile.identity.language.clone(),
                new: language.clone(),
                evidence: evidence.clone(),
            });
            profile.identity.language = language.clone();
            record(profile, "identity.language", &language, &evidence, now);
        }
    }

    for constraint in detect_constraints(&lower) {
        if !profile
            .constraints
            .iter()
            .any(|existing| normalize(existing) == constraint)
        {
            changes.push(ProfileChange {
                key: "constraint".into(),
                old: "—".into(),
                new: constraint.clone(),
                evidence: evidence.clone(),
            });
            profile.constraints.push(capitalize(&constraint));
            record(profile, "constraint", &constraint, &evidence, now);
        }
    }

    if let Some(custom) = after_marker(
        trimmed,
        &lower,
        &["запомни как предпочтение: ", "запомни предпочтение: "],
    ) {
        let value = sanitize_value(&stop_at(&custom));
        if !value.is_empty() {
            let key = format!("preference.custom.{}", profile.preferences.len() + 1);
            changes.push(ProfileChange {
                key: key.clone(),
                old: "—".into(),
                new: value.clone(),
                evidence: evidence.clone(),
            });
            record(profile, &key, &value, &evidence, now);
        }
    }

    if !changes.is_empty() {
        profile.source = ProfileSource::Dialogue;
        profile.touch(now);
        if profile.preferences.len() > MAX_LEARNED {
            let excess = profile.preferences.len() - MAX_LEARNED;
            profile.preferences.drain(0..excess);
        }
    }
    changes
}

fn record(profile: &mut UserProfile, key: &str, value: &str, evidence: &str, now: DateTime<Utc>) {
    if !key.starts_with("constraint") && !key.starts_with("preference.custom") {
        profile.preferences.retain(|item| item.key != key);
    }
    profile.preferences.push(LearnedPreference {
        key: key.to_string(),
        value: value.to_string(),
        source: ProfileSource::Dialogue,
        evidence: Some(evidence.to_string()),
        confidence: 0.9,
        at: now,
    });
}

/// Блок персонализации идёт первым после ядра системного промпта.
pub fn render_profile_block(profile: &UserProfile) -> RenderedBlock {
    let mut lines = vec![
        "## Персонализация: предпочтения пользователя".to_string(),
        "Это настройки пользователя. Соблюдай их в каждом ответе; текущая явная просьба важнее профиля."
            .to_string(),
        String::new(),
        format!(
            "- Профиль: {} ({})",
            sanitize_value(&profile.title),
            sanitize_key(&profile.id)
        ),
        format!("- Тон: {}", tone_instruction(profile)),
        format!("- Объём: {}", verbosity_instruction(profile)),
        format!("- Код: {}", code_instruction(profile)),
        format!(
            "- Списки: {}",
            if profile.format.lists {
                "использовать"
            } else {
                "не использовать"
            }
        ),
        format!(
            "- Таблицы: {}",
            if profile.format.tables {
                "использовать"
            } else {
                "не использовать"
            }
        ),
        format!(
            "- Эмодзи: {}",
            if profile.format.emoji {
                "использовать"
            } else {
                "не использовать"
            }
        ),
    ];
    if let Some(name) = profile
        .identity
        .name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("- Пользователь: {}", sanitize_value(name)));
    }
    if let Some(role) = profile
        .identity
        .role
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(format!("- Роль: {}", sanitize_value(role)));
    }
    if profile.identity.language != "ru" {
        lines.push(format!(
            "- Язык ответа: {}",
            sanitize_key(&profile.identity.language)
        ));
    }
    let constraints: Vec<String> = profile
        .constraints
        .iter()
        .map(|value| sanitize_value(value))
        .filter(|value| !value.is_empty())
        .collect();
    if !constraints.is_empty() {
        lines.push(format!("- Ограничения: {}", constraints.join("; ")));
    }
    let text = lines.join("\n");
    RenderedBlock {
        tokens: estimate_text_tokens(&text),
        text,
        ..RenderedBlock::default()
    }
}

fn tone_instruction(profile: &UserProfile) -> String {
    match profile.tone {
        Tone::Formal => "формальный, обращайся на «вы»".into(),
        Tone::Neutral => "нейтральный".into(),
        Tone::Informal => "неформальный, обращайся на «ты»".into(),
    }
}

fn verbosity_instruction(profile: &UserProfile) -> String {
    match profile.verbosity {
        Verbosity::Brief => "кратко, не более 80 слов".into(),
        Verbosity::Normal => "обычный объём".into(),
        Verbosity::Detailed => "подробно, не менее 70 слов".into(),
    }
}

fn code_instruction(profile: &UserProfile) -> String {
    match profile.format.code {
        CodePolicy::WithCode => "с примерами кода, приведи блок кода".into(),
        CodePolicy::NoCode => "без кода, не приводи фрагменты кода".into(),
        CodePolicy::Neutral => "по ситуации".into(),
    }
}

fn has_preference_cue(lower: &str) -> bool {
    const CUES: [&str; 16] = [
        "отвечай",
        "пиши",
        "говори",
        "используй",
        "не используй",
        "предпочитаю",
        "мне удобнее",
        "мне нравится",
        "хочу",
        "давай",
        "запомни",
        "пожалуйста",
        "сделай",
        "настрой",
        "формат",
        "стиль",
    ];
    if CUES.iter().any(|cue| lower.contains(cue)) {
        return true;
    }
    // Прямая команда без глагола: «Кратко.», «Без кода», «С кодом, пожалуйста».
    const DIRECT: [&str; 10] = [
        "кратко",
        "коротко",
        "подробно",
        "формально",
        "неформально",
        "без кода",
        "с кодом",
        "без списков",
        "без таблиц",
        "без эмодзи",
    ];
    DIRECT.iter().any(|prefix| lower.starts_with(prefix))
}

fn detect_verbosity(lower: &str) -> Option<Verbosity> {
    if contains_any(lower, &["не подробно", "без подробностей", "не развёрнуто"])
    {
        return Some(Verbosity::Brief);
    }
    if contains_any(
        lower,
        &[
            "подробно",
            "подробнее",
            "развёрнуто",
            "развернуто",
            "детально",
            "подробный",
            "подробный ответ",
        ],
    ) {
        return Some(Verbosity::Detailed);
    }
    if contains_any(
        lower,
        &[
            "кратко",
            "коротко",
            "покороче",
            "без воды",
            "лаконично",
            "по делу",
            "краткий",
        ],
    ) {
        return Some(Verbosity::Brief);
    }
    if contains_any(lower, &["обычный объём", "обычно", "средне"]) {
        return Some(Verbosity::Normal);
    }
    None
}

fn detect_tone(lower: &str) -> Option<Tone> {
    if contains_any(
        lower,
        &[
            "неформально",
            "неформальный",
            "по-простому",
            "на ты",
            "дружелюбно",
            "без формальностей",
        ],
    ) {
        return Some(Tone::Informal);
    }
    if contains_any(
        lower,
        &[
            "формально",
            "формальный",
            "официально",
            "на вы",
            "деловой",
            "деловой тон",
        ],
    ) {
        return Some(Tone::Formal);
    }
    if contains_any(lower, &["нейтрально", "нейтральный"]) {
        return Some(Tone::Neutral);
    }
    None
}

fn detect_code(lower: &str) -> Option<CodePolicy> {
    if contains_any(
        lower,
        &[
            "без кода",
            "не пиши код",
            "не показывай код",
            "не нужен код",
            "без примеров кода",
            "no code",
        ],
    ) {
        return Some(CodePolicy::NoCode);
    }
    if contains_any(
        lower,
        &[
            "с кодом",
            "покажи код",
            "пример кода",
            "примеры кода",
            "с примерами кода",
            "пиши код",
            "код на rust",
            "with code",
        ],
    ) {
        return Some(CodePolicy::WithCode);
    }
    if contains_any(lower, &["код по ситуации", "код нейтрально"]) {
        return Some(CodePolicy::Neutral);
    }
    None
}

fn detect_lists(lower: &str) -> Option<bool> {
    if contains_any(
        lower,
        &[
            "без списков",
            "не списком",
            "одним абзацем",
            "связным текстом",
            "без маркированных списков",
        ],
    ) {
        return Some(false);
    }
    if contains_any(lower, &["списком", "списки", "маркированн", "пунктами"])
    {
        return Some(true);
    }
    None
}

fn detect_tables(lower: &str) -> Option<bool> {
    if contains_any(lower, &["без таблиц", "не таблицей", "не в таблице"])
    {
        return Some(false);
    }
    if contains_any(lower, &["таблицей", "таблицу", "таблицы", "в таблице"])
    {
        return Some(true);
    }
    None
}

fn detect_emoji(lower: &str) -> Option<bool> {
    if contains_any(
        lower,
        &[
            "без эмодзи",
            "без смайл",
            "не используй эмодзи",
            "не используй смайл",
        ],
    ) {
        return Some(false);
    }
    if contains_any(lower, &["с эмодзи", "эмодзи", "смайлик", "смайлики"])
    {
        return Some(true);
    }
    None
}

fn detect_language(lower: &str) -> Option<String> {
    if contains_any(
        lower,
        &[
            "на английском",
            "по-английски",
            "in english",
            "answer in english",
        ],
    ) {
        return Some("en".into());
    }
    if contains_any(
        lower,
        &["на русском", "по-русски", "in russian", "отвечай по-русски"],
    ) {
        return Some("ru".into());
    }
    if contains_any(
        lower,
        &["на немецком", "по-немецки", "in german", "auf deutsch"],
    ) {
        return Some("de".into());
    }
    None
}

fn detect_constraints(lower: &str) -> Vec<String> {
    let mut constraints = Vec::new();
    if contains_any(
        lower,
        &[
            "без вступлений",
            "без вступления",
            "не вступ",
            "без прелюдий",
        ],
    ) {
        constraints.push("без вступлений".into());
    }
    if contains_any(lower, &["без извинений", "не извиняйся", "не извиняйтесь"])
    {
        constraints.push("без извинений".into());
    }
    if contains_any(lower, &["только факты", "без домыслов", "не выдумывай"])
    {
        constraints.push("только факты".into());
    }
    if contains_any(
        lower,
        &["сначала вывод", "сначала ответ", "сначала главное"],
    ) {
        constraints.push("сначала вывод".into());
    }
    if contains_any(lower, &["без markdown", "без разметки"]) {
        constraints.push("без markdown".into());
    }
    constraints
}

fn contains_any(lower: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| lower.contains(needle))
}

fn normalize(value: &str) -> String {
    value
        .to_lowercase()
        .replace('ё', "е")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn snippet(value: &str) -> String {
    let clean: String = value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let mut result: String = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if result.chars().count() > EVIDENCE_LIMIT {
        result = result.chars().take(EVIDENCE_LIMIT - 1).collect::<String>();
        result.push('…');
    }
    result
}

fn after_marker(text: &str, lower: &str, markers: &[&str]) -> Option<String> {
    for marker in markers {
        if let Some(position) = lower.find(marker) {
            let rest = &text[position + marker.len()..];
            return Some(stop_at(rest));
        }
    }
    None
}

fn stop_at(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if matches!(ch, '.' | ',' | ';' | '!' | '?' | '\n') {
            break;
        }
        out.push(ch);
    }
    out.trim().to_string()
}

fn trim_name(value: &str) -> String {
    let name = stop_at(value);
    name.split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "да" } else { "нет" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 19, 10, 0, 0).unwrap()
    }

    #[test]
    fn learns_all_three_axes_in_one_message() {
        let mut profile = UserProfile::preset_default(now());
        let changes = learn_from_message(
            &mut profile,
            "Отвечай кратко, без кода и неформально, без вступлений.",
            now(),
        );
        assert_eq!(profile.verbosity, Verbosity::Brief);
        assert_eq!(profile.tone, Tone::Informal);
        assert_eq!(profile.format.code, CodePolicy::NoCode);
        assert!(
            profile
                .constraints
                .iter()
                .any(|item| item.contains("вступлен"))
        );
        assert!(changes.iter().any(|change| change.key == "style.verbosity"));
        assert!(changes.iter().any(|change| change.key == "format.code"));
        assert_eq!(profile.source, ProfileSource::Dialogue);
        assert_eq!(profile.revision, 1);
    }

    #[test]
    fn last_preference_wins() {
        let mut profile = UserProfile::preset_default(now());
        learn_from_message(&mut profile, "Отвечай кратко", now());
        assert_eq!(profile.verbosity, Verbosity::Brief);
        learn_from_message(&mut profile, "Нет, отвечай подробно", now());
        assert_eq!(profile.verbosity, Verbosity::Detailed);
        assert_eq!(profile.revision, 2);
        let last = profile.preferences.last().unwrap();
        assert_eq!(last.key, "style.verbosity");
        assert_eq!(last.value, "detailed");
    }

    #[test]
    fn small_talk_and_questions_do_not_change_profile() {
        let mut profile = UserProfile::preset_default(now());
        for text in [
            "Привет",
            "Спасибо, это всё",
            "Как лучше отвечать кратко?",
            "Наверное, успеем к декабрю",
        ] {
            let changes = learn_from_message(&mut profile, text, now());
            assert!(changes.is_empty(), "ложное срабатывание на {text}");
        }
        assert_eq!(profile, UserProfile::preset_default(now()));
    }

    #[test]
    fn learns_identity_and_language() {
        let mut profile = UserProfile::preset_default(now());
        learn_from_message(
            &mut profile,
            "Меня зовут Алексей, я работаю backend-разработчиком. Отвечай на английском.",
            now(),
        );
        assert_eq!(profile.identity.name.as_deref(), Some("Алексей"));
        assert_eq!(
            profile.identity.role.as_deref(),
            Some("backend-разработчиком")
        );
        assert_eq!(profile.identity.language, "en");
    }

    #[test]
    fn profile_block_contains_preferences() {
        let profile = UserProfile::preset_detailed(now());
        let block = render_profile_block(&profile);
        assert!(block.tokens > 0);
        assert!(block.text.contains("## Персонализация"));
        assert!(block.text.contains("подробно"));
        assert!(block.text.contains("с примерами кода"));
        assert!(block.text.contains("формальный"));
        assert!(block.text.contains("текущая явная просьба важнее"));
    }
}
