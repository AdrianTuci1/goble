use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use futures::StreamExt;
use goble_core::agent::{McpManifest, McpRuntime, McpServer, McpSource};
use goble_core::harness::{Harness, HarnessEvent};
use goble_core::llm::{CompletionResponse, LlmToolCall, MockProvider};
use goble_core::mcp_manager::McpManager;
use goble_core::store::Store;

fn create_chat(store: &Store, title: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    store
        .insert_chat(&id, title, Some("mock"), Some("m"), &now, &now)
        .expect("insert chat");
    id
}

fn install_local_mock_server(store: &Store) -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("index.js"), include_str!("mcp_mock_server.js")).unwrap();

    let server = McpServer {
        id: "mock-echo".to_string(),
        name: "Mock Echo".to_string(),
        source: McpSource::Local {
            path: src.to_string_lossy().to_string(),
        },
        manifest: McpManifest {
            schema_version: "1".to_string(),
            entrypoint: "index.js".to_string(),
            runtime: McpRuntime::V8Isolate,
            auth_schema: vec![],
            capabilities: vec!["tools".to_string()],
            config_schema: serde_json::json!({}),
        },
        credentials_key: None,
        installed_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let source_value = match &server.source {
        McpSource::Local { path } => Some(path.as_str()),
        _ => None,
    };
    store
        .insert_mcp_server(
            &server.id,
            &server.name,
            "local",
            source_value,
            &serde_json::to_string(&server.manifest).unwrap(),
            server.credentials_key.as_deref(),
            "[]",
            "[]",
            &server.installed_at.to_rfc3339(),
            &server.updated_at.to_rfc3339(),
        )
        .expect("insert mcp server");
    tmp
}

async fn run_harness_turn(harness: &Harness, chat_id: &str) -> (bool, Vec<String>, Vec<String>) {
    let mut finished = false;
    let mut tool_results = Vec::new();
    let mut errors = Vec::new();
    let mut stream = harness.run_turn(chat_id, "trigger", "mock", "m");
    while let Some(event) = stream.next().await {
        match event {
            HarnessEvent::Done => finished = true,
            HarnessEvent::Error(e) => errors.push(e),
            HarnessEvent::ToolCallFinished { result, .. } => tool_results.push(result),
            HarnessEvent::ToolCallError { id, message } => eprintln!("tool error {id}: {message}"),
            _ => {}
        }
    }
    (finished, tool_results, errors)
}

#[tokio::test]
async fn test_harness_mcp_mock_tool_call() {
    let store = Store::open_in_memory().expect("store");
    let chat_id = create_chat(&store, "mcp mock test");
    let _tmp = install_local_mock_server(&store);

    let manager = McpManager::new();
    manager
        .discover_and_enable_all(&store, "mock-echo")
        .unwrap();

    let harness = Harness::new(store)
        .with_llm(Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: "".to_string(),
                tool_calls: vec![LlmToolCall {
                    id: "tc1".to_string(),
                    name: "mcp_mock_echo_echo".to_string(),
                    arguments: serde_json::json!({ "message": "gobble mcp ok" }),
                }],
            },
        )))
        .with_cancel(Arc::new(AtomicBool::new(false)));

    let (finished, tool_results, errors) = run_harness_turn(&harness, &chat_id).await;
    assert!(finished, "stream did not finish; errors: {errors:?}");
    assert!(errors.is_empty(), "harness errors: {errors:?}");
    assert!(
        tool_results.iter().any(|r| r.contains("gobble mcp ok")),
        "mcp echo result not found: {tool_results:?}"
    );
}

#[tokio::test]
async fn test_harness_mcp_generic_call_fallback() {
    let store = Store::open_in_memory().expect("store");
    let chat_id = create_chat(&store, "mcp generic test");
    let _tmp = install_local_mock_server(&store);

    let harness = Harness::new(store)
        .with_llm(Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: "".to_string(),
                tool_calls: vec![LlmToolCall {
                    id: "tc2".to_string(),
                    name: "mcp_call".to_string(),
                    arguments: serde_json::json!({
                        "server_id": "mock-echo",
                        "tool": "echo",
                        "arguments": { "message": "generic fallback" }
                    }),
                }],
            },
        )))
        .with_cancel(Arc::new(AtomicBool::new(false)));

    let (finished, tool_results, errors) = run_harness_turn(&harness, &chat_id).await;
    assert!(finished, "stream did not finish; errors: {errors:?}");
    assert!(errors.is_empty(), "harness errors: {errors:?}");
    assert!(
        tool_results.iter().any(|r| r.contains("generic fallback")),
        "generic mcp call result not found: {tool_results:?}"
    );
}

#[tokio::test]
async fn test_harness_mcp_install_list_delete() {
    let store = Store::open_in_memory().expect("store");
    let chat_id = create_chat(&store, "mcp install test");
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("index.js"), include_str!("mcp_mock_server.js")).unwrap();

    let harness = Harness::new(store.clone())
        .with_llm(Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: "".to_string(),
                tool_calls: vec![LlmToolCall {
                    id: "tc_install".to_string(),
                    name: "install_mcp_server".to_string(),
                    arguments: serde_json::json!({
                        "id": "mock-echo",
                        "name": "Mock Echo",
                        "source": "local",
                        "source_value": src.to_string_lossy(),
                    }),
                }],
            },
        )))
        .with_cancel(Arc::new(AtomicBool::new(false)));

    let (finished, _results, errors) = run_harness_turn(&harness, &chat_id).await;
    assert!(finished, "stream did not finish; errors: {errors:?}");
    assert!(errors.is_empty(), "harness errors: {errors:?}");

    let rows = store.list_mcp_servers().expect("list mcp servers");
    assert!(
        rows.iter().any(|(id, ..)| id == "mock-echo"),
        "mcp server not installed"
    );
}
