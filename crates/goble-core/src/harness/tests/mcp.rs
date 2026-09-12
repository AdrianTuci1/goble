use super::*;
use crate::harness::*;

use futures::StreamExt;

#[tokio::test]
async fn test_harness_install_and_list_mcp() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![
                LlmToolCall {
                    id: "tc_mcp".to_string(),
                    name: "install_mcp_server".to_string(),
                    arguments: serde_json::json!({"id": "mcp-1", "name": "Files", "source": "npm", "source_value": "@modelcontextprotocol/server-files"}),
                },
                LlmToolCall {
                    id: "tc_list".to_string(),
                    name: "list_mcp_servers".to_string(),
                    arguments: serde_json::json!({}),
                },
            ],
            usage: None,
        },
    ));
    let harness = Harness::new(store.clone()).with_llm(llm);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "mcp", "mock", "mock")
        .collect()
        .await;
    let finished: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            HarnessEvent::ToolCallFinished { result, .. } => Some(result.clone()),
            _ => None,
        })
        .collect();
    assert!(finished.iter().any(|r| r.contains("mcp-1")));
    assert_eq!(store.list_mcp_servers().unwrap().len(), 1);
}

#[tokio::test]
async fn test_harness_search_mcp_servers_builtin() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![LlmToolCall {
                id: "tc_search".to_string(),
                name: "search_mcp_servers".to_string(),
                arguments: serde_json::json!({"query": "filesystem"}),
            }],
            usage: None,
        },
    ));
    let harness = Harness::new(store).with_llm(llm);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "find mcp", "mock", "mock")
        .collect()
        .await;
    let finished = events.iter().any(|e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result.contains("mcp-filesystem")));
    assert!(finished);
}

#[tokio::test]
async fn test_harness_install_update_delete_mcp_server() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![
                LlmToolCall {
                    id: "tc_install".to_string(),
                    name: "install_mcp_server".to_string(),
                    arguments: serde_json::json!({"id": "mcp-2", "name": "Test", "source": "npm", "source_value": "@modelcontextprotocol/server-sequential-thinking"}),
                },
                LlmToolCall {
                    id: "tc_list".to_string(),
                    name: "list_mcp_servers".to_string(),
                    arguments: serde_json::json!({}),
                },
                LlmToolCall {
                    id: "tc_update".to_string(),
                    name: "update_mcp_server".to_string(),
                    arguments: serde_json::json!({"id": "mcp-2", "name": "Test Updated"}),
                },
                LlmToolCall {
                    id: "tc_delete".to_string(),
                    name: "delete_mcp_server".to_string(),
                    arguments: serde_json::json!({"id": "mcp-2"}),
                },
            ],
            usage: None,
        },
    ));
    let harness = Harness::new(store.clone()).with_llm(llm);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "manage mcp", "mock", "mock")
        .collect()
        .await;
    let finished: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            HarnessEvent::ToolCallFinished { result, .. } => Some(result.clone()),
            _ => None,
        })
        .collect();
    assert!(finished
        .iter()
        .any(|r| r.contains("mcp-2") && r.contains("installed")));
    assert!(finished
        .iter()
        .any(|r| r.contains("mcp server mcp-2 updated")));
    assert!(finished
        .iter()
        .any(|r| r.contains("mcp server mcp-2 deleted")));
    assert!(store.list_mcp_servers().unwrap().is_empty());
}

#[tokio::test]
async fn test_harness_discover_and_call_mcp_tool() {
    use crate::mcp_installer::McpInstaller;
    use crate::mcp_manager::McpManager;

    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let server_path = src.join("index.js");
    let script = include_str!("../../../tests/mcp_mock_server.js");
    std::fs::write(&server_path, script).unwrap();

    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let manager = McpManager::new().with_installer(McpInstaller::new(tmp.path().join("cache")));
    manager
        .install_mcp_server(
            &store,
            "mock-echo",
            "Mock Echo",
            "local",
            Some(&src.to_string_lossy()),
            &[],
            Some(crate::agent::McpManifest {
                schema_version: "1".to_string(),
                entrypoint: "index.js".to_string(),
                runtime: crate::agent::McpRuntime::V8Isolate,
                auth_schema: vec![],
                capabilities: vec!["tools".to_string()],
                config_schema: serde_json::json!({}),
            }),
        )
        .await
        .unwrap();

    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![
                LlmToolCall {
                    id: "tc_discover".to_string(),
                    name: "discover_mcp_tools".to_string(),
                    arguments: serde_json::json!({"id": "mock-echo"}),
                },
                LlmToolCall {
                    id: "tc_call".to_string(),
                    name: "mcp_mock_echo_echo".to_string(),
                    arguments: serde_json::json!({"message": "hello from harness"}),
                },
            ],
            usage: None,
        },
    ));
    let harness = Harness::new(store).with_llm(llm).with_mcp_manager(manager);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "use mcp", "mock", "mock")
        .collect()
        .await;
    let finished: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            HarnessEvent::ToolCallFinished { result, .. } => Some(result.clone()),
            _ => None,
        })
        .collect();
    assert!(finished.iter().any(|r| r.contains("echo")));
    assert!(finished.iter().any(|r| r.contains("hello from harness")));
}
