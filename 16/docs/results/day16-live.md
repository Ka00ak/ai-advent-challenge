# День 16 — результаты живой проверки

Дата прогона: **2026-11-24**. Окружение: Linux, Rust 1.100, Node.js 24,
`rmcp 3.11`, GitHub MCP 0.6.2 (stdio), модель `deepseek-chat`.

## 1. Соединение и список инструментов

```sh
cargo run -- mcp tools
```

```text
MCP-сервер: github-mcp-server 0.6.2 · протокол 2024-11-05
команда: npx -y @modelcontextprotocol/server-github · инструментов: 26
 1. create_or_update_file
 2. search_repositories
 3. create_repository
 4. get_file_contents
 5. push_files
 6. create_issue
 7. create_pull_request
...
26. get_pull_request_reviews
```

Соединение устанавливается, `tools/list` возвращает 26 инструментов, которых
нет в Rust-коде приложения.

## 2. Прямой вызов MCP-инструмента

```sh
cargo run -- mcp call search_repositories \
  '{"query":"modelcontextprotocol rust","perPage":2}'
```

Результат: JSON GitHub с `total_count` и массивом репозиториев
(`microsoft/mcp-for-beginners`, `4t145/rmcp`, ...). Вызов выполнен MCP-сервером,
а не локальной функцией.

## 3. Смешанная задача: MCP + локальный калькулятор

Команда:

```sh
cargo run -- chat "Через GitHub MCP найди количество репозиториев по запросу \
'modelcontextprotocol rust' (используй search_repositories). Возьми поле total_count \
из результата и с помощью локального calculate посчитай total_count * 3. \
Ответь коротко: total_count и результат умножения."
```

Ход tool-loop:

```text
=== Тело запроса к модели · итерация 1 · инструментов: 29 ===
{ ... 3 локальные схемы + 26 схем GitHub MCP ... }

→ вызов инструмента search_repositories [mcp] {"query": "modelcontextprotocol rust", "perPage": 1}
← результат search_repositories [mcp] {"incomplete_results":false,...,"total_count":31}

=== Тело запроса к модели · итерация 2 · инструментов: 29 ===
→ вызов инструмента calculate [local] {"expression": "31 * 3"}
← результат calculate [local] {"expression":"31 * 3","result":93.0}

=== Тело запроса к модели · итерация 3 · инструментов: 29 ===

=== Ответ ===
total_count = 31, 31 * 3 = 93.
```

- инструмент `search_repositories` — **[mcp]**, его написал GitHub MCP;
- инструмент `calculate` — **[local]**, портирован из Дня 6;
- модель сама выбрала оба инструмента и аргументы;
- результат MCP вернулся в модель как `tool`-сообщение, после чего она вызвала
  локальный инструмент.

Токены прогона: prompt 17 208 · completion 112 · total 17 320 (3 шага).
Точное тело первого запроса (29 инструментов):
[`day16-request-body.json`](day16-request-body.json).

## 4. TUI

Запуск:

```sh
cargo run -- tui
```

Шапка и вкладки:

```text
 lab16 · deepseek-chat · MCP github · github-mcp-server · 26 инстр. · ответ · 3 шаг(ов) · 17111 токенов
 1 Чат   2 API    · только две вкладки
```

После задачи «Use GitHub MCP search_repositories ... then calculate ...»
во вкладке `1 Чат` видны:

- сообщение пользователя;
- `→ search_repositories({"query": "repo:rust-lang/rust"})`;
- `← search_repositories · результат` с JSON от MCP;
- `→ calculate({"expression": "1 * 3"})`;
- `← calculate · результат`;
- финальный ответ.

Снимок: [`day16-tui-chat.txt`](day16-tui-chat.txt).

Вкладка `2 API` показывает три HTTP-обмена с моделью, сводку и **тело запроса**:

```text
#1  02:56:59  итерация 1  ok  1863 мс
#2  02:57:02  итерация 2  ok  1373 мс
>#3 02:57:03  итерация 3  ok  2079 мс
модель: deepseek-chat · инструментов в запросе: 29
tokens: prompt 5826 · completion 35 · total 5861
Тело запроса к модели:
{ "messages": [ ... ],
  "tools": [ ... 29 функций, включая calculate и search_repositories ... ] }
```

Снимок: [`day16-tui-api.txt`](day16-tui-api.txt).

## 5. Автотесты

```sh
cargo test
cargo test --test live_github -- --ignored --nocapture
```

- 27 unit-тестов: инструменты, DTO, тело запроса, TUI (2 вкладки, тело API);
- 2 интеграционных теста с настоящим stdio MCP-протоколом и fixture-сервером:
  соединение/`tools/list`/`tools/call` и смешанный tool-loop;
- 1 живой GitHub-тест: `github-mcp-server 0.6.2`, 26 инструментов,
  `search_repositories` → `rust-lang/rust` — **passed**.
