//! Разбор clap-команд и их выполнение.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Parser, Subcommand, ValueEnum};

use crate::app::controller::{AppController, AppError, AppRuntime};
use crate::cli::output;
use crate::domain::memory::MemoryLayer;
use crate::domain::profile::{CodePolicy, ProfileSource, Tone, UserProfile, Verbosity};
use crate::domain::settings::Settings;
use crate::experiment::personalization::PersonalizationOptions;
use crate::experiment::runner::{ExperimentMode, ExperimentOptions};
use crate::llm::client::{FakeChatClient, ReqwestClient};
use crate::llm::dto::ChatClient;
use crate::storage::atomic::quarantine_corrupt;
use crate::storage::paths::resolve_data_dir;
use crate::storage::{acquire_lock, load_settings, save_settings};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum LayerArg {
    Stm,
    Wm,
    Ltm,
}

impl LayerArg {
    fn layer(self) -> MemoryLayer {
        match self {
            LayerArg::Stm => MemoryLayer::Stm,
            LayerArg::Wm => MemoryLayer::Wm,
            LayerArg::Ltm => MemoryLayer::Ltm,
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "lab14",
    version,
    about = "День 14 — инварианты и ограничения состояния поверх памяти, профиля и автомата задачи"
)]
pub struct Cli {
    /// Каталог данных (по умолчанию XDG или LAB14_HOME)
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,

    /// Путь к settings.json (переопределяет data-dir/settings.json)
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,

    /// Подробный вывод (прогресс, диагностика)
    #[arg(long, global = true)]
    pub verbose: bool,

    /// Файл с API-ключом (права 0600)
    #[arg(long, global = true)]
    pub api_key_file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Полноэкранный терминальный интерфейс
    Tui,
    /// Один запрос к агенту
    Chat {
        text: String,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        no_stream: bool,
    },
    /// Просмотр и ручное управление слоями
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    /// Импорт facts Дня 10 в LTM
    ImportFacts { path: PathBuf },
    /// Воспроизводимый эксперимент с памятью
    Experiment {
        #[arg(long, conflicts_with = "full")]
        quick: bool,
        #[arg(long)]
        full: bool,
        /// Добавить 3 прогона очистки (54 запроса)
        #[arg(long)]
        cleanup: bool,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: Option<PathBuf>,
        #[arg(long)]
        check: bool,
    },
    /// Дамп слоёв в JSON
    Dump {
        #[arg(long)]
        json: bool,
    },
    /// Профиль пользователя и предпочтения
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Проверка персонализации: цепочка из 10 вопросов для каждого из 3 профилей
    Personalize {
        /// Переопределить первый вопрос цепочки (остальные 9 — стандартные)
        #[arg(long)]
        question: Option<String>,
        /// ID профилей через запятую (по умолчанию brief,detailed,default)
        #[arg(long, value_delimiter = ',')]
        profiles: Vec<String>,
        /// Короткий режим: 1 вопрос на профиль вместо цепочки из 10
        #[arg(long)]
        no_stability: bool,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        json: Option<PathBuf>,
        #[arg(long)]
        check: bool,
    },
    /// Опциональный импорт настроек Дня 10 (P2)
    ImportSettings { path: PathBuf },
    /// Инварианты проекта (День 14)
    Invariants {
        #[command(subcommand)]
        command: InvariantCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum InvariantCommand {
    /// Список инвариантов
    List {
        #[arg(long)]
        json: bool,
    },
    /// Показать один инвариант
    Show {
        id: String,
        #[arg(long)]
        json: bool,
    },
    /// Явная команда изменения: фиксирует «было → стало» и причину
    Set {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        rule: Option<String>,
        /// Запрещённая фраза-признак нарушения (можно повторять)
        #[arg(long = "forbid")]
        forbid: Vec<String>,
        /// Причина обязательна
        #[arg(long)]
        reason: Option<String>,
    },
    /// История изменений инвариантов
    History {
        #[arg(long)]
        json: bool,
    },
    /// Аудит конфликтов и попыток обхода
    Audit {
        #[arg(long)]
        json: bool,
    },
    /// Проверить текст кодом без запроса к модели
    Check {
        text: String,
        #[arg(long)]
        json: bool,
    },
    /// Прогнать 10 формулировок обхода и посчитать, сколько заблокировано
    Probe {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum MemoryCommand {
    Show {
        #[arg(long, value_enum)]
        layer: Option<LayerArg>,
        #[arg(long)]
        json: bool,
    },
    Add {
        #[arg(long, value_enum)]
        layer: LayerArg,
        #[arg(long)]
        key: String,
        #[arg(long)]
        value: String,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        pinned: bool,
    },
    Update {
        #[arg(long, value_enum)]
        layer: LayerArg,
        #[arg(long)]
        key: String,
        #[arg(long)]
        value: String,
        #[arg(long)]
        kind: Option<String>,
    },
    Delete {
        #[arg(long, value_enum)]
        layer: LayerArg,
        #[arg(long)]
        key: String,
    },
    Move {
        #[arg(long, value_enum)]
        from: LayerArg,
        #[arg(long, value_enum)]
        to: LayerArg,
        #[arg(long)]
        key: String,
    },
    Pin {
        #[arg(long)]
        key: String,
        #[arg(long)]
        off: bool,
    },
    Pending {
        #[command(subcommand)]
        command: PendingCommand,
    },
    Clear {
        #[arg(long, value_enum)]
        layer: LayerArg,
        #[arg(long)]
        yes: bool,
    },
    /// Убрать битый файл слоя в *.corrupt-* (не трогая другие слои)
    Reset {
        #[arg(long, value_enum)]
        layer: LayerArg,
    },
}

#[derive(Subcommand, Debug)]
pub enum PendingCommand {
    List,
    Accept { id: String },
    Reject { id: String },
}

#[derive(Subcommand, Debug)]
pub enum ProfileCommand {
    /// Список профилей
    List {
        #[arg(long)]
        json: bool,
    },
    /// Показать профиль (по умолчанию активный)
    Show {
        id: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Сделать профиль активным
    Use { id: String },
    /// Создать профиль (можно взять за основу preset через --like)
    Add {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[command(flatten)]
        edit: ProfileEditArgs,
    },
    /// Изменить профиль
    Set {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[command(flatten)]
        edit: ProfileEditArgs,
    },
    /// Удалить профиль
    Delete { id: String },
    /// Проверить обучение профиля на тексте (без запроса к модели)
    Learn { text: String },
}

#[derive(clap::Args, Debug, Clone, Default)]
pub struct ProfileEditArgs {
    #[arg(long, value_enum)]
    pub verbosity: Option<VerbosityArg>,
    #[arg(long, value_enum)]
    pub tone: Option<ToneArg>,
    #[arg(long, value_enum)]
    pub code: Option<CodeArg>,
    #[arg(long)]
    pub language: Option<String>,
    #[arg(long)]
    pub user_name: Option<String>,
    #[arg(long)]
    pub role: Option<String>,
    #[arg(long, value_enum)]
    pub lists: Option<ToggleArg>,
    #[arg(long, value_enum)]
    pub tables: Option<ToggleArg>,
    #[arg(long, value_enum)]
    pub emoji: Option<ToggleArg>,
    /// Ограничение профиля (можно повторять)
    #[arg(long = "constraint")]
    pub constraints: Vec<String>,
    #[arg(long)]
    pub clear_constraints: bool,
    /// Взять предпочтения из preset: default | brief | detailed
    #[arg(long, value_enum)]
    pub like: Option<PresetArg>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum VerbosityArg {
    Brief,
    Normal,
    Detailed,
}

impl VerbosityArg {
    fn value(self) -> Verbosity {
        match self {
            VerbosityArg::Brief => Verbosity::Brief,
            VerbosityArg::Normal => Verbosity::Normal,
            VerbosityArg::Detailed => Verbosity::Detailed,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ToneArg {
    Formal,
    Neutral,
    Informal,
}

impl ToneArg {
    fn value(self) -> Tone {
        match self {
            ToneArg::Formal => Tone::Formal,
            ToneArg::Neutral => Tone::Neutral,
            ToneArg::Informal => Tone::Informal,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CodeArg {
    With,
    Without,
    Neutral,
}

impl CodeArg {
    fn value(self) -> CodePolicy {
        match self {
            CodeArg::With => CodePolicy::WithCode,
            CodeArg::Without => CodePolicy::NoCode,
            CodeArg::Neutral => CodePolicy::Neutral,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ToggleArg {
    On,
    Off,
}

impl ToggleArg {
    fn value(self) -> bool {
        matches!(self, ToggleArg::On)
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PresetArg {
    Default,
    Brief,
    Detailed,
}

impl PresetArg {
    fn id(self) -> &'static str {
        match self {
            PresetArg::Default => "default",
            PresetArg::Brief => "brief",
            PresetArg::Detailed => "detailed",
        }
    }
}

pub fn run() -> i32 {
    let cli = Cli::parse();
    match execute(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Ошибка: {error}");
            exit_code(&error)
        }
    }
}

fn execute(cli: Cli) -> Result<i32, AppError> {
    let paths = resolve_data_dir(cli.data_dir.as_deref())
        .map_err(|error| AppError::Io(error.to_string()))?;
    paths
        .ensure()
        .map_err(|error| AppError::Io(error.to_string()))?;
    let _lock = acquire_lock(&paths).map_err(|error| AppError::Io(error.to_string()))?;

    if let Command::Memory {
        command: MemoryCommand::Reset { layer },
    } = &cli.command
    {
        reset_layer(&paths, layer.layer())?;
        println!("Слой {} сброшен", layer.layer().as_str());
        return Ok(0);
    }

    let settings = load_settings_or_default(&paths, cli.config.as_deref())?;
    let client = build_client(&settings, cli.api_key_file.as_deref())?;
    let runtime = AppRuntime::new(paths.clone(), settings, client);

    match cli.command {
        Command::Tui => {
            crate::tui::run(runtime)?;
            Ok(0)
        }
        Command::Chat {
            text,
            json,
            no_stream,
        } => {
            let _ = no_stream;
            require_key_if_needed()?;
            let mut controller = AppController::load(runtime)?;
            let outcome = controller.send_user_message(&text, &mut |progress| {
                if cli.verbose {
                    eprintln!("[{progress:?}]");
                }
            })?;
            if json {
                let payload = serde_json::json!({
                    "final": outcome.final_text,
                    "profile": {
                        "id": outcome.profile_id,
                        "changes": outcome.profile_changes,
                    },
                    "tokens": {
                        "prompt_tokens": outcome.token_stats.prompt_tokens,
                        "completion_tokens": outcome.token_stats.completion_tokens,
                        "total_tokens": outcome.token_stats.total_tokens,
                        "cost_usd": outcome.token_stats.cost_usd,
                        "reported": outcome.token_stats.reported,
                    },
                    "breakdown": outcome.breakdown,
                    "pending": outcome.memory.pending.len(),
                    "audit": outcome.audit,
                    "invariants": outcome.invariant_assessment,
                    "invariant_blocked": outcome.invariant_blocked,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&payload).unwrap_or_default()
                );
            } else {
                println!("{}", outcome.final_text);
                if cli.verbose {
                    output::print_token_stats(&outcome.token_stats);
                    output::print_breakdown(&outcome.breakdown);
                }
            }
            Ok(0)
        }
        Command::Memory { command } => {
            let mut controller = AppController::load(runtime)?;
            match command {
                MemoryCommand::Show { layer, json } => {
                    if json {
                        let payload = match layer {
                            Some(layer) => match layer.layer() {
                                MemoryLayer::Stm => serde_json::json!(&controller.bundle().stm),
                                MemoryLayer::Wm => serde_json::json!(&controller.bundle().wm),
                                MemoryLayer::Ltm => serde_json::json!(&controller.bundle().ltm),
                            },
                            None => serde_json::to_value(controller.bundle())
                                .map_err(|error| AppError::Invalid(error.to_string()))?,
                        };
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&payload).unwrap_or_default()
                        );
                    } else {
                        output::print_bundle(controller.bundle(), layer.map(LayerArg::layer));
                    }
                    Ok(0)
                }
                MemoryCommand::Add {
                    layer,
                    key,
                    value,
                    kind,
                    pinned,
                } => {
                    let status = controller.memory_add(
                        layer.layer(),
                        &key,
                        &value,
                        kind.as_deref(),
                        pinned,
                    )?;
                    println!("{status}: {key}");
                    Ok(0)
                }
                MemoryCommand::Update {
                    layer,
                    key,
                    value,
                    kind,
                } => {
                    let status =
                        controller.memory_update(layer.layer(), &key, &value, kind.as_deref())?;
                    println!("{status}: {key}");
                    Ok(0)
                }
                MemoryCommand::Delete { layer, key } => {
                    let count = controller.memory_delete(layer.layer(), &key)?;
                    println!("удалено: {count}");
                    Ok(0)
                }
                MemoryCommand::Move { from, to, key } => {
                    let status = controller.memory_move(from.layer(), &key, to.layer())?;
                    println!("{status}: {key}");
                    Ok(0)
                }
                MemoryCommand::Pin { key, off } => {
                    controller.memory_pin(&key, !off)?;
                    println!("{}: {key}", if off { "снят pin" } else { "pin" });
                    Ok(0)
                }
                MemoryCommand::Pending { command } => match command {
                    PendingCommand::List => {
                        if controller.pending_list().is_empty() {
                            println!("Все записи подтверждены");
                        }
                        for pending in controller.pending_list() {
                            println!(
                                "{} · {} · {} · {}",
                                pending.id, pending.candidate.key, pending.rule_id, pending.reason
                            );
                            println!(
                                "   Было: {} → Станет: {}",
                                pending.old_value.as_deref().unwrap_or("—"),
                                pending.candidate.value
                            );
                        }
                        Ok(0)
                    }
                    PendingCommand::Accept { id } => {
                        controller.accept_pending(&id)?;
                        println!("принято: {id}");
                        Ok(0)
                    }
                    PendingCommand::Reject { id } => {
                        controller.reject_pending(&id)?;
                        println!("отклонено: {id}");
                        Ok(0)
                    }
                },
                MemoryCommand::Clear { layer, yes } => {
                    if !yes {
                        eprintln!("Очистка необратима: повторите с --yes");
                        return Ok(2);
                    }
                    controller.clear_layer(layer.layer())?;
                    println!("Слой {} очищен", layer.layer().as_str());
                    Ok(0)
                }
                MemoryCommand::Reset { .. } => Ok(0),
            }
        }
        Command::Invariants { command } => {
            let mut controller = AppController::load(runtime)?;
            match command {
                InvariantCommand::List { json } => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(controller.invariants())
                                .unwrap_or_default()
                        );
                    } else {
                        for invariant in &controller.invariants().invariants {
                            println!(
                                "{} [{}] {} · ревизия {}",
                                invariant.id,
                                invariant.category.label(),
                                if invariant.active {
                                    "активен"
                                } else {
                                    "выключен"
                                },
                                invariant.revision
                            );
                            println!("  {} — {}", invariant.title, invariant.rule);
                            if !invariant.forbidden_phrases.is_empty() {
                                println!("  признаки: {}", invariant.forbidden_phrases.join("; "));
                            }
                        }
                    }
                    Ok(0)
                }
                InvariantCommand::Show { id, json } => {
                    let invariant = controller
                        .invariants()
                        .find(&id)
                        .ok_or_else(|| AppError::NotFound(format!("инвариант {id}")))?;
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(invariant).unwrap_or_default()
                        );
                    } else {
                        println!("ID: {}", invariant.id);
                        println!("Категория: {}", invariant.category.label());
                        println!("Заголовок: {}", invariant.title);
                        println!("Правило: {}", invariant.rule);
                        println!(
                            "Признаки нарушения: {}",
                            if invariant.forbidden_phrases.is_empty() {
                                "—".into()
                            } else {
                                invariant.forbidden_phrases.join("; ")
                            }
                        );
                        println!("Ревизия: {}", invariant.revision);
                    }
                    Ok(0)
                }
                InvariantCommand::Set {
                    id,
                    title,
                    rule,
                    forbid,
                    reason,
                } => {
                    let reason = reason.ok_or_else(|| {
                        AppError::Invalid(
                            "изменение инварианта требует --reason: зафиксируйте, почему правило меняется"
                                .into(),
                        )
                    })?;
                    if title.is_none() && rule.is_none() && forbid.is_empty() {
                        return Err(AppError::Invalid(
                            "укажите --rule, --title или --forbid".into(),
                        ));
                    }
                    let forbidden = if forbid.is_empty() {
                        None
                    } else {
                        Some(forbid)
                    };
                    let change =
                        controller.change_invariant(&id, title, rule, forbidden, &reason, "cli")?;
                    println!(
                        "Инвариант {} изменён: ревизия {}",
                        change.invariant_id,
                        controller
                            .invariants()
                            .find(&change.invariant_id)
                            .map(|invariant| invariant.revision)
                            .unwrap_or(0)
                    );
                    println!("Было: {}", change.old_rule);
                    println!("Стало: {}", change.new_rule);
                    println!("Причина: {}", change.reason);
                    Ok(0)
                }
                InvariantCommand::History { json } => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&controller.invariants().changes)
                                .unwrap_or_default()
                        );
                    } else if controller.invariants().changes.is_empty() {
                        println!("Изменений инвариантов ещё не было");
                    } else {
                        for change in &controller.invariants().changes {
                            println!(
                                "{} · {} · {} → {} · причина: {}",
                                change.at.format("%Y-%m-%d %H:%M"),
                                change.invariant_id,
                                change.old_rule,
                                change.new_rule,
                                change.reason
                            );
                        }
                    }
                    Ok(0)
                }
                InvariantCommand::Audit { json } => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(controller.invariant_audit())
                                .unwrap_or_default()
                        );
                    } else {
                        println!(
                            "Попыток: {} · заблокировано: {} · probe: {}",
                            controller.invariant_audit().attempt_count(),
                            controller.invariant_audit().blocked_count(),
                            controller.invariant_audit().probe_count()
                        );
                        for entry in &controller.invariant_audit().entries {
                            println!(
                                "{} · {} · {} · инварианты: {} · обход: {} · {}",
                                entry.at.format("%Y-%m-%d %H:%M"),
                                entry.kind.label(),
                                entry.source,
                                if entry.matched_invariants.is_empty() {
                                    "—".into()
                                } else {
                                    entry.matched_invariants.join(", ")
                                },
                                if entry.bypass_patterns.is_empty() {
                                    "—".into()
                                } else {
                                    entry.bypass_patterns.join("; ")
                                },
                                entry.request
                            );
                        }
                    }
                    Ok(0)
                }
                InvariantCommand::Check { text, json } => {
                    let assessment = controller.assess_user_request(&text, "cli-check");
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&assessment).unwrap_or_default()
                        );
                    } else {
                        println!("Разрешено: {}", assessment.allowed);
                        println!("Проверено инвариантов: {}", assessment.checked_ids.len());
                        if !assessment.violations.is_empty() {
                            for violation in &assessment.violations {
                                println!(
                                    "Нарушение: {} — {} (совпадение: «{}»)",
                                    violation.invariant_id, violation.title, violation.evidence
                                );
                            }
                        }
                        if !assessment.bypass_patterns.is_empty() {
                            println!("Попытка обхода: {}", assessment.bypass_patterns.join("; "));
                        }
                        if !assessment.allowed {
                            println!();
                            println!("{}", crate::invariants::refusal_text(&assessment));
                        }
                    }
                    Ok(if assessment.allowed { 0 } else { 4 })
                }
                InvariantCommand::Probe { json } => {
                    let (blocked, total) = controller.run_invariant_probe();
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "blocked": blocked,
                                "total": total,
                                "attempts": blocked,
                                "audit": controller.invariant_audit(),
                            }))
                            .unwrap_or_default()
                        );
                    } else {
                        println!(
                            "Проверка обхода инвариантов: заблокировано {blocked} из {total} попыток"
                        );
                        if blocked < total {
                            println!("Порог не пройден: есть прошедшая попытка");
                        } else {
                            println!("Все попытки обхода заблокированы кодом");
                        }
                    }
                    Ok(if blocked == total { 0 } else { 4 })
                }
            }
        }
        Command::ImportFacts { path } => {
            let mut controller = AppController::load(runtime)?;
            let report = controller.import_facts(&path)?;
            println!(
                "Импорт facts: найдено={}, добавлено={}, обновлено={}, пропущено={}, ошибок={}",
                report.found,
                report.added,
                report.updated,
                report.skipped,
                report.errors.len()
            );
            for error in &report.errors {
                eprintln!("  ! {error}");
            }
            if !report.ok {
                return Ok(2);
            }
            Ok(0)
        }
        Command::Experiment {
            quick,
            full: _,
            cleanup,
            out,
            json,
            check,
        } => {
            require_key_if_needed()?;
            let mut controller = AppController::load(runtime)?;
            let options = ExperimentOptions {
                mode: if quick {
                    ExperimentMode::Quick
                } else {
                    ExperimentMode::Full
                },
                include_cleanup: cleanup,
            };
            let interactive = std::io::stderr().is_terminal();
            let mut last_config = String::new();
            let result = controller.run_experiment(options, &mut |progress| {
                if cli.verbose {
                    eprintln!(
                        "{} · вопрос {}/{} · {}",
                        progress.config,
                        progress.probe,
                        progress.probes_total,
                        progress.usage_line()
                    );
                    return;
                }
                if interactive {
                    let errors = if progress.errors > 0 {
                        format!(" · ошибок {}", progress.errors)
                    } else {
                        String::new()
                    };
                    eprint!(
                        "\r\x1b[K[{}] вопрос {}/{} · запросов {}/{} · prompt {} · completion {} · ${:.6}{}",
                        progress.config,
                        progress.probe,
                        progress.probes_total,
                        progress.done,
                        progress.total,
                        crate::memory::tokens::format_tokens(progress.prompt_tokens),
                        crate::memory::tokens::format_tokens(progress.completion_tokens),
                        progress.cost_usd,
                        errors
                    );
                    let _ = std::io::stderr().flush();
                    return;
                }
                // В файл/пайп пишем строку на каждый завершённый запрос,
                // чтобы расход API был виден и без TTY.
                if progress.last_matched.is_some() || progress.config != last_config {
                    eprintln!(
                        "[{}] вопрос {}/{} · {}",
                        progress.config,
                        progress.probe,
                        progress.probes_total,
                        progress.usage_line()
                    );
                    last_config = progress.config.clone();
                }
            });
            if interactive {
                eprintln!();
            }
            let report = result?;
            output::print_experiment(&report);
            if let Some(out) = &out {
                let json_path = json.clone().unwrap_or_else(|| out.with_extension("json"));
                controller.save_report(&report, out, &json_path)?;
                println!(
                    "Отчёт сохранён: {} и {}",
                    out.display(),
                    json_path.display()
                );
            } else if let Some(json) = &json {
                let md_path = json.with_extension("md");
                controller.save_report(&report, &md_path, json)?;
                println!("Отчёт сохранён: {}", json.display());
            }
            if !report.errors.is_empty() {
                return Ok(3);
            }
            if check && !report.passed() {
                return Ok(4);
            }
            Ok(0)
        }
        Command::Dump { json: _ } => {
            let controller = AppController::load(runtime)?;
            println!("{}", controller.dump_json()?);
            Ok(0)
        }
        Command::Profile { command } => {
            let mut controller = AppController::load(runtime)?;
            match command {
                ProfileCommand::List { json } => {
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(controller.profiles())
                                .map_err(|error| AppError::Invalid(error.to_string()))?
                        );
                    } else {
                        for profile in &controller.profiles().profiles {
                            let active =
                                controller.profiles().active.as_deref() == Some(&profile.id);
                            println!(
                                "{} {:<10} {:<32} {}",
                                if active { "*" } else { " " },
                                profile.id,
                                profile.title,
                                profile.summary()
                            );
                        }
                    }
                    Ok(0)
                }
                ProfileCommand::Show { id, json } => {
                    let profile = controller
                        .profile(id.as_deref())
                        .cloned()
                        .ok_or_else(|| AppError::NotFound("профиль".into()))?;
                    if json {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&profile)
                                .map_err(|error| AppError::Invalid(error.to_string()))?
                        );
                    } else {
                        output::print_profile(&profile, controller.profiles().active.as_deref());
                    }
                    Ok(0)
                }
                ProfileCommand::Use { id } => {
                    controller.set_active_profile(&id)?;
                    println!("активный профиль: {id}");
                    Ok(0)
                }
                ProfileCommand::Add { id, title, edit } => {
                    let base = edit
                        .like
                        .map(PresetArg::id)
                        .and_then(|preset| UserProfile::preset(preset, chrono::Utc::now()))
                        .unwrap_or_else(|| UserProfile::empty(&id, chrono::Utc::now()));
                    let mut profile = base;
                    profile.id = id.clone();
                    profile.title = title.unwrap_or_else(|| id.clone());
                    profile.source = ProfileSource::Manual;
                    apply_profile_edit(&mut profile, &edit);
                    controller.upsert_profile(profile)?;
                    controller.set_active_profile(&id)?;
                    println!("профиль создан и активирован: {id}");
                    Ok(0)
                }
                ProfileCommand::Set { id, title, edit } => {
                    let mut profile = controller
                        .profile(Some(&id))
                        .cloned()
                        .ok_or_else(|| AppError::NotFound(format!("профиль {id}")))?;
                    if let Some(title) = title {
                        profile.title = title;
                    }
                    if let Some(preset) = edit.like {
                        if let Some(base) = UserProfile::preset(preset.id(), chrono::Utc::now()) {
                            profile.identity = base.identity;
                            profile.verbosity = base.verbosity;
                            profile.tone = base.tone;
                            profile.format = base.format;
                            profile.constraints = base.constraints;
                        }
                    }
                    apply_profile_edit(&mut profile, &edit);
                    profile.source = ProfileSource::Manual;
                    controller.upsert_profile(profile)?;
                    println!("профиль обновлён: {id}");
                    Ok(0)
                }
                ProfileCommand::Delete { id } => {
                    controller.delete_profile(&id)?;
                    println!("профиль удалён: {id}");
                    Ok(0)
                }
                ProfileCommand::Learn { text } => {
                    let changes = controller.learn_profile_from_text(&text)?;
                    if changes.is_empty() {
                        println!("профиль не изменился");
                    } else {
                        for change in changes {
                            println!("{}", change.describe());
                        }
                    }
                    Ok(0)
                }
            }
        }
        Command::Personalize {
            question,
            profiles,
            no_stability,
            out,
            json,
            check,
        } => {
            require_key_if_needed()?;
            let mut controller = AppController::load(runtime)?;
            let options = PersonalizationOptions {
                question,
                profile_ids: profiles,
                include_chain: !no_stability,
            };
            let result = controller.run_personalization(options, &mut |progress| {
                eprintln!(
                    "[{}] {} {}/{} · всего {}/{}",
                    progress.phase,
                    progress.profile,
                    progress.step,
                    progress.steps_total,
                    progress.done,
                    progress.total
                );
            });
            let report = result?;
            output::print_personalization(&report);
            if let Some(out) = &out {
                let json_path = json.clone().unwrap_or_else(|| out.with_extension("json"));
                controller.save_personalization_report(&report, out, &json_path)?;
                println!(
                    "Отчёт сохранён: {} и {}",
                    out.display(),
                    json_path.display()
                );
            } else if let Some(json) = &json {
                let md_path = json.with_extension("md");
                controller.save_personalization_report(&report, &md_path, json)?;
                println!("Отчёт сохранён: {}", json.display());
            }
            if check && !report.passed() {
                return Ok(4);
            }
            Ok(0)
        }
        Command::ImportSettings { path } => {
            let mut settings = load_settings_or_default(&paths, None)?;
            let raw = std::fs::read_to_string(&path)
                .map_err(|error| AppError::Io(format!("{}: {error}", path.display())))?;
            let value: serde_json::Value = serde_json::from_str(&raw)
                .map_err(|error| AppError::Invalid(format!("settings: {error}")))?;
            if let Some(mode) = value.get("factsMode").and_then(|v| v.as_str()) {
                settings.memory.extraction_mode = if mode.eq_ignore_ascii_case("llm") {
                    crate::domain::settings::ExtractionMode::Llm
                } else {
                    crate::domain::settings::ExtractionMode::Local
                };
            }
            if let Some(window) = value.get("keepRecentMessages").and_then(|v| v.as_u64()) {
                settings.memory.stm.window = (window as usize).clamp(4, 20);
            }
            save_settings(&paths, &settings)?;
            println!("Настройки Дня 10 импортированы");
            Ok(0)
        }
    }
}

fn apply_profile_edit(profile: &mut UserProfile, edit: &ProfileEditArgs) {
    if let Some(value) = edit.verbosity {
        profile.verbosity = value.value();
    }
    if let Some(value) = edit.tone {
        profile.tone = value.value();
    }
    if let Some(value) = edit.code {
        profile.format.code = value.value();
    }
    if let Some(value) = edit.language.as_deref() {
        if !value.trim().is_empty() {
            profile.identity.language = value.trim().to_string();
        }
    }
    if let Some(value) = edit.user_name.as_deref() {
        profile.identity.name = (!value.trim().is_empty()).then(|| value.trim().to_string());
    }
    if let Some(value) = edit.role.as_deref() {
        profile.identity.role = (!value.trim().is_empty()).then(|| value.trim().to_string());
    }
    if let Some(value) = edit.lists {
        profile.format.lists = value.value();
    }
    if let Some(value) = edit.tables {
        profile.format.tables = value.value();
    }
    if let Some(value) = edit.emoji {
        profile.format.emoji = value.value();
    }
    if edit.clear_constraints {
        profile.constraints.clear();
    }
    for constraint in &edit.constraints {
        let constraint = constraint.trim();
        if !constraint.is_empty()
            && !profile
                .constraints
                .iter()
                .any(|existing| existing.to_lowercase() == constraint.to_lowercase())
        {
            profile.constraints.push(constraint.to_string());
        }
    }
}

fn load_settings_or_default(
    paths: &crate::storage::paths::DataPaths,
    config: Option<&Path>,
) -> Result<Settings, AppError> {
    if let Some(path) = config {
        let raw = std::fs::read_to_string(path)
            .map_err(|error| AppError::Io(format!("{}: {error}", path.display())))?;
        let settings: Settings = serde_json::from_str(&raw)
            .map_err(|error| AppError::Invalid(format!("settings: {error}")))?;
        return Ok(settings);
    }
    match load_settings(paths) {
        Ok(settings) => Ok(settings),
        Err(error) if error.is_not_found() => {
            let settings = Settings::default();
            save_settings(paths, &settings)?;
            Ok(settings)
        }
        Err(error) => Err(AppError::Load(error)),
    }
}

fn is_fake_llm_environment() -> bool {
    crate::app::controller::fake_llm_enabled()
}

fn build_client(
    settings: &Settings,
    api_key_file: Option<&Path>,
) -> Result<Arc<dyn ChatClient>, AppError> {
    if is_fake_llm_environment() {
        return Ok(Arc::new(FakeChatClient));
    }
    let mut key = std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("KIMI_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty())
        });
    if key.is_none() {
        if let Some(path) = api_key_file {
            check_key_file_permissions(path)?;
            let raw = std::fs::read_to_string(path)
                .map_err(|error| AppError::Io(format!("{}: {error}", path.display())))?;
            let value = raw.trim().to_string();
            if !value.is_empty() {
                key = Some(value);
            }
        }
    }
    let client = ReqwestClient::new(settings.base_url.clone(), key)
        .map_err(|error| AppError::Invalid(error.to_string()))?;
    Ok(Arc::new(client))
}

fn check_key_file_permissions(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .map_err(|error| AppError::Io(format!("{}: {error}", path.display())))?
            .permissions()
            .mode()
            & 0o777;
        if mode & 0o077 != 0 {
            return Err(AppError::Invalid(
                "файл с ключом должен иметь права 0600".into(),
            ));
        }
    }
    Ok(())
}

fn require_key_if_needed() -> Result<(), AppError> {
    if is_fake_llm_environment() {
        return Ok(());
    }
    let has_env = std::env::var("DEEPSEEK_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || std::env::var("KIMI_API_KEY")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);
    if has_env {
        Ok(())
    } else {
        Err(AppError::Invalid(
            "API-ключ не задан: экспортируйте DEEPSEEK_API_KEY или KIMI_API_KEY".into(),
        ))
    }
}

fn reset_layer(
    paths: &crate::storage::paths::DataPaths,
    layer: MemoryLayer,
) -> Result<(), AppError> {
    let files = match layer {
        MemoryLayer::Stm => vec![paths.stm_conversation(), paths.stm_summary()],
        MemoryLayer::Wm => vec![paths.working()],
        MemoryLayer::Ltm => vec![paths.ltm(), paths.pending()],
    };
    for path in files {
        if path.exists() {
            quarantine_corrupt(&path).map_err(AppError::Save)?;
        }
    }
    Ok(())
}

fn exit_code(error: &AppError) -> i32 {
    match error {
        AppError::Agent(agent) => {
            let text = agent.to_string();
            if text.contains("HTTP") || text.contains("статус") || text.contains("модели")
            {
                3
            } else {
                2
            }
        }
        AppError::Experiment(_) => 3,
        AppError::Personalization(_) => 3,
        _ => 2,
    }
}
