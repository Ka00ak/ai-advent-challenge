# День 16 — MCP-клиент и смешанный набор инструментов

## 1. Задача

1. Установить MCP SDK/клиент и подключиться к готовому MCP-серверу.
2. Получить от сервера список инструментов.
3. Передать схемы инструментов в запрос к модели вместе с локальными
   инструментами Дня 6.
4. Дать модели выбрать инструмент и аргументы; вызвать инструмент через MCP
   и вернуть результат в модель.
5. Проверить соединение, список инструментов и решение задачи инструментом,
   который не реализован в коде приложения.
6. Проверить смешанный набор: один локальный инструмент + один MCP-инструмент
   в одной задаче.
7. Распечатать тело запроса к модели.

Сервер: **GitHub MCP** (`@modelcontextprotocol/server-github`, stdio).
Стек: Rust + ratatui (как День 15). TUI — ровно две вкладки: `Чат` и `API`.

## 2. MCP-клиент

Используется официальный Rust SDK [`rmcp`](https://crates.io/crates/rmcp) 3.11
с feature `transport-child-process`.

```rust
let command = tokio::process::Command::new(&config.command).configure(|command| {
    command.args(&config.args);
    // GITHUB_PERSONAL_ACCESS_TOKEN и др.
});
let transport = TokioChildProcess::new(command)?;
let service = ().serve(transport).await?;          // initialize
let tools = service.list_all_tools().await?;       // tools/list (все страницы)
let params = CallToolRequestParams::new(name)
    .with_arguments(arguments);
let result = service.call_tool(params).await?;     // tools/call
```

`McpClient` держит tokio-runtime и `RunningService`. Наружу отдаёт
синхронные методы `connect`, `tools`, `call_tool`: TUI и агент работают в
обычных потоках, а MCP-протокол живёт в tokio-runtime. При `Drop` соединение
закрывается, дочерний процесс завершается.

### Соединение и список инструментов

```sh
cargo run -- mcp tools
```

Фактический вывод:

```text
MCP-сервер: github-mcp-server 0.6.2 · протокол 2024-11-05
команда: npx -y @modelcontextprotocol/server-github · инструментов: 26
 1. create_or_update_file ...
 2. search_repositories ...
...
26. get_pull_request_reviews ...
```

Инструменты не описаны в Rust-коде: они приходят из `tools/list` сервера.

## 3. Единый каталог инструментов

`ToolCatalog` объединяет:

- локальные инструменты Дня 6: `calculate`, `get_current_time`,
  `convert_currency`;
- `McpClient::schemas()` — 26 схем GitHub MCP.

Итого **29 функций** в `ChatRequest.tools`. При совпадении имён локальный
инструмент имеет приоритет; в текущем наборе коллизий нет.

```rust
pub fn execute(&self, name: &str, arguments: &Value) -> Value {
    if self.local.contains(name) {
        return local_result_or_error(...);
    }
    if let Some(mcp) = &self.mcp && mcp.has_tool(name) {
        return mcp.call_tool(name, arguments)...;
    }
    json!({"error": format!("неизвестный инструмент «{name}»")})
}
```

Ошибка инструмента не прерывает диалог: она сериализуется в `tool`-сообщение,
и модель может исправить аргументы или объяснить проблему.

## 4. Tool-loop агента

`Agent::run` повторяет цикл:

1. собрать `ChatRequest` (`model`, `messages`, `temperature`, `tools`);
2. сохранить точное тело через `build_body` в `Exchange`;
3. вызвать модель;
4. если есть `tool_calls` — выполнить каждый через `ToolCatalog`, добавить
   `assistant(tool_calls)` и `tool(tool_call_id, name, content)`;
5. повторять, пока модель не вернёт ответ без `tool_calls` (не более
   `max_iterations`).

Модель сама выбирает:
- имя (`search_repositories` или `calculate`);
- аргументы (`query`, `expression`);
- порядок вызовов (MCP → локальный инструмент).

## 5. Тело запроса к модели

`build_body` формирует OpenAI-совместимый JSON. В нём есть все схемы:

```json
{
  "model": "deepseek-chat",
  "messages": [
    {"role": "system", "content": "..."},
    {"role": "user", "content": "..."}
  ],
  "tools": [
    {"type": "function", "function": {"name": "calculate", ...}},
    {"type": "function", "function": {"name": "convert_currency", ...}},
    {"type": "function", "function": {"name": "get_current_time", ...}},
    {"type": "function", "function": {"name": "search_repositories", ...}},
    {"type": "function", "function": {"name": "get_file_contents", ...}}
  ],
  "tool_choice": "auto",
  "stream": false
}
```

CLI `chat` печатает каждый запрос целиком (заголовок + pretty JSON). TUI
показывает то же тело на вкладке `2 API`: `←→` — запрос/ответ, `Tab` — фокус,
`↑↓/PgUp/PgDn` — прокрутка. Пример полного тела первого запроса:
[`results/day16-request-body.json`](results/day16-request-body.json)
(29 инструментов, 35 КБ).

## 6. Смешанная задача

Запрос:

> Через GitHub MCP найди количество репозиториев по запросу
> `modelcontextprotocol rust` (`search_repositories`). Возьми `total_count`
> из результата и с помощью локального `calculate` посчитай `total_count * 3`.

Ход выполнения:

```text
итерация 1: tools = 29 (3 local + 26 MCP)
→ search_repositories [mcp] {"query": "modelcontextprotocol rust", "perPage": 1}
← {"total_count": 31, "items": [...]}

итерация 2: в messages добавлен tool-результат MCP
→ calculate [local] {"expression": "31 * 3"}
← {"expression":"31 * 3","result":93.0}

итерация 3: модель закончила
Ответ: total_count = 31, 31 * 3 = 93.
```

Один инструмент написан в Rust (`calculate`), второй — внешний
(`search_repositories` из GitHub MCP). Агент использовал оба в одной задаче.

## 7. TUI: только две вкладки

`Tab::ALL = [Chat, Api]`; рендер и тест `render_shows_exactly_two_tabs`
проверяют, что на экране нет вкладок `Память`, `Настройки` и т.п.

- `1 Чат` — история сообщений, вызовы инструментов (`→ имя(аргументы)`),
  результаты (`← имя`), поле ввода.
- `2 API` — список запросов, сводка, точное тело запроса/ответа.

Снимки реального TUI сохранены как текст:
[`results/day16-tui-chat.txt`](results/day16-tui-chat.txt),
[`results/day16-tui-api.txt`](results/day16-tui-api.txt).

## 8. Обработка ошибок

| Ситуация | Поведение |
|---|---|
| `npx`/сеть недоступны | TUI/CLI продолжают с локальными инструментами и показывают предупреждение; `--require-mcp` делает это ошибкой |
| инструмент вернул `isError` | результат уходит модели с `is_error: true` |
| неизвестное имя инструмента | `{"error": ...}` в tool-сообщении |
| неверный JSON аргументов | агент передаёт `{}`; ошибку вернёт сам инструмент |
| превышен лимит итераций | `AgentError::Iterations`, запросы уже в API-журнале |
| нет ключа модели | сообщение в TUI/CLI, код выхода 2 |

## 9. Ограничения и развитие

- GitHub MCP запускается через архивный npm-пакет; официальный бинарник
  подключается через `LAB16_MCP_COMMAND`/`LAB16_MCP_ARGS`.
- Без токена доступны только публичные данные и общий лимит GitHub API.
- Все 26 MCP-схем передаются всегда; фильтр по toolsets/read-only — следующий
  шаг (у официального сервера есть `GITHUB_TOOLSETS`/`--read-only`).
- История чата не сохраняется между запусками: фокус Дня 16 — MCP.
