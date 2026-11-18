//! End-to-end against the **real** Goble gateway crate (`goble-gateway`,
//! task-40's Phase-1 API), not the axum mock in `tests/e2e.rs`.
//!
//! A fake worker plays the sandbox side: it connects to `/ws/worker`, reads the
//! `run_agent_for_thread_reply` frame the gateway dispatches, and streams back
//! the `WorkerMessage` frames a real worker would. Everything in between — PIN
//! pairing, session creation, run dispatch, SSE with real `index` frames,
//! gateway-side cancel — is the production gateway code.

use std::net::SocketAddr;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use goble_gateway::{hash_pin, Gateway, GatewayConfig, DEFAULT_SESSION_TTL};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

const PIN: &str = "123456";
const WORKER_TOKEN: &str = "acp-e2e-worker-token";

type Ws = WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A real gateway (production router, production state) on an ephemeral port.
struct RealGateway {
    addr: SocketAddr,
    token: String,
    client: reqwest::Client,
    join: tokio::task::JoinHandle<()>,
}

impl Drop for RealGateway {
    fn drop(&mut self) {
        self.join.abort();
    }
}

impl RealGateway {
    async fn start(data_dir: Option<std::path::PathBuf>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind gateway");
        let addr = listener.local_addr().expect("gateway address");
        let mut config = GatewayConfig::new(addr, Some(hash_pin(PIN)), WORKER_TOKEN);
        config.session_ttl = DEFAULT_SESSION_TTL;
        config.data_dir = data_dir;
        let gateway = Gateway::new(config).expect("gateway config is valid");
        let router = gateway.router();
        let join = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("http client");
        let token = pair(&client, &addr).await;
        Self {
            addr,
            token,
            client,
            join,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    async fn get(&self, path: &str) -> Value {
        let response = self
            .client
            .get(self.url(path))
            .bearer_auth(&self.token)
            .send()
            .await
            .expect("gateway GET");
        response.json().await.unwrap_or(Value::Null)
    }

    async fn wait_for_worker(&self, worker_id: &str) {
        for _ in 0..300 {
            let body = self.get("/api/v1/workers").await;
            if body["workers"]
                .as_array()
                .map(|workers| workers.iter().any(|worker| worker["id"] == worker_id))
                .unwrap_or(false)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("worker {worker_id} never registered with the gateway");
    }

    async fn wait_run_status(&self, run_id: &str, expected: &str) -> Value {
        for _ in 0..300 {
            let run = self.get(&format!("/api/v1/runs/{run_id}")).await;
            if run["status"] == expected {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("run {run_id} never reached status {expected}");
    }
}

async fn pair(client: &reqwest::Client, addr: &SocketAddr) -> String {
    let response = client
        .post(format!("http://{addr}/api/v1/session"))
        .json(&json!({"pin": PIN}))
        .send()
        .await
        .expect("pair request");
    assert!(response.status().is_success(), "PIN pairing must succeed");
    let body: Value = response.json().await.expect("pair body");
    body["token"].as_str().expect("pair token").to_string()
}

async fn connect_worker(addr: SocketAddr, worker_id: &str) -> Ws {
    let url = format!("ws://{addr}/ws/worker?id={worker_id}");
    let mut request = url.into_client_request().expect("worker URL");
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {WORKER_TOKEN}")).expect("header value"),
    );
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
        .expect("fake worker connects to /ws/worker")
}

async fn next_worker_frame(worker: &mut Ws) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(10), worker.next())
            .await
            .expect("timed out waiting for a worker frame")
            .expect("worker socket closed")
            .expect("websocket error");
        match message {
            WsMessage::Text(text) => {
                return serde_json::from_str(&text).expect("gateway frames are JSON")
            }
            WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
            other => panic!("expected a text frame, got {other:?}"),
        }
    }
}

async fn send_worker_frame(worker: &mut Ws, frame: Value) {
    worker
        .send(WsMessage::Text(frame.to_string().into()))
        .await
        .expect("send worker frame");
}

// ---------------------------------------------------------------------------
// The real binary on a pipe, driven with ACP frames.
// ---------------------------------------------------------------------------

struct AcpClient {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: tokio::io::Lines<BufReader<ChildStdout>>,
    logs: Arc<Mutex<String>>,
}

impl AcpClient {
    async fn spawn(gateway_url: &str, state_dir: &Path) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_goble-acp"));
        command
            .env("GOBLE_GATEWAY_URL", gateway_url)
            .env("GOBLE_GATEWAY_PIN", PIN)
            .env("GOBLE_ACP_STATE_DIR", state_dir)
            .env("RUST_LOG", "warn")
            .env_remove("GOBLE_GATEWAY_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().expect("goble-acp binary starts");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let stderr = child.stderr.take().expect("stderr");
        let logs = Arc::new(Mutex::new(String::new()));
        let sink = logs.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut sink = sink.lock().unwrap();
                sink.push_str(&line);
                sink.push('\n');
            }
        });
        Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout).lines(),
            logs,
        }
    }

    fn logs(&self) -> String {
        self.logs.lock().unwrap().clone()
    }

    async fn send(&mut self, frame: Value) {
        let mut line = serde_json::to_string(&frame).expect("frame serializes");
        line.push('\n');
        let stdin = self.stdin.as_mut().expect("stdin open");
        stdin.write_all(line.as_bytes()).await.expect("write");
        stdin.flush().await.expect("flush");
    }

    async fn next_frame(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(20), self.stdout.next_line())
            .await
            .unwrap_or_else(|_| panic!("timed out reading a frame; logs:\n{}", self.logs()))
            .expect("stdout readable")
            .expect("stdout open");
        serde_json::from_str(&line).expect("stdout is JSON")
    }

    async fn response(&mut self, id: i64) -> (Value, Vec<Value>) {
        let mut notifications = Vec::new();
        loop {
            let frame = self.next_frame().await;
            if frame["id"].as_i64() == Some(id)
                && (frame.get("result").is_some() || frame.get("error").is_some())
            {
                return (frame, notifications);
            }
            notifications.push(frame);
        }
    }

    async fn request(&mut self, id: i64, method: &str, params: Value) -> (Value, Vec<Value>) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        self.response(id).await
    }

    async fn initialize(&mut self) -> Value {
        let (response, _) = self
            .request(1, "initialize", json!({"protocolVersion": 1}))
            .await;
        response["result"].clone()
    }

    async fn shutdown(mut self) {
        drop(self.stdin.take());
        let _ = tokio::time::timeout(Duration::from_secs(10), self.child.wait()).await;
    }
}

fn updates(notifications: &[Value]) -> Vec<Value> {
    notifications
        .iter()
        .filter(|frame| frame["method"] == "session/update")
        .map(|frame| frame["params"]["update"].clone())
        .collect()
}

#[tokio::test]
async fn acp_turn_runs_through_the_real_gateway_and_real_sse() {
    let gateway = RealGateway::start(None).await;
    let mut worker = connect_worker(gateway.addr, "worker-acp-turn").await;
    gateway.wait_for_worker("worker-acp-turn").await;

    let state = TempDir::new().expect("state dir");
    let mut acp = AcpClient::spawn(&gateway.url(""), state.path()).await;
    acp.initialize().await;

    let (response, _) = acp
        .request(
            2,
            "session/new",
            json!({"cwd": "/tmp/goble-acp-real", "mcpServers": []}),
        )
        .await;
    let acp_session_id = response["result"]["sessionId"]
        .as_str()
        .unwrap_or_else(|| panic!("session/new failed: {response}"))
        .to_string();
    let gateway_session_id = response["result"]["_meta"]["bridge"]["gatewaySessionId"]
        .as_str()
        .unwrap_or_else(|| panic!("missing gateway session id: {response}"))
        .to_string();

    acp.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/prompt",
        "params": {
            "sessionId": acp_session_id,
            "prompt": [{"type": "text", "text": "say hi"}],
        },
    }))
    .await;

    // The production gateway dispatched the worker frame; answer it like a
    // real worker would.
    let frame = next_worker_frame(&mut worker).await;
    assert_eq!(frame["kind"], "run_agent_for_thread_reply");
    assert_eq!(frame["prompt"], "say hi");
    let trace_id = frame["trace_id"].clone();
    let thread_id = frame["thread_id"].clone();
    send_worker_frame(
        &mut worker,
        json!({"kind": "assistant_delta", "trace_id": trace_id, "delta": "hel"}),
    )
    .await;
    send_worker_frame(
        &mut worker,
        json!({"kind": "assistant_delta", "trace_id": trace_id, "delta": "lo"}),
    )
    .await;
    send_worker_frame(
        &mut worker,
        json!({
            "kind": "tool_call_started",
            "trace_id": trace_id,
            "id": "call-real-1",
            "name": "bash",
            "arguments": {"cmd": "ls"},
        }),
    )
    .await;
    send_worker_frame(
        &mut worker,
        json!({
            "kind": "tool_call_finished",
            "trace_id": trace_id,
            "id": "call-real-1",
            "result": "file.txt",
        }),
    )
    .await;
    send_worker_frame(
        &mut worker,
        json!({"kind": "agent_finished", "trace_id": trace_id, "status": "Success"}),
    )
    .await;
    send_worker_frame(
        &mut worker,
        json!({
            "kind": "thread_agent_reply",
            "trace_id": trace_id,
            "thread_id": thread_id,
            "content": "hello",
        }),
    )
    .await;

    let (response, notifications) = acp.response(3).await;
    assert_eq!(
        response["result"],
        json!({"stopReason": "end_turn"}),
        "real gateway turn should end_turn: {response}"
    );
    let updates = updates(&notifications);
    let kinds: Vec<&str> = updates
        .iter()
        .map(|update| update["sessionUpdate"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        vec![
            "agent_message_chunk",
            "agent_message_chunk",
            "tool_call",
            "tool_call_update",
        ],
        "updates: {updates:#?}"
    );
    let streamed_text: String = updates
        .iter()
        .filter(|update| update["sessionUpdate"] == "agent_message_chunk")
        .map(|update| update["content"]["text"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(streamed_text, "hello");
    assert_eq!(updates[2]["toolCallId"], json!("call-real-1"));
    assert_eq!(updates[2]["title"], json!("bash"));
    assert_eq!(updates[3]["status"], json!("completed"));

    // The run really settled in the gateway's store.
    let session = gateway
        .get(&format!("/api/v1/sessions/{gateway_session_id}"))
        .await;
    assert_eq!(session["status"], json!("idle"));
    let run_id = session["last_run_id"]
        .as_str()
        .unwrap_or_else(|| panic!("session has no last_run_id: {session}"));
    let run = gateway.wait_run_status(run_id, "succeeded").await;
    assert_eq!(run["final_text"], json!("hello"));
    acp.shutdown().await;
}

#[tokio::test]
async fn acp_cancel_uses_the_real_gateway_cancel_endpoint() {
    let gateway = RealGateway::start(None).await;
    let mut worker = connect_worker(gateway.addr, "worker-acp-cancel").await;
    gateway.wait_for_worker("worker-acp-cancel").await;

    let state = TempDir::new().expect("state dir");
    let mut acp = AcpClient::spawn(&gateway.url(""), state.path()).await;
    acp.initialize().await;
    let (response, _) = acp
        .request(
            2,
            "session/new",
            json!({"cwd": "/tmp/goble-acp-real", "mcpServers": []}),
        )
        .await;
    let acp_session_id = response["result"]["sessionId"]
        .as_str()
        .expect("session id")
        .to_string();
    let gateway_session_id = response["result"]["_meta"]["bridge"]["gatewaySessionId"]
        .as_str()
        .expect("gateway session id")
        .to_string();

    acp.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/prompt",
        "params": {
            "sessionId": acp_session_id,
            "prompt": [{"type": "text", "text": "hold this turn"}],
        },
    }))
    .await;
    // The run is dispatched and in flight; deliberately never answer it.
    let frame = next_worker_frame(&mut worker).await;
    assert_eq!(frame["kind"], "run_agent_for_thread_reply");
    let run_id = loop {
        let session = gateway
            .get(&format!("/api/v1/sessions/{gateway_session_id}"))
            .await;
        if let Some(run_id) = session["active_run_id"].as_str() {
            break run_id.to_string();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };

    acp.send(json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": {"sessionId": acp_session_id},
    }))
    .await;
    let (response, _) = acp.response(3).await;
    assert_eq!(
        response["result"],
        json!({"stopReason": "cancelled"}),
        "cancel should settle the ACP turn as cancelled: {response}"
    );
    // The real gateway settled the run cancelled.
    let run = gateway.wait_run_status(&run_id, "cancelled").await;
    assert_eq!(run["status"], json!("cancelled"));
    acp.shutdown().await;
}
