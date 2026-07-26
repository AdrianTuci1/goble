use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use futures::StreamExt;
use goble_core::agent::{McpManifest, McpRuntime, McpServer, McpSource};
use goble_core::harness::{Harness, HarnessEvent, SandboxedCommandRunner};
use goble_core::llm::OpenAiProvider;
use goble_core::store::Store;

fn create_chat(store: &Store, title: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    store
        .insert_chat(
            &id,
            title,
            Some("deepseek"),
            Some("deepseek-v4-pro"),
            &now,
            &now,
        )
        .expect("insert chat");
    id
}

fn deepseek_provider() -> Arc<dyn goble_core::llm::LlmProvider> {
    let api_key = std::fs::read_to_string("/tmp/.goble_deepseek_key")
        .expect("deepseek key file")
        .trim()
        .to_string();
    Arc::new(OpenAiProvider::new(
        "deepseek",
        api_key,
        "https://api.deepseek.com/v1",
    ))
}

fn insert_mcp_server(store: &Store, server: &goble_core::agent::McpServer) {
    let source_value = match &server.source {
        goble_core::agent::McpSource::Local { path } => Some(path.as_str()),
        _ => serde_json::to_string(&server.source)
            .ok()
            .map(|s| s.leak())
            .map(|s| s as &str),
    };
    let manifest = serde_json::to_string(&server.manifest).unwrap();
    let now = server.installed_at.to_rfc3339();
    let updated = server.updated_at.to_rfc3339();
    store
        .insert_mcp_server(
            &server.id,
            &server.name,
            "local",
            source_value,
            &manifest,
            server.credentials_key.as_deref(),
            "[]",
            "[]",
            &now,
            &updated,
        )
        .expect("insert mcp server");
}

#[tokio::test]
async fn test_harness_deepseek_creates_agent_and_writes_file() {
    let store = Store::open_in_memory().expect("store");
    let chat_id = create_chat(&store, "deepseek harness test");

    let runner = Arc::new(SandboxedCommandRunner::new(
        HashSet::from([
            "echo".to_string(),
            "ls".to_string(),
            "cat".to_string(),
            "pwd".to_string(),
        ]),
        30,
        std::env::current_dir().unwrap(),
    ));
    let harness = Harness::new(store.clone())
        .with_llm(deepseek_provider())
        .with_runner(runner)
        .with_cancel(Arc::new(AtomicBool::new(false)));

    let prompt = "Create an agent named 'greeter' with prompt 'Greet the user warmly' and then write a file called greeter.txt containing exactly 'hello from goble agent' in the workspace.";
    let mut stream = harness.run_turn(&chat_id, prompt, "deepseek", "deepseek-v4-pro");

    let mut deltas = String::new();
    let mut finished = false;
    let mut errors = Vec::new();
    while let Some(event) = stream.next().await {
        match event {
            HarnessEvent::AssistantDelta(d) => deltas.push_str(&d),
            HarnessEvent::Done => finished = true,
            HarnessEvent::Error(e) => errors.push(e),
            HarnessEvent::ToolCallFinished { id, result } => eprintln!("tool {id}: {result}"),
            HarnessEvent::ToolCallError { id, message } => eprintln!("tool error {id}: {message}"),
            _ => {}
        }
    }
    assert!(finished, "stream did not finish; errors: {errors:?}");
    assert!(errors.is_empty(), "harness errors: {errors:?}");
    eprintln!("deltas: {deltas}");

    // Verify the agent was created
    let agents = store.list_agents().expect("list agents");
    assert!(
        agents.iter().any(|a| a.1 == "greeter"),
        "greeter agent not found: {agents:?}"
    );

    // Verify file was written
    let path = std::path::Path::new("greeter.txt");
    assert!(path.exists(), "greeter.txt not written");
    let content = std::fs::read_to_string(path).unwrap();
    assert!(
        content.contains("hello from goble agent"),
        "unexpected content: {content}"
    );
    std::fs::remove_file(path).ok();
}

#[tokio::test]
async fn test_harness_deepseek_with_mcp_mock() {
    let store = Store::open_in_memory().expect("store");
    let chat_id = create_chat(&store, "deepseek mcp test");

    // Register a local MCP server in the store
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
    insert_mcp_server(&store, &server);

    let harness = Harness::new(store)
        .with_llm(deepseek_provider())
        .with_cancel(Arc::new(AtomicBool::new(false)));

    let mut stream = harness.run_turn(
        &chat_id,
        "Use the mcp_mock-echo tool to echo the message 'gobble mcp ok' and report the result.",
        "deepseek",
        "deepseek-v4-pro",
    );

    let mut tool_results = Vec::new();
    let mut finished = false;
    let mut errors = Vec::new();
    while let Some(event) = stream.next().await {
        match event {
            HarnessEvent::Done => finished = true,
            HarnessEvent::Error(e) => errors.push(e),
            HarnessEvent::ToolCallFinished { result, .. } => tool_results.push(result),
            _ => {}
        }
    }
    assert!(finished, "stream did not finish; errors: {errors:?}");
    assert!(errors.is_empty(), "harness errors: {errors:?}");
    assert!(
        tool_results.iter().any(|r| r.contains("gobble mcp ok")),
        "mcp echo not found in results: {tool_results:?}"
    );
}

#[tokio::test]
async fn test_harness_deepseek_generates_web_server_project() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().join("webserver");
    let store = Store::open_in_memory().expect("store");
    let chat_id = create_chat(&store, "deepseek web server project");

    // Allow cargo/node only within the temp project directory
    let runner = Arc::new(SandboxedCommandRunner::new(
        HashSet::from([
            "echo".to_string(),
            "ls".to_string(),
            "cat".to_string(),
            "pwd".to_string(),
            "cargo".to_string(),
            "npm".to_string(),
            "node".to_string(),
        ]),
        120,
        project_dir.clone(),
    ));

    let harness = Harness::new(store)
        .with_llm(deepseek_provider())
        .with_runner(runner)
        .with_workspace_dir(project_dir.clone())
        .with_cancel(Arc::new(AtomicBool::new(false)));

    let prompt = "Create a minimal Rust web server project in the current directory. Write a Cargo.toml and a src/main.rs that starts an HTTP server on port 0 (random free port) and responds with 'Hello from Goble' at GET /. Use only std and no external dependencies. Do not run cargo build.";
    std::fs::create_dir_all(&project_dir).unwrap();
    let mut stream = harness.run_turn(&chat_id, prompt, "deepseek", "deepseek-v4-pro");

    let mut finished = false;
    let mut errors = Vec::new();
    while let Some(event) = stream.next().await {
        match event {
            HarnessEvent::Done => finished = true,
            HarnessEvent::Error(e) => errors.push(e),
            HarnessEvent::ToolCallFinished { id, result } => eprintln!("tool {id}: {result}"),
            HarnessEvent::ToolCallError { id, message } => eprintln!("tool error {id}: {message}"),
            _ => {}
        }
    }
    assert!(finished, "stream did not finish; errors: {errors:?}");
    assert!(errors.is_empty(), "harness errors: {errors:?}");

    // Verify project files
    assert!(
        project_dir.join("Cargo.toml").exists(),
        "Cargo.toml missing"
    );
    assert!(
        project_dir.join("src/main.rs").exists(),
        "src/main.rs missing"
    );
    let main_rs = std::fs::read_to_string(project_dir.join("src/main.rs")).unwrap();
    assert!(main_rs.contains("fn main"), "main.rs invalid: {main_rs}");
}
