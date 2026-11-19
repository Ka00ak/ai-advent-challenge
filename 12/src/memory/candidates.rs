//! Локальное извлечение кандидатов (0 дополнительных запросов).
//!
//! Эвристика Дня 10 + маркеры задачи/scope. Значения не переписываются:
//! числа, даты и имена сохраняются дословно.

use crate::domain::memory::{CandidateScope, LtmKind, MemoryCandidate};

pub struct LocalExtractor;

impl LocalExtractor {
    pub fn extract_candidates(&self, text: &str) -> Vec<MemoryCandidate> {
        extract_local(text)
    }
}

fn candidate(
    key: &str,
    value: &str,
    kind: LtmKind,
    scope: CandidateScope,
    confidence: f32,
) -> MemoryCandidate {
    MemoryCandidate {
        key: key.to_string(),
        value: value.trim().to_string(),
        kind,
        tags: Vec::new(),
        confidence,
        scope,
        ttl_turns: None,
        legacy_category: None,
    }
}

/// Основная локальная эвристика. Порядок важен: более специфичные шаблоны
/// идут раньше, чтобы не породить лишних кандидатов.
pub fn extract_local(text: &str) -> Vec<MemoryCandidate> {
    let mut out: Vec<MemoryCandidate> = Vec::new();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return out;
    }
    let lower = trimmed.to_lowercase();

    if let Some(name) = project_name(trimmed) {
        out.push(candidate(
            "project.name",
            &name,
            LtmKind::Project,
            CandidateScope::Project,
            0.8,
        ));
    }
    if let Some(value) = after_marker(
        trimmed,
        &lower,
        &["заказчик — ", "заказчик - ", "заказчик: ", "заказчик "],
    ) {
        out.push(candidate(
            "customer",
            &value,
            LtmKind::Project,
            CandidateScope::Project,
            0.8,
        ));
    }
    if let Some(value) = after_marker(trimmed, &lower, &["тимлид — ", "тимлид - ", "тимлид: "])
    {
        out.push(candidate(
            "team.lead",
            &value,
            LtmKind::Project,
            CandidateScope::Project,
            0.8,
        ));
    }
    if let Some(value) = after_marker(trimmed, &lower, &["меня зовут ", "моё имя ", "мое имя "])
    {
        out.push(candidate(
            "user.name",
            &value,
            LtmKind::Profile,
            CandidateScope::Global,
            0.8,
        ));
    }
    if let Some(value) = date_after(
        trimmed,
        &lower,
        &["дедлайн проекта", "дедлайн", "срок проекта", "срок"],
    ) {
        out.push(candidate(
            "project.deadline",
            &value,
            LtmKind::Knowledge,
            CandidateScope::Project,
            0.8,
        ));
    }
    if lower.contains("маркетинг") {
        if let Some(value) = money_near(&lower, trimmed, "маркетинг") {
            out.push(candidate(
                "budget.marketing",
                &value,
                LtmKind::Knowledge,
                CandidateScope::Project,
                0.8,
            ));
        }
    }
    if !lower.contains("маркетинг") {
        if let Some(value) = money_after(
            trimmed,
            &lower,
            &[
                "бюджет проекта — ",
                "бюджет проекта - ",
                "бюджет — ",
                "бюджет - ",
                "бюджет: ",
            ],
        ) {
            out.push(candidate(
                "budget.total",
                &value,
                LtmKind::Knowledge,
                CandidateScope::Project,
                0.8,
            ));
        }
    }
    if lower.contains("цод")
        || lower.contains("облака")
        || lower.contains("ограничение")
        || lower.contains("запрещ")
    {
        if let Some(value) = constraint_value(trimmed, &lower) {
            out.push(candidate(
                "constraint.infra",
                &value,
                LtmKind::Knowledge,
                CandidateScope::Project,
                0.8,
            ));
        }
    }
    if let Some(value) = team_size(trimmed, &lower) {
        out.push(candidate(
            "team.size",
            &value,
            LtmKind::Knowledge,
            CandidateScope::Project,
            0.8,
        ));
    }
    if let Some(value) = date_after(trimmed, &lower, &["первый прототип", "прототип"])
    {
        out.push(candidate(
            "milestone.prototype",
            &value,
            LtmKind::Knowledge,
            CandidateScope::Project,
            0.8,
        ));
    }
    if let Some(value) = date_after(trimmed, &lower, &["публичный релиз", "релиз"])
    {
        out.push(candidate(
            "milestone.release",
            &value,
            LtmKind::Knowledge,
            CandidateScope::Project,
            0.8,
        ));
    }
    if lower.contains("postgresql")
        || lower.contains("mysql")
        || lower.contains("mongodb")
        || lower.contains("базе данных")
    {
        if let Some(value) = decision_value(trimmed, "PostgreSQL", "MongoDB") {
            out.push(candidate(
                "decision.database",
                &value,
                LtmKind::Decision,
                CandidateScope::Project,
                0.8,
            ));
        }
    }
    if lower.contains("rest") || lower.contains("graphql") || lower.contains("api") {
        if let Some(value) = decision_value(trimmed, "REST", "GraphQL") {
            out.push(candidate(
                "decision.api",
                &value,
                LtmKind::Decision,
                CandidateScope::Project,
                0.8,
            ));
        }
    }
    if let Some(value) = preference_value(trimmed, &lower) {
        out.push(candidate(
            "preference.style",
            &value,
            LtmKind::Preference,
            CandidateScope::Global,
            0.8,
        ));
    }
    if let Some(value) = after_marker(trimmed, &lower, &["текущая задача: ", "текущая задача — "])
    {
        out.push(candidate(
            "task.goal",
            &value,
            LtmKind::Project,
            CandidateScope::Task,
            0.8,
        ));
    }
    for (key, value, order) in plan_items(&lower, trimmed) {
        out.push(MemoryCandidate {
            key,
            value,
            kind: LtmKind::Project,
            tags: Vec::new(),
            confidence: 0.8,
            scope: CandidateScope::Task,
            ttl_turns: None,
            legacy_category: Some(format!("plan-order:{order}")),
        });
    }
    if lower.contains("сейчас на шаге") {
        if let Some(number) = first_number(&lower) {
            out.push(candidate(
                "plan.current",
                &format!("шаг {number}"),
                LtmKind::Project,
                CandidateScope::Task,
                0.8,
            ));
        }
    }
    if let Some(value) = after_marker(trimmed, &lower, &["статус: ", "статус — "]) {
        out.push(candidate(
            "status.current",
            &value,
            LtmKind::Project,
            CandidateScope::Task,
            0.8,
        ));
    }
    if let Some(value) = after_marker(
        trimmed,
        &lower,
        &["открытый вопрос: ", "открытый вопрос — "],
    ) {
        let key = if value.to_lowercase().contains("pr")
            || value.to_lowercase().contains("пиар")
            || value.to_lowercase().contains("подрядчик")
        {
            "question.pr"
        } else {
            "question.open"
        };
        out.push(candidate(
            key,
            &value,
            LtmKind::Project,
            CandidateScope::Task,
            0.8,
        ));
    }
    if (lower.contains("выбери вариант") || lower.contains("рабочий вариант"))
        && (lower.contains("вариант b") || lower.contains("вариант в"))
    {
        out.push(candidate(
            "estimate.variant",
            "Вариант B",
            LtmKind::Project,
            CandidateScope::Task,
            0.8,
        ));
    }
    if lower.contains("черновик") || lower.contains("файл") {
        if let Some(value) = file_name(trimmed) {
            out.push(candidate(
                "note.file",
                &value,
                LtmKind::Project,
                CandidateScope::Task,
                0.6,
            ));
        }
    }
    if lower.contains("для этой задачи") {
        if lower.contains("курс") {
            if let Some(value) = money_near(&lower, trimmed, "курс")
                .or_else(|| after_marker(trimmed, &lower, &["курс "]))
            {
                out.push(candidate(
                    "task.rate",
                    &value,
                    LtmKind::Project,
                    CandidateScope::Task,
                    0.6,
                ));
            }
        } else if let Some(value) = after_marker(trimmed, &lower, &["для этой задачи "])
        {
            out.push(candidate(
                "task.note",
                &value,
                LtmKind::Project,
                CandidateScope::Task,
                0.6,
            ));
        }
    }

    out
}

fn project_name(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let project_pos = lower.find("проект")?;
    let start = text[project_pos..].find('«')? + project_pos;
    let end = text[start..].find('»')? + start;
    let name = text[start + '«'.len_utf8()..end].trim();
    if name.is_empty() {
        None
    } else {
        Some(format!("«{name}»"))
    }
}

fn after_marker(text: &str, lower: &str, markers: &[&str]) -> Option<String> {
    for marker in markers {
        if let Some(pos) = lower.find(marker) {
            let rest = &text[pos + marker.len()..];
            return Some(stop_at(rest));
        }
    }
    None
}

fn date_after(text: &str, lower: &str, markers: &[&str]) -> Option<String> {
    for marker in markers {
        if let Some(pos) = lower.find(marker) {
            let rest = &text[pos + marker.len()..];
            let cleaned = rest.trim_start_matches([' ', '—', '-', ':', '–']);
            if let Some(date) = find_date(cleaned) {
                return Some(date);
            }
        }
    }
    None
}

fn find_date(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    for start in 0..chars.len() {
        if !chars[start].is_ascii_digit() {
            continue;
        }
        let mut end = start;
        while end < chars.len()
            && (chars[end].is_ascii_alphanumeric()
                || chars[end].is_alphabetic()
                || chars[end].is_whitespace()
                || chars[end] == '.'
                || chars[end] == ' ')
        {
            end += 1;
        }
        let candidate: String = chars[start..end].iter().collect();
        let candidate = trim_trailing_words(&candidate);
        if candidate.len() >= 6 && candidate.chars().any(|ch| ch.is_alphabetic()) {
            return Some(candidate);
        }
    }
    None
}

fn trim_trailing_words(value: &str) -> String {
    let mut text = value.trim().to_string();
    for suffix in [" года", " г.", " года."] {
        if text.ends_with(suffix) {
            text.truncate(text.len() - suffix.len());
        }
    }
    text.trim_end_matches('.').to_string()
}

fn money_after(text: &str, lower: &str, markers: &[&str]) -> Option<String> {
    for marker in markers {
        if let Some(pos) = lower.find(marker) {
            let rest = &text[pos + marker.len()..];
            if let Some(value) = find_money(rest) {
                return Some(value);
            }
        }
    }
    None
}

fn money_near(lower: &str, text: &str, marker: &str) -> Option<String> {
    let pos = lower.find(marker)?;
    let from = pos.saturating_sub(20);
    let to = (pos + marker.len() + 80).min(text.len());
    let mut start = from;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = to;
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    find_money(&text[start..end])
}

fn find_money(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    for start in 0..chars.len() {
        if !chars[start].is_ascii_digit() {
            continue;
        }
        let mut end = start;
        while end < chars.len()
            && (chars[end].is_ascii_digit()
                || chars[end] == ' '
                || chars[end] == '\u{00a0}'
                || chars[end] == '.'
                || chars[end] == ',')
        {
            end += 1;
        }
        let mut value: String = chars[start..end].iter().collect();
        value = value.trim().to_string();
        if value.chars().filter(|ch| ch.is_ascii_digit()).count() < 3 && !value.contains(' ') {
            continue;
        }
        let tail: String = chars[end..(end + 12).min(chars.len())].iter().collect();
        let tail_lower = tail.to_lowercase();
        if tail_lower.starts_with(" руб") || tail_lower.starts_with(" ₽") {
            let units_len = tail_lower
                .find(|ch: char| {
                    !(ch.is_alphabetic() || ch.is_whitespace() || ch == '₽' || ch == '.')
                })
                .unwrap_or(tail.len().min(8));
            let units: String = tail.chars().take(units_len).collect();
            value.push_str(units.trim_end_matches('.'));
            return Some(value.trim().to_string());
        }
        if value.chars().filter(|ch| ch.is_ascii_digit()).count() >= 3 {
            return Some(value);
        }
    }
    None
}

fn team_size(text: &str, lower: &str) -> Option<String> {
    let pos = lower.find("команде")?;
    let rest = &text[pos..];
    let digits_start = rest.find(|ch: char| ch.is_ascii_digit())?;
    let rest = &rest[digits_start..];
    let mut value = String::new();
    for word in rest.split_whitespace().take(2) {
        if !value.is_empty() {
            value.push(' ');
        }
        value.push_str(word.trim_matches(['.', ',']));
    }
    if value.contains("разработчик") {
        Some(value)
    } else {
        None
    }
}

fn constraint_value(text: &str, lower: &str) -> Option<String> {
    if let Some(value) = after_marker(text, lower, &["ограничение: ", "ограничение — "])
    {
        return Some(value);
    }
    let pos = lower.find("только").or_else(|| lower.find("запрещ"))?;
    let rest = &text[pos..];
    Some(stop_at(rest))
}

fn decision_value(text: &str, primary: &str, secondary: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let pos = lower.find(&primary.to_lowercase())?;
    let rest = &text[pos..];
    let value = stop_at(rest);
    if value.is_empty() {
        return None;
    }
    if value.to_lowercase().contains(&secondary.to_lowercase()) {
        // уже включает оба варианта
    }
    Some(value)
}

fn preference_value(text: &str, lower: &str) -> Option<String> {
    if let Some(value) = after_marker(text, lower, &["предпочтение: ", "предпочтение — "])
    {
        return Some(capitalize_first(&value));
    }
    if lower.contains("отвечай коротко") {
        return Some("Отвечать коротко".to_string());
    }
    if lower.contains("отвечай ") {
        if let Some(pos) = lower.find("отвечай ") {
            return Some(capitalize_first(&stop_at(&text[pos..])));
        }
    }
    None
}

fn plan_items(lower: &str, text: &str) -> Vec<(String, String, u32)> {
    let Some(pos) = lower.find("план:") else {
        return Vec::new();
    };
    let rest = &text[pos + "план:".len()..];
    let mut items = Vec::new();
    let chars: Vec<char> = rest.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_ascii_digit() {
            let mut number = String::new();
            while index < chars.len() && chars[index].is_ascii_digit() {
                number.push(chars[index]);
                index += 1;
            }
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
            if index < chars.len() && (chars[index] == ')' || chars[index] == '.') {
                index += 1;
                while index < chars.len() && chars[index].is_whitespace() {
                    index += 1;
                }
                let start = index;
                while index < chars.len() && chars[index] != ',' && chars[index] != '.' {
                    index += 1;
                }
                let value: String = chars[start..index].iter().collect();
                let value = value.trim().trim_end_matches('.').to_string();
                if !value.is_empty() {
                    if let Ok(order) = number.parse::<u32>() {
                        items.push((format!("plan.{order}"), value, order));
                    }
                }
            }
        }
        index += 1;
    }
    items
}

fn first_number(text: &str) -> Option<String> {
    let mut value = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            value.push(ch);
        } else if !value.is_empty() {
            break;
        }
    }
    (!value.is_empty()).then_some(value)
}

fn file_name(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|word| {
            word.trim_matches(|ch: char| {
                ch == '.' || ch == ',' || ch == '«' || ch == '»' || ch == '"' || ch == '\''
            })
        })
        .find(|word| {
            let lower = word.to_lowercase();
            lower.contains('.')
                && ["xlsx", "xls", "docx", "pdf", "txt", "csv", "md", "json"]
                    .iter()
                    .any(|ext| lower.ends_with(ext))
        })
        .map(str::to_string)
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

fn capitalize_first(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(text: &str) -> Vec<String> {
        extract_local(text).into_iter().map(|c| c.key).collect()
    }

    #[test]
    fn extracts_project_and_customer() {
        let candidates = extract_local(
            "Запускаем проект «Атлас». Заказчик — Марина Ковалёва, она принимает результат.",
        );
        assert!(candidates.iter().any(|c| c.key == "project.name"
            && c.value == "«Атлас»"
            && c.kind == LtmKind::Project));
        assert!(candidates.iter().any(|c| c.key == "customer"
            && c.value == "Марина Ковалёва"
            && c.scope == CandidateScope::Project));
    }

    #[test]
    fn extracts_deadline_and_budget_verbatim() {
        let keys = extract_local(
            "Дедлайн проекта — 15 декабря 2026 года. Бюджет — 480 000 рублей, больше не согласуют.",
        );
        let deadline = keys.iter().find(|c| c.key == "project.deadline").unwrap();
        assert!(deadline.value.starts_with("15 декабря 2026"));
        let budget = keys.iter().find(|c| c.key == "budget.total").unwrap();
        assert!(budget.value.contains("480 000"));
    }

    #[test]
    fn extracts_marketing_budget_update() {
        let candidates = extract_local("Бюджет маркетинга подняли до 150 000 рублей.");
        let budget = candidates
            .iter()
            .find(|c| c.key == "budget.marketing")
            .unwrap();
        assert!(budget.value.contains("150 000"));
    }

    #[test]
    fn extracts_decisions() {
        let candidates =
            extract_local("По базе данных выбрали PostgreSQL, MongoDB не берём. API делаем REST.");
        assert!(candidates.iter().any(|c| c.key == "decision.database"));
        assert!(candidates.iter().any(|c| c.key == "decision.api"));
    }

    #[test]
    fn extracts_task_and_plan_as_task_scope() {
        let candidates = extract_local(
            "Текущая задача: подготовить смету. План: 1) собрать каналы, 2) оценить каналы, 3) согласовать.",
        );
        let goal = candidates.iter().find(|c| c.key == "task.goal").unwrap();
        assert_eq!(goal.scope, CandidateScope::Task);
        let plans: Vec<&MemoryCandidate> = candidates
            .iter()
            .filter(|c| c.key.starts_with("plan."))
            .collect();
        assert_eq!(plans.len(), 3);
        assert!(plans.iter().all(|c| c.scope == CandidateScope::Task));
    }

    #[test]
    fn extracts_scope_markers() {
        let entity = extract_local("Выбери вариант B рабочим.");
        assert!(entity.iter().any(|c| c.key == "estimate.variant"));
        let question = extract_local("Открытый вопрос: нужен ли PR-подрядчик?");
        assert!(question.iter().any(|c| c.key == "question.pr"));
        let rate = extract_local("Для этой задачи курс 90 ₽.");
        assert!(rate.iter().any(|c| c.key == "task.rate"));
    }

    #[test]
    fn small_talk_gives_nothing() {
        assert!(keys("Привет").is_empty());
        assert!(keys("Спасибо, это всё").is_empty());
        assert!(keys("Наверное, успеем к декабрю").is_empty());
        assert!(keys("Напомни, что решили по БД").is_empty());
    }

    #[test]
    fn preference_and_profile() {
        assert!(keys("Отвечай коротко").contains(&"preference.style".to_string()));
        assert!(keys("Меня зовут Алексей").contains(&"user.name".to_string()));
    }
}
