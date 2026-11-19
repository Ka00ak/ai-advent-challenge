//! Проверка персонализации: один вопрос под полярными профилями и
//! устойчивость предпочтений на 10 ответах подряд.
//!
//! Оценка детерминированная и не требует второй модели: проверяются
//! наблюдаемые признаки (объём, код, тон, списки, таблицы, эмодзи).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::memory::{ChatMessage, LayerFlags, MemoryBundle};
use crate::domain::profile::{CodePolicy, ProfileState, Tone, UserProfile, Verbosity};
use crate::domain::settings::Settings;
use crate::llm::dto::{ChatClient, ChatRequest, model_profile};
use crate::memory::prompt::{AssembleInput, SYSTEM_PROMPT, assemble_prompt};
use crate::memory::tokens::{estimate_text_tokens, format_tokens};
use crate::storage::atomic::{SaveError, atomic_write_bytes, atomic_write_json};

pub const SAME_QUESTION: &str = "Как в Rust безопасно прочитать JSON-файл и обработать ошибку?";

pub const STABILITY_QUESTIONS: [&str; 10] = [
    "Как в Rust безопасно прочитать JSON-файл и обработать ошибку?",
    "Чем Result отличается от panic! в Rust?",
    "Как обработать ошибку serde_json при разборе JSON?",
    "Как проверить, что файл существует, перед чтением?",
    "Что делает оператор ? в Rust?",
    "Как вернуть ошибку из функции в Rust?",
    "Как ограничить размер файла при чтении?",
    "Как протестировать чтение JSON без реального файла?",
    "Что делать, если файл оказался в другой кодировке?",
    "Как залогировать ошибку и не потерять исходную причину?",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceCheck {
    pub name: String,
    pub expected: String,
    pub observed: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileAnswer {
    pub profile_id: String,
    pub profile_title: String,
    pub preferences: String,
    pub answer: String,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub compliant: bool,
    pub checks: Vec<PreferenceCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityStep {
    pub index: usize,
    pub question: String,
    pub answer: String,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub compliant: bool,
    pub checks: Vec<PreferenceCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityReport {
    pub profile_id: String,
    pub profile_title: String,
    pub steps: Vec<StabilityStep>,
    /// Номер первого ответа (1-based), где предпочтение нарушено.
    pub first_violation: Option<usize>,
    pub compliant_count: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalizationThreshold {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalizationReport {
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub model: String,
    pub base_url: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub data_dir: Option<String>,
    pub question: String,
    pub profiles: Vec<ProfileAnswer>,
    pub compliant_profiles: usize,
    pub total_profiles: usize,
    pub stability: Option<StabilityReport>,
    pub thresholds: Vec<PersonalizationThreshold>,
    pub total_requests: usize,
    pub errors: Vec<String>,
}

impl PersonalizationReport {
    pub fn passed(&self) -> bool {
        self.thresholds.iter().all(|threshold| threshold.ok)
    }

    pub fn usage_totals(&self) -> (usize, usize, f64) {
        let mut prompt = 0;
        let mut completion = 0;
        for profile in &self.profiles {
            prompt += profile.prompt_tokens;
            completion += profile.completion_tokens;
        }
        if let Some(stability) = &self.stability {
            for step in &stability.steps {
                prompt += step.prompt_tokens;
                completion += step.completion_tokens;
            }
        }
        let (input_cost, output_cost) = model_profile(&self.model);
        let cost = (prompt as f64 / 1_000_000.0) * input_cost
            + (completion as f64 / 1_000_000.0) * output_cost;
        (prompt, completion, cost)
    }
}

#[derive(Debug, Clone)]
pub struct PersonalizationOptions {
    pub question: Option<String>,
    /// Пустой список — взять полярные профили `brief` и `detailed`.
    pub profile_ids: Vec<String>,
    pub stability_profile: Option<String>,
    pub include_stability: bool,
}

impl Default for PersonalizationOptions {
    fn default() -> Self {
        Self {
            question: None,
            profile_ids: Vec::new(),
            stability_profile: None,
            include_stability: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PersonalizationProgress {
    pub phase: String,
    pub profile: String,
    pub step: usize,
    pub steps_total: usize,
    pub done: usize,
    pub total: usize,
    pub last_ok: Option<bool>,
}

#[derive(Debug, Error)]
pub enum PersonalizationError {
    #[error("проверка персонализации отменена")]
    Cancelled,
    #[error("ошибка сборки промпта: {0}")]
    Prompt(String),
    #[error("нужно минимум два профиля для сравнения")]
    NotEnoughProfiles,
}

pub fn run_personalization(
    state: &ProfileState,
    settings: &Settings,
    client: &dyn ChatClient,
    options: &PersonalizationOptions,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(PersonalizationProgress),
) -> Result<PersonalizationReport, PersonalizationError> {
    let started_at = Utc::now();
    let now = Utc::now();
    let profiles = select_profiles(state, &options.profile_ids, now);
    if profiles.len() < 2 {
        return Err(PersonalizationError::NotEnoughProfiles);
    }
    let question = options
        .question
        .clone()
        .unwrap_or_else(|| SAME_QUESTION.to_string());
    let stability_profile =
        select_stability_profile(state, options.stability_profile.as_deref(), &profiles, now);
    let steps_total = if options.include_stability {
        STABILITY_QUESTIONS.len()
    } else {
        0
    };
    let total = profiles.len() + steps_total;
    let mut done = 0;
    let mut errors = Vec::new();
    let mut answers = Vec::new();

    for profile in &profiles {
        if cancel.load(Ordering::Relaxed) {
            return Err(PersonalizationError::Cancelled);
        }
        let mut bundle = MemoryBundle::empty(now);
        let result = ask(client, settings, profile, &mut bundle, &question)?;
        let checks = evaluate_profile(profile, &result.answer);
        let compliant = checks.iter().all(|check| check.ok);
        if let Some(error) = &result.error {
            errors.push(format!("{} / сравнение: {error}", profile.id));
        }
        done += 1;
        progress(PersonalizationProgress {
            phase: "compare".into(),
            profile: profile.id.clone(),
            step: done,
            steps_total: profiles.len(),
            done,
            total,
            last_ok: Some(compliant),
        });
        answers.push(ProfileAnswer {
            profile_id: profile.id.clone(),
            profile_title: profile.title.clone(),
            preferences: profile.summary(),
            answer: result.answer,
            prompt_tokens: result.prompt_tokens,
            completion_tokens: result.completion_tokens,
            compliant,
            checks,
        });
    }

    let stability = if options.include_stability {
        let profile = stability_profile.ok_or(PersonalizationError::NotEnoughProfiles)?;
        let mut bundle = MemoryBundle::empty(now);
        let mut steps = Vec::new();
        let mut first_violation = None;
        for (index, question) in STABILITY_QUESTIONS.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(PersonalizationError::Cancelled);
            }
            let result = ask(client, settings, &profile, &mut bundle, question)?;
            let checks = evaluate_profile(&profile, &result.answer);
            let compliant = checks.iter().all(|check| check.ok);
            if !compliant && first_violation.is_none() {
                first_violation = Some(index + 1);
            }
            if let Some(error) = &result.error {
                errors.push(format!(
                    "{} / устойчивость #{}: {error}",
                    profile.id,
                    index + 1
                ));
            }
            done += 1;
            progress(PersonalizationProgress {
                phase: "stability".into(),
                profile: profile.id.clone(),
                step: index + 1,
                steps_total: STABILITY_QUESTIONS.len(),
                done,
                total,
                last_ok: Some(compliant),
            });
            steps.push(StabilityStep {
                index: index + 1,
                question: (*question).to_string(),
                answer: result.answer,
                prompt_tokens: result.prompt_tokens,
                completion_tokens: result.completion_tokens,
                compliant,
                checks,
            });
        }
        let compliant_count = steps.iter().filter(|step| step.compliant).count();
        Some(StabilityReport {
            profile_id: profile.id.clone(),
            profile_title: profile.title.clone(),
            steps,
            first_violation,
            compliant_count,
            total: STABILITY_QUESTIONS.len(),
        })
    } else {
        None
    };

    let compliant_profiles = answers.iter().filter(|answer| answer.compliant).count();
    let mut report = PersonalizationReport {
        started_at,
        finished_at: Utc::now(),
        model: settings.model.clone(),
        base_url: settings.base_url.clone(),
        source: "api".into(),
        data_dir: None,
        question,
        profiles: answers,
        compliant_profiles,
        total_profiles: profiles.len(),
        stability,
        thresholds: Vec::new(),
        total_requests: done,
        errors,
    };
    report.thresholds = check_thresholds(&report);
    Ok(report)
}

struct AskResult {
    answer: String,
    prompt_tokens: usize,
    completion_tokens: usize,
    error: Option<String>,
}

fn ask(
    client: &dyn ChatClient,
    settings: &Settings,
    profile: &UserProfile,
    bundle: &mut MemoryBundle,
    question: &str,
) -> Result<AskResult, PersonalizationError> {
    bundle.stm.messages.push(ChatMessage::user(question));
    let now = Utc::now();
    let assembly = assemble_prompt(AssembleInput {
        system_prompt: SYSTEM_PROMPT,
        stm: &bundle.stm,
        wm: &bundle.wm,
        ltm: &bundle.ltm,
        config: &settings.memory,
        tools: &[],
        layers: LayerFlags::ALL,
        profile: Some(profile),
        now,
    })
    .map_err(|error| PersonalizationError::Prompt(error.to_string()))?;
    let estimated_prompt = assembly.breakdown.total_tokens;
    let request = ChatRequest {
        model: settings.model.clone(),
        messages: assembly.messages,
        temperature: settings.temperature,
        max_tokens: settings.max_tokens.or(Some(700)),
        tools: Vec::new(),
    };
    match client.complete(&request) {
        Ok(response) => {
            let answer = response.content.unwrap_or_default();
            let prompt_tokens = response
                .usage
                .as_ref()
                .map(|usage| usage.prompt_tokens)
                .unwrap_or(estimated_prompt);
            let completion_tokens = response
                .usage
                .as_ref()
                .map(|usage| usage.completion_tokens)
                .unwrap_or_else(|| estimate_text_tokens(&answer));
            bundle
                .stm
                .messages
                .push(ChatMessage::assistant(answer.clone()));
            bundle.stm.updated_at = Utc::now();
            Ok(AskResult {
                answer,
                prompt_tokens,
                completion_tokens,
                error: None,
            })
        }
        Err(error) => {
            let answer = "ошибка запроса".to_string();
            bundle
                .stm
                .messages
                .push(ChatMessage::assistant(answer.clone()));
            Ok(AskResult {
                answer,
                prompt_tokens: estimated_prompt,
                completion_tokens: 0,
                error: Some(error.to_string()),
            })
        }
    }
}

fn select_profiles(state: &ProfileState, ids: &[String], now: DateTime<Utc>) -> Vec<UserProfile> {
    if !ids.is_empty() {
        return ids
            .iter()
            .filter_map(|id| state.find(id).cloned())
            .collect();
    }
    let mut selected = Vec::new();
    for id in ["brief", "detailed"] {
        if let Some(profile) = state.find(id) {
            selected.push(profile.clone());
        }
    }
    if selected.len() < 2 {
        for profile in state
            .profiles
            .iter()
            .filter(|profile| profile.id != "default")
        {
            if selected.len() >= 3 {
                break;
            }
            if !selected.iter().any(|item| item.id == profile.id) {
                selected.push(profile.clone());
            }
        }
    }
    if selected.len() < 2 {
        for id in ["brief", "detailed"] {
            if !selected.iter().any(|item| item.id == id) {
                if let Some(profile) = UserProfile::preset(id, now) {
                    selected.push(profile);
                }
            }
        }
    }
    selected.truncate(3);
    selected
}

fn select_stability_profile(
    state: &ProfileState,
    requested: Option<&str>,
    selected: &[UserProfile],
    now: DateTime<Utc>,
) -> Option<UserProfile> {
    if let Some(profile) = requested.and_then(|id| state.find(id)) {
        return Some(profile.clone());
    }
    if let Some(profile) = state.find("detailed") {
        return Some(profile.clone());
    }
    if let Some(profile) = state.find("brief") {
        return Some(profile.clone());
    }
    selected
        .first()
        .cloned()
        .or_else(|| UserProfile::preset("detailed", now))
}

/// Детерминированная оценка: проверяются только наблюдаемые признаки.
pub fn evaluate_profile(profile: &UserProfile, answer: &str) -> Vec<PreferenceCheck> {
    let words = word_count(answer);
    let mut checks = Vec::new();
    match profile.verbosity {
        Verbosity::Brief => checks.push(check(
            "объём",
            "≤ 80 слов",
            format!("{words} слов"),
            words <= 80,
        )),
        Verbosity::Detailed => checks.push(check(
            "объём",
            "≥ 70 слов",
            format!("{words} слов"),
            words >= 70,
        )),
        Verbosity::Normal => {}
    }
    match profile.tone {
        Tone::Formal => {
            let informal = informal_words(answer);
            checks.push(check(
                "тон",
                "без «ты/тебе/слушай»",
                if informal.is_empty() {
                    "формальный".into()
                } else {
                    informal.join(", ")
                },
                informal.is_empty(),
            ));
        }
        Tone::Informal => {
            let informal = informal_words(answer);
            checks.push(check(
                "тон",
                "есть «ты/тебе/слушай»",
                if informal.is_empty() {
                    "нейтральный".into()
                } else {
                    informal.join(", ")
                },
                !informal.is_empty(),
            ));
        }
        Tone::Neutral => {}
    }
    match profile.format.code {
        CodePolicy::WithCode => {
            let code = has_code(answer);
            checks.push(check("код", "есть код", yes_no(code).into(), code));
        }
        CodePolicy::NoCode => {
            let code = has_code(answer);
            checks.push(check("код", "без кода", yes_no(code).into(), !code));
        }
        CodePolicy::Neutral => {}
    }
    let lists = has_list(answer);
    checks.push(check(
        "списки",
        if profile.format.lists {
            "есть"
        } else {
            "нет"
        },
        yes_no(lists).into(),
        lists == profile.format.lists,
    ));
    let tables = has_table(answer);
    checks.push(check(
        "таблицы",
        if profile.format.tables {
            "есть"
        } else {
            "нет"
        },
        yes_no(tables).into(),
        tables == profile.format.tables,
    ));
    let emoji = has_emoji(answer);
    checks.push(check(
        "эмодзи",
        if profile.format.emoji {
            "есть"
        } else {
            "нет"
        },
        yes_no(emoji).into(),
        emoji == profile.format.emoji,
    ));
    for constraint in &profile.constraints {
        let lower = constraint.to_lowercase();
        if lower.contains("вступлен") {
            let intro = has_intro(answer);
            checks.push(check(
                "ограничение",
                "без вступлений",
                if intro {
                    "есть вступление".into()
                } else {
                    "без вступления".into()
                },
                !intro,
            ));
        }
    }
    checks
}

fn check(name: &str, expected: &str, observed: String, ok: bool) -> PreferenceCheck {
    PreferenceCheck {
        name: name.to_string(),
        expected: expected.to_string(),
        observed,
        ok,
    }
}

fn word_count(answer: &str) -> usize {
    answer
        .split_whitespace()
        .filter(|word| word.chars().any(|ch| ch.is_alphabetic()))
        .count()
}

fn tokenize(answer: &str) -> Vec<String> {
    answer
        .to_lowercase()
        .split(|ch: char| !ch.is_alphabetic())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

fn informal_words(answer: &str) -> Vec<String> {
    const MARKERS: [&str; 29] = [
        "ты",
        "тебе",
        "тебя",
        "твой",
        "твоя",
        "твоё",
        "твои",
        "слушай",
        "короче",
        "окей",
        "возьми",
        "читай",
        "читаешь",
        "смотри",
        "посмотри",
        "делай",
        "делаешь",
        "бери",
        "запомни",
        "проверь",
        "открой",
        "пиши",
        "пробрось",
        "замапь",
        "оборачивай",
        "используй",
        "возвращай",
        "парсишь",
        "обрабатываешь",
    ];
    let tokens = tokenize(answer);
    let mut found: Vec<String> = MARKERS
        .iter()
        .filter(|marker| tokens.iter().any(|token| token == *marker))
        .map(|marker| (*marker).to_string())
        .collect();
    // Второе лицо единственного числа: «читаешь», «пробрасываешь»,
    // «декодируешь». Это надёжный признак неформального обращения.
    for token in &tokens {
        if token.chars().count() >= 5
            && (token.ends_with("ешь") || token.ends_with("ишь"))
            && !found.contains(token)
        {
            found.push(token.clone());
        }
    }
    found
}

fn has_code(answer: &str) -> bool {
    if answer.contains("```") {
        return true;
    }
    answer.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("fn ")
            || line.starts_with("pub fn ")
            || line.starts_with("let ")
            || line.starts_with("use ")
            || line.starts_with("impl ")
            || line.starts_with("#[derive")
    })
}

fn has_list(answer: &str) -> bool {
    answer.lines().any(|line| {
        let line = line.trim_start();
        if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("• ") {
            return true;
        }
        let mut chars = line.chars().peekable();
        let mut digits = 0;
        while chars.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            chars.next();
            digits += 1;
        }
        digits > 0 && chars.next().is_some_and(|ch| ch == '.' || ch == ')')
    })
}

fn has_table(answer: &str) -> bool {
    answer
        .lines()
        .any(|line| line.matches('|').count() >= 2 && line.contains("---"))
        || answer
            .lines()
            .filter(|line| line.matches('|').count() >= 2)
            .count()
            >= 3
}

fn has_emoji(answer: &str) -> bool {
    answer.chars().any(|ch| {
        matches!(ch as u32,
            0x1F300..=0x1FAFF
            | 0x2600..=0x27BF
            | 0xFE00..=0xFE0F
        )
    })
}

fn has_intro(answer: &str) -> bool {
    let start = answer.trim_start().to_lowercase();
    ["конечно", "отличный вопрос", "хороший вопрос", "разумеется"]
        .iter()
        .any(|marker| start.starts_with(marker))
}

fn yes_no(value: bool) -> &'static str {
    if value { "да" } else { "нет" }
}

fn check_thresholds(report: &PersonalizationReport) -> Vec<PersonalizationThreshold> {
    let mut thresholds = Vec::new();
    thresholds.push(PersonalizationThreshold {
        name: "профилей ≥ 2".into(),
        ok: report.total_profiles >= 2,
        detail: format!("{}", report.total_profiles),
    });
    thresholds.push(PersonalizationThreshold {
        name: "предпочтения соблюдены".into(),
        ok: report.compliant_profiles == report.total_profiles,
        detail: format!(
            "{}/{} профилей",
            report.compliant_profiles, report.total_profiles
        ),
    });
    if let Some(stability) = &report.stability {
        thresholds.push(PersonalizationThreshold {
            name: "устойчивость ≥ 8/10".into(),
            ok: stability.compliant_count >= 8,
            detail: format!(
                "{}/{}; первое нарушение: {}",
                stability.compliant_count,
                stability.total,
                stability
                    .first_violation
                    .map(|index| format!("#{index}"))
                    .unwrap_or_else(|| "нет".into())
            ),
        });
    }
    thresholds.push(PersonalizationThreshold {
        name: "без ошибок API".into(),
        ok: report.errors.is_empty(),
        detail: format!("{} ошибок", report.errors.len()),
    });
    thresholds
}

pub fn render_markdown(report: &PersonalizationReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Персонализация ассистента — {}\n\n",
        report.started_at.format("%Y-%m-%d")
    ));
    if report.source.to_lowercase().contains("fake") {
        out.push_str(
            "> **Mock-прогон:** fake-клиент. Проверена механика профиля, инъекции\n> и оценки; это не измерение качества реальной модели.\n\n",
        );
    }
    out.push_str(&format!(
        "Модель: {} · источник: {} · запросов: {}\n\n",
        report.model,
        if report.source.is_empty() {
            "api"
        } else {
            &report.source
        },
        report.total_requests
    ));
    let (prompt_tokens, completion_tokens, cost) = report.usage_totals();
    out.push_str("## Условия\n\n| Параметр | Значение |\n|---|---|\n");
    out.push_str(&format!("| Модель | {} |\n", report.model));
    out.push_str(&format!("| Base URL | {} |\n", report.base_url));
    out.push_str(&format!(
        "| Data-dir | {} |\n",
        report.data_dir.as_deref().unwrap_or("—")
    ));
    out.push_str(&format!("| Вопрос | {} |\n", escape(&report.question)));
    out.push_str(&format!(
        "| Токены API | prompt {} · completion {} · всего {} |\n",
        format_tokens(prompt_tokens),
        format_tokens(completion_tokens),
        format_tokens(prompt_tokens + completion_tokens)
    ));
    out.push_str(&format!("| Стоимость (оценка) | ${cost:.6} |\n\n"));

    out.push_str("## Профили\n\n| Профиль | Предпочтения | Соблюдено |\n|---|---|---|\n");
    for profile in &report.profiles {
        out.push_str(&format!(
            "| {} (`{}`) | {} | {} |\n",
            escape(&profile.profile_title),
            profile.profile_id,
            escape(&profile.preferences),
            if profile.compliant { "да" } else { "нет" }
        ));
    }
    out.push_str(&format!(
        "\n**Соблюдено профилей: {} из {}.**\n\n",
        report.compliant_profiles, report.total_profiles
    ));

    out.push_str("## Один вопрос — разные профили\n\n");
    for profile in &report.profiles {
        out.push_str(&format!(
            "### {} (`{}`)\n\nПредпочтения: {}\n\n",
            profile.profile_title, profile.profile_id, profile.preferences
        ));
        out.push_str(&format!(
            "Ответ:\n\n> {}\n\n",
            escape_multiline(&profile.answer)
        ));
        out.push_str("| Проверка | Ожидание | Наблюдение | Итог |\n|---|---|---|---|\n");
        for check in &profile.checks {
            out.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                check.name,
                escape(&check.expected),
                escape(&check.observed),
                if check.ok { "ok" } else { "FAIL" }
            ));
        }
        out.push('\n');
    }

    if let Some(stability) = &report.stability {
        out.push_str("## Устойчивость: 10 вопросов подряд\n\n");
        out.push_str(&format!(
            "Профиль: {} (`{}`). Соблюдено {}/{}.\n\n",
            stability.profile_title,
            stability.profile_id,
            stability.compliant_count,
            stability.total
        ));
        out.push_str("| # | Вопрос | Слов | Соблюдено | Нарушения |\n|---:|---|---:|---|---|\n");
        for step in &stability.steps {
            let failed: Vec<String> = step
                .checks
                .iter()
                .filter(|check| !check.ok)
                .map(|check| format!("{}: {}", check.name, check.observed))
                .collect();
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                step.index,
                escape(&step.question),
                word_count(&step.answer),
                if step.compliant { "да" } else { "нет" },
                if failed.is_empty() {
                    "—".into()
                } else {
                    escape(&failed.join("; "))
                }
            ));
        }
        out.push_str(&format!(
            "\nПервое нарушение: **{}**.\n\n",
            stability
                .first_violation
                .map(|index| format!("ответ #{index}"))
                .unwrap_or_else(|| "не зафиксировано".into())
        ));
    }

    out.push_str("## Пороги\n\n| Проверка | Результат | Детали |\n|---|---|---|\n");
    for threshold in &report.thresholds {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            threshold.name,
            if threshold.ok { "ok" } else { "FAIL" },
            escape(&threshold.detail)
        ));
    }
    out.push_str(&format!(
        "\nИтог: **{}**.\n\n",
        if report.passed() {
            "персонализация подтверждена"
        } else {
            "есть нарушения предпочтений"
        }
    ));

    out.push_str("## Выводы\n\n");
    out.push_str(&format!(
        "1. Один и тот же вопрос задан {} профилям; предпочтения соблюдены в {} ответах.\n",
        report.total_profiles, report.compliant_profiles
    ));
    if let Some(stability) = &report.stability {
        out.push_str(&format!(
            "2. В серии из {} вопросов соблюдено {}; первое нарушение — {}.\n",
            stability.total,
            stability.compliant_count,
            stability
                .first_violation
                .map(|index| format!("ответ #{index}"))
                .unwrap_or_else(|| "не зафиксировано".into())
        ));
    }
    out.push_str("3. Профиль передаётся отдельным системным блоком перед блоками памяти; текущая реплика пользователя имеет приоритет.\n");
    out.push_str("4. Профиль пополняется из диалога детерминированными правилами и хранит происхождение изменений.\n");
    if !report.errors.is_empty() {
        out.push_str(&format!(
            "\nОшибки запросов: {} (см. JSON).\n",
            report.errors.len()
        ));
    }
    out.push_str("\n## Ограничения\n\n");
    out.push_str("- Оценка по наблюдаемым признакам (слова, код, списки, тон), не семантика.\n");
    out.push_str("- Одна модель и один прогон; температура и версия модели влияют на результат.\n");
    out.push_str("- Реальный прогон требует API-ключа; fake-прогон проверяет только механику.\n");
    out
}

fn escape(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn escape_multiline(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', "\n> ")
}

pub fn save_personalization_report(
    report: &PersonalizationReport,
    markdown_path: &Path,
    json_path: &Path,
) -> Result<(), SaveError> {
    atomic_write_bytes(markdown_path, render_markdown(report).as_bytes())?;
    atomic_write_json(json_path, report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::profile::UserProfile;
    use crate::llm::client::FakeChatClient;

    fn profiles() -> ProfileState {
        ProfileState::with_presets(Utc::now())
    }

    fn settings() -> Settings {
        Settings {
            temperature: 0.0,
            max_tokens: Some(700),
            ..Settings::default()
        }
    }

    #[test]
    fn fake_run_is_compliant_for_both_profiles_and_stable() {
        let cancel = AtomicBool::new(false);
        let report = run_personalization(
            &profiles(),
            &settings(),
            &FakeChatClient,
            &PersonalizationOptions::default(),
            &cancel,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(report.total_profiles, 2);
        assert_eq!(report.compliant_profiles, 2);
        assert_eq!(report.total_requests, 12);
        let stability = report.stability.as_ref().unwrap();
        assert_eq!(stability.compliant_count, 10);
        assert_eq!(stability.first_violation, None);
        assert!(report.passed(), "{:?}", report.thresholds);
        assert!(report.usage_totals().0 > 0);
    }

    #[test]
    fn evaluator_detects_violations() {
        let brief = UserProfile::preset_brief(Utc::now());
        let long_code = "```rust\nfn main() {}\n```\n".to_string() + &"слово ".repeat(150);
        let checks = evaluate_profile(&brief, &long_code);
        assert!(checks.iter().any(|check| !check.ok));

        let detailed = UserProfile::preset_detailed(Utc::now());
        let checks = evaluate_profile(&detailed, "Слушай, коротко: всё ок.");
        assert!(checks.iter().any(|check| !check.ok));
    }

    #[test]
    fn markdown_contains_required_sections_and_count() {
        let cancel = AtomicBool::new(false);
        let report = run_personalization(
            &profiles(),
            &settings(),
            &FakeChatClient,
            &PersonalizationOptions::default(),
            &cancel,
            &mut |_| {},
        )
        .unwrap();
        let markdown = render_markdown(&report);
        for needle in [
            "## Профили",
            "## Один вопрос — разные профили",
            "## Устойчивость",
            "## Пороги",
            "Соблюдено профилей: 2 из 2",
            "Первое нарушение",
        ] {
            assert!(markdown.contains(needle), "нет {needle}");
        }
        let json = serde_json::to_string(&report).unwrap();
        let parsed: PersonalizationReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.total_requests, report.total_requests);
    }

    #[test]
    fn selected_profiles_are_brief_and_detailed_by_default() {
        let selected = select_profiles(&profiles(), &[], Utc::now());
        let ids: Vec<&str> = selected.iter().map(|profile| profile.id.as_str()).collect();
        assert_eq!(ids, vec!["brief", "detailed"]);
    }
}
