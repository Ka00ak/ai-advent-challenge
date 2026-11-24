//! CLI: TUI, чат с агентом, диагностика MCP и прямой вызов MCP-инструмента.

use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use serde_json::{Value, json};

use crate::agent::{Agent, AgentEvent};
use crate::catalog::ToolCatalog;
use crate::llm::ReqwestClient;
use crate::mcp::{McpClient, McpConfig};
use crate::settings::{Settings, load_api_key};
use crate::tools::LocalToolRegistry;
use crate::tui;

#[derive(Debug, Parser)]
#[command(
    name = "lab16",
    version,
    about = "День 16: GitHub MCP + локальные инструменты Дня 6"
)]
struct Cli {
    /// OpenAI-совместимый base URL (по умолчанию DeepSeek).
    #[arg(long, global = true)]
    base_url: Option<String>,
    /// Имя модели.
    #[arg(long, global = true)]
    model: Option<String>,
    /// Не подключать MCP; оставить только локальные инструменты.
    #[arg(long, global = true)]
    no_mcp: bool,
    /// Завершиться с ошибкой, если MCP не подключился.
    #[arg(long, global = true)]
    require_mcp: bool,
    /// Файл с API-ключом.
    #[arg(long, global = true)]
    api_key_file: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Интерактивный TUI: вкладки «1 Чат» и «2 API».
    Tui,
    /// Один ход агента; печатает тело каждого запроса к модели.
    Chat {
        text: String,
        /// JSON-результат вместо человекочитаемого вывода.
        #[arg(long)]
        json: bool,
        /// Не печатать тела запросов.
        #[arg(long)]
        hide_request: bool,
    },
    /// Работа с MCP-сервером напрямую.
    Mcp {
        #[command(subcommand)]
        command: McpCommand,
    },
}

#[derive(Debug, Subcommand)]
enum McpCommand {
    /// Установить соединение и вывести список инструментов.
    Tools {
        #[arg(long)]
        json: bool,
    },
    /// Вызвать MCP-инструмент: mcp call <tool> '<json-args>'.
    Call {
        tool: String,
        #[arg(default_value = "{}")]
        arguments: String,
    },
}

pub fn run() -> i32 {
    let cli = Cli::parse();
    let settings = Settings::from_overrides(cli.base_url.clone(), cli.model.clone());
    if let Err(error) = settings.validate() {
        eprintln!("Ошибка настроек: {error}");
        return 2;
    }
    let Cli {
        no_mcp,
        require_mcp,
        api_key_file,
        command,
        ..
    } = cli;

    match command {
        Command::Tui => run_tui(settings, no_mcp, require_mcp),
        Command::Chat {
            text,
            json,
            hide_request,
        } => run_chat(
            settings,
            no_mcp,
            require_mcp,
            api_key_file,
            text,
            json,
            hide_request,
        ),
        Command::Mcp { command } => run_mcp(command),
    }
}

fn run_tui(settings: Settings, no_mcp: bool, require_mcp: bool) -> i32 {
    let (catalog, warning) = build_catalog(no_mcp);
    if require_mcp && catalog.mcp().is_none() {
        eprintln!(
            "MCP не подключён: {}",
            warning.unwrap_or_else(|| "неизвестная ошибка".into())
        );
        return 3;
    }
    if let Some(warning) = &warning {
        eprintln!("Предупреждение: {warning}");
    }
    match tui::run(settings, catalog, warning) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Ошибка TUI: {error}");
            2
        }
    }
}

fn run_chat(
    settings: Settings,
    no_mcp: bool,
    require_mcp: bool,
    api_key_file: Option<PathBuf>,
    text: String,
    json_output: bool,
    hide_request: bool,
) -> i32 {
    let (catalog, warning) = build_catalog(no_mcp);
    if require_mcp && catalog.mcp().is_none() {
        eprintln!(
            "MCP не подключён: {}",
            warning.unwrap_or_else(|| "неизвестная ошибка".into())
        );
        return 3;
    }
    if let Some(warning) = &warning {
        eprintln!("Предупреждение: {warning}");
    }
    let api_key = match load_api_key(api_key_file.as_ref()) {
        Some(key) => key,
        None => {
            eprintln!("API-ключ не задан: экспортируйте DEEPSEEK_API_KEY или KIMI_API_KEY");
            return 2;
        }
    };
    let client = match ReqwestClient::new(&settings.base_url, Some(api_key)) {
        Ok(client) => client,
        Err(error) => {
            eprintln!("Ошибка HTTP-клиента: {error}");
            return 3;
        }
    };
    let agent = Agent {
        settings: &settings,
        client: &client,
        catalog: &catalog,
    };
    let mut exchanges = Vec::new();
    let result = agent.run(&[], &text, &mut |event| match event {
        AgentEvent::Exchange(exchange) => {
            if !json_output && !hide_request {
                println!(
                    "\n=== Тело запроса к модели · итерация {} · инструментов: {} ===",
                    exchange.iteration,
                    exchange.tool_names().len()
                );
                match serde_json::to_string_pretty(&exchange.request) {
                    Ok(body) => println!("{body}"),
                    Err(error) => println!("<не удалось сериализовать: {error}>"),
                }
                println!();
            }
            exchanges.push(exchange);
        }
        AgentEvent::ToolCall {
            name,
            source,
            arguments,
        } => {
            if !json_output {
                println!(
                    "→ вызов инструмента {name} [{}] {arguments}",
                    source.as_str()
                );
            }
        }
        AgentEvent::ToolResult {
            name,
            source,
            ok,
            preview,
        } => {
            if !json_output {
                println!(
                    "← {} {name} [{}] {}",
                    if ok {
                        "результат"
                    } else {
                        "ошибка"
                    },
                    source.as_str(),
                    preview
                );
            }
        }
    });

    match result {
        Ok(result) => {
            if json_output {
                let output = json!({
                    "final": result.final_text,
                    "steps": result.steps,
                    "usage": result.usage,
                    "history": result.history,
                    "exchanges": result.exchanges,
                    "mcp": catalog.mcp().map(|client| json!({
                        "server": client.server(),
                        "tool_count": client.tools().len(),
                        "tools": client.tools(),
                    })),
                });
                match serde_json::to_string_pretty(&output) {
                    Ok(text) => println!("{text}"),
                    Err(error) => {
                        eprintln!("Ошибка сериализации ответа: {error}");
                        return 3;
                    }
                }
            } else {
                println!("\n=== Ответ ===\n{}", result.final_text);
                if result.usage.total_tokens > 0 {
                    eprintln!(
                        "токены: prompt {} · completion {} · total {} · шагов {}",
                        result.usage.prompt_tokens,
                        result.usage.completion_tokens,
                        result.usage.total_tokens,
                        result.steps
                    );
                }
            }
            0
        }
        Err(error) => {
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "error": error.to_string(),
                        "exchanges": exchanges,
                    }))
                    .unwrap_or_else(|_| "{\"error\":\"agent failed\"}".into())
                );
            }
            eprintln!("Ошибка агента: {error}");
            3
        }
    }
}

fn run_mcp(command: McpCommand) -> i32 {
    let client = match McpClient::connect(McpConfig::github()) {
        Ok(client) => Arc::new(client),
        Err(error) => {
            eprintln!("Не удалось подключиться к GitHub MCP: {error}");
            eprintln!(
                "Проверьте npx/Node.js и сеть; команда: {}",
                McpConfig::github().command_line()
            );
            return 3;
        }
    };
    match command {
        McpCommand::Tools { json } => {
            if json {
                let output = json!({
                    "server": client.server(),
                    "config": {
                        "label": client.config().label,
                        "command": client.config().command_line(),
                    },
                    "tools": client.tools(),
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{}".into())
                );
            } else {
                println!(
                    "MCP-сервер: {} {} · протокол {}",
                    client.server().name,
                    client.server().version,
                    client.server().protocol
                );
                println!(
                    "команда: {} · инструментов: {}",
                    client.config().command_line(),
                    client.tools().len()
                );
                for (index, tool) in client.tools().iter().enumerate() {
                    println!(
                        "{:>2}. {:<34} {}",
                        index + 1,
                        tool.name,
                        first_line(&tool.description)
                    );
                }
            }
            0
        }
        McpCommand::Call { tool, arguments } => {
            let arguments: Value = match serde_json::from_str(&arguments) {
                Ok(Value::Object(object)) => Value::Object(object),
                Ok(_) => {
                    eprintln!("Аргументы должны быть JSON-объектом");
                    return 2;
                }
                Err(error) => {
                    eprintln!("Некорректный JSON аргументов: {error}");
                    return 2;
                }
            };
            match client.call_tool(&tool, &arguments) {
                Ok(value) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
                    );
                    0
                }
                Err(error) => {
                    eprintln!("Ошибка MCP-инструмента: {error}");
                    3
                }
            }
        }
    }
}

fn build_catalog(no_mcp: bool) -> (ToolCatalog, Option<String>) {
    if no_mcp || McpConfig::disabled() {
        return (ToolCatalog::local_only(), None);
    }
    match McpClient::connect(McpConfig::github()) {
        Ok(client) => (
            ToolCatalog::with_mcp(LocalToolRegistry::day6(), Arc::new(client)),
            None,
        ),
        Err(error) => (
            ToolCatalog::local_only(),
            Some(format!(
                "MCP не подключён ({error}); доступны только локальные инструменты"
            )),
        ),
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}
