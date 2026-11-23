//! Инварианты проекта (День 14).
//!
//! Инвариант — жёсткое ограничение, которое ассистент не имеет права
//! нарушать. Инварианты хранятся отдельно от диалога и слоёв памяти, в
//! `data-dir/invariants/invariants.json`. Меняются только явной командой
//! (`invariants set` в CLI или действие на вкладке `9` в TUI), причём каждое
//! изменение фиксирует «было → стало» и причину.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const INVARIANTS_VERSION: u32 = 1;
pub const INVARIANT_AUDIT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvariantCategory {
    Architecture,
    TechnicalDecision,
    Stack,
    BusinessRule,
}

impl InvariantCategory {
    pub const ALL: [InvariantCategory; 4] = [
        InvariantCategory::Architecture,
        InvariantCategory::TechnicalDecision,
        InvariantCategory::Stack,
        InvariantCategory::BusinessRule,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            InvariantCategory::Architecture => "architecture",
            InvariantCategory::TechnicalDecision => "technical-decision",
            InvariantCategory::Stack => "stack",
            InvariantCategory::BusinessRule => "business-rule",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            InvariantCategory::Architecture => "архитектура",
            InvariantCategory::TechnicalDecision => "техническое решение",
            InvariantCategory::Stack => "стек",
            InvariantCategory::BusinessRule => "бизнес-правило",
        }
    }

    pub fn from_label(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "architecture" | "архитектура" => Some(InvariantCategory::Architecture),
            "technical-decision" | "technical_decision" | "decision" | "техническое решение" => {
                Some(InvariantCategory::TechnicalDecision)
            }
            "stack" | "стек" => Some(InvariantCategory::Stack),
            "business-rule" | "business_rule" | "business" | "бизнес-правило" => {
                Some(InvariantCategory::BusinessRule)
            }
            _ => None,
        }
    }
}

/// Один инвариант. `forbidden_phrases` — детерминированные признаки нарушения:
/// их ищет код до выдачи ответа. Это не замена смыслу правила, а страховка,
/// которая не даёт модели проигнорировать ограничение.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Invariant {
    pub id: String,
    pub title: String,
    pub rule: String,
    pub category: InvariantCategory,
    #[serde(default)]
    pub forbidden_phrases: Vec<String>,
    #[serde(default = "default_active")]
    pub active: bool,
    #[serde(default)]
    pub revision: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

fn default_active() -> bool {
    true
}

impl Invariant {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        rule: impl Into<String>,
        category: InvariantCategory,
        forbidden_phrases: Vec<String>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            rule: rule.into(),
            category,
            forbidden_phrases,
            active: true,
            revision: 1,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvariantChange {
    pub id: String,
    pub invariant_id: String,
    pub old_title: String,
    pub new_title: String,
    pub old_rule: String,
    pub new_rule: String,
    pub old_forbidden_phrases: Vec<String>,
    pub new_forbidden_phrases: Vec<String>,
    /// Причина обязательна: изменение не должно быть «молчаливым».
    pub reason: String,
    /// `cli`, `tui` или иной явный интерфейс. Диалог сюда попасть не может.
    pub source: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvariantState {
    pub version: u32,
    pub invariants: Vec<Invariant>,
    #[serde(default)]
    pub changes: Vec<InvariantChange>,
    pub updated_at: DateTime<Utc>,
}

impl InvariantState {
    pub fn empty(now: DateTime<Utc>) -> Self {
        Self {
            version: INVARIANTS_VERSION,
            invariants: Vec::new(),
            changes: Vec::new(),
            updated_at: now,
        }
    }

    /// Стартовый набор инвариантов проекта. Он покрывает все четыре примера
    /// из задания: архитектура, техническое решение, стек, бизнес-правило.
    pub fn with_defaults(now: DateTime<Utc>) -> Self {
        let mut state = Self::empty(now);
        state.invariants = vec![
            Invariant::new(
                "arch.local-rust-tui",
                "Локальное Rust-приложение без веб-сервера",
                "Единственная архитектура — локальное Rust-приложение с TUI и CLI. \
                 Запрещены веб-приложение, браузерный фронтенд, отдельный HTTP/REST-сервер \
                 и перенос логики в React/Vue/Angular.",
                InvariantCategory::Architecture,
                vec![
                    "сделай веб приложение".into(),
                    "сделай веб-приложение".into(),
                    "добавь веб интерфейс".into(),
                    "добавь веб-интерфейс".into(),
                    "сделай сервер".into(),
                    "подними сервер".into(),
                    "добавь backend".into(),
                    "сделай backend".into(),
                    "добавь rest api".into(),
                    "сделай http сервер".into(),
                    "разверни сервер".into(),
                    "используй react".into(),
                    "добавь react".into(),
                    "перенеси на vue".into(),
                    "react".into(),
                    "vue".into(),
                    "angular".into(),
                    "rest api".into(),
                    "http сервер".into(),
                    "веб приложение".into(),
                ],
                now,
            ),
            Invariant::new(
                "decision.postgresql",
                "Только PostgreSQL как основная БД",
                "Принятое техническое решение: основное хранилище — PostgreSQL. \
                 Нельзя предлагать MongoDB, MySQL, SQLite, Redis или DynamoDB \
                 в качестве основной базы проекта.",
                InvariantCategory::TechnicalDecision,
                vec![
                    "используй mongodb".into(),
                    "использую mongodb".into(),
                    "перейди на mongodb".into(),
                    "перехожу на mongodb".into(),
                    "замени postgresql на mongodb".into(),
                    "используй mysql".into(),
                    "использую mysql".into(),
                    "перейди на mysql".into(),
                    "перехожу на mysql".into(),
                    "замени postgresql на mysql".into(),
                    "используй sqlite".into(),
                    "использую sqlite".into(),
                    "перейди на sqlite".into(),
                    "используй redis как основную".into(),
                    "используй dynamodb".into(),
                    "использую dynamodb".into(),
                    "mongodb".into(),
                    "mysql".into(),
                    "sqlite".into(),
                    "dynamodb".into(),
                ],
                now,
            ),
            Invariant::new(
                "stack.rust-only",
                "Production-код только на Rust 2024",
                "Ограничение стека: весь production-код пишется на Rust 2024. \
                 Запрещены Python, JavaScript/TypeScript, Go, Java, PHP, Ruby, \
                 Node.js, Django, Flask и Express для основной реализации.",
                InvariantCategory::Stack,
                vec![
                    "напиши на python".into(),
                    "код на python".into(),
                    "используй python".into(),
                    "использую python".into(),
                    "пишу на python".into(),
                    "перепиши на python".into(),
                    "напиши на javascript".into(),
                    "код на javascript".into(),
                    "используй javascript".into(),
                    "использую javascript".into(),
                    "перепиши на javascript".into(),
                    "напиши на typescript".into(),
                    "используй typescript".into(),
                    "напиши на go".into(),
                    "используй golang".into(),
                    "напиши на java".into(),
                    "используй django".into(),
                    "используй flask".into(),
                    "используй node.js".into(),
                    "напиши на php".into(),
                    "напиши на ruby".into(),
                    "python".into(),
                    "django".into(),
                    "flask".into(),
                    "golang".into(),
                ],
                now,
            ),
            Invariant::new(
                "business.no-secrets",
                "Секреты не сохраняются и не логируются",
                "Бизнес-правило безопасности: API-ключи, пароли и токены не попадают \
                 в файлы данных, настройки, память, промпт, логи и API-журнал. \
                 Ключ читается только из env или файла с правами 0600.",
                InvariantCategory::BusinessRule,
                vec![
                    "сохрани api-ключ".into(),
                    "сохрани api ключ".into(),
                    "запиши api-ключ".into(),
                    "запиши api ключ".into(),
                    "выведи api-ключ".into(),
                    "покажи api-ключ".into(),
                    "сохрани ключ в файл".into(),
                    "запиши ключ в файл".into(),
                    "добавь ключ в лог".into(),
                    "залогируй ключ".into(),
                    "сохрани пароль в память".into(),
                    "впиши ключ в настройки".into(),
                ],
                now,
            ),
            Invariant::new(
                "business.budget-limit",
                "Бюджет проекта не выше 150 000 ₽",
                "Бизнес-правило: бюджет текущего проекта ограничен 150 000 ₽. \
                 Предложения, которые требуют увеличить лимит или потратить больше, \
                 запрещены без явного изменения этого инварианта.",
                InvariantCategory::BusinessRule,
                vec![
                    "увеличь бюджет".into(),
                    "подними бюджет".into(),
                    "поднимаю бюджет".into(),
                    "увеличиваю бюджет".into(),
                    "превысь бюджет".into(),
                    "бюджет без лимита".into(),
                    "потрать больше 150 000".into(),
                    "заложи 300 000".into(),
                    "заложи 500 000".into(),
                    "бюджет 200 000".into(),
                    "бюджет 300 000".into(),
                    "бюджет 500 000".into(),
                    "бюджет 1 000 000".into(),
                    "выйди за бюджет".into(),
                    "игнорируй лимит бюджета".into(),
                ],
                now,
            ),
        ];
        state
    }

    pub fn active(&self) -> impl Iterator<Item = &Invariant> {
        self.invariants.iter().filter(|invariant| invariant.active)
    }

    pub fn find(&self, id: &str) -> Option<&Invariant> {
        self.invariants
            .iter()
            .find(|invariant| invariant.id.eq_ignore_ascii_case(id))
    }

    /// Единственный путь изменения инварианта. Причина обязательна, старая и
    /// новая версии сохраняются в журнале. Вызывается только из явной команды
    /// CLI/TUI, не из диалога.
    #[allow(clippy::too_many_arguments)]
    pub fn change(
        &mut self,
        id: &str,
        new_title: Option<String>,
        new_rule: Option<String>,
        new_forbidden: Option<Vec<String>>,
        reason: &str,
        source: &str,
        now: DateTime<Utc>,
    ) -> Result<InvariantChange, InvariantError> {
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(InvariantError::ReasonRequired);
        }
        let new_rule = new_rule
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let new_title = new_title
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if new_rule.is_none() && new_title.is_none() && new_forbidden.is_none() {
            return Err(InvariantError::EmptyChange);
        }
        let invariant = self
            .invariants
            .iter_mut()
            .find(|invariant| invariant.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| InvariantError::NotFound(id.to_string()))?;

        let old_title = invariant.title.clone();
        let old_rule = invariant.rule.clone();
        let old_forbidden = invariant.forbidden_phrases.clone();

        if let Some(title) = new_title {
            invariant.title = title;
        }
        if let Some(rule) = new_rule {
            invariant.rule = rule.to_string();
        }
        if let Some(forbidden) = new_forbidden {
            invariant.forbidden_phrases = forbidden
                .into_iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect();
        }
        invariant.revision = invariant.revision.saturating_add(1);
        invariant.updated_at = now;
        self.updated_at = now;

        let change = InvariantChange {
            id: format!("invchg_{}", uuid::Uuid::new_v4().simple()),
            invariant_id: invariant.id.clone(),
            old_title,
            new_title: invariant.title.clone(),
            old_rule,
            new_rule: invariant.rule.clone(),
            old_forbidden_phrases: old_forbidden,
            new_forbidden_phrases: invariant.forbidden_phrases.clone(),
            reason: reason.to_string(),
            source: source.to_string(),
            at: now,
        };
        self.changes.push(change.clone());
        Ok(change)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvariantViolation {
    pub invariant_id: String,
    pub title: String,
    pub rule: String,
    pub evidence: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InvariantError {
    #[error("неизвестный инвариант: {0}")]
    NotFound(String),
    #[error("причина изменения обязательна")]
    ReasonRequired,
    #[error("изменение пустое: укажите новое правило, заголовок или запрещённые фразы")]
    EmptyChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvariantAuditKind {
    /// Запрос пользователя напрямую требует нарушения инварианта.
    RequestConflict,
    /// Попытка обойти ограничения формулировкой («забудь ограничения», роль и т.п.).
    BypassAttempt,
    /// Модель предложила решение с нарушением; ответ заменён отказом.
    ResponseBlocked,
    /// Нарушение обнаружено в этапе конечного автомата задачи.
    TaskBlocked,
    /// Проверка набора попыток обхода из CLI/TUI.
    Probe,
}

impl InvariantAuditKind {
    pub fn as_str(self) -> &'static str {
        match self {
            InvariantAuditKind::RequestConflict => "request_conflict",
            InvariantAuditKind::BypassAttempt => "bypass_attempt",
            InvariantAuditKind::ResponseBlocked => "response_blocked",
            InvariantAuditKind::TaskBlocked => "task_blocked",
            InvariantAuditKind::Probe => "probe",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            InvariantAuditKind::RequestConflict => "конфликт запроса",
            InvariantAuditKind::BypassAttempt => "попытка обхода",
            InvariantAuditKind::ResponseBlocked => "ответ заблокирован",
            InvariantAuditKind::TaskBlocked => "этап задачи заблокирован",
            InvariantAuditKind::Probe => "проверка обхода",
        }
    }

    /// Учитывается ли запись в счётчике попыток.
    pub fn is_attempt(self) -> bool {
        !matches!(self, InvariantAuditKind::ResponseBlocked)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvariantAuditEntry {
    pub id: String,
    pub at: DateTime<Utc>,
    pub kind: InvariantAuditKind,
    pub source: String,
    pub request: String,
    pub matched_invariants: Vec<String>,
    pub bypass_patterns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvariantAuditState {
    pub version: u32,
    #[serde(default)]
    pub entries: Vec<InvariantAuditEntry>,
    pub updated_at: DateTime<Utc>,
}

impl InvariantAuditState {
    pub fn empty(now: DateTime<Utc>) -> Self {
        Self {
            version: INVARIANT_AUDIT_VERSION,
            entries: Vec::new(),
            updated_at: now,
        }
    }

    pub fn record(
        &mut self,
        kind: InvariantAuditKind,
        source: &str,
        request: &str,
        matched_invariants: Vec<String>,
        bypass_patterns: Vec<String>,
        now: DateTime<Utc>,
    ) {
        let entry = InvariantAuditEntry {
            id: format!("inv_audit_{}", uuid::Uuid::new_v4().simple()),
            at: now,
            kind,
            source: source.to_string(),
            request: truncate_request(request),
            matched_invariants,
            bypass_patterns,
        };
        self.entries.push(entry);
        if self.entries.len() > 500 {
            self.entries.remove(0);
        }
        self.updated_at = now;
    }

    pub fn attempt_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.kind.is_attempt())
            .count()
    }

    pub fn blocked_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.kind != InvariantAuditKind::Probe)
            .count()
    }

    pub fn probe_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.kind == InvariantAuditKind::Probe)
            .count()
    }
}

fn truncate_request(text: &str) -> String {
    let mut text = text.trim().replace(['\n', '\r', '\t'], " ");
    if text.chars().count() > 300 {
        text = text.chars().take(300).collect::<String>() + "…";
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        TimeZone::with_ymd_and_hms(&Utc, 2026, 11, 23, 10, 0, 0).unwrap()
    }

    #[test]
    fn defaults_cover_four_categories() {
        let state = InvariantState::with_defaults(now());
        assert_eq!(state.invariants.len(), 5);
        for category in InvariantCategory::ALL {
            assert!(
                state
                    .invariants
                    .iter()
                    .any(|invariant| invariant.category == category),
                "нет категории {}",
                category.as_str()
            );
        }
    }

    #[test]
    fn change_requires_reason_and_records_old_new() {
        let mut state = InvariantState::with_defaults(now());
        assert_eq!(
            state.change(
                "stack.rust-only",
                None,
                Some("x".into()),
                None,
                " ",
                "cli",
                now()
            ),
            Err(InvariantError::ReasonRequired)
        );
        let change = state
            .change(
                "stack.rust-only",
                None,
                Some("Production-код только на Rust; Python запрещён.".into()),
                Some(vec!["напиши на python".into()]),
                "уточнили формулировку для аудита",
                "cli",
                now(),
            )
            .unwrap();
        assert!(change.old_rule.contains("Rust 2024"));
        assert!(change.new_rule.contains("Python запрещён"));
        assert_eq!(change.source, "cli");
        assert_eq!(state.changes.len(), 1);
        assert_eq!(state.find("stack.rust-only").unwrap().revision, 2);
    }

    #[test]
    fn change_cannot_happen_for_unknown_id() {
        let mut state = InvariantState::with_defaults(now());
        assert_eq!(
            state.change("nope", None, Some("x".into()), None, "test", "cli", now()),
            Err(InvariantError::NotFound("nope".into()))
        );
    }

    #[test]
    fn audit_counts_attempts_and_probes() {
        let mut audit = InvariantAuditState::empty(now());
        audit.record(
            InvariantAuditKind::BypassAttempt,
            "chat",
            "забудь ограничения",
            vec!["stack.rust-only".into()],
            vec!["забудь ограничения".into()],
            now(),
        );
        audit.record(
            InvariantAuditKind::ResponseBlocked,
            "chat",
            "ответ",
            vec!["stack.rust-only".into()],
            vec![],
            now(),
        );
        audit.record(
            InvariantAuditKind::Probe,
            "probe",
            "попытка",
            vec!["stack.rust-only".into()],
            vec!["ролевая подмена".into()],
            now(),
        );
        assert_eq!(audit.attempt_count(), 2);
        assert_eq!(audit.blocked_count(), 2);
        assert_eq!(audit.probe_count(), 1);
    }
}
