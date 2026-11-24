//! Живая проверка GitHub MCP. По умолчанию игнорируется, потому что нужны
//! Node.js/npx и сеть:
//!
//! cargo test --test live_github -- --ignored --nocapture

use lab16::mcp::{McpClient, McpConfig};
use serde_json::json;

#[test]
#[ignore = "требует Node.js/npx и доступ к GitHub API"]
fn live_github_mcp_lists_tools_and_searches_repositories() {
    let client = McpClient::connect(McpConfig::github()).expect("GitHub MCP должен подключиться");
    println!(
        "server: {} {} · protocol {} · tools {}",
        client.server().name,
        client.server().version,
        client.server().protocol,
        client.tools().len()
    );
    assert!(
        client.tools().len() >= 20,
        "ожидался полный набор инструментов GitHub MCP"
    );
    assert!(client.has_tool("search_repositories"));
    let value = client
        .call_tool(
            "search_repositories",
            &json!({"query": "repo:rust-lang/rust", "perPage": 1}),
        )
        .expect("search_repositories");
    let items = value["items"].as_array().expect("GitHub вернул items");
    assert_eq!(items[0]["full_name"], "rust-lang/rust");
    assert!(value["total_count"].as_u64().unwrap_or(0) > 0);
}
