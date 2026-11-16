//! Loopback tests for the desktop gateway client.
//!
//! Everything here runs against an in-process `goble_gateway::Gateway` or a
//! tiny axum stub on `127.0.0.1:0`; no external network is touched.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use goble_desktop_service::gateway_client::{
    self, GatewayClient, GatewayError, Health, DEFAULT_TIMEOUT,
};
use goble_gateway::{hash_pin, Gateway, GatewayConfig};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

const PIN: &str = "ABCD2345";
const WRONG_PIN: &str = "WRONG234";
const WORKER_TOKEN: &str = "test-worker-token";

#[derive(Clone, Debug)]
struct Recorded {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

type Recorder = Arc<Mutex<Vec<Recorded>>>;

async fn record(State(records): State<Recorder>, request: Request, next: Next) -> Response {
    let method = request.method().as_str().to_owned();
    let path = request.uri().path().to_owned();
    let headers = request
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, 64 * 1024).await.unwrap_or_default();
    let body = String::from_utf8_lossy(&bytes).into_owned();
    records.lock().unwrap().push(Recorded {
        method,
        path,
        headers,
        body: body.clone(),
    });
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

struct TestGateway {
    base: String,
    addr: SocketAddr,
    records: Recorder,
    join: JoinHandle<()>,
}

async fn start_gateway(prefix: Option<&str>) -> TestGateway {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = GatewayConfig::new(addr, Some(hash_pin(PIN)), WORKER_TOKEN);
    let gateway = Gateway::new(config).unwrap();

    let records: Recorder = Arc::new(Mutex::new(Vec::new()));
    let router = gateway
        .router()
        .layer(middleware::from_fn_with_state(records.clone(), record));
    let router = match prefix {
        Some(prefix) => Router::new().nest(prefix, router),
        None => router,
    };

    let join = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let base = match prefix {
        Some(prefix) => format!("http://{addr}{prefix}"),
        None => format!("http://{addr}"),
    };
    TestGateway {
        base,
        addr,
        records,
        join,
    }
}

async fn serve(router: Router) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let join = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (format!("http://{addr}"), join)
}

fn header<'a>(record: &'a Recorded, name: &str) -> Option<&'a str> {
    record
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

async fn connect_worker(
    addr: SocketAddr,
    id: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::HeaderValue;

    let mut request = format!("ws://{addr}/ws/worker?id={id}")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {WORKER_TOKEN}")).unwrap(),
    );
    let (socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    socket
}

#[tokio::test]
async fn health_pair_workers_and_logout_round_trip() {
    let server = start_gateway(None).await;

    let health = gateway_client::health(&server.base).await.unwrap();
    assert_eq!(
        health,
        Health {
            status: "ok".into(),
            workers: 0,
        }
    );

    let session = gateway_client::pair(&server.base, PIN).await.unwrap();
    assert!(!session.token.is_empty());
    assert_eq!(session.expires_in, 12 * 60 * 60);
    assert!(!format!("{session:?}").contains(&session.token));

    // A connected worker makes the list non-empty and proves decode of the
    // real `{id, connected_at, last_seen}` wire shape.
    let _worker = connect_worker(server.addr, "worker-a").await;
    let workers = gateway_client::workers(&server.base, &session.token)
        .await
        .unwrap();
    assert_eq!(workers.len(), 1);
    assert_eq!(workers[0].id, "worker-a");
    assert!(workers[0].connected_at <= workers[0].last_seen);

    gateway_client::logout(&server.base, &session.token).await;
    assert_eq!(
        gateway_client::workers(&server.base, &session.token)
            .await
            .unwrap_err(),
        GatewayError::Expired
    );

    let expected = format!("Bearer {}", session.token);
    let records = server.records.lock().unwrap();

    let health_record = records
        .iter()
        .find(|record| record.path == "/health")
        .expect("health call should have been recorded");
    assert_eq!(health_record.method, "GET");
    assert_eq!(header(health_record, "authorization"), None);

    let pair_record = records
        .iter()
        .find(|record| record.path == "/api/v1/session")
        .expect("pair call should have been recorded");
    assert_eq!(pair_record.method, "POST");
    assert_eq!(
        header(pair_record, "content-type"),
        Some("application/json")
    );
    assert_eq!(pair_record.body, format!(r#"{{"pin":"{PIN}"}}"#));
    assert_eq!(header(pair_record, "authorization"), None);

    let worker_records: Vec<&Recorded> = records
        .iter()
        .filter(|record| record.path == "/api/workers")
        .collect();
    assert_eq!(worker_records.len(), 2);
    for record in worker_records {
        assert_eq!(record.method, "GET");
        assert_eq!(header(record, "authorization"), Some(expected.as_str()));
    }

    let logout_record = records
        .iter()
        .find(|record| record.path == "/logout")
        .expect("logout call should have been recorded");
    assert_eq!(logout_record.method, "POST");
    assert_eq!(
        header(logout_record, "authorization"),
        Some(expected.as_str())
    );

    // The PIN belongs only in the pair body. It is never a header, and no
    // other request body carries it.
    for record in records.iter() {
        assert_eq!(
            header(record, "cookie"),
            None,
            "the client uses bearers only"
        );
        assert_eq!(header(record, "x-worker-token"), None);
        for (name, value) in &record.headers {
            assert!(
                !value.contains(PIN),
                "the PIN leaked into the {name} header of {}",
                record.path
            );
        }
        if record.path != "/api/v1/session" {
            assert!(
                !record.body.contains(PIN),
                "the PIN leaked into the body of {}",
                record.path
            );
        }
    }

    drop(records);
    drop(_worker);
    server.join.abort();
}

#[tokio::test]
async fn wrong_pin_is_unauthorized() {
    let server = start_gateway(None).await;

    let error = gateway_client::pair(&server.base, WRONG_PIN)
        .await
        .unwrap_err();
    assert_eq!(error, GatewayError::Unauthorized);

    server.join.abort();
}

#[tokio::test]
async fn revoked_token_is_expired() {
    let server = start_gateway(None).await;
    let session = gateway_client::pair(&server.base, PIN).await.unwrap();

    // A second pair proves the token is live before logout revokes it.
    assert!(gateway_client::workers(&server.base, &session.token)
        .await
        .is_ok());
    gateway_client::logout(&server.base, &session.token).await;

    let error = gateway_client::workers(&server.base, &session.token)
        .await
        .unwrap_err();
    assert_eq!(error, GatewayError::Expired);

    server.join.abort();
}

#[tokio::test]
async fn stopped_listener_is_unreachable() {
    let TestGateway {
        base,
        join,
        records,
        ..
    } = start_gateway(None).await;
    assert!(gateway_client::health(&base).await.is_ok());
    drop(records);

    join.abort();
    let _ = join.await;

    assert_eq!(
        gateway_client::health(&base).await.unwrap_err(),
        GatewayError::Unreachable
    );
}

async fn slow_health() -> &'static str {
    tokio::time::sleep(Duration::from_secs(5)).await;
    r#"{"status":"ok","workers":0}"#
}

#[tokio::test]
async fn timeout_on_a_real_loopback_socket() {
    let router = Router::new().route("/health", get(slow_health));
    let (base, join) = serve(router).await;

    let client = GatewayClient::with_timeout(&base, Duration::from_millis(100)).unwrap();
    assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(10));
    assert_eq!(client.health().await.unwrap_err(), GatewayError::Timeout);

    join.abort();
}

#[tokio::test]
async fn pair_retries_429_with_backoff_and_then_succeeds() {
    let attempts = Arc::new(AtomicUsize::new(0));

    async fn flaky_pair(State(attempts): State<Arc<AtomicUsize>>) -> Response {
        if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            (
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                [(axum::http::header::RETRY_AFTER, "0")],
                "slow down",
            )
                .into_response()
        } else {
            Json(serde_json::json!({"token": "retried-token", "expires_in": 60})).into_response()
        }
    }

    let router = Router::new()
        .route("/api/v1/session", post(flaky_pair))
        .with_state(attempts.clone());
    let (base, join) = serve(router).await;

    let session = gateway_client::pair(&base, PIN).await.unwrap();
    assert_eq!(session.token, "retried-token");
    assert_eq!(session.expires_in, 60);
    assert_eq!(attempts.load(Ordering::SeqCst), 2);

    join.abort();
}

#[tokio::test]
async fn persistent_429_is_bounded_and_typed() {
    let attempts = Arc::new(AtomicUsize::new(0));

    async fn throttled(State(attempts): State<Arc<AtomicUsize>>) -> Response {
        attempts.fetch_add(1, Ordering::SeqCst);
        (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            [(axum::http::header::RETRY_AFTER, "0")],
            "still too fast",
        )
            .into_response()
    }

    let router = Router::new()
        .route("/api/v1/session", post(throttled))
        .with_state(attempts.clone());
    let (base, join) = serve(router).await;

    assert_eq!(
        gateway_client::pair(&base, PIN).await.unwrap_err(),
        GatewayError::Http(429)
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 3);

    join.abort();
}

#[tokio::test]
async fn gateway_statuses_are_typed() {
    async fn unavailable() -> axum::http::StatusCode {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    }
    async fn missing_pairing() -> axum::http::StatusCode {
        axum::http::StatusCode::NOT_FOUND
    }

    let router = Router::new()
        .route("/health", get(unavailable))
        .route("/api/v1/session", post(missing_pairing));
    let (base, join) = serve(router).await;

    assert_eq!(
        gateway_client::health(&base).await.unwrap_err(),
        GatewayError::Http(503)
    );
    // A gateway build without the pairing endpoint is a typed status so the
    // UI can offer its "upgrade the gateway" copy.
    assert_eq!(
        gateway_client::pair(&base, PIN).await.unwrap_err(),
        GatewayError::Http(404)
    );

    join.abort();
}

#[tokio::test]
async fn prefixed_base_is_kept_for_http_and_socket_urls() {
    let server = start_gateway(Some("/prefix")).await;
    let pasted = format!("ws://{}/prefix/ws/worker", server.addr);

    let client = GatewayClient::new(&pasted).unwrap();
    assert_eq!(
        client.base().as_str(),
        format!("http://{}/prefix", server.addr)
    );
    assert_eq!(
        client.base().socket_url(),
        format!("ws://{}/prefix/ws/client", server.addr)
    );
    assert_eq!(
        client.base().client_socket_url("worker-a"),
        format!("ws://{}/prefix/ws/client?worker=worker-a", server.addr)
    );

    let health = client.health().await.unwrap();
    assert_eq!(health.status, "ok");

    server.join.abort();
}
