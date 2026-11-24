# День 16 — трассировка требований

| # | Требование | Реализация | Проверка |
|---|---|---|---|
| 1 | Установить MCP SDK/клиент | `rmcp = 3.11` (`client`, `transport-child-process`) в `Cargo.toml`; `src/mcp.rs` | `cargo build`, `cargo test` |
| 2 | Подключиться к готовому MCP-серверу | `McpClient::connect` запускает GitHub MCP по stdio, `initialize` через `().serve(transport)` | `cargo run -- mcp tools`; `tests/live_github.rs` (ignored); fixture-тест |
| 3 | Получить список инструментов | `service.list_all_tools()` → `McpClient::tools`, `schemas` | живой вывод: 26 инструментов; `mcp_connection_lists_tools_and_calls_them` |
| 4 | Передать схемы в запрос к модели | `ToolCatalog::schemas()` → `ChatRequest.tools` → `build_body` | `request_body_contains_both_tool_sources`; `docs/results/day16-request-body.json` |
| 5 | Модель сама выбирает инструмент и аргументы | `Agent::run` читает `response.tool_calls`, не хардкодит имя/аргументы | живой mixed-прогон; `mixed_local_and_mcp_tool_loop` |
| 6 | Вызвать инструмент через MCP и вернуть результат в модель | `ToolCatalog::execute` → `McpClient::call_tool`; `ChatMessage::tool_result` | fixture `tools/call`; живой `search_repositories`; tool-loop |
| 7 | Инструмент, который не писали в приложении | GitHub MCP: `search_repositories`, `get_file_contents` и ещё 24 | `cargo run -- mcp call search_repositories ...`; live-тест |
| 8 | Смешанный набор: локальный + MCP | 3 local (`calculate`, `get_current_time`, `convert_currency`) + 26 MCP = 29 | `mixed_local_and_mcp_tool_loop`; живой прогон `search_repositories` + `calculate` |
| 9 | Задача, где нужны оба инструмента | запрос `total_count` через MCP и `total_count * 3` через локальный калькулятор | `docs/results/day16-live.md` |
| 10 | Распечатать тело запроса к модели | CLI `chat` печатает pretty JSON каждого запроса; TUI `2 API` показывает `request` | `docs/results/day16-request-body.json`; `api_tab_renders_request_body` |
| 11 | Rust/ratatui стек как в Дне 15 | Rust 2024, ratatui 0.29, crossterm, clap, serde, reqwest | `cargo build` |
| 12 | Локальные инструменты из Дня 6 | `src/tools.rs`: calculator/currency/time, портированы из `06/src/tools/*` | unit-тесты `tools::tests` |
| 13 | GitHub MCP | `McpConfig::github()` → `npx -y @modelcontextprotocol/server-github`; токен опционален | `mcp tools`, live-тест |
| 14 | Только 2 вкладки в TUI (чат и API) | `Tab::ALL = [Chat, Api]`, `draw_tabs`, нет других экранов | `only_two_tabs_exist`; `render_shows_exactly_two_tabs`; снимок TUI |

## Гейт

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
cargo test
cargo test --test live_github -- --ignored --nocapture
```

Результат: `cargo test` — 29 passed, 1 ignored; живой GitHub-тест — passed.
