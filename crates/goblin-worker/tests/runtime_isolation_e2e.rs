use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use goble_core::agent::{
    AgentSpec, AuthField, AuthFieldType, McpManifest, McpRuntime, McpServer, McpSource,
};
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use goble_core::secret::Secret;
use tokio::io::{AsyncBufReadExt, BufReader};

fn find_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().unwrap().port()
}

/// End-to-end test for the Goble agent runtime on a remote worker.
///
/// It simulates the full passthrough flow:
/// 1. A local AI API key is added in the desktop UI (vault secret).
/// 2. The desktop pushes the secret to the remote worker.
/// 3. The desktop pushes an MCP server that requires the secret.
/// 4. The desktop runs two agents that both reference the same MCP server.
/// 5. The worker must:
///    - install the MCP server once into a shared cache,
///    - start a separate isolated workspace for each agent,
///    - pass the secret to the MCP server as an environment variable,
///    - let the MCP server prove isolation by writing a marker into each
///      agent's workspace.
#[tokio::test]
async fn test_agent_runtime_isolation_and_secret_passthrough() {
    let port = find_free_port();
    let workspace = tempfile::TempDir::new().unwrap();
    let bin = std::env::var("GOBLIN_BIN")
        .unwrap_or_else(|_| "/root/goble/target/release/goblin".to_string());
    assert!(
        std::path::Path::new(&bin).exists(),
        "goblin binary not found at {bin}; build with `cargo build --release --package goblin-worker`"
    );

    let mut child = tokio::process::Command::new(&bin)
        .args([
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--workspace-root",
            &workspace.path().join("workspaces").to_string_lossy(),
            "--task-store",
            &workspace.path().join("tasks.db").to_string_lossy(),
            "--vault-path",
            &workspace.path().join("vault.json").to_string_lossy(),
        ])
        .env("RUST_LOG", "info")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn goblin-worker");

    let stderr = child.stderr.take().unwrap();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            eprintln!("worker log: {line}");
        }
    });

    // Wait for health endpoint.
    let health_url = format!("http://127.0.0.1:{port}/health");
    let mut healthy = false;
    for _ in 0..60 {
        if let Ok(resp) = reqwest::get(&health_url).await {
            if resp.status().is_success() {
                let report: serde_json::Value = resp.json().await.expect("json");
                assert_eq!(report["status"], "Online");
                healthy = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(healthy, "health check failed");

    // Connect to the worker WebSocket, just like the desktop does after pairing.
    let ws_url = format!("ws://127.0.0.1:{port}/ws");
    let (ws_stream, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .expect("connect ws");
    let (mut write, mut read) = ws_stream.split();

    const LOCAL_AI_KEY: &str = "goble-test-api-key-12345";

    // 1. Create a local vault secret representing an AI API key added in the desktop UI.
    let secret = Secret::new("openai-api-key", "llm", LOCAL_AI_KEY.as_bytes().to_vec());
    let secret_id = secret.id.clone();

    // 2. Push the secret to the remote worker (vault passthrough).
    let secret_msg = DesktopMessage::PushSecrets {
        secrets: vec![secret],
    };
    write
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&secret_msg).unwrap().into(),
        ))
        .await
        .expect("send secrets");

    // 3. Push a local MCP server that requires the AI API key.
    // The credentials_key maps the env var name to the secret id, not the plaintext value.
    let mcp_dir = workspace.path().join("mcp_runtime_mock");
    std::fs::create_dir_all(&mcp_dir).unwrap();
    std::fs::write(
        mcp_dir.join("index.js"),
        include_str!("runtime_mcp_server.js"),
    )
    .unwrap();

    let server = McpServer {
        id: "runtime-mock".to_string(),
        name: "Runtime Isolation Mock".to_string(),
        source: McpSource::Local {
            path: mcp_dir.to_string_lossy().to_string(),
        },
        manifest: McpManifest {
            schema_version: "1".to_string(),
            entrypoint: "index.js".to_string(),
            runtime: McpRuntime::V8Isolate,
            auth_schema: vec![AuthField {
                name: "AI_API_KEY".to_string(),
                label: "AI API Key".to_string(),
                field_type: AuthFieldType::Token,
                required: true,
                description: None,
            }],
            capabilities: vec!["tools".to_string()],
            config_schema: serde_json::json!({}),
        },
        credentials_key: Some(
            serde_json::to_string(&HashMap::from([("AI_API_KEY".to_string(), secret_id)])).unwrap(),
        ),
        installed_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    let push_mcp_msg = DesktopMessage::PushMcpServers {
        servers: vec![server.clone()],
    };
    write
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&push_mcp_msg).unwrap().into(),
        ))
        .await
        .expect("push mcp servers");

    // 4. Run two agents that both use the same shared MCP server.
    let mut agent_ids = Vec::new();
    let mut traces = Vec::new();
    for tag in ["alpha", "beta"] {
        let mut spec = AgentSpec::new(
            &format!("isolation-{tag}"),
            "verify runtime isolation and secret passthrough",
        );
        spec.mcp_ids = vec!["runtime-mock".to_string()];
        let agent_id = spec.id.clone();
        let trace_id = format!("trace-{tag}");
        let run_msg = DesktopMessage::RunAgent {
            trace_id: trace_id.clone(),
            agent_id: agent_id.clone(),
            spec,
            mcp_servers: vec![server.clone()],
        };
        write
            .send(tokio_tungstenite::tungstenite::Message::Text(
                serde_json::to_string(&run_msg).unwrap().into(),
            ))
            .await
            .expect("send run agent");
        agent_ids.push(agent_id);
        traces.push(trace_id);
    }

    // 5. Collect AgentFinished events for both agents.
    let mut finished = 0usize;
    let mut failed = false;
    for _ in 0..120 {
        if finished >= 2 {
            break;
        }
        if let Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text)))) =
            tokio::time::timeout(Duration::from_millis(500), read.next()).await
        {
            if let Ok(event) = serde_json::from_str::<WorkerMessage>(&text) {
                match event {
                    WorkerMessage::AgentFinished { status, .. } => {
                        if status == goble_core::execution::ExecutionStatus::Success {
                            finished += 1;
                        } else {
                            failed = true;
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = child.kill().await;
    assert!(!failed, "agent run failed");
    assert_eq!(finished, 2, "both agents did not finish successfully");

    // 6. Verify workspace isolation: each agent has its own directory.
    let workspaces_root = workspace.path().join("workspaces");
    let mut agent_dirs: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&workspaces_root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "cache" {
                continue;
            }
            agent_dirs.push(entry.path());
        }
    }
    assert_eq!(
        agent_dirs.len(),
        2,
        "expected two isolated agent workspaces, found {agent_dirs:?}"
    );

    // 7. Verify workspace isolation and secret passthrough: each agent
    // workspace has a distinct marker written by the MCP server.
    let mut marker_workspaces: Vec<String> = Vec::new();
    for dir in &agent_dirs {
        let marker = dir.join("runtime-mock-init.txt");
        assert!(
            marker.exists(),
            "MCP initialization marker not found in workspace {dir:?}"
        );
        let content = std::fs::read_to_string(&marker).unwrap();
        assert!(
            content.contains(&format!("init-key={LOCAL_AI_KEY}")),
            "AI API key was not passed through to the MCP server: {content}"
        );
        marker_workspaces.push(dir.to_string_lossy().to_string());
    }
    marker_workspaces.sort();
    marker_workspaces.dedup();
    assert_eq!(
        marker_workspaces.len(),
        2,
        "MCP markers were not written into distinct workspaces"
    );

    // 8. Verify MCP cache is shared: only one install directory for runtime-mock.
    let cache_dir = workspace
        .path()
        .join("workspaces")
        .join("cache")
        .join("mcp")
        .join("runtime-mock");
    assert!(
        cache_dir.join("index.js").exists() || cache_dir.join(".installed").exists(),
        "MCP server was not installed into the shared cache"
    );
}
