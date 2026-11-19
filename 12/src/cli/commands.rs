//! Разбор clap-команд и их выполнение.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Parser, Subcommand, ValueEnum};

use crate::app::controller::{AppController, AppError, AppRuntime};
use crate::cli::output;
use crate::domain::memory::MemoryLayer;
use crate::domain::settings::Settings;
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
    name = "lab11",
    version,
    about = "День 11 — явная модель памяти ассистента (STM / WM / LTM)"
)]
pub struct Cli {
    /// Каталог данных (по умолчанию XDG или LAB11_HOME)
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
    /// Опциональный импорт настроек Дня 10 (P2)
    ImportSettings { path: PathBuf },
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

fn build_client(
    settings: &Settings,
    api_key_file: Option<&Path>,
) -> Result<Arc<dyn ChatClient>, AppError> {
    if std::env::var("LAB11_FAKE_LLM").as_deref() == Ok("1") {
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
    if std::env::var("LAB11_FAKE_LLM").as_deref() == Ok("1") {
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
        _ => 2,
    }
}
