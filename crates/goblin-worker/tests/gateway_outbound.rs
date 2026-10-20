//! The sandbox side of the gateway relay.
//!
//! A mock gateway plays the browser-facing relay: the worker binary starts with
//! the deploy environment, dials `/ws/worker`, answers the desktop protocol,
//! opens a PTY through the same outbound path, and reconnects after the gateway
//! closes the first socket.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use futures::{SinkExt, StreamExt};
use goble_core::protocol::{decode_pty_bytes, encode_pty_bytes};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::time::timeout;

const GOBLIN_BIN: &str = env!("CARGO_BIN_EXE_goblin");
const TOKEN: &str = "worker-secret";
const WORKER_ID: &str = "worker-1";
const TEST_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone)]
struct ConnectionMeta {
    worker_id: String,
    authorization: Option<String>,
}

#[derive(Debug)]
struct Report {
    index: usize,
    pong: bool,
    status: bool,
    pty_output: Option<bool>,
}

struct MockGateway {
    connections: Mutex<Vec<ConnectionMeta>>,
    reports: mpsc::UnboundedSender<Report>,
    cwd: String,
}

async fn worker_ws(
    State(gateway): State<Arc<MockGateway>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let index = {
        let mut connections = gateway.connections.lock().unwrap();
        connections.push(ConnectionMeta {
            worker_id: query.get("id").cloned().unwrap_or_default(),
            authorization: headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(str::to_string),
        });
        connections.len() - 1
    };
    ws.on_upgrade(move |socket| handle_worker(socket, gateway, index))
}

async fn handle_worker(socket: WebSocket, gateway: Arc<MockGateway>, index: usize) {
    let (mut sink, mut stream) = socket.split();
    let mut report = Report {
        index,
        pong: false,
        status: false,
        pty_output: None,
    };

    if sink
        .send(Message::Text(r#"{"kind":"ping"}"#.into()))
        .await
        .is_err()
    {
        let _ = gateway.reports.send(report);
        return;
    }

    while !(report.pong && report.status) {
        let Some(text) = next_text(&mut stream).await else {
            break;
        };
        match kind(&text).as_deref() {
            Some("pong") => report.pong = true,
            Some("status_report") => report.status = true,
            _ => {}
        }
    }

    if index == 1 {
        report.pty_output = Some(pty_round_trip(&mut sink, &mut stream, &gateway.cwd).await);
    }

    let _ = sink.send(Message::Close(None)).await;
    let _ = gateway.reports.send(report);
}

/// Drive one terminal end to end over the outbound connection: open, type,
/// observe the child's output, close.
async fn pty_round_trip(
    sink: &mut futures::stream::SplitSink<WebSocket, Message>,
    stream: &mut futures::stream::SplitStream<WebSocket>,
    cwd: &str,
) -> bool {
    let open = serde_json::json!({
        "kind": "open_pty",
        "pane_id": "pane-1",
        "cwd": cwd,
        "cols": 80,
        "rows": 24,
    })
    .to_string();
    if sink.send(Message::Text(open.into())).await.is_err() {
        return false;
    }

    let mut opened = false;
    while let Some(text) = next_text(stream).await {
        if kind(&text).as_deref() == Some("pty_opened") {
            opened = true;
            break;
        }
    }
    if !opened {
        return false;
    }

    // The terminal echoes the command, so the marker is split in the printf
    // format: only the child's expanded output contains `goblin-pty-ok`.
    let input = serde_json::json!({
        "kind": "pty_input",
        "pane_id": "pane-1",
        "data": encode_pty_bytes(b"printf 'goblin-%s-ok\\n' pty\n"),
    })
    .to_string();
    if sink.send(Message::Text(input.into())).await.is_err() {
        return false;
    }

    let mut output = false;
    while let Some(text) = next_text(stream).await {
        let Ok(frame) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if frame.get("kind").and_then(|value| value.as_str()) != Some("pty_output") {
            continue;
        }
        let data = frame
            .get("data")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if decode_pty_bytes(data)
            .map(|bytes| String::from_utf8_lossy(&bytes).contains("goblin-pty-ok"))
            .unwrap_or(false)
        {
            output = true;
            break;
        }
    }

    let close = serde_json::json!({"kind": "close_pty", "pane_id": "pane-1"}).to_string();
    let _ = sink.send(Message::Text(close.into())).await;
    output
}

/// One text frame, or `None` when the socket ends or the wait expires.
async fn next_text(stream: &mut futures::stream::SplitStream<WebSocket>) -> Option<String> {
    timeout(TEST_TIMEOUT, async {
        while let Some(message) = stream.next().await {
            match message {
                Ok(Message::Text(text)) => return Some(text.to_string()),
                Ok(Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
}

fn kind(text: &str) -> Option<String> {
    let frame: serde_json::Value = serde_json::from_str(text).ok()?;
    frame
        .get("kind")
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

/// Kills the worker even when the test fails before its explicit cleanup.
struct WorkerProcess(tokio::process::Child);

impl Drop for WorkerProcess {
    fn drop(&mut self) {
        let _ = self.0.start_kill();
    }
}

fn spawn_worker(gateway_port: u16, dir: &Path) -> WorkerProcess {
    assert!(
        Path::new(GOBLIN_BIN).exists(),
        "cargo did not build the `goblin` binary at {GOBLIN_BIN}"
    );
    let mut child = tokio::process::Command::new(GOBLIN_BIN)
        .arg("--bind")
        .arg("127.0.0.1:0")
        .arg("--workspace-root")
        .arg(dir.join("workspace"))
        .arg("--task-store")
        .arg(dir.join("tasks.db"))
        .arg("--vault-path")
        .arg(dir.join("vault.json"))
        .env("RUST_LOG", "info")
        .env("LLM_PROVIDER", "mock")
        .env(
            "GOBLE_GATEWAY_URL",
            format!("ws://127.0.0.1:{gateway_port}"),
        )
        .env("GOBLE_WORKER_TOKEN", TOKEN)
        .env("GOBLE_WORKER_ID", WORKER_ID)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn goblin-worker");

    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(drain_logs(stderr));
    }
    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(drain_logs(stdout));
    }
    WorkerProcess(child)
}

async fn drain_logs<R>(reader: R)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        eprintln!("worker log: {line}");
    }
}

#[tokio::test]
async fn test_worker_dials_the_gateway_and_reconnects() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock gateway");
    let port = listener.local_addr().expect("mock gateway address").port();
    let (reports_tx, mut reports_rx) = mpsc::unbounded_channel();
    let workspace = tempfile::tempdir().expect("temp dir");
    let gateway = Arc::new(MockGateway {
        connections: Mutex::new(Vec::new()),
        reports: reports_tx,
        cwd: workspace.path().to_string_lossy().into_owned(),
    });

    let app = Router::new()
        .route("/ws/worker", get(worker_ws))
        .with_state(Arc::clone(&gateway));
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve mock gateway");
    });

    let _worker = spawn_worker(port, workspace.path());

    let first = timeout(TEST_TIMEOUT, reports_rx.recv())
        .await
        .expect("the worker never dialed the gateway")
        .expect("report channel closed");
    assert_eq!(first.index, 0);
    assert!(first.pong, "the worker did not answer the ping");
    assert!(first.status, "the worker did not send a status report");

    let second = timeout(TEST_TIMEOUT, reports_rx.recv())
        .await
        .expect("the worker did not reconnect after the first socket closed")
        .expect("report channel closed");
    assert_eq!(second.index, 1);
    assert!(second.pong && second.status);
    assert_eq!(
        second.pty_output,
        Some(true),
        "the pty did not round-trip over the outbound connection"
    );

    let connections = gateway.connections.lock().unwrap();
    assert_eq!(connections.len(), 2, "expected exactly two connections");
    for meta in connections.iter() {
        assert_eq!(meta.worker_id, WORKER_ID);
        assert_eq!(meta.authorization.as_deref(), Some("Bearer worker-secret"));
    }
    drop(connections);

    server.abort();
    let _ = server.await;
}
