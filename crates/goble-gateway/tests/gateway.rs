use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use goble_gateway::{hash_pin, Gateway, GatewayConfig, DEFAULT_SESSION_TTL, SESSION_COOKIE};
use reqwest::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, HOST, LOCATION, SET_COOKIE};
use reqwest::StatusCode;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

const PIN: &str = "123456";
const WORKER_TOKEN: &str = "test-worker-token";
const WASM_BYTES: &[u8] = b"\0asm\x01\x00\x00\x00";

type Ws = WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct TestServer {
    addr: SocketAddr,
    client: reqwest::Client,
    join: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.join.abort();
    }
}

async fn start(pin: Option<&str>, session_ttl: Duration) -> TestServer {
    start_with_web_dir(pin, session_ttl, None).await
}

async fn start_with_web_dir(
    pin: Option<&str>,
    session_ttl: Duration,
    web_dir: Option<PathBuf>,
) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut config = GatewayConfig::new(addr, pin.map(hash_pin), WORKER_TOKEN);
    config.session_ttl = session_ttl;
    config.web_dir = web_dir;

    let gateway = Gateway::new(config).unwrap();
    let router = gateway.router();
    let join = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    TestServer {
        addr,
        client: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap(),
        join,
    }
}

/// A throwaway client bundle: no generated wasm is required to run these tests.
struct ClientAssets {
    dir: tempfile::TempDir,
}

impl ClientAssets {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("index.html"),
            "<!doctype html><title>goble client</title>\n<script src=\"app.js\"></script>\n",
        )
        .unwrap();
        std::fs::write(root.join("app.js"), "export function boot() {}\n").unwrap();
        std::fs::write(root.join("app.css"), "body { margin: 0 }\n").unwrap();
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        std::fs::write(root.join("pkg/app_bg.wasm"), WASM_BYTES).unwrap();
        std::fs::write(root.join("secret.txt"), "only-this-file\n").unwrap();
        Self { dir }
    }

    fn path(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }
}

async fn login(server: &TestServer, pin: &str) -> reqwest::Response {
    server
        .client
        .post(format!("http://{}/login", server.addr))
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(format!("pin={pin}"))
        .send()
        .await
        .unwrap()
}

fn session_from(response: &reqwest::Response) -> Option<String> {
    let raw = response.headers().get(SET_COOKIE)?.to_str().ok()?;
    let pair = raw.split(';').next().unwrap_or(raw);
    let (name, token) = pair.split_once('=')?;
    (name == SESSION_COOKIE).then(|| token.to_string())
}

async fn login_token(server: &TestServer) -> String {
    let response = login(server, PIN).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    session_from(&response).expect("login should set a session cookie")
}

async fn panel(server: &TestServer, token: Option<&str>) -> reqwest::Response {
    get_path(server, "/panel", token).await
}

async fn get_path(server: &TestServer, path: &str, token: Option<&str>) -> reqwest::Response {
    let mut request = server.client.get(format!("http://{}{path}", server.addr));
    if let Some(token) = token {
        request = request.header(COOKIE, format!("{SESSION_COOKIE}={token}"));
    }
    request.send().await.unwrap()
}

/// Send a request without URL normalization, so `..` reaches the router intact.
async fn raw_get(server: &TestServer, target: &str, token: Option<&str>) -> String {
    let mut stream = tokio::net::TcpStream::connect(server.addr).await.unwrap();
    let cookie = token
        .map(|token| format!("Cookie: {SESSION_COOKIE}={token}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "GET {target} HTTP/1.1\r\nHost: {}\r\n{cookie}Connection: close\r\n\r\n",
        server.addr
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    String::from_utf8_lossy(&response).into_owned()
}

async fn health(server: &TestServer) -> serde_json::Value {
    server
        .client
        .get(format!("http://{}/health", server.addr))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

async fn wait_for_worker_count(server: &TestServer, expected: u64) {
    for _ in 0..200 {
        if health(server).await["workers"].as_u64() == Some(expected) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("gateway never reported {expected} connected worker(s)");
}

async fn connect_worker(server: &TestServer, worker_id: &str, token: &str) -> Result<Ws, WsError> {
    let url = format!("ws://{}/ws/worker?id={worker_id}", server.addr);
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
}

async fn connect_client(
    server: &TestServer,
    worker_id: &str,
    session: &str,
) -> Result<Ws, WsError> {
    let url = format!("ws://{}/ws/client?worker={worker_id}", server.addr);
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        "cookie",
        HeaderValue::from_str(&format!("{SESSION_COOKIE}={session}")).unwrap(),
    );
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(socket, _)| socket)
}

async fn next_text(socket: &mut Ws) -> String {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("timed out waiting for a frame")
            .expect("socket closed")
            .expect("websocket error");
        match message {
            WsMessage::Text(text) => return text.to_string(),
            WsMessage::Ping(_) | WsMessage::Pong(_) => continue,
            other => panic!("expected a text frame, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn health_is_unauthenticated_and_reports_the_worker_count() {
    let server = start(Some(PIN), DEFAULT_SESSION_TTL).await;

    let body = health(&server).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["workers"], 0);

    let _worker = connect_worker(&server, "worker-health", WORKER_TOKEN)
        .await
        .expect("worker should connect");
    wait_for_worker_count(&server, 1).await;
    assert_eq!(health(&server).await["workers"], 1);
}

#[tokio::test]
async fn wrong_pin_is_rejected_and_the_right_pin_opens_the_panel() {
    let server = start(Some(PIN), DEFAULT_SESSION_TTL).await;

    let without_cookie = panel(&server, None).await;
    assert_eq!(without_cookie.status(), StatusCode::SEE_OTHER);

    let wrong = login(&server, "000000").await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    assert!(wrong.headers().get(SET_COOKIE).is_none());
    assert_eq!(panel(&server, None).await.status(), StatusCode::SEE_OTHER);

    let right = login(&server, PIN).await;
    assert_eq!(right.status(), StatusCode::SEE_OTHER);
    let token = session_from(&right).expect("session cookie");
    let cookie = right.headers().get(SET_COOKIE).unwrap().to_str().unwrap();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
    assert!(cookie.contains("Path=/"));

    let panel = panel(&server, Some(&token)).await;
    assert_eq!(panel.status(), StatusCode::OK);
    let page = panel.text().await.unwrap();
    assert!(page.contains("Connected workers"));
    assert!(page.contains("Transport console"));
}

#[tokio::test]
async fn text_frames_relay_both_ways_between_client_and_worker() {
    let server = start(Some(PIN), DEFAULT_SESSION_TTL).await;
    let mut worker = connect_worker(&server, "worker-relay", WORKER_TOKEN)
        .await
        .expect("worker should connect");
    wait_for_worker_count(&server, 1).await;

    let token = login_token(&server).await;
    let mut client = connect_client(&server, "worker-relay", &token)
        .await
        .expect("client should connect");

    let to_worker = r#"{"kind":"ping","from":"client"}"#;
    client
        .send(WsMessage::Text(to_worker.into()))
        .await
        .unwrap();
    assert_eq!(next_text(&mut worker).await, to_worker);

    let to_client = r#"{"kind":"agent_log","from":"worker"}"#;
    worker
        .send(WsMessage::Text(to_client.into()))
        .await
        .unwrap();
    assert_eq!(next_text(&mut client).await, to_client);
}

#[tokio::test]
async fn worker_frames_are_broadcast_to_every_client() {
    let server = start(Some(PIN), DEFAULT_SESSION_TTL).await;
    let mut worker = connect_worker(&server, "worker-broadcast", WORKER_TOKEN)
        .await
        .expect("worker should connect");
    wait_for_worker_count(&server, 1).await;

    let token = login_token(&server).await;
    let mut first = connect_client(&server, "worker-broadcast", &token)
        .await
        .expect("first client");
    let mut second = connect_client(&server, "worker-broadcast", &token)
        .await
        .expect("second client");

    let frame = r#"{"kind":"status_report"}"#;
    worker.send(WsMessage::Text(frame.into())).await.unwrap();

    assert_eq!(next_text(&mut first).await, frame);
    assert_eq!(next_text(&mut second).await, frame);
}

#[tokio::test]
async fn a_disconnected_client_asks_the_worker_to_close_its_panes() {
    let server = start(Some(PIN), DEFAULT_SESSION_TTL).await;
    let mut worker = connect_worker(&server, "worker-panes", WORKER_TOKEN)
        .await
        .expect("worker should connect");
    wait_for_worker_count(&server, 1).await;

    let token = login_token(&server).await;
    let mut client = connect_client(&server, "worker-panes", &token)
        .await
        .expect("client should connect");

    let open_one =
        r#"{"kind":"open_pty","pane_id":"pane-1","cwd":"/workspace","cols":80,"rows":24}"#;
    let open_two =
        r#"{"kind":"open_pty","pane_id":"pane-2","cwd":"/workspace","cols":80,"rows":24}"#;
    let close_one = r#"{"kind":"close_pty","pane_id":"pane-1"}"#;
    for frame in [open_one, open_two, close_one] {
        client.send(WsMessage::Text(frame.into())).await.unwrap();
        assert_eq!(next_text(&mut worker).await, frame);
    }

    client.close(None).await.unwrap();

    let frame: serde_json::Value =
        serde_json::from_str(&next_text(&mut worker).await).expect("JSON close frame");
    assert_eq!(frame["kind"], "close_pty");
    assert_eq!(
        frame["pane_id"], "pane-2",
        "only the pane the client left open is closed"
    );
}

#[tokio::test]
async fn a_worker_cannot_authenticate_with_the_client_pin() {
    let server = start(Some(PIN), DEFAULT_SESSION_TTL).await;

    match connect_worker(&server, "worker-pin", PIN).await {
        Err(WsError::Http(response)) => assert_eq!(response.status(), 401),
        Err(other) => panic!("expected an HTTP handshake rejection, got {other:?}"),
        Ok(_) => panic!("a worker using the client PIN must be rejected"),
    }
}

#[tokio::test]
async fn x_worker_token_header_is_accepted() {
    let server = start(Some(PIN), DEFAULT_SESSION_TTL).await;
    let url = format!("ws://{}/ws/worker?id=worker-header", server.addr);
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert(
        "x-worker-token",
        HeaderValue::from_str(WORKER_TOKEN).unwrap(),
    );

    let _worker = tokio_tungstenite::connect_async(request)
        .await
        .expect("worker should connect")
        .0;
    wait_for_worker_count(&server, 1).await;
}

#[tokio::test]
async fn a_client_socket_requires_a_session() {
    let server = start(Some(PIN), DEFAULT_SESSION_TTL).await;
    let url = format!("ws://{}/ws/client?worker=nobody", server.addr);
    let request = url.into_client_request().unwrap();

    match tokio_tungstenite::connect_async(request).await {
        Err(WsError::Http(response)) => assert_eq!(response.status(), 401),
        Err(other) => panic!("expected an HTTP handshake rejection, got {other:?}"),
        Ok(_) => panic!("an unauthenticated client must be rejected"),
    }
}

#[tokio::test]
async fn unknown_and_expired_session_cookies_are_rejected() {
    let server = start(Some(PIN), Duration::from_millis(30)).await;

    assert_eq!(
        panel(&server, Some("this-token-was-never-issued"))
            .await
            .status(),
        StatusCode::SEE_OTHER
    );

    let token = login_token(&server).await;
    assert_eq!(panel(&server, Some(&token)).await.status(), StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        panel(&server, Some(&token)).await.status(),
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn logout_clears_the_session() {
    let server = start(Some(PIN), DEFAULT_SESSION_TTL).await;
    let token = login_token(&server).await;

    let response = server
        .client
        .post(format!("http://{}/logout", server.addr))
        .header(COOKIE, format!("{SESSION_COOKIE}={token}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let cleared = response
        .headers()
        .get(SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cleared.contains("Max-Age=0"));

    assert_eq!(
        panel(&server, Some(&token)).await.status(),
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn loopback_no_auth_mode_opens_the_panel() {
    let server = start(None, DEFAULT_SESSION_TTL).await;
    let response = panel(&server, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.text().await.unwrap().contains("Connected workers"));
}

#[tokio::test]
async fn an_unlisted_host_header_is_rejected() {
    let server = start(Some(PIN), DEFAULT_SESSION_TTL).await;
    let response = server
        .client
        .get(format!("http://{}/health", server.addr))
        .header(HOST, "attacker.example")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_browser_client_requires_a_session() {
    let assets = ClientAssets::new();
    let server = start_with_web_dir(Some(PIN), DEFAULT_SESSION_TTL, Some(assets.path())).await;

    let anonymous = get_path(&server, "/app/", None).await;
    assert_eq!(anonymous.status(), StatusCode::SEE_OTHER);
    assert_eq!(anonymous.headers().get(LOCATION).unwrap(), "/");
    assert_eq!(
        get_path(&server, "/app/app.js", None).await.status(),
        StatusCode::SEE_OTHER
    );

    let token = login_token(&server).await;
    let client = get_path(&server, "/app/", Some(&token)).await;
    assert_eq!(client.status(), StatusCode::OK);
    assert_eq!(
        client.headers().get(CONTENT_TYPE).unwrap(),
        "text/html; charset=utf-8"
    );
    assert!(client.text().await.unwrap().contains("goble client"));
    assert_eq!(
        get_path(&server, "/app", Some(&token)).await.status(),
        StatusCode::OK
    );

    let script = get_path(&server, "/app/app.js", Some(&token)).await;
    assert_eq!(script.status(), StatusCode::OK);
    assert_eq!(
        script.headers().get(CONTENT_TYPE).unwrap(),
        "text/javascript; charset=utf-8"
    );
}

#[tokio::test]
async fn an_authenticated_root_redirects_to_the_client() {
    let assets = ClientAssets::new();
    let server = start_with_web_dir(Some(PIN), DEFAULT_SESSION_TTL, Some(assets.path())).await;

    let login_page = get_path(&server, "/", None).await;
    assert_eq!(login_page.status(), StatusCode::OK);
    assert!(login_page.text().await.unwrap().contains("Unlock"));

    let token = login_token(&server).await;
    let root = get_path(&server, "/", Some(&token)).await;
    assert_eq!(root.status(), StatusCode::SEE_OTHER);
    assert_eq!(root.headers().get(LOCATION).unwrap(), "/app/");
}

#[tokio::test]
async fn wasm_assets_are_served_with_the_right_type_and_bytes() {
    let assets = ClientAssets::new();
    let server = start_with_web_dir(Some(PIN), DEFAULT_SESSION_TTL, Some(assets.path())).await;
    let token = login_token(&server).await;

    let response = get_path(&server, "/app/pkg/app_bg.wasm", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/wasm"
    );
    assert_eq!(response.headers().get(CACHE_CONTROL).unwrap(), "no-cache");
    assert_eq!(response.bytes().await.unwrap().as_ref(), WASM_BYTES);
}

#[tokio::test]
async fn unknown_assets_are_404_without_listing_directories() {
    let assets = ClientAssets::new();
    let server = start_with_web_dir(Some(PIN), DEFAULT_SESSION_TTL, Some(assets.path())).await;
    let token = login_token(&server).await;

    assert_eq!(
        get_path(&server, "/app/nope.js", Some(&token))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get_path(&server, "/app/pkg", Some(&token)).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get_path(&server, "/app/pkg/", Some(&token)).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn path_traversal_never_leaves_the_asset_root() {
    let assets = ClientAssets::new();
    let server = start_with_web_dir(Some(PIN), DEFAULT_SESSION_TTL, Some(assets.path())).await;
    let token = login_token(&server).await;

    for target in [
        "/app/../Cargo.toml",
        "/app/../secret.txt",
        "/app/pkg/../../secret.txt",
        "/app/..",
    ] {
        let raw = raw_get(&server, target, Some(&token)).await;
        let status = raw.lines().next().unwrap_or_default().to_string();
        assert!(
            status.contains(" 404 ") || status.contains(" 400 "),
            "{target} should be refused, got {status}"
        );
        assert!(!raw.contains("[package]"), "{target} leaked Cargo.toml");
        assert!(!raw.contains("only-this-file"), "{target} leaked a file");
    }

    for target in [
        "/app/%2e%2e/Cargo.toml",
        "/app/%2e%2e%2fCargo.toml",
        "/app/..%2fsecret.txt",
        "/app/pkg/%2e%2e/%2e%2e/secret.txt",
        "/app/%2fetc%2fpasswd",
        "/app/index.html%00.png",
    ] {
        let response = get_path(&server, target, Some(&token)).await;
        assert!(
            matches!(
                response.status(),
                StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST
            ),
            "{target} should be refused, got {}",
            response.status()
        );
        let body = response.text().await.unwrap();
        assert!(!body.contains("[package]"), "{target} leaked Cargo.toml");
        assert!(!body.contains("only-this-file"), "{target} leaked a file");
    }
}

#[tokio::test]
async fn a_missing_client_build_serves_the_build_page() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("client-not-built");
    let server = start_with_web_dir(Some(PIN), DEFAULT_SESSION_TTL, Some(missing)).await;
    let token = login_token(&server).await;

    for path in ["/app/", "/app/app.js"] {
        let response = get_path(&server, path, Some(&token)).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "text/html; charset=utf-8"
        );
        let page = response.text().await.unwrap();
        assert!(page.contains("scripts/build-web.sh"));
        assert!(page.contains("goble-web --target wasm32-unknown-unknown --release"));
        assert!(page.contains("wasm-bindgen"));
        assert!(page.contains("GOBLE_WEB_DIR"));
        assert!(!page.contains("http://"));
        assert!(!page.contains("https://"));
    }
}

#[tokio::test]
async fn the_worker_api_requires_a_session() {
    let server = start(Some(PIN), DEFAULT_SESSION_TTL).await;

    let response = get_path(&server, "/api/workers", None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert_eq!(response.headers().get(CACHE_CONTROL).unwrap(), "no-store");
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["error"], "login required");
}

#[tokio::test]
async fn the_worker_api_lists_connected_workers_sorted_by_id() {
    let server = start(Some(PIN), DEFAULT_SESSION_TTL).await;
    let token = login_token(&server).await;

    let empty: serde_json::Value = get_path(&server, "/api/workers", Some(&token))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(empty["workers"].as_array().unwrap().len(), 0);

    let _second = connect_worker(&server, "worker-zeta", WORKER_TOKEN)
        .await
        .expect("worker should connect");
    let _first = connect_worker(&server, "worker-alpha", WORKER_TOKEN)
        .await
        .expect("worker should connect");
    wait_for_worker_count(&server, 2).await;

    let response = get_path(&server, "/api/workers", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert_eq!(response.headers().get(CACHE_CONTROL).unwrap(), "no-store");

    let body: serde_json::Value = response.json().await.unwrap();
    let workers = body["workers"].as_array().unwrap();
    assert_eq!(workers.len(), 2);
    assert_eq!(workers[0]["id"], "worker-alpha");
    assert_eq!(workers[1]["id"], "worker-zeta");

    for worker in workers {
        let object = worker.as_object().unwrap();
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["connected_at", "id", "last_seen"]);
        for field in ["connected_at", "last_seen"] {
            let raw = worker[field].as_str().unwrap();
            chrono::DateTime::parse_from_rfc3339(raw)
                .unwrap_or_else(|_| panic!("{field} is not RFC 3339: {raw}"));
        }
    }
    assert!(!body.to_string().contains(WORKER_TOKEN));
}

#[tokio::test]
async fn a_disconnected_worker_leaves_the_api_list() {
    let server = start(Some(PIN), DEFAULT_SESSION_TTL).await;
    let token = login_token(&server).await;

    let worker = connect_worker(&server, "worker-gone", WORKER_TOKEN)
        .await
        .expect("worker should connect");
    wait_for_worker_count(&server, 1).await;
    drop(worker);
    wait_for_worker_count(&server, 0).await;

    let body: serde_json::Value = get_path(&server, "/api/workers", Some(&token))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["workers"].as_array().unwrap().len(), 0);
}

/// A symlink inside the asset directory must not become a read primitive for
/// files outside it; the canonical root check is what stops this.
#[cfg(unix)]
#[tokio::test]
async fn a_symlink_cannot_escape_the_asset_root() {
    let assets = ClientAssets::new();
    let outside = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    std::os::unix::fs::symlink(outside, assets.path().join("escape.txt")).unwrap();

    let server = start_with_web_dir(Some(PIN), DEFAULT_SESSION_TTL, Some(assets.path())).await;
    let token = login_token(&server).await;

    let response = get_path(&server, "/app/escape.txt", Some(&token)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(!response.text().await.unwrap().contains("[package]"));
}
