use std::time::Duration;

use futures::{SinkExt, StreamExt};
use goble_core::agent::AgentSpec;
use goble_core::protocol::DesktopMessage;
use tokio::io::{AsyncBufReadExt, BufReader};

fn find_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().unwrap().port()
}

#[tokio::test]
async fn test_worker_health_and_websocket_run_agent() {
    let port = find_free_port();
    let workspace = tempfile::TempDir::new().unwrap();
    let bin = std::env::var("GOBLIN_BIN")
        .unwrap_or_else(|_| "/root/goble/target/release/goblin".to_string());
    assert!(std::path::Path::new(&bin).exists(), "goblin binary not found at {bin}; build with `cargo build --release --package goblin-worker`");

    let mut child = tokio::process::Command::new(&bin)
        .args([
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--workspace-root",
            &workspace.path().join("workspace").to_string_lossy(),
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

    // Read stderr for diagnostics, but primarily poll health endpoint.
    let stderr = child.stderr.take().unwrap();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            eprintln!("worker log: {line}");
        }
    });

    // Health check
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

    // WebSocket run agent
    let ws_url = format!("ws://127.0.0.1:{port}/ws");
    let (ws_stream, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .expect("connect ws");
    let (mut write, mut read) = ws_stream.split();

    let spec = AgentSpec::new("health-test", "do nothing and finish");
    let agent_id = spec.id.clone();
    let run_msg = DesktopMessage::RunAgent {
        trace_id: "trace-health-test".to_string(),
        agent_id,
        spec,
        mcp_servers: vec![],
    };
    write
        .send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::to_string(&run_msg).unwrap().into(),
        ))
        .await
        .expect("send run agent");

    let mut finished = false;
    for _ in 0..60 {
        if let Ok(Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text)))) =
            tokio::time::timeout(Duration::from_millis(500), read.next()).await
        {
            if let Ok(event) = serde_json::from_str::<goble_core::protocol::WorkerMessage>(&text) {
                match event {
                    goble_core::protocol::WorkerMessage::AgentFinished { status, .. } => {
                        assert_eq!(status, goble_core::execution::ExecutionStatus::Success);
                        finished = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    let _ = child.kill().await;
    assert!(finished, "agent did not finish");
}

#[test]
fn test_desktop_message_serialize_run_agent() {
    let spec = AgentSpec::new("demo", "do nothing");
    let msg = DesktopMessage::RunAgent {
        trace_id: "t".to_string(),
        agent_id: spec.id.clone(),
        spec,
        mcp_servers: vec![],
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("run_agent"));
}
