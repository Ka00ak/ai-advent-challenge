#!/usr/bin/env python3
"""Минимальный MCP-сервер для автотестов Дня 16.

Транспорт — stdio, JSON-RPC 2.0 построчно. Сервер не использует сторонние
пакеты и отдаёт два инструмента, которых нет в Rust-коде приложения:
repo_stars и repo_languages. Это позволяет проверить, что агент вызывает
именно MCP-инструмент, а не локальную функцию.
"""

import json
import sys

PROTOCOL_VERSION = "2024-11-05"

TOOLS = [
    {
        "name": "repo_stars",
        "description": "Возвращает число звёзд публичного репозитория (тестовый MCP).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repo": {
                    "type": "string",
                    "description": "Репозиторий в формате owner/name",
                }
            },
            "required": ["repo"],
            "additionalProperties": False,
        },
    },
    {
        "name": "repo_languages",
        "description": "Возвращает список языков репозитория (тестовый MCP).",
        "inputSchema": {
            "type": "object",
            "properties": {"repo": {"type": "string"}},
            "required": ["repo"],
            "additionalProperties": False,
        },
    },
]


def send(message):
    sys.stdout.write(json.dumps(message, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def result(request_id, payload):
    send({"jsonrpc": "2.0", "id": request_id, "result": payload})


def error(request_id, code, message):
    send(
        {
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {"code": code, "message": message},
        }
    )


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
        except json.JSONDecodeError:
            continue
        method = request.get("method", "")
        request_id = request.get("id")
        if request_id is None:
            # notifications/initialized и другие уведомления
            continue
        if method == "initialize":
            result(
                request_id,
                {
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "fixture-mcp", "version": "1.0.0"},
                    "instructions": "Тестовый MCP-сервер для проверки Day 16.",
                },
            )
        elif method == "tools/list":
            result(request_id, {"tools": TOOLS})
        elif method == "tools/call":
            params = request.get("params", {})
            name = params.get("name")
            arguments = params.get("arguments") or {}
            if name == "repo_stars":
                repo = arguments.get("repo", "unknown/unknown")
                payload = {"repo": repo, "stars": 37}
                result(
                    request_id,
                    {
                        "content": [
                            {"type": "text", "text": json.dumps(payload, ensure_ascii=False)}
                        ],
                        "structuredContent": payload,
                        "isError": False,
                    },
                )
            elif name == "repo_languages":
                payload = {"repo": arguments.get("repo", "unknown/unknown"), "languages": ["Rust"]}
                result(
                    request_id,
                    {
                        "content": [
                            {"type": "text", "text": json.dumps(payload, ensure_ascii=False)}
                        ],
                        "structuredContent": payload,
                        "isError": False,
                    },
                )
            else:
                result(
                    request_id,
                    {
                        "content": [{"type": "text", "text": f"unknown tool: {name}"}],
                        "isError": True,
                    },
                )
        elif method == "ping":
            result(request_id, {})
        else:
            error(request_id, -32601, f"method not found: {method}")


if __name__ == "__main__":
    main()
