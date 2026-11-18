//! The remote DSH runtime over the worker's outbound relay protocol.
//!
//! This is the full path a phone/web client takes: a desktop-protocol frame
//! arrives on the worker's socket, the worker runs the DSH harness that is
//! installed in its sandbox, and the harness's NDJSON events come back as the
//! ordinary stream frames. The mock gateway here copies frames opaquely in both
//! directions, exactly like `goble-gateway`'s `/ws/worker` relay
//! (`crates/goble-gateway/src/ws.rs`), so the test covers the wire protocol
//! rather than an in-process shortcut.
//!
//! The `dsh` binary is a shell fixture: the real harness image is only needed
//! for the docker smoke test documented in `docs/remote-dsh-runtime.md`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use futures::{SinkExt, StreamExt};
use goble_core::agent::{AgentId, AgentSpec};
use goble_core::protocol::DesktopMessage;
use tokio::sync::mpsc;
use tokio::time::timeout;

const GOBLIN_BIN: &str = env!("CARGO_BIN_EXE_goblin");
const TOKEN: &str = "worker-secret";
const WORKER_ID: &str = "dsh-worker";
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
struct Report {
    /// Text the harness deltas carried, as the client would render it.
    deltas: String,
    tool_calls: usize,
    tool_results: usize,
    usage_lines: usize,
    turn_ended: bool,
    reply: Option<String>,
    finished: Option<String>,
    /// The second run was cancelled after its first delta.
    cancelled: bool,
}

struct MockGateway {
    reports: mpsc::UnboundedSender<Report>,
}

async fn worker_ws(
    State(gateway): State<std::sync::Arc<MockGateway>>,
    Query(_query): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| handle_worker(socket, gateway))
}

fn frame_text(value: &DesktopMessage) -> Message {
    Message::Text(serde_json::to_string(value).unwrap().into())
}

async fn send_frame(
    sink: &mut futures::stream::SplitSink<WebSocket, Message>,
    value: &DesktopMessage,
) -> bool {
    sink.send(frame_text(value)).await.is_ok()
}

fn run_frame(trace: &str, prompt: &str) -> DesktopMessage {
    let now = chrono::Utc::now().to_rfc3339();
    let spec = AgentSpec {
        id: AgentId("agent-dsh".to_string()),
        name: "Phone session".to_string(),
        description: String::new(),
        prompt: "You are a coding agent.".to_string(),
        tools: Vec::new(),
        triggers: Vec::new(),
        mcp_ids: Vec::new(),
        created_at: now.clone(),
        updated_at: now,
    };
    DesktopMessage::RunAgentForThreadReply {
        trace_id: trace.to_string(),
        thread_id: format!("thread-{trace}"),
        agent_id: AgentId("agent-dsh".to_string()),
        prompt: prompt.to_string(),
        spec,
        mcp_servers: Vec::new(),
    }
}

async fn handle_worker(socket: WebSocket, gateway: std::sync::Arc<MockGateway>) {
    let (mut sink, mut stream) = socket.split();
    let mut report = Report::default();

    // Wait until the worker answers, so the run frame cannot be dropped before
    // the outbound connection is registered with the relay.
    if sink
        .send(Message::Text(r#"{"kind":"ping"}"#.into()))
        .await
        .is_err()
    {
        let _ = gateway.reports.send(report);
        return;
    }
    let mut pong = false;
    while let Some(text) = next_text(&mut stream).await {
        if kind(&text).as_deref() == Some("pong") {
            pong = true;
            break;
        }
    }
    if !pong {
        let _ = gateway.reports.send(report);
        return;
    }

    // -- run 1: a normal turn -------------------------------------------------
    let trace_one = "trace-dsh-1";
    if !send_frame(&mut sink, &run_frame(trace_one, "say hello")).await {
        let _ = gateway.reports.send(report);
        return;
    }
    let deadline = tokio::time::Instant::now() + TEST_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        let Some(text) = next_text(&mut stream).await else {
            break;
        };
        let Ok(frame) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if frame.get("trace_id").and_then(|value| value.as_str()) != Some(trace_one) {
            continue;
        }
        match frame.get("kind").and_then(|value| value.as_str()) {
            Some("assistant_delta") => {
                report.deltas.push_str(
                    frame
                        .get("delta")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default(),
                );
            }
            Some("tool_call_started") => report.tool_calls += 1,
            Some("tool_call_finished") => report.tool_results += 1,
            Some("agent_log") => report.usage_lines += 1,
            Some("done") => report.turn_ended = true,
            Some("thread_agent_reply") => {
                report.reply = frame
                    .get("content")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
            }
            Some("agent_finished") => {
                report.finished = frame.get("status").map(|status| status.to_string());
            }
            _ => {}
        }
        // The runner settles the trace (`agent_finished`) and only then posts
        // `thread_agent_reply`, so the reply is the last frame to wait for.
        if report.reply.is_some() {
            break;
        }
    }

    // -- run 2: cancelled while the harness is working ------------------------
    let trace_two = "trace-dsh-2";
    if send_frame(&mut sink, &run_frame(trace_two, "CANCEL and wait")).await {
        let mut saw_delta = false;
        let mut cancel_sent = false;
        while let Some(text) = next_text(&mut stream).await {
            let Ok(frame) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            if frame.get("trace_id").and_then(|value| value.as_str()) != Some(trace_two) {
                continue;
            }
            match frame.get("kind").and_then(|value| value.as_str()) {
                Some("assistant_delta") => {
                    saw_delta = true;
                    if !cancel_sent {
                        cancel_sent = send_frame(
                            &mut sink,
                            &DesktopMessage::CancelRun {
                                trace_id: trace_two.to_string(),
                            },
                        )
                        .await;
                    }
                }
                Some("agent_finished") if saw_delta => {
                    report.cancelled = frame
                        .get("status")
                        .and_then(|status| status.as_str())
                        .map(|status| status == "Cancelled")
                        .unwrap_or(false);
                    break;
                }
                _ => {}
            }
        }
    }

    let _ = sink.send(Message::Close(None)).await;
    let _ = gateway.reports.send(report);
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

#[allow(clippy::too_many_arguments)]
fn spawn_worker(
    port: u16,
    dir: &Path,
    script: &Path,
    home: &Path,
    harness: &Path,
    workspace: &Path,
) -> WorkerProcess {
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
        .env("RUST_LOG", "warn")
        .env("GOBLE_GATEWAY_URL", format!("ws://127.0.0.1:{port}"))
        .env("GOBLE_WORKER_TOKEN", TOKEN)
        .env("GOBLE_WORKER_ID", WORKER_ID)
        // The image settings: run the remote DSH harness instead of the native
        // runtime, with the fixture standing in for the real `dsh`.
        .env("GOBLE_HARNESS", "dsh")
        .env("GOBLE_DSH_BIN", gw_bin(script))
        .env("GOBLE_DSH_HARNESS", gw_bin(harness))
        .env("GOBLE_DSH_HOME", gw_bin(home))
        .env("GOBLE_DSH_WORKSPACE", gw_bin(workspace))
        .env("GOBLE_DSH_TIMEOUT_SECONDS", "30")
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

fn gw_bin(path: &Path) -> String {
    path.display().to_string()
}

async fn drain_logs<R>(reader: R)
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;
    let mut lines = tokio::io::BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        eprintln!("worker log: {line}");
    }
}

/// The fake harness: a normal turn for most prompts, a long silent turn for the
/// one that asks to be cancelled.
fn write_fake_dsh(dir: &Path) -> PathBuf {
    let path = dir.join("fake-dsh.sh");
    std::fs::write(
        &path,
        r#"#!/bin/sh
task=$(cat)
case "$task" in
  *CANCEL*)
    printf '%s\n' '{"type":"session","sessionId":"session-cancel"}'
    printf '%s\n' '{"type":"status","phase":"turn_start","turn":1}'
    printf '%s\n' '{"type":"text","text":"working"}'
    sleep 120
    ;;
  *)
    printf '%s\n' '{"type":"session","sessionId":"session-one"}'
    printf '%s\n' '{"type":"status","phase":"turn_start","turn":1}'
    printf '%s\n' '{"type":"text","text":"hello "}'
    printf '%s\n' '{"type":"text","text":"from dsh"}'
    printf '%s\n' '{"type":"tool_call","callId":"call-1","tool":"bash","input":{"command":"ls"}}'
    printf '%s\n' '{"type":"tool_result","callId":"call-1","status":"completed","result":"file.txt"}'
    printf '%s\n' '{"type":"status","phase":"step_end","turn":1,"step":1,"usage":{"inputTokens":10,"outputTokens":5}}'
    printf '%s\n' '{"type":"status","phase":"turn_end","turn":1,"reason":{"kind":"completed"}}'
    printf '%s\n' '{"type":"final","sessionId":"session-one","reason":{"kind":"completed"},"text":"hello from dsh"}'
    exit 0
    ;;
esac
"#,
    )
    .expect("write fake dsh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake dsh");
    }
    path
}

#[tokio::test]
async fn a_remote_dsh_turn_streams_through_the_relay_and_can_be_cancelled() {
    let dir = tempfile::tempdir().expect("temp dir");
    let script = write_fake_dsh(dir.path());
    let home = dir.path().join("dsh-home");
    let harness = dir.path().join("harness.yml");
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(&harness, "# fixture composition\n").unwrap();

    let (report_tx, mut report_rx) = mpsc::unbounded_channel();
    let gateway = std::sync::Arc::new(MockGateway { reports: report_tx });
    let app = Router::new()
        .route("/ws/worker", get(worker_ws))
        .with_state(gateway);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock gateway");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve mock gateway");
    });

    let _worker = spawn_worker(port, dir.path(), &script, &home, &harness, &workspace);

    let report = timeout(TEST_TIMEOUT, report_rx.recv())
        .await
        .expect("the worker completes both turns")
        .expect("the gateway reports");

    assert_eq!(report.deltas, "hello from dsh", "text deltas: {report:?}");
    assert_eq!(report.tool_calls, 1, "tool call frames: {report:?}");
    assert_eq!(report.tool_results, 1, "tool result frames: {report:?}");
    assert!(
        report.usage_lines >= 1,
        "the usage line arrives: {report:?}"
    );
    assert!(
        report.turn_ended,
        "the turn end is a `done` frame: {report:?}"
    );
    assert_eq!(
        report.reply.as_deref(),
        Some("hello from dsh"),
        "the reply is the final text: {report:?}"
    );
    assert_eq!(
        report.finished.as_deref(),
        Some(r#""Success""#),
        "the run settles as a success: {report:?}"
    );
    assert!(
        report.cancelled,
        "the second run ends as cancelled: {report:?}"
    );

    // Credentials are not part of this test's config, so the harness home is
    // left with the generated overlay and sessions dir but no credentials file.
    assert!(!dir.path().join("dsh-home/.credentials.yaml").exists());
}
