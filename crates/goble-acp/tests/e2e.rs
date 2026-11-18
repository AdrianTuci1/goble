// The mock handler error type mirrors axum's `Response`; keeping the
// readable `Result<Response, Response>` shape is worth the clippy lint here.
#![allow(clippy::result_large_err)]

//! End-to-end tests: the real `goble-acp` binary on a pipe, driven with ACP
//! JSON-RPC frames, against an in-process axum mock of the gateway Phase-1 API.
//!
//! The mock implements the contract agreed with the `gateway-api` lane
//! (documented in `docs/gateway-acp.md`): bearer/PIN auth, create-or-reuse
//! sessions, one run per session with 409 while busy, SSE events carrying a
//! global `index`, and a gateway-side cancel. Scenario selection is driven by
//! the prompt text so one mock covers the normal, failing, dropped-stream,
//! poll-fallback, and hold-until-cancel cases.

use std::collections::HashMap;
use std::convert::Infallible;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Notify;

// ---------------------------------------------------------------------------
// Mock gateway
// ---------------------------------------------------------------------------

const MOCK_PIN: &str = "1234";
const MOCK_TOKEN: &str = "mock-bearer-token";
const MOCK_WORKER: &str = "worker-1";

#[derive(Clone)]
struct MockGateway {
    inner: Arc<Mutex<MockInner>>,
}

struct MockInner {
    sessions: HashMap<String, MockSession>,
    sessions_by_name: HashMap<String, String>,
    runs: HashMap<String, MockRun>,
    next_session: u64,
    next_run: u64,
    pair_calls: u64,
    reused_sessions: u64,
    stream_from_indexes: Vec<u64>,
    cancel_calls: u64,
}

struct MockSession {
    id: String,
    name: Option<String>,
    worker_id: String,
    thread_id: String,
    status: String,
    active_run: Option<String>,
}

struct MockRun {
    id: String,
    session_id: String,
    task: String,
    status: String,
    final_text: Option<String>,
    error: Option<String>,
    events: Vec<Value>,
    cancel: Arc<Notify>,
}

/// Which scripted scenario a run follows, selected from the prompt text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunMode {
    Normal,
    Fail,
    Drop,
    Poll,
    Hold,
}

impl RunMode {
    fn from_task(task: &str) -> Self {
        if task.contains("hold") {
            Self::Hold
        } else if task.contains("drop") {
            Self::Drop
        } else if task.contains("fail") {
            Self::Fail
        } else if task.contains("poll") {
            Self::Poll
        } else {
            Self::Normal
        }
    }
}

impl MockGateway {
    fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(MockInner {
                sessions: HashMap::new(),
                sessions_by_name: HashMap::new(),
                runs: HashMap::new(),
                next_session: 0,
                next_run: 0,
                pair_calls: 0,
                reused_sessions: 0,
                stream_from_indexes: Vec::new(),
                cancel_calls: 0,
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, MockInner> {
        self.inner.lock().unwrap()
    }

    fn session_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .lock()
            .sessions
            .values()
            .filter_map(|s| s.name.clone())
            .collect();
        names.sort();
        names
    }

    fn run_tasks(&self) -> Vec<String> {
        let mut tasks: Vec<String> = self.lock().runs.values().map(|r| r.task.clone()).collect();
        tasks.sort();
        tasks
    }

    fn stream_from_indexes(&self) -> Vec<u64> {
        self.lock().stream_from_indexes.clone()
    }

    fn pair_calls(&self) -> u64 {
        self.lock().pair_calls
    }

    fn cancel_calls(&self) -> u64 {
        self.lock().cancel_calls
    }

    fn session_count(&self) -> usize {
        self.lock().sessions.len()
    }
}

fn gateway_event(index: u64, value: Value) -> Value {
    let mut object = value.as_object().cloned().unwrap_or_default();
    object.insert("index".to_string(), json!(index));
    object.insert("at".to_string(), json!("2026-01-01T00:00:00Z"));
    Value::Object(object)
}

fn status_event(index: u64, status: &str) -> Value {
    gateway_event(index, json!({"type": "status", "status": status}))
}

fn text_event(index: u64, text: &str) -> Value {
    gateway_event(index, json!({"type": "text", "text": text}))
}

fn tool_call_event(index: u64, call_id: &str, tool: &str, input: Value) -> Value {
    gateway_event(
        index,
        json!({"type": "tool_call", "callId": call_id, "tool": tool, "input": input}),
    )
}

fn tool_result_event(index: u64, call_id: &str, tool: &str, status: &str, result: &str) -> Value {
    gateway_event(
        index,
        json!({
            "type": "tool_result",
            "callId": call_id,
            "tool": tool,
            "status": status,
            "result": result,
        }),
    )
}

fn final_event(index: u64, reason: Value, text: Option<&str>) -> Value {
    let mut value = json!({"type": "final", "reason": reason});
    if let Some(text) = text {
        value["text"] = json!(text);
    }
    gateway_event(index, value)
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": "unauthorized", "message": "invalid or missing bearer token"})),
    )
        .into_response()
}

fn authorized(headers: &HeaderMap) -> bool {
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value == format!("Bearer {MOCK_TOKEN}"))
        .unwrap_or(false)
}

fn body_error(code: &str, message: &str, status: StatusCode) -> Response {
    (status, Json(json!({"error": code, "message": message}))).into_response()
}

type MockResult = Result<Response, Response>;

fn run_json(run: &MockRun) -> Value {
    json!({
        "id": run.id,
        "session_id": run.session_id,
        "worker_id": MOCK_WORKER,
        "thread_id": "thread-1",
        "trace_id": run.id,
        "task": run.task,
        "status": run.status,
        "created_at": "2026-01-01T00:00:00Z",
        "started_at": "2026-01-01T00:00:01Z",
        "finished_at": null,
        "final_text": run.final_text,
        "error": run.error,
        "usage": null,
        "events_count": run.events.len(),
    })
}

fn session_json(session: &MockSession, reused: bool) -> Value {
    json!({
        "id": session.id,
        "name": session.name,
        "worker_id": session.worker_id,
        "worker_connected": true,
        "thread_id": session.thread_id,
        "agent_id": "agent-1",
        "status": session.status,
        "active_run_id": session.active_run,
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "run_count": 0,
        "last_run_id": null,
        "reused": reused,
    })
}

async fn pair(State(gateway): State<MockGateway>, Json(body): Json<Value>) -> Response {
    let pin = body.get("pin").and_then(Value::as_str).unwrap_or_default();
    gateway.lock().pair_calls += 1;
    if pin == MOCK_PIN {
        Json(json!({"token": MOCK_TOKEN, "expires_in": 3600})).into_response()
    } else {
        unauthorized()
    }
}

async fn create_session(
    State(gateway): State<MockGateway>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> MockResult {
    if !authorized(&headers) {
        return Err(unauthorized());
    }
    let name = body.get("name").and_then(Value::as_str).map(str::to_string);
    let mut inner = gateway.lock();
    if let Some(name) = name.as_ref() {
        if let Some(existing) = inner.sessions_by_name.get(name) {
            if let Some(session) = inner.sessions.get(existing) {
                let value = session_json(session, true);
                inner.reused_sessions += 1;
                return Ok((StatusCode::OK, Json(value)).into_response());
            }
        }
    }
    let number = inner.next_session;
    inner.next_session += 1;
    let id = format!("ses_{number}");
    let session = MockSession {
        id: id.clone(),
        name: name.clone(),
        worker_id: body
            .get("worker")
            .and_then(Value::as_str)
            .unwrap_or(MOCK_WORKER)
            .to_string(),
        thread_id: format!("thr_{number}"),
        status: "idle".to_string(),
        active_run: None,
    };
    let value = session_json(&session, false);
    if let Some(name) = name {
        inner.sessions_by_name.insert(name, id.clone());
    }
    inner.sessions.insert(id, session);
    Ok((StatusCode::CREATED, Json(value)).into_response())
}

async fn get_session(
    State(gateway): State<MockGateway>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> MockResult {
    if !authorized(&headers) {
        return Err(unauthorized());
    }
    let inner = gateway.lock();
    match inner.sessions.get(&session_id) {
        Some(session) => Ok(Json(session_json(session, false)).into_response()),
        None => Err(body_error(
            "unknown_session",
            "session not found",
            StatusCode::NOT_FOUND,
        )),
    }
}

fn scripted_run(id: String, session_id: String, task: String, mode: RunMode) -> MockRun {
    let cancel = Arc::new(Notify::new());
    let (status, final_text, error, events) = match mode {
        RunMode::Normal => (
            "succeeded".to_string(),
            Some("Hello world".to_string()),
            None,
            vec![
                status_event(0, "running"),
                text_event(1, "Hello"),
                tool_call_event(2, "call_1", "bash", json!({"command": "echo hi"})),
                tool_result_event(3, "call_1", "bash", "completed", "hi\n"),
                text_event(4, " world"),
                final_event(5, json!({"kind": "completed"}), Some("Hello world")),
            ],
        ),
        RunMode::Fail => (
            "failed".to_string(),
            None,
            Some("boom".to_string()),
            vec![
                status_event(0, "running"),
                text_event(1, "oops"),
                final_event(
                    2,
                    json!({"kind": "error", "error": {"message": "boom"}}),
                    None,
                ),
            ],
        ),
        RunMode::Drop => (
            "succeeded".to_string(),
            Some("part one part two".to_string()),
            None,
            vec![
                status_event(0, "running"),
                text_event(1, "part one"),
                text_event(2, " part two"),
                final_event(3, json!({"kind": "completed"}), None),
            ],
        ),
        RunMode::Poll => (
            "succeeded".to_string(),
            Some("polled".to_string()),
            None,
            vec![status_event(0, "running"), text_event(1, "polled")],
        ),
        RunMode::Hold => (
            "running".to_string(),
            None,
            None,
            vec![status_event(0, "running"), text_event(1, "holding")],
        ),
    };
    MockRun {
        id,
        session_id,
        task,
        status,
        final_text,
        error,
        events,
        cancel,
    }
}

async fn create_run(
    State(gateway): State<MockGateway>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
    Json(body): Json<Value>,
) -> MockResult {
    if !authorized(&headers) {
        return Err(unauthorized());
    }
    let task = body
        .get("task")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if task.is_empty() {
        return Err(body_error(
            "invalid_task",
            "task is required",
            StatusCode::BAD_REQUEST,
        ));
    }
    let mode = RunMode::from_task(&task);
    let mut inner = gateway.lock();
    if !inner.sessions.contains_key(&session_id) {
        return Err(body_error(
            "unknown_session",
            "session not found",
            StatusCode::NOT_FOUND,
        ));
    }
    // `busy` in the task models the gateway's one-run-per-session 409.
    if task.contains("busy") {
        return Err(body_error(
            "session_busy",
            "session already has a running run",
            StatusCode::CONFLICT,
        ));
    }
    if let Some(session) = inner.sessions.get(&session_id) {
        if session.active_run.is_some() {
            return Err(body_error(
                "session_busy",
                "session already has a running run",
                StatusCode::CONFLICT,
            ));
        }
    }
    let number = inner.next_run;
    inner.next_run += 1;
    let id = format!("run_{number}");
    let run = scripted_run(id.clone(), session_id.clone(), task, mode);
    let value = run_json(&run);
    if let Some(session) = inner.sessions.get_mut(&session_id) {
        session.status = "running".to_string();
        session.active_run = Some(id.clone());
    }
    inner.runs.insert(id, run);
    Ok((StatusCode::ACCEPTED, Json(value)).into_response())
}

async fn get_run(
    State(gateway): State<MockGateway>,
    headers: HeaderMap,
    AxumPath(run_id): AxumPath<String>,
) -> MockResult {
    if !authorized(&headers) {
        return Err(unauthorized());
    }
    let inner = gateway.lock();
    match inner.runs.get(&run_id) {
        Some(run) => Ok(Json(run_json(run)).into_response()),
        None => Err(body_error(
            "unknown_run",
            "run not found",
            StatusCode::NOT_FOUND,
        )),
    }
}

async fn cancel_session(
    State(gateway): State<MockGateway>,
    headers: HeaderMap,
    AxumPath(session_id): AxumPath<String>,
) -> MockResult {
    if !authorized(&headers) {
        return Err(unauthorized());
    }
    let mut inner = gateway.lock();
    inner.cancel_calls += 1;
    if !inner.sessions.contains_key(&session_id) {
        return Err(body_error(
            "unknown_session",
            "session not found",
            StatusCode::NOT_FOUND,
        ));
    }
    let run_id = inner
        .sessions
        .get(&session_id)
        .and_then(|session| session.active_run.clone());
    if let Some(run_id) = run_id.as_ref() {
        if let Some(run) = inner.runs.get_mut(run_id) {
            run.status = "cancelled".to_string();
            run.error = Some("cancelled by client".to_string());
            run.cancel.notify_waiters();
        }
        if let Some(session) = inner.sessions.get_mut(&session_id) {
            session.status = "idle".to_string();
            session.active_run = None;
        }
    }
    Ok(Json(json!({"ok": true, "run_id": run_id})).into_response())
}

async fn stream_run(
    State(gateway): State<MockGateway>,
    headers: HeaderMap,
    AxumPath(run_id): AxumPath<String>,
    Query(query): Query<HashMap<String, String>>,
) -> MockResult {
    if !authorized(&headers) {
        return Err(unauthorized());
    }
    let from_index = query
        .get("from_index")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let (events, mode, cancel) = {
        let mut inner = gateway.lock();
        inner.stream_from_indexes.push(from_index);
        let run = match inner.runs.get(&run_id) {
            Some(run) => run,
            None => {
                return Err(body_error(
                    "unknown_run",
                    "run not found",
                    StatusCode::NOT_FOUND,
                ))
            }
        };
        let mode = RunMode::from_task(&run.task);
        let events: Vec<Value> = run
            .events
            .iter()
            .filter(|event| event["index"].as_u64().unwrap_or(0) >= from_index)
            .cloned()
            .collect();
        (events, mode, run.cancel.clone())
    };

    if mode == RunMode::Hold && from_index == 0 {
        // Send the replayed events, then hold the connection until the gateway
        // cancel endpoint fires, and close with the cancelled terminal frame.
        let queue = events;
        let held = stream::unfold(
            (queue, cancel, false),
            |(mut queue, cancel, sent_final)| async move {
                if let Some(event) = queue.first().cloned() {
                    queue.remove(0);
                    return Some((
                        Ok::<Event, Infallible>(Event::default().data(event.to_string())),
                        (queue, cancel, sent_final),
                    ));
                }
                if sent_final {
                    return None;
                }
                // Wait for the cancel; the test sends it quickly.
                let _ = tokio::time::timeout(Duration::from_secs(20), cancel.notified()).await;
                let terminal = final_event(2, json!({"kind": "cancelled"}), None);
                Some((
                    Ok::<Event, Infallible>(Event::default().data(terminal.to_string())),
                    (Vec::new(), cancel, true),
                ))
            },
        );
        return Ok(Sse::new(held).into_response());
    }

    // A dropped connection is modelled as EOF before the terminal frame: the
    // first stream (from_index 0) stops after the first two events.
    let events = if mode == RunMode::Drop && from_index == 0 {
        events.into_iter().take(2).collect()
    } else {
        events
    };
    let stream = stream::iter(
        events
            .into_iter()
            .map(|event| Ok::<Event, Infallible>(Event::default().data(event.to_string()))),
    );
    Ok(Sse::new(stream).into_response())
}

async fn start_mock_gateway() -> (String, MockGateway) {
    let gateway = MockGateway::new();
    let app = Router::new()
        .route("/health", get(|| async { Json(json!({"ok": true})) }))
        .route("/api/v1/session", post(pair))
        .route("/api/v1/sessions", post(create_session))
        .route("/api/v1/sessions/{id}", get(get_session))
        .route("/api/v1/sessions/{id}/runs", post(create_run))
        .route("/api/v1/sessions/{id}/cancel", post(cancel_session))
        .route("/api/v1/runs/{id}", get(get_run))
        .route("/api/v1/runs/{id}/stream", get(stream_run))
        .with_state(gateway.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock gateway binds an ephemeral port");
    let address = listener.local_addr().expect("local address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{address}"), gateway)
}

// ---------------------------------------------------------------------------
// ACP client driving the real binary over stdio
// ---------------------------------------------------------------------------

struct AcpClient {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: tokio::io::Lines<BufReader<ChildStdout>>,
    logs: Arc<Mutex<String>>,
}

impl AcpClient {
    async fn spawn(gateway_url: &str, state_dir: &Path, extra_env: &[(&str, &str)]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_goble-acp"));
        command
            .env("GOBLE_GATEWAY_URL", gateway_url)
            .env("GOBLE_GATEWAY_PIN", MOCK_PIN)
            .env("GOBLE_ACP_STATE_DIR", state_dir)
            .env("RUST_LOG", "warn")
            .env_remove("GOBLE_GATEWAY_TOKEN")
            .env_remove("GOBLE_ACP_SESSION_NAME")
            .env_remove("GOBLE_ACP_WORKER")
            .env_remove("GOBLE_ACP_STRICT_SETUP")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (key, value) in extra_env {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("goble-acp binary starts");
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
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
        self.send_raw(&line).await;
    }

    async fn send_raw(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().expect("stdin still open");
        stdin.write_all(line.as_bytes()).await.expect("write frame");
        stdin.flush().await.expect("flush frame");
    }

    async fn next_frame(&mut self) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(20), self.stdout.next_line())
            .await
            .unwrap_or_else(|_| {
                panic!("timed out waiting for an ACP frame; logs:\n{}", self.logs())
            })
            .expect("stdout readable")
            .expect("stdout still open");
        serde_json::from_str(&line).unwrap_or_else(|error| {
            panic!(
                "invalid JSON on stdout: {error}; line={line:?}; logs:\n{}",
                self.logs()
            )
        })
    }

    /// Read frames until the response with `id` arrives, returning it plus every
    /// notification seen in between.
    async fn response(&mut self, id: i64) -> (Value, Vec<Value>) {
        let mut notifications = Vec::new();
        loop {
            let frame = self.next_frame().await;
            let is_response = frame["id"].as_i64() == Some(id)
                && (frame.get("result").is_some() || frame.get("error").is_some());
            if is_response {
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
            .request(
                1,
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientCapabilities": {
                        "fs": {"readTextFile": false, "writeTextFile": false},
                        "terminal": false,
                    },
                    "clientInfo": {"name": "goble-acp-e2e", "version": "0.0.0"},
                }),
            )
            .await;
        response["result"].clone()
    }

    async fn new_session(&mut self, id: i64) -> String {
        let (response, _) = self
            .request(
                id,
                "session/new",
                json!({"cwd": "/tmp/e2e-workspace", "mcpServers": []}),
            )
            .await;
        response["result"]["sessionId"]
            .as_str()
            .unwrap_or_else(|| panic!("session/new failed: {response}"))
            .to_string()
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn initialize_advertises_only_what_the_bridge_serves() {
    let (url, gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[]).await;

    let result = client.initialize().await;
    assert_eq!(result["protocolVersion"], json!(1));
    assert_eq!(result["agentInfo"]["name"], json!("goble-acp"));
    assert_eq!(
        result["agentCapabilities"]["loadSession"],
        json!(false),
        "loadSession must be false: no history replay"
    );
    assert_eq!(
        result["agentCapabilities"]["promptCapabilities"],
        json!({"text": true, "image": false, "audio": false, "embeddedContext": true})
    );
    assert_eq!(result["authMethods"], json!([]));
    // initialize is answered locally; it must not create gateway traffic.
    assert_eq!(gateway.pair_calls(), 0);
    assert_eq!(gateway.session_count(), 0);
    client.shutdown().await;
}

#[tokio::test]
async fn session_new_creates_gateway_session_and_returns_an_acp_id() {
    let (url, gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[]).await;
    client.initialize().await;

    let (response, _) = client
        .request(
            2,
            "session/new",
            json!({"cwd": "/tmp/e2e-workspace", "mcpServers": []}),
        )
        .await;
    let session_id = response["result"]["sessionId"]
        .as_str()
        .unwrap_or_else(|| panic!("session/new failed: {response}"));
    assert!(!session_id.is_empty());
    assert_eq!(response["result"]["configOptions"], json!([]));

    // The gateway session name follows the reference scheme and the PIN was
    // paired into a bearer token.
    let names = gateway.session_names();
    assert_eq!(names.len(), 1);
    assert!(
        names[0].starts_with("acp/e2e-workspace/"),
        "unexpected gateway session name: {}",
        names[0]
    );
    assert_eq!(gateway.pair_calls(), 1);
    client.shutdown().await;
}

#[tokio::test]
async fn prompt_streams_updates_then_settles_end_turn() {
    let (url, gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[]).await;
    client.initialize().await;
    let session_id = client.new_session(2).await;

    let (response, notifications) = client
        .request(
            3,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": "say hello"}],
            }),
        )
        .await;
    assert_eq!(
        response["result"],
        json!({"stopReason": "end_turn"}),
        "prompt should end the turn: {response}"
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
            "tool_call",
            "tool_call_update",
            "agent_message_chunk",
        ],
        "unexpected update sequence: {updates:#?}"
    );
    assert_eq!(updates[0]["content"]["text"], json!("Hello"));
    assert_eq!(updates[1]["title"], json!("bash"));
    assert_eq!(updates[1]["kind"], json!("other"));
    assert_eq!(updates[1]["status"], json!("in_progress"));
    assert_eq!(updates[1]["rawInput"], json!({"command": "echo hi"}));
    assert_eq!(updates[2]["toolCallId"], json!("call_1"));
    assert_eq!(updates[2]["status"], json!("completed"));
    assert_eq!(
        updates[2]["content"],
        json!([{"type": "content", "content": {"type": "text", "text": "hi\n"}}])
    );
    assert_eq!(updates[3]["content"]["text"], json!(" world"));
    // The final.text must not be replayed as a fifth chunk.
    assert_eq!(updates.len(), 4);
    // The prompt text reached the gateway as the run task.
    assert_eq!(gateway.run_tasks(), vec!["say hello".to_string()]);
    client.shutdown().await;
}

#[tokio::test]
async fn gateway_busy_409_becomes_jsonrpc_server_error() {
    let (url, _gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[]).await;
    client.initialize().await;
    let session_id = client.new_session(2).await;

    // The mock answers 409 for a task containing "busy", like a session that
    // already has a running run.
    let (response, _) = client
        .request(
            3,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": "busy please"}],
            }),
        )
        .await;
    assert_eq!(response["error"]["code"], json!(-32000), "{response}");
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("session busy"),
        "{response}"
    );
    client.shutdown().await;
}

#[tokio::test]
async fn failed_run_is_a_jsonrpc_error_not_a_silent_end_turn() {
    let (url, _gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[]).await;
    client.initialize().await;
    let session_id = client.new_session(2).await;

    let (response, notifications) = client
        .request(
            3,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": "fail now"}],
            }),
        )
        .await;
    assert_eq!(response["error"]["code"], json!(-32603), "{response}");
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("turn failed: boom"),
        "{response}"
    );
    // The partial text still streamed before the error.
    let updates = updates(&notifications);
    assert_eq!(updates[0]["content"]["text"], json!("oops"));
    client.shutdown().await;
}

#[tokio::test]
async fn session_cancel_settles_the_turn_as_cancelled() {
    let (url, gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[]).await;
    client.initialize().await;
    let session_id = client.new_session(2).await;

    client
        .send(json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "session/prompt",
            "params": {
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": "hold the turn"}],
            },
        }))
        .await;

    // Wait for the first streamed update, then cancel while the turn is live.
    let mut saw_update = false;
    while !saw_update {
        let frame = client.next_frame().await;
        if frame["method"] == "session/update" {
            saw_update = true;
        }
    }
    client
        .send(json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {"sessionId": session_id},
        }))
        .await;

    let (response, _) = client.response(3).await;
    assert_eq!(
        response["result"],
        json!({"stopReason": "cancelled"}),
        "cancel should settle the prompt as cancelled: {response}"
    );
    let mut cancel_calls = gateway.cancel_calls();
    for _ in 0..50 {
        if cancel_calls >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel_calls = gateway.cancel_calls();
    }
    assert!(
        cancel_calls >= 1,
        "the bridge must ask the gateway to cancel the run"
    );
    client.shutdown().await;
}

#[tokio::test]
async fn dropped_stream_reconnects_from_the_last_index() {
    let (url, gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[]).await;
    client.initialize().await;
    let session_id = client.new_session(2).await;

    let (response, notifications) = client
        .request(
            3,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": "drop the stream"}],
            }),
        )
        .await;
    assert_eq!(
        response["result"],
        json!({"stopReason": "end_turn"}),
        "{response}"
    );
    let updates = updates(&notifications);
    let text: String = updates
        .iter()
        .filter(|update| update["sessionUpdate"] == "agent_message_chunk")
        .map(|update| update["content"]["text"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(text, "part one part two", "each delta must arrive once");
    // The reconnect resumed at the first unconsumed global index (2), not 0.
    let indexes = gateway.stream_from_indexes();
    assert_eq!(indexes, vec![0, 2], "reconnect cursor: {indexes:?}");
    client.shutdown().await;
}

#[tokio::test]
async fn stream_without_final_settles_from_the_run_record() {
    let (url, _gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[]).await;
    client.initialize().await;
    let session_id = client.new_session(2).await;

    let (response, notifications) = client
        .request(
            3,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": "poll the run"}],
            }),
        )
        .await;
    assert_eq!(
        response["result"],
        json!({"stopReason": "end_turn"}),
        "{response}"
    );
    let updates = updates(&notifications);
    assert_eq!(updates[0]["content"]["text"], json!("polled"));
    client.shutdown().await;
}

#[tokio::test]
async fn transport_error_is_reported_as_a_jsonrpc_error() {
    // Port 1 is not listening: the PIN pairing itself cannot reach a gateway.
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn("http://127.0.0.1:1", state.path(), &[]).await;
    let (response, _) = client
        .request(
            1,
            "session/new",
            json!({"cwd": "/tmp/e2e-workspace", "mcpServers": []}),
        )
        .await;
    assert_eq!(response["error"]["code"], json!(-32603), "{response}");
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("gateway unreachable"),
        "{response}"
    );
    client.shutdown().await;
}

#[tokio::test]
async fn prompt_for_an_unknown_session_is_rejected_with_32002() {
    let (url, _gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[]).await;
    client.initialize().await;

    let (response, _) = client
        .request(
            2,
            "session/prompt",
            json!({"sessionId": "does-not-exist", "prompt": [{"type": "text", "text": "hi"}]}),
        )
        .await;
    assert_eq!(response["error"]["code"], json!(-32002), "{response}");
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unknown session"),
        "{response}"
    );
    client.shutdown().await;
}

#[tokio::test]
async fn filesystem_and_terminal_methods_are_refused() {
    let (url, _gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[]).await;
    client.initialize().await;

    for (id, method) in [
        (2, "fs/read_text_file"),
        (3, "fs/write_text_file"),
        (4, "terminal/create"),
        (5, "terminal/output"),
    ] {
        let (response, _) = client
            .request(id, method, json!({"sessionId": "x", "path": "/etc/passwd"}))
            .await;
        assert_eq!(
            response["error"]["code"],
            json!(-32601),
            "{method}: {response}"
        );
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains("sandbox"),
            "{method}: {response}"
        );
    }
    client.shutdown().await;
}

#[tokio::test]
async fn session_load_is_refused_like_dsh() {
    let (url, _gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[]).await;
    client.initialize().await;

    let (response, _) = client
        .request(
            2,
            "session/load",
            json!({"sessionId": "x", "cwd": "/tmp", "mcpServers": []}),
        )
        .await;
    assert_eq!(response["error"]["code"], json!(-32601), "{response}");
    assert_eq!(response["error"]["data"], json!({"method": "session/load"}));
    client.shutdown().await;
}

#[tokio::test]
async fn malformed_and_invalid_frames_get_jsonrpc_errors() {
    let (url, _gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[]).await;

    client.send_raw("this is not json\n").await;
    let frame = client.next_frame().await;
    assert_eq!(frame["error"]["code"], json!(-32700), "{frame}");
    assert_eq!(frame["id"], json!(null));

    client.send(json!({"jsonrpc": "2.0", "id": 9})).await;
    let frame = client.next_frame().await;
    assert_eq!(frame["error"]["code"], json!(-32600), "{frame}");
    client.shutdown().await;
}

#[tokio::test]
async fn strict_setup_refuses_mcp_servers_instead_of_ignoring_them() {
    let (url, _gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[("GOBLE_ACP_STRICT_SETUP", "1")]).await;
    client.initialize().await;

    let (response, _) = client
        .request(
            2,
            "session/new",
            json!({
                "cwd": "/tmp/e2e-workspace",
                "mcpServers": [{"name": "t3-code", "command": "t3", "args": []}],
            }),
        )
        .await;
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("cannot mount"),
        "{response}"
    );
    client.shutdown().await;
}

#[tokio::test]
async fn sessions_persist_across_restart_and_resume_reattaches() {
    let (url, gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut first = AcpClient::spawn(&url, state.path(), &[]).await;
    first.initialize().await;
    let session_id = first.new_session(2).await;
    first.shutdown().await;

    // A fresh bridge process loads sessions.json and re-attaches by name.
    let mut second = AcpClient::spawn(&url, state.path(), &[]).await;
    second.initialize().await;
    let (response, _) = second
        .request(2, "session/resume", json!({"sessionId": session_id}))
        .await;
    assert_eq!(
        response["result"],
        json!({"configOptions": []}),
        "resume should re-attach: {response}"
    );

    // The resumed session can still drive a prompt.
    let (response, _) = second
        .request(
            3,
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": "after restart"}],
            }),
        )
        .await;
    assert_eq!(
        response["result"],
        json!({"stopReason": "end_turn"}),
        "{response}"
    );
    // Same gateway session (created-or-reused by name), not a second one.
    assert_eq!(gateway.session_count(), 1);
    second.shutdown().await;
}

#[tokio::test]
async fn session_close_forgets_the_acp_session() {
    let (url, _gateway) = start_mock_gateway().await;
    let state = TempDir::new().expect("state dir");
    let mut client = AcpClient::spawn(&url, state.path(), &[]).await;
    client.initialize().await;
    let session_id = client.new_session(2).await;

    let (response, _) = client
        .request(3, "session/close", json!({"sessionId": session_id}))
        .await;
    assert_eq!(response["result"], json!({}));

    let (response, _) = client
        .request(
            4,
            "session/prompt",
            json!({"sessionId": session_id, "prompt": [{"type": "text", "text": "hi"}]}),
        )
        .await;
    assert_eq!(response["error"]["code"], json!(-32002), "{response}");
    client.shutdown().await;
}
