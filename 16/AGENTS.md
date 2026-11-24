# AGENTS.md — реализация Дня 16

Ты работаешь над **Днём 16: подключение MCP** на **Rust + ratatui**.
Источник истины — `README.md`, `docs/16-mcp.md`, `docs/16-traceability.md`.

## 1. Жёсткие требования

- MCP-клиент — официальный Rust SDK `rmcp` (stdio, `TokioChildProcess`).
- Сервер — GitHub MCP (`@modelcontextprotocol/server-github` по умолчанию).
- Локальные инструменты — из Дня 6: `calculate`, `get_current_time`,
  `convert_currency`.
- Схемы локальных и MCP-инструментов уходят в один запрос к модели; модель
  сама выбирает инструмент и аргументы.
- Вызов MCP-инструмента выполняется через MCP, результат возвращается модели
  как `tool`-сообщение.
- Тело запроса к модели печатается в CLI и показывается на вкладке API.
- В TUI **ровно две вкладки**: `1 Чат` и `2 API`. Не добавлять другие экраны.
- UI и сообщения — на русском.
- Тесты не должны требовать сети по умолчанию. Живой GitHub-тест — `#[ignore]`.

## 2. Гейт

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
cargo test
cargo test --test live_github -- --ignored --nocapture
```

## 3. Полезные команды

```sh
cargo run -- tui
cargo run -- mcp tools
cargo run -- mcp call search_repositories '{"query":"repo:rust-lang/rust","perPage":1}'
cargo run -- chat "..."
LAB16_MCP_DISABLE=1 cargo run -- tui
```

## 4. Перед изменениями

1. Прочитай `docs/16-mcp.md` и `docs/16-traceability.md`.
2. Проверь, что новые инструменты попадают и в `schemas()`, и в `execute()`.
3. Не логируй API-ключи и GitHub-токены в request body/логи.
4. Добавь unit/integration-тест вместе с изменением.
5. Обнови трассировку и, при необходимости, `docs/results/`.
