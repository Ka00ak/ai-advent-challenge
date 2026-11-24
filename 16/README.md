# День 16 — MCP: GitHub-инструменты внутри агента (Rust + ratatui)

Агент Дня 6 получил **MCP-клиент**: приложение поднимает GitHub MCP-сервер по
stdio, получает от него список инструментов и передаёт их схемы модели вместе
с локальными инструментами Дня 6. Модель сама выбирает имя и аргументы
инструмента; локальный вызов выполняет Rust-код, внешний — MCP-сервер.

**Стек:** Rust 2024 (1.88+), ratatui + crossterm, официальный Rust MCP SDK
[`rmcp`](https://crates.io/crates/rmcp) 3.11, tokio, reqwest (blocking, rustls),
clap, serde, chrono.

В TUI **ровно две вкладки**:

```text
1 Чат · 2 API
```

`1 Чат` — диалог с агентом. `2 API` — журнал HTTP-запросов к модели: слева
список запросов, справа **точное тело запроса** (включая все 29 схем
инструментов) и тело ответа.

## Быстрый старт

Требуются Rust 1.88+, Node.js 20+ и `npx` (для запуска GitHub MCP-сервера),
а также ключ модели.

```sh
cd 16
cargo build

export DEEPSEEK_API_KEY=...
cargo run -- tui
```

MCP-диагностика:

```sh
# соединение + список инструментов GitHub MCP
cargo run -- mcp tools

# прямой вызов инструмента, который написал не я
cargo run -- mcp call search_repositories '{"query":"repo:rust-lang/rust","perPage":1}'
```

Смешанная задача (MCP + локальный `calculate`) в CLI; печатает тело каждого
запроса к модели:

```sh
cargo run -- chat "Через GitHub MCP найди количество репозиториев по запросу \
'modelcontextprotocol rust' (search_repositories). Возьми total_count и с помощью \
локального calculate посчитай total_count * 3. Ответь коротко."
```

Фактический результат:

```text
→ вызов инструмента search_repositories [mcp] {"query": "modelcontextprotocol rust", "perPage": 1}
← результат search_repositories [mcp] {"incomplete_results":false,...,"total_count":31}
→ вызов инструмента calculate [local] {"expression": "31 * 3"}
← результат calculate [local] {"expression":"31 * 3","result":93.0}

=== Ответ ===
total_count = 31, 31 * 3 = 93.
```

## Что происходит под капотом

```text
GitHub MCP server (npx @modelcontextprotocol/server-github)
        │  stdio JSON-RPC
        ▼
rmcp::TokioChildProcess → McpClient (connect, list_all_tools, call_tool)
        │
        ├── 26 схем MCP ─┐
        │                ├── ToolCatalog.schemas() → ChatRequest.tools[29]
День 6: 3 схемы ─────────┘                             │
                                                       ▼
                                        DeepSeek Chat Completions
                                                       │ tool_calls
                                                       ▼
                             ToolCatalog.execute(name, arguments)
                               ├── local: calculator/time/currency
                               └── mcp:   McpClient.call_tool
                                                       │
                                                       ▼
                                  ChatMessage::tool_result → следующий запрос
```

- `src/mcp.rs` — MCP-клиент на `rmcp`: запуск дочернего процесса, initialize,
  `tools/list`, `tools/call`, `tools`-схемы, корректное закрытие.
- `src/catalog.rs` — единый каталог: 3 локальных инструмента + инструменты MCP,
  исполнение по имени.
- `src/agent.rs` — tool-loop Дня 6: схемы в запросе, выбор модели, результаты
  инструментов обратно в модель; каждый HTTP-обмен сохраняется в `Exchange`.
- `src/llm.rs` — OpenAI-совместимые DTO, `build_body` с точным телом запроса.
- `src/tui.rs` — две вкладки, API-журнал с телом запроса.
- `src/tools.rs` — локальные инструменты Дня 6: `calculate`,
  `get_current_time`, `convert_currency`.
- `src/cli.rs` — `tui`, `chat`, `mcp tools`, `mcp call`.

## Настройка

| Переменная / флаг | Назначение |
|---|---|
| `DEEPSEEK_API_KEY`, `KIMI_API_KEY`, `LAB16_API_KEY` | ключ модели |
| `LAB16_BASE_URL`, `LAB16_MODEL` | OpenAI-совместимый endpoint и модель |
| `GITHUB_PERSONAL_ACCESS_TOKEN` / `GITHUB_TOKEN` / `GH_TOKEN` | токен GitHub (необязателен для публичного чтения, расширяет лимиты) |
| `LAB16_MCP_COMMAND`, `LAB16_MCP_ARGS` | заменить stdio-команду MCP-сервера |
| `LAB16_MCP_DISABLE=1`, `--no-mcp` | оставить только локальные инструменты |
| `--require-mcp` | завершиться с ошибкой, если MCP не подключился |
| `--api-key-file PATH` | ключ модели из файла |

По умолчанию запускается:

```text
npx -y @modelcontextprotocol/server-github
```

Можно подключить официальный бинарник GitHub MCP:

```sh
LAB16_MCP_COMMAND=/path/to/github-mcp-server \
LAB16_MCP_ARGS=stdio \
cargo run -- mcp tools
```

## TUI

| Клавиши | Действие |
|---|---|
| `1`, `2` | переключить вкладку |
| `Tab` | в чате открыть API; в API перейти к телу/списку |
| `i` / `Enter` | начать ввод сообщения |
| `Enter` | отправить; `Esc` — выйти из ввода |
| `↑↓`, `PgUp/PgDn`, `Home/End` | прокрутка чата |
| `c` | очистить чат или API-журнал текущей вкладки |
| в API: `↑↓` | выбрать запрос |
| в API: `Tab`/`Enter` | перейти к телу |
| в API: `←→` | запрос / ответ |
| `q`, `Ctrl+C` | выход |

В шапке видно модель, MCP-сервер и число его инструментов. В API-вкладке
тело запроса переносится по ширине и прокручивается по вертикали.

## Проверка

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
cargo test
```

- 27 unit-тестов;
- 2 интеграционных теста с настоящим stdio MCP-протоколом и
  fixture-сервером (`tests/mcp_integration.rs`): соединение, `tools/list`,
  `tools/call`, смешанный tool-loop;
- 1 живой тест GitHub MCP, по умолчанию `#[ignore]`:

```sh
cargo test --test live_github -- --ignored --nocapture
```

Фактический живой прогон: `github-mcp-server 0.6.2`, протокол `2024-11-05`,
**26 инструментов**, `search_repositories` вернул `rust-lang/rust`.
Подробности и тела запросов: [`docs/16-mcp.md`](docs/16-mcp.md),
[`docs/16-traceability.md`](docs/16-traceability.md),
[`docs/results/day16-live.md`](docs/results/day16-live.md).

## Ограничения

- Пакет `@modelcontextprotocol/server-github` архивный и запускается без
  токена с лимитами GitHub API; для записи/приватных данных нужен токен.
- В запрос уходят все 26 MCP-инструментов: это заметный объём схем
  (≈5,8 тыс. токенов на первом шаге). Фильтрация набора — следующий шаг.
- MCP-вызов синхронно мостится в tokio-runtime; он выполняется в worker-потоке
  TUI, поэтому интерфейс не блокируется.
- История чата живёт в памяти процесса; постоянного хранилища в Дне 16 нет.
