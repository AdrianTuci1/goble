//! End-to-end tests for the Goble agent runtime on a remote worker.
//!
//! Both tests drive the real `goblin` binary (`CARGO_BIN_EXE_goblin`) over the
//! same HTTP + WebSocket surface the desktop speaks after pairing:
//!
//! 1. A local AI API key is added in the desktop UI (vault secret).
//! 2. The desktop pushes the secret to the remote worker.
//! 3. The desktop pushes an MCP server that requires the secret.
//! 4. The desktop runs two agents that both reference the same MCP server.
//! 5. The worker must start a separate isolated workspace for each agent and
//!    pass the secret through to the MCP server.
//!
//! Steps 1-4 and the per-agent workspace isolation run in the default suite.
//! The MCP half — the worker installing the server once into a shared cache and
//! spawning it per agent with the secret in its environment — needs a
//! worker-side MCP isolate runtime that this repo does not build yet, so it
//! lives in the `#[ignore]`d `test_mcp_isolate_runtime_writes_per_workspace_markers`.
//! Run it with:
//!
//! ```text
//! cargo test -p goblin-worker --test runtime_isolation_e2e -- --ignored \
//!   test_mcp_isolate_runtime_writes_per_workspace_markers
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use goble_core::agent::{
    AgentSpec, AuthField, AuthFieldType, McpManifest, McpRuntime, McpServer, McpSource,
};
use goble_core::execution::ExecutionStatus;
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use goble_core::secret::Secret;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio_tungstenite::tungstenite::Message;

/// The `goblin` binary cargo builds for this package's integration tests.
const GOBLIN_BIN: &str = env!("CARGO_BIN_EXE_goblin");

/// The AI API key the desktop "adds" locally before pushing it to the worker.
const LOCAL_AI_KEY: &str = "goble-test-api-key-12345";

/// The two agent runs the desktop starts against the worker.
const AGENT_TAGS: [&str; 2] = ["alpha", "beta"];

fn trace_id(tag: &str) -> String {
    format!("trace-{tag}")
}

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
type WsSender = futures::stream::SplitSink<WsStream, Message>;
type WsReceiver = futures::stream::SplitStream<WsStream>;

fn find_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().unwrap().port()
}

/// Spawn the worker on `port` rooted at `workspace`, forward its output, and
/// return once `/health` reports `Online`.
async fn start_worker(workspace: &Path, port: u16) -> Child {
    assert!(
        Path::new(GOBLIN_BIN).exists(),
        "cargo did not build the `goblin` binary at {GOBLIN_BIN}"
    );

    let mut child = tokio::process::Command::new(GOBLIN_BIN)
        .args([
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--workspace-root",
            &workspace.join("workspaces").to_string_lossy(),
            "--task-store",
            &workspace.join("tasks.db").to_string_lossy(),
            "--vault-path",
            &workspace.join("vault.json").to_string_lossy(),
        ])
        .env("RUST_LOG", "info")
        .env("LLM_PROVIDER", "mock")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn goblin-worker");

    // Both pipes are drained: the worker's `tracing` output goes to stdout, so
    // reading only stderr leaves the run's diagnostics invisible.
    let stderr = child.stderr.take().unwrap();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            eprintln!("worker log: {line}");
        }
    });
    let stdout = child.stdout.take().unwrap();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            eprintln!("worker log: {line}");
        }
    });

    let health_url = format!("http://127.0.0.1:{port}/health");
    for _ in 0..60 {
        if let Ok(resp) = reqwest::get(&health_url).await {
            if resp.status().is_success() {
                let report: serde_json::Value = resp.json().await.expect("json");
                assert_eq!(report["status"], "Online");
                return child;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let _ = child.kill().await;
    panic!("health check failed");
}

/// Connect to the worker WebSocket, just like the desktop does after pairing.
async fn connect_ws(port: u16) -> (WsSender, WsReceiver) {
    let (stream, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws"))
        .await
        .expect("connect ws");
    stream.split()
}

async fn send(write: &mut WsSender, msg: &DesktopMessage) {
    let json = serde_json::to_string(msg).expect("serialize desktop message");
    write
        .send(Message::Text(json.into()))
        .await
        .expect("send desktop message");
}

/// Wait for `expected` successful `AgentFinished` events, panicking on a run
/// the worker reports as failed.
async fn wait_for_successful_agents(read: &mut WsReceiver, expected: usize) -> usize {
    let mut finished = 0usize;
    for _ in 0..120 {
        if finished >= expected {
            break;
        }
        match tokio::time::timeout(Duration::from_millis(500), read.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                if let Ok(WorkerMessage::AgentFinished { status, .. }) =
                    serde_json::from_str::<WorkerMessage>(&text)
                {
                    assert_eq!(status, ExecutionStatus::Success, "agent run failed");
                    finished += 1;
                }
            }
            Ok(Some(Ok(_))) => {}
            Ok(Some(Err(e))) => panic!("worker websocket failed: {e}"),
            Ok(None) => panic!("worker closed the websocket before {expected} agents finished"),
            Err(_) => {}
        }
    }
    finished
}

/// The MCP server the desktop pushes: a local V8-isolate server whose
/// `credentials_key` maps the `AI_API_KEY` environment variable to the pushed
/// secret's id, never to the plaintext key.
fn runtime_mock_server(mcp_dir: &Path, secret_id: &str) -> McpServer {
    std::fs::create_dir_all(mcp_dir).unwrap();
    std::fs::write(
        mcp_dir.join("index.js"),
        include_str!("runtime_mcp_server.js"),
    )
    .unwrap();

    McpServer {
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
            serde_json::to_string(&HashMap::from([(
                "AI_API_KEY".to_string(),
                secret_id.to_string(),
            )]))
            .unwrap(),
        ),
        installed_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// The per-run agent workspaces the worker materialized for `traces`: the
/// `<workspace_root>/harness/<trace_id>` directory each run's harness is handed.
/// A workspace shared between runs would collapse to a single directory.
fn agent_workspaces(workspace_root: &Path, traces: &[String]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = traces
        .iter()
        .map(|trace| workspace_root.join("harness").join(trace))
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// Drive the desktop-side flow against a worker rooted at `workspace`: push the
/// AI API key, push the MCP server that requires it, then run two agents that
/// both reference that server.
///
/// Returns the still-running worker and the two agent workspaces it created.
async fn run_isolation_flow(workspace: &Path, port: u16) -> (Child, Vec<PathBuf>) {
    let child = start_worker(workspace, port).await;
    let (mut write, mut read) = connect_ws(port).await;

    // 1. Create a local vault secret representing an AI API key added in the desktop UI.
    let secret = Secret::new("openai-api-key", "llm", LOCAL_AI_KEY.as_bytes().to_vec());
    let secret_id = secret.id.clone();

    // 2. Push the secret to the remote worker (vault passthrough).
    send(
        &mut write,
        &DesktopMessage::PushSecrets {
            secrets: vec![secret],
        },
    )
    .await;

    // 3. Push a local MCP server that requires the AI API key.
    let server = runtime_mock_server(&workspace.join("mcp_runtime_mock"), &secret_id);
    send(
        &mut write,
        &DesktopMessage::PushMcpServers {
            servers: vec![server.clone()],
        },
    )
    .await;

    // 4. Run two agents that both use the same shared MCP server.
    let traces: Vec<String> = AGENT_TAGS.iter().map(|tag| trace_id(tag)).collect();
    for (tag, trace) in AGENT_TAGS.iter().zip(&traces) {
        let mut spec = AgentSpec::new(
            format!("isolation-{tag}"),
            "verify runtime isolation and secret passthrough",
        );
        spec.mcp_ids = vec!["runtime-mock".to_string()];
        send(
            &mut write,
            &DesktopMessage::RunAgent {
                trace_id: trace.clone(),
                agent_id: spec.id.clone(),
                spec,
                mcp_servers: vec![server.clone()],
            },
        )
        .await;
    }

    // 5. Both agents must report success.
    let finished = wait_for_successful_agents(&mut read, traces.len()).await;
    assert_eq!(finished, 2, "both agents did not finish successfully");

    let dirs = agent_workspaces(&workspace.join("workspaces"), &traces);
    (child, dirs)
}

#[tokio::test]
async fn test_agent_runtime_isolation_and_secret_passthrough() {
    let port = find_free_port();
    let workspace = tempfile::TempDir::new().unwrap();
    let (mut child, agent_dirs) = run_isolation_flow(workspace.path(), port).await;

    // The socket handler processes desktop messages in order, so by the time both
    // runs finished the two pushes above are registered: the pushed server is
    // known to the worker with its V8-isolate manifest.
    let listed: serde_json::Value = reqwest::get(format!("http://127.0.0.1:{port}/mcp"))
        .await
        .expect("GET /mcp")
        .json()
        .await
        .expect("mcp list json");
    assert_eq!(listed["servers"][0]["id"], "runtime-mock");
    assert_eq!(listed["servers"][0]["runtime"], "V8Isolate");

    let _ = child.kill().await;

    // 6. Verify workspace isolation: each agent run has its own directory.
    assert_eq!(
        agent_dirs.len(),
        AGENT_TAGS.len(),
        "expected one isolated agent workspace per run, found {agent_dirs:?}"
    );
    let names: Vec<String> = agent_dirs
        .iter()
        .map(|dir| dir.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, vec![trace_id("alpha"), trace_id("beta")]);

    // 7. Verify the secret passthrough reached the run: the worker persists every
    // secret a run is handed for the harness (the hand-off of the same key to the
    // MCP server's environment is asserted by the ignored test below).
    let store =
        goble_core::store::Store::open(workspace.path().join("workspaces").join("worker.db"))
            .expect("open worker store");
    let secrets = store.list_vault_secrets().expect("list vault secrets");
    let (_, value, _, _) = secrets
        .iter()
        .find(|(key, ..)| key == "openai-api-key")
        .expect("pushed secret was not persisted by the worker");
    assert_eq!(
        value,
        LOCAL_AI_KEY.as_bytes(),
        "AI API key was not passed through to the worker"
    );
}

/// The MCP half of the isolation guarantee: the worker installs the pushed
/// server once into a shared cache and starts it inside each agent's workspace
/// with `AI_API_KEY` in its environment, so the marker the server writes on
/// startup proves the isolation.
///
/// `#[ignore]`d because this repo builds no worker-side MCP isolate runtime:
/// `Runner::prepare_run` builds `McpManager::new()` with no installer (so its
/// cache dir is `None` and nothing is installed or spawned) and drives the
/// harness with a provider that makes no tool calls. The assertions below are
/// the acceptance criteria for that runtime, and running them needs `node` on
/// `PATH`.
#[tokio::test]
#[ignore = "needs a worker-side MCP/V8 isolate runtime (installs and spawns MCP servers) + node on PATH; run: cargo test -p goblin-worker --test runtime_isolation_e2e -- --ignored test_mcp_isolate_runtime_writes_per_workspace_markers"]
async fn test_mcp_isolate_runtime_writes_per_workspace_markers() {
    let port = find_free_port();
    let workspace = tempfile::TempDir::new().unwrap();
    let (mut child, agent_dirs) = run_isolation_flow(workspace.path(), port).await;

    let _ = child.kill().await;

    // Verify workspace isolation and secret passthrough: each agent workspace has
    // a distinct marker written by the MCP server on startup.
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

    // Verify MCP cache is shared: only one install directory for runtime-mock.
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
