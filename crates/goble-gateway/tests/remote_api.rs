//! Integration tests for the gateway's remote-execution control plane.
//!
//! A fake worker plays the sandbox side: it connects to `/ws/worker`, reads the
//! `run_agent_for_thread_reply` frame the gateway dispatches, and sends the
//! `WorkerMessage` frames a real worker would. Everything else is real HTTP and
//! real SSE over the gateway's router.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use goble_gateway::{hash_pin, Gateway, GatewayConfig, DEFAULT_SESSION_TTL};
use reqwest::header::{CONTENT_TYPE, COOKIE};
use reqwest::StatusCode;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

const PIN: &str = "123456";
const WORKER_TOKEN: &str = "test-worker-token";

type Ws = WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct TestServer {
    addr: SocketAddr,
    client: reqwest::Client,
    token: String,
    join: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.join.abort();
    }
}

async fn start(data_dir: Option<PathBuf>) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut config = GatewayConfig::new(addr, Some(hash_pin(PIN)), WORKER_TOKEN);
    config.session_ttl = DEFAULT_SESSION_TTL;
    config.data_dir = data_dir;

    let gateway = Gateway::new(config).unwrap();
    let router = gateway.router();
    let join = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let token = pair(&addr, &client).await;
    TestServer {
        addr,
        client,
        token,
        join,
    }
}

async fn pair(addr: &SocketAddr, client: &reqwest::Client) -> String {
    let response = client
        .post(format!("http://{addr}/api/v1/session"))
        .json(&json!({ "pin": PIN }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let body: Value = response.json().await.unwrap();
    body["token"].as_str().unwrap().to_string()
}

fn url(server: &TestServer, path: &str) -> String {
    format!("http://{}{path}", server.addr)
}

async fn authed_get(server: &TestServer, path: &str) -> (StatusCode, Value) {
    authed_get_with(&server.client, &server.addr, &server.token, path).await
}

async fn authed_get_with(
    client: &reqwest::Client,
    addr: &SocketAddr,
    token: &str,
    path: &str,
) -> (StatusCode, Value) {
    let response = client
        .get(format!("http://{addr}{path}"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn authed_post(server: &TestServer, path: &str, body: Value) -> (StatusCode, Value) {
    authed_post_with(&server.client, &server.addr, &server.token, path, body).await
}

async fn authed_post_with(
    client: &reqwest::Client,
    addr: &SocketAddr,
    token: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = client
        .post(format!("http://{addr}{path}"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn connect_worker(server: &TestServer, worker_id: &str) -> Ws {
    let url = format!("ws://{}/ws/worker?id={worker_id}", server.addr);
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {WORKER_TOKEN}")).unwrap(),
    );
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
        .expect("fake worker should connect")
}

async fn next_frame(worker: &mut Ws) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(3), worker.next())
            .await
            .expect("timed out waiting for a worker frame")
            .expect("worker socket closed")
            .expect("websocket error");
        match message {
            WsMessage::Text(text) => {
                return serde_json::from_str(&text).expect("gateway frames are JSON");
            }
            WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
            other => panic!("expected a text frame, got {other:?}"),
        }
    }
}

async fn send_frame(worker: &mut Ws, frame: Value) {
    worker
        .send(WsMessage::Text(frame.to_string().into()))
        .await
        .unwrap();
}

async fn wait_for_worker(server: &TestServer, worker_id: &str) {
    for _ in 0..200 {
        let (_, body) = authed_get(server, "/api/v1/workers").await;
        if body["workers"]
            .as_array()
            .map(|workers| workers.iter().any(|worker| worker["id"] == worker_id))
            .unwrap_or(false)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("worker {worker_id} never showed up");
}

async fn create_session(server: &TestServer, body: Value) -> Value {
    let (status, session) = authed_post(server, "/api/v1/sessions", body).await;
    assert!(
        status == StatusCode::CREATED || status == StatusCode::OK,
        "unexpected session status {status}: {session}"
    );
    session
}

async fn dispatch(server: &TestServer, session_id: &str, body: Value) -> (StatusCode, Value) {
    authed_post(server, &format!("/api/v1/sessions/{session_id}/runs"), body).await
}

async fn run_status(server: &TestServer, run_id: &str) -> Value {
    let (status, run) = authed_get(server, &format!("/api/v1/runs/{run_id}")).await;
    assert_eq!(status, StatusCode::OK, "GET run failed: {run}");
    run
}

async fn wait_run(server: &TestServer, run_id: &str, expected: &str) -> Value {
    for _ in 0..600 {
        let run = run_status(server, run_id).await;
        if run["status"] == expected {
            return run;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!(
        "run {run_id} never reached {expected}; last: {}",
        run_status(server, run_id).await
    );
}

async fn wait_next_index(server: &TestServer, run_id: &str, at_least: u64) {
    for _ in 0..400 {
        let (_, events) = authed_get(server, &format!("/api/v1/runs/{run_id}/events")).await;
        if events["next_index"].as_u64().unwrap_or(0) >= at_least {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("run {run_id} never reached {at_least} events");
}

/// A raw HTTP/1.1 GET that leaves the SSE body on the socket so the test can
/// watch it arrive, rather than reading a buffered body.
async fn open_stream(server: &TestServer, target: &str, token: &str) -> tokio::net::TcpStream {
    let mut stream = tokio::net::TcpStream::connect(server.addr).await.unwrap();
    let request = format!(
        "GET {target} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {token}\r\n\
         Accept: text/event-stream\r\nConnection: keep-alive\r\n\r\n",
        server.addr
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    stream
}

async fn read_until(stream: &mut tokio::net::TcpStream, buffer: &mut String, needle: &str) {
    let mut chunk = [0u8; 4096];
    for _ in 0..200 {
        if buffer.contains(needle) {
            return;
        }
        let read = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut chunk))
            .await
            .expect("timed out waiting for the stream")
            .expect("stream read failed");
        if read == 0 {
            return;
        }
        buffer.push_str(&String::from_utf8_lossy(&chunk[..read]));
    }
}

/// Read until the server closes the SSE stream (or a short quiet period).
async fn read_to_end(stream: &mut tokio::net::TcpStream, buffer: &mut String) {
    let mut chunk = [0u8; 4096];
    for _ in 0..200 {
        match tokio::time::timeout(Duration::from_secs(2), stream.read(&mut chunk)).await {
            Ok(Ok(0)) | Err(_) => return,
            Ok(Ok(read)) => buffer.push_str(&String::from_utf8_lossy(&chunk[..read])),
            Ok(Err(_)) => return,
        }
    }
}

/// Parse the `data: {...}` frames out of a raw SSE response body.
fn sse_events(raw: &str) -> Vec<Value> {
    raw.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|payload| serde_json::from_str::<Value>(payload.trim()).ok())
        .collect()
}

#[tokio::test]
async fn every_control_plane_route_requires_a_session() {
    let server = start(None).await;

    // /health stays open; the control plane does not.
    let health = server
        .client
        .get(url(&server, "/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);

    for path in [
        "/api/v1/workers",
        "/api/v1/sessions",
        "/api/v1/sessions/whatever",
        "/api/v1/runs/whatever",
        "/api/v1/runs/whatever/events",
        "/api/v1/runs/whatever/stream",
        "/api/workers",
    ] {
        let response = server.client.get(url(&server, path)).send().await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "GET {path} should require a session"
        );
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/json"
        );
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-store");
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["error"], "login required", "GET {path}");
    }

    for (method, path) in [
        ("POST", "/api/v1/sessions"),
        ("POST", "/api/v1/sessions/whatever/cancel"),
        ("POST", "/api/v1/sessions/whatever/runs"),
    ] {
        let response = server
            .client
            .request(method.parse().unwrap(), url(&server, path))
            .json(&json!({ "task": "x" }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path} should require a session"
        );
    }

    // And the v1 worker alias answers once authenticated.
    let (status, body) = authed_get(&server, "/api/v1/workers").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["workers"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn a_browser_cookie_session_opens_the_control_plane() {
    // A browser served from the gateway's origin never sees a bearer token; the
    // HttpOnly cookie from /login has to work for the control plane, which is
    // what makes EventSource usable for the web and mobile builds.
    let server = start(None).await;
    let response = server
        .client
        .post(url(&server, "/login"))
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(format!("pin={PIN}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let set_cookie = response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap();
    let cookie = set_cookie.split(';').next().unwrap().to_string();

    let response = server
        .client
        .get(url(&server, "/api/v1/sessions"))
        .header(COOKIE, cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.unwrap();
    assert!(body["sessions"].is_array());
}

#[tokio::test]
async fn sessions_are_created_and_reused_by_name_on_one_worker() {
    let server = start(None).await;
    let _worker = connect_worker(&server, "worker-a").await;
    wait_for_worker(&server, "worker-a").await;

    let first = create_session(&server, json!({ "name": "alpha" })).await;
    assert_eq!(first["reused"], false);
    assert_eq!(first["worker_id"], "worker-a");
    assert_eq!(first["status"], "idle");
    assert_eq!(first["worker_connected"], true);
    let id = first["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("ses_"), "{id}");
    let thread_id = first["thread_id"].as_str().unwrap().to_string();
    assert!(thread_id.starts_with("thr_"), "{thread_id}");
    assert!(first["agent_id"].as_str().unwrap().starts_with("agent_"));

    // Same name -> same binding (200, reused).
    let (status, second) =
        authed_post(&server, "/api/v1/sessions", json!({ "name": "alpha" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second["reused"], true);
    assert_eq!(second["id"], first["id"]);
    assert_eq!(second["thread_id"], first["thread_id"]);

    // A different name is a different binding.
    let other = create_session(&server, json!({ "name": "beta", "worker": "worker-a" })).await;
    assert_ne!(other["id"], first["id"]);

    // No name -> a fresh binding each time.
    let anonymous_one = create_session(&server, json!({})).await;
    let anonymous_two = create_session(&server, json!({})).await;
    assert_ne!(anonymous_one["id"], anonymous_two["id"]);
    assert_eq!(anonymous_one["name"], Value::Null);

    let (status, list) = authed_get(&server, "/api/v1/sessions").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["sessions"].as_array().unwrap().len(), 4);

    let (status, single) = authed_get(&server, &format!("/api/v1/sessions/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(single["id"], first["id"]);

    let (status, missing) = authed_get(&server, "/api/v1/sessions/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(missing["error"], "unknown_session");
}

#[tokio::test]
async fn creating_a_session_without_a_unique_worker_is_explained() {
    let server = start(None).await;

    let (status, body) = authed_post(&server, "/api/v1/sessions", json!({ "name": "x" })).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"], "no_worker_connected");

    let _first = connect_worker(&server, "worker-1").await;
    let _second = connect_worker(&server, "worker-2").await;
    wait_for_worker(&server, "worker-2").await;
    let (status, body) = authed_post(&server, "/api/v1/sessions", json!({ "name": "x" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "worker_required");

    // Naming the worker works even with two connected.
    let session = create_session(&server, json!({ "name": "x", "worker": "worker-2" })).await;
    assert_eq!(session["worker_id"], "worker-2");
}

#[tokio::test]
async fn a_run_dispatches_the_agent_frame_and_settles_on_the_thread_reply() {
    let server = start(None).await;
    let mut worker = connect_worker(&server, "worker-run").await;
    wait_for_worker(&server, "worker-run").await;

    let session = create_session(&server, json!({ "name": "dispatch" })).await;
    let session_id = session["id"].as_str().unwrap().to_string();
    let thread_id = session["thread_id"].as_str().unwrap().to_string();
    let agent_id = session["agent_id"].as_str().unwrap().to_string();

    let (status, run) = dispatch(
        &server,
        &session_id,
        json!({ "task": "say hi", "timeout_s": 60 }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "run: {run}");
    assert_eq!(run["status"], "running");
    assert_eq!(run["session_id"], session_id);
    assert_eq!(run["task"], "say hi");
    assert_eq!(run["final_text"], Value::Null);
    let run_id = run["id"].as_str().unwrap().to_string();
    let trace_id = run["trace_id"].as_str().unwrap().to_string();
    assert!(run_id.starts_with("run_"));

    // The fake worker sees the frame a browser client would have relayed.
    let frame = next_frame(&mut worker).await;
    assert_eq!(frame["kind"], "run_agent_for_thread_reply");
    assert_eq!(frame["trace_id"], trace_id);
    assert_eq!(frame["thread_id"], thread_id);
    assert_eq!(frame["agent_id"], agent_id);
    assert_eq!(frame["prompt"], "say hi");
    assert_eq!(frame["mcp_servers"], json!([]));
    let spec = &frame["spec"];
    assert_eq!(spec["id"], agent_id);
    assert_eq!(spec["name"], "dispatch");
    assert_eq!(spec["prompt"], "");
    assert_eq!(spec["tools"], json!([]));
    assert_eq!(spec["triggers"], json!([]));
    assert_eq!(spec["mcp_ids"], json!([]));
    assert!(spec["created_at"].as_str().unwrap().contains('T'));
    assert!(spec["updated_at"].as_str().unwrap().contains('T'));

    // The session reports itself busy while that run is in flight.
    let (_, busy) = authed_get(&server, &format!("/api/v1/sessions/{session_id}")).await;
    assert_eq!(busy["status"], "running");
    assert_eq!(busy["active_run_id"], run_id);

    // Stream a turn back the way the worker does.
    send_frame(
        &mut worker,
        json!({ "kind": "assistant_delta", "trace_id": trace_id, "delta": "hel" }),
    )
    .await;
    send_frame(
        &mut worker,
        json!({ "kind": "assistant_delta", "trace_id": trace_id, "delta": "lo" }),
    )
    .await;
    send_frame(
        &mut worker,
        json!({
            "kind": "tool_call_started",
            "trace_id": trace_id,
            "id": "call-1",
            "name": "bash",
            "arguments": { "cmd": "ls" },
        }),
    )
    .await;
    send_frame(
        &mut worker,
        json!({
            "kind": "tool_call_finished",
            "trace_id": trace_id,
            "id": "call-1",
            "result": "file.txt",
        }),
    )
    .await;
    send_frame(
        &mut worker,
        json!({ "kind": "agent_finished", "trace_id": trace_id, "status": "Success" }),
    )
    .await;
    send_frame(
        &mut worker,
        json!({
            "kind": "thread_agent_reply",
            "trace_id": trace_id,
            "thread_id": thread_id,
            "content": "hello",
        }),
    )
    .await;

    let settled = wait_run(&server, &run_id, "succeeded").await;
    assert_eq!(settled["final_text"], "hello");
    assert_eq!(settled["error"], Value::Null);
    assert!(settled["finished_at"].as_str().is_some());

    let (_, session_after) = authed_get(&server, &format!("/api/v1/sessions/{session_id}")).await;
    assert_eq!(session_after["status"], "idle");
    assert_eq!(session_after["active_run_id"], Value::Null);
    assert_eq!(session_after["run_count"], 1);
    assert_eq!(session_after["last_run_id"], run_id);

    // The event log carries text deltas, the tool call pair and the terminal
    // status/final events.
    let (status, events) = authed_get(&server, &format!("/api/v1/runs/{run_id}/events")).await;
    assert_eq!(status, StatusCode::OK);
    let log = events["events"].as_array().unwrap();
    assert_eq!(events["from_index"], 0);
    assert_eq!(events["next_index"].as_u64().unwrap(), log.len() as u64);
    let types: Vec<&str> = log
        .iter()
        .map(|event| event["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        types,
        [
            "status",
            "text",
            "text",
            "tool_call",
            "tool_result",
            "status",
            "final"
        ]
    );
    assert_eq!(log[1]["text"], "hel");
    assert_eq!(log[3]["callId"], "call-1");
    assert_eq!(log[3]["tool"], "bash");
    assert_eq!(log[3]["input"]["cmd"], "ls");
    assert_eq!(log[4]["status"], "completed");
    assert_eq!(log[4]["result"], "file.txt");
    assert_eq!(log[6]["reason"]["kind"], "completed");
    assert_eq!(log[6]["text"], "hello");
    for (index, event) in log.iter().enumerate() {
        assert_eq!(event["index"].as_u64().unwrap(), index as u64);
    }

    // from_index is inclusive and clamped.
    let (_, tail) = authed_get(
        &server,
        &format!("/api/v1/runs/{run_id}/events?from_index=4"),
    )
    .await;
    let tail_log = tail["events"].as_array().unwrap();
    assert_eq!(tail_log[0]["index"], 4);
    assert_eq!(tail["next_index"].as_u64().unwrap(), 7);
    let (_, past_end) = authed_get(
        &server,
        &format!("/api/v1/runs/{run_id}/events?from_index=99"),
    )
    .await;
    assert_eq!(past_end["events"], json!([]));
    assert_eq!(past_end["next_index"], 7);
}

#[tokio::test]
async fn one_run_per_session_returns_409_until_cancel_frees_it() {
    let server = start(None).await;
    let mut worker = connect_worker(&server, "worker-one-run").await;
    wait_for_worker(&server, "worker-one-run").await;
    let session = create_session(&server, json!({ "name": "one" })).await;
    let session_id = session["id"].as_str().unwrap().to_string();

    let (status, run) = dispatch(&server, &session_id, json!({ "task": "first" })).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let run_id = run["id"].as_str().unwrap().to_string();
    let _frame = next_frame(&mut worker).await;

    let (status, busy) = dispatch(&server, &session_id, json!({ "task": "second" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(busy["error"], "session_busy");
    assert!(busy["message"].as_str().unwrap().contains(&run_id));

    // Cancel settles the run gateway-side and tells the worker to stop.
    let (status, cancelled) = authed_post(
        &server,
        &format!("/api/v1/sessions/{session_id}/cancel"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["ok"], true);
    assert_eq!(cancelled["run_id"], run_id);

    let stop = next_frame(&mut worker).await;
    assert_eq!(stop["kind"], "cancel_run");
    assert_eq!(stop["trace_id"], run["trace_id"]);

    let run_after = wait_run(&server, &run_id, "cancelled").await;
    assert_eq!(run_after["error"], Value::Null);
    assert_eq!(run_after["final_text"], Value::Null);
    let (_, events) = authed_get(&server, &format!("/api/v1/runs/{run_id}/events")).await;
    let last = events["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["type"], "final");
    assert_eq!(last["reason"]["kind"], "cancelled");
    assert_eq!(events["status"], "cancelled");

    // Cancelling again is a no-op with no run id.
    let (status, idle) = authed_post(
        &server,
        &format!("/api/v1/sessions/{session_id}/cancel"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(idle["run_id"], Value::Null);

    // And the slot is free for the next turn.
    let (status, next) = dispatch(&server, &session_id, json!({ "task": "second" })).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(next["status"], "running");
}

#[tokio::test]
async fn a_silent_worker_run_times_out_and_frees_the_session() {
    let server = start(None).await;
    let mut worker = connect_worker(&server, "worker-timeout").await;
    wait_for_worker(&server, "worker-timeout").await;
    let session = create_session(&server, json!({ "name": "timeout" })).await;
    let session_id = session["id"].as_str().unwrap().to_string();

    let (status, run) = dispatch(
        &server,
        &session_id,
        json!({ "task": "never answers", "timeout_s": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let run_id = run["id"].as_str().unwrap().to_string();
    let _frame = next_frame(&mut worker).await;

    let timed_out = wait_run(&server, &run_id, "timeout").await;
    assert_eq!(
        timed_out["error"], "run timed out after 1s",
        "run: {timed_out}"
    );
    let (_, events) = authed_get(&server, &format!("/api/v1/runs/{run_id}/events")).await;
    let last = events["events"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["type"], "final");
    assert_eq!(last["reason"]["kind"], "error");
    assert_eq!(last["reason"]["error"]["message"], "run timed out after 1s");

    let (_, session_after) = authed_get(&server, &format!("/api/v1/sessions/{session_id}")).await;
    assert_eq!(session_after["status"], "idle");
    let (status, _) = dispatch(&server, &session_id, json!({ "task": "next" })).await;
    assert_eq!(status, StatusCode::ACCEPTED);
}

#[tokio::test]
async fn a_disconnected_worker_settles_its_running_run() {
    let server = start(None).await;
    let mut worker = connect_worker(&server, "worker-gone").await;
    wait_for_worker(&server, "worker-gone").await;
    let session = create_session(&server, json!({ "name": "gone" })).await;
    let session_id = session["id"].as_str().unwrap().to_string();

    let (status, run) = dispatch(&server, &session_id, json!({ "task": "orphan" })).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let run_id = run["id"].as_str().unwrap().to_string();
    let _frame = next_frame(&mut worker).await;

    drop(worker);
    let failed = wait_run(&server, &run_id, "failed").await;
    assert_eq!(
        failed["error"],
        "the worker disconnected before the run finished"
    );
    let (_, session_after) = authed_get(&server, &format!("/api/v1/sessions/{session_id}")).await;
    assert_eq!(session_after["status"], "idle");
}

#[tokio::test]
async fn sse_replays_from_an_index_sends_keepalive_and_closes_after_final() {
    let server = start(None).await;
    let mut worker = connect_worker(&server, "worker-sse").await;
    wait_for_worker(&server, "worker-sse").await;
    let session = create_session(&server, json!({ "name": "sse" })).await;
    let session_id = session["id"].as_str().unwrap().to_string();

    let (status, run) = dispatch(&server, &session_id, json!({ "task": "stream" })).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let run_id = run["id"].as_str().unwrap().to_string();
    let trace_id = run["trace_id"].as_str().unwrap().to_string();
    let _frame = next_frame(&mut worker).await;

    send_frame(
        &mut worker,
        json!({ "kind": "assistant_delta", "trace_id": trace_id, "delta": "one" }),
    )
    .await;
    send_frame(
        &mut worker,
        json!({ "kind": "assistant_delta", "trace_id": trace_id, "delta": "two" }),
    )
    .await;
    // status(running)=0, text=1, text=2
    wait_next_index(&server, &run_id, 3).await;

    let token = server.token.clone();
    let mut stream = open_stream(
        &server,
        &format!("/api/v1/runs/{run_id}/stream?from_index=1"),
        &token,
    )
    .await;
    let mut raw = String::new();
    read_until(&mut stream, &mut raw, ": keepalive").await;
    assert!(
        raw.starts_with("HTTP/1.1 200"),
        "unexpected stream response: {}",
        &raw[..raw.len().min(200)]
    );
    assert!(raw.contains("content-type: text/event-stream"));
    let replay = sse_events(&raw);
    assert_eq!(
        replay
            .iter()
            .map(|event| event["index"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2],
        "replay should start at from_index=1: {raw}"
    );
    assert_eq!(replay[0]["type"], "text");
    assert_eq!(replay[0]["text"], "one");
    assert_eq!(replay[1]["text"], "two");

    // The live half: a terminal event arrives and the stream closes.
    send_frame(
        &mut worker,
        json!({
            "kind": "thread_agent_reply",
            "trace_id": trace_id,
            "thread_id": session["thread_id"],
            "content": "onetwo",
        }),
    )
    .await;
    read_to_end(&mut stream, &mut raw).await;
    let all = sse_events(&raw);
    let final_event = all
        .iter()
        .find(|event| event["type"] == "final")
        .expect("stream should deliver the final event");
    assert_eq!(final_event["reason"]["kind"], "completed");
    assert_eq!(final_event["text"], "onetwo");
    let final_index = final_event["index"].as_u64().unwrap();
    let indices: Vec<u64> = all
        .iter()
        .map(|event| event["index"].as_u64().unwrap())
        .collect();
    assert!(
        indices.windows(2).all(|pair| pair[0] < pair[1]),
        "SSE event indexes must increase: {indices:?}"
    );
    assert_eq!(indices.first().copied(), Some(1));
    assert_eq!(indices.last().copied(), Some(final_index));
}

#[tokio::test]
async fn wait_true_returns_the_settled_run() {
    let server = start(None).await;
    let mut worker = connect_worker(&server, "worker-wait").await;
    wait_for_worker(&server, "worker-wait").await;
    let session = create_session(&server, json!({ "name": "wait" })).await;
    let session_id = session["id"].as_str().unwrap().to_string();

    let client = server.client.clone();
    let addr = server.addr;
    let token = server.token.clone();
    let path = format!("/api/v1/sessions/{session_id}/runs");
    let request = tokio::spawn(async move {
        authed_post_with(
            &client,
            &addr,
            &token,
            &path,
            json!({ "task": "wait for me", "wait": true, "timeout_s": 30 }),
        )
        .await
    });

    let frame = next_frame(&mut worker).await;
    assert_eq!(frame["kind"], "run_agent_for_thread_reply");
    let trace_id = frame["trace_id"].as_str().unwrap().to_string();
    send_frame(
        &mut worker,
        json!({
            "kind": "thread_agent_reply",
            "trace_id": trace_id,
            "thread_id": frame["thread_id"],
            "content": "done",
        }),
    )
    .await;

    let (status, run) = request.await.unwrap();
    assert_eq!(status, StatusCode::OK, "run: {run}");
    assert_eq!(run["status"], "succeeded");
    assert_eq!(run["final_text"], "done");
}

#[tokio::test]
async fn session_bindings_survive_a_gateway_restart() {
    let directory = tempfile::tempdir().unwrap();
    let data_dir = directory.path().to_path_buf();

    let first_server = start(Some(data_dir.clone())).await;
    let _worker = connect_worker(&first_server, "worker-persist").await;
    wait_for_worker(&first_server, "worker-persist").await;
    let session = create_session(&first_server, json!({ "name": "persisted" })).await;
    let id = session["id"].as_str().unwrap().to_string();
    let thread_id = session["thread_id"].as_str().unwrap().to_string();
    assert!(data_dir.join("sessions.json").exists());
    drop(first_server);

    let second_server = start(Some(data_dir)).await;
    let (status, restored) = authed_get(&second_server, &format!("/api/v1/sessions/{id}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["id"], id);
    assert_eq!(restored["thread_id"], thread_id);
    assert_eq!(restored["worker_id"], "worker-persist");
    assert_eq!(restored["status"], "idle");

    // The name still names it after the restart.
    let (status, reused) = authed_post(
        &second_server,
        "/api/v1/sessions",
        json!({ "name": "persisted" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reused["reused"], true);
    assert_eq!(reused["id"], id);
}

#[tokio::test]
async fn the_session_list_reports_worker_connectivity() {
    let server = start(None).await;
    let worker = connect_worker(&server, "worker-conn").await;
    wait_for_worker(&server, "worker-conn").await;
    let session = create_session(&server, json!({ "name": "conn" })).await;
    assert_eq!(session["worker_connected"], true);

    drop(worker);
    for _ in 0..200 {
        let (_, list) = authed_get(&server, "/api/v1/sessions").await;
        if list["sessions"][0]["worker_connected"] == false {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("session never reported its worker as disconnected");
}
