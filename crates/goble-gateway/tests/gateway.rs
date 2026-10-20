use std::net::SocketAddr;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use goble_gateway::{hash_pin, Gateway, GatewayConfig, DEFAULT_SESSION_TTL, SESSION_COOKIE};
use reqwest::header::{CONTENT_TYPE, COOKIE, HOST, SET_COOKIE};
use reqwest::StatusCode;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

const PIN: &str = "123456";
const WORKER_TOKEN: &str = "test-worker-token";

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
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut config = GatewayConfig::new(addr, pin.map(hash_pin), WORKER_TOKEN);
    config.session_ttl = session_ttl;

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
    let mut request = server.client.get(format!("http://{}/panel", server.addr));
    if let Some(token) = token {
        request = request.header(COOKIE, format!("{SESSION_COOKIE}={token}"));
    }
    request.send().await.unwrap()
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
