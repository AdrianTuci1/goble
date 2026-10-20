//! Outbound connection from the worker to a `goble-gateway`.
//!
//! A sandbox behind NAT has no address the browser can reach, so the worker
//! dials the gateway instead: it opens `<gateway>/ws/worker?id=<worker id>`
//! with the worker bearer token and then relays the same desktop protocol the
//! inbound axum `/ws` endpoint serves. The gateway matches the id with the
//! browser client's `?worker=` and copies frames between the two sockets.
//!
//! The connection is best-effort and never stops the worker: every close or
//! error reconnects with exponential backoff while the local HTTP endpoint
//! keeps serving. A misconfigured environment is different — that fails before
//! the first connection is attempted (see [`GatewaySettings::from_env`]).

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use goble_core::worker::WorkerId;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, USER_AGENT};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

use crate::state::AppState;
use crate::websocket::{run_connection, ConnectionTask, WorkerFrame, FRAME_QUEUE};

/// Gateway base URL or worker endpoint, e.g. `wss://gateway.example/ws/worker`.
pub const GATEWAY_URL_ENV: &str = "GOBLE_GATEWAY_URL";
/// Shared secret the gateway's worker endpoint checks as a bearer token.
pub const WORKER_TOKEN_ENV: &str = "GOBLE_WORKER_TOKEN";
/// Id the gateway registers this worker under.
pub const WORKER_ID_ENV: &str = "GOBLE_WORKER_ID";
/// Id the worker binary itself uses; the fallback when `GOBLE_WORKER_ID` is unset.
const FALLBACK_WORKER_ID_ENV: &str = "GOBLIN_WORKER_ID";

const WORKER_PATH: &str = "/ws/worker";
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// A connection that lived this long earns a fresh backoff on the next retry.
const STABLE_CONNECTION: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Give the writer a moment to flush its close frame before abandoning it.
const CLOSE_GRACE: Duration = Duration::from_millis(500);

/// The outbound connection settings read from the environment.
#[derive(Clone)]
pub struct GatewaySettings {
    /// Complete `ws(s)://…/ws/worker?id=…` URL.
    pub url: String,
    /// Bearer token for the gateway's worker endpoint.
    pub token: String,
    /// Worker id the gateway registers; already URL-encoded in [`Self::url`].
    pub worker_id: String,
}

impl fmt::Debug for GatewaySettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GatewaySettings")
            .field("url", &self.url)
            .field("worker_id", &self.worker_id)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl GatewaySettings {
    /// Read the gateway connection from the environment.
    ///
    /// `Ok(None)` means [`GATEWAY_URL_ENV`] is unset: the worker is not
    /// configured to dial out and only serves its inbound `/ws`. A URL without
    /// a token, or with an `http(s)://` URL, is an error.
    pub fn from_env() -> Result<Option<Self>, String> {
        Self::from_lookup(|name| std::env::var(name).ok(), None)
    }

    /// Like [`Self::from_env`], but fall back to the running worker's id
    /// before generating one. The environment still wins.
    pub fn from_env_with_worker_id(fallback_worker_id: &str) -> Result<Option<Self>, String> {
        Self::from_lookup(|name| std::env::var(name).ok(), Some(fallback_worker_id))
    }

    fn from_lookup(
        lookup: impl Fn(&str) -> Option<String>,
        fallback_worker_id: Option<&str>,
    ) -> Result<Option<Self>, String> {
        let Some(raw_url) = lookup(GATEWAY_URL_ENV)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        else {
            return Ok(None);
        };

        let token = lookup(WORKER_TOKEN_ENV)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                format!(
                    "{WORKER_TOKEN_ENV} is not set; {GATEWAY_URL_ENV} needs the gateway's worker token"
                )
            })?;

        let worker_id = lookup(WORKER_ID_ENV)
            .or_else(|| lookup(FALLBACK_WORKER_ID_ENV))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                fallback_worker_id
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| WorkerId::generate().0);

        let url = worker_endpoint(&raw_url, &worker_id)?;
        Ok(Some(Self {
            url,
            token,
            worker_id,
        }))
    }
}

/// Build the worker endpoint from a gateway base URL.
///
/// A URL with no path gets [`WORKER_PATH`]; an existing path is kept as given
/// (the gateway may mount the worker route elsewhere). A missing `id` query
/// parameter is appended with the URL-encoded worker id.
fn worker_endpoint(base: &str, worker_id: &str) -> Result<String, String> {
    let base = base.trim();
    let lower = base.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let scheme = if lower.starts_with("https://") {
            "https"
        } else {
            "http"
        };
        return Err(format!(
            "{GATEWAY_URL_ENV} must use ws:// or wss://, not {scheme}://: {base}"
        ));
    }
    if !lower.starts_with("ws://") && !lower.starts_with("wss://") {
        return Err(format!(
            "{GATEWAY_URL_ENV} must start with ws:// or wss://: {base}"
        ));
    }
    if base.contains('#') {
        return Err(format!(
            "{GATEWAY_URL_ENV} must not contain a fragment: {base}"
        ));
    }

    let (rest, query) = match base.split_once('?') {
        Some((rest, query)) => (rest, Some(query)),
        None => (base, None),
    };

    let scheme_end = rest.find("://").map(|index| index + 3).unwrap_or(0);
    let authority = &rest[scheme_end..];
    let has_path = authority.trim_end_matches('/').contains('/');
    let mut url = if has_path {
        rest.to_string()
    } else {
        format!("{}{WORKER_PATH}", rest.trim_end_matches('/'))
    };

    let has_id = query.is_some_and(|query| {
        query
            .split('&')
            .any(|pair| pair.split('=').next() == Some("id"))
    });
    let encoded_id = urlencoding::encode(worker_id);
    match query {
        Some(query) if has_id => {
            url.push('?');
            url.push_str(query);
        }
        Some("") => url.push_str(&format!("?id={encoded_id}")),
        Some(query) => url.push_str(&format!("?{query}&id={encoded_id}")),
        None => url.push_str(&format!("?id={encoded_id}")),
    }
    Ok(url)
}

/// Dial the gateway and keep the connection bridged to [`run_connection`].
///
/// Runs until the task is aborted. Every close or error is logged and retried
/// with exponential backoff (1s, 2s, 4s, 8s, 16s, capped at 30s); a connection
/// that lasted at least [`STABLE_CONNECTION`] resets the delay.
pub async fn run(state: Arc<AppState>, settings: GatewaySettings) {
    let mut backoff = INITIAL_BACKOFF;
    loop {
        match connect_and_run(Arc::clone(&state), &settings).await {
            Ok(live) => {
                tracing::info!(
                    url = %settings.url,
                    worker_id = %settings.worker_id,
                    seconds = live.as_secs(),
                    "gateway connection closed"
                );
                if live >= STABLE_CONNECTION {
                    backoff = INITIAL_BACKOFF;
                }
            }
            Err(error) => tracing::warn!(
                url = %settings.url,
                worker_id = %settings.worker_id,
                %error,
                "gateway connection failed"
            ),
        }

        tracing::info!(
            url = %settings.url,
            seconds = backoff.as_secs(),
            "reconnecting to goble-gateway"
        );
        tokio::time::sleep(backoff).await;
        backoff = next_backoff(backoff);
    }
}

fn next_backoff(current: Duration) -> Duration {
    (current * 2).min(MAX_BACKOFF)
}

/// One gateway connection: handshake, bridge, then return how long it lived.
async fn connect_and_run(
    state: Arc<AppState>,
    settings: &GatewaySettings,
) -> Result<Duration, String> {
    let mut request = settings
        .url
        .as_str()
        .into_client_request()
        .map_err(|error| format!("invalid gateway URL {}: {error}", settings.url))?;

    let bearer = HeaderValue::from_str(&format!("Bearer {}", settings.token))
        .map_err(|error| format!("invalid {WORKER_TOKEN_ENV}: {error}"))?;
    request.headers_mut().insert(AUTHORIZATION, bearer);
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static("goblin-worker"));
    let worker_header = HeaderValue::from_str(&settings.worker_id)
        .map_err(|_| "the worker id is not a valid header value".to_string())?;
    request
        .headers_mut()
        .insert("x-goble-worker", worker_header);

    let (socket, response) =
        tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(request))
            .await
            .map_err(|_| format!("connect to {} timed out", settings.url))?
            .map_err(|error| format!("connect to {}: {error}", settings.url))?;

    tracing::info!(
        url = %settings.url,
        worker_id = %settings.worker_id,
        status = response.status().as_u16(),
        "connected to goble-gateway"
    );
    let connected_at = Instant::now();

    let (mut sink, mut stream) = socket.split();
    let (incoming_tx, incoming) = mpsc::channel(FRAME_QUEUE);
    let (outgoing, mut outgoing_rx) = mpsc::channel(FRAME_QUEUE);

    let reader = ConnectionTask::spawn(async move {
        while let Some(message) = stream.next().await {
            let frame = match message {
                Ok(Message::Text(text)) => WorkerFrame::Text(text.to_string()),
                Ok(Message::Binary(bytes)) => WorkerFrame::Binary(bytes.to_vec()),
                Ok(Message::Close(_)) => WorkerFrame::Close,
                // Control frames are answered by tungstenite; raw frames never
                // surface while reading.
                Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => continue,
                Err(_) => WorkerFrame::Close,
            };
            let closed = matches!(frame, WorkerFrame::Close);
            if incoming_tx.send(frame).await.is_err() || closed {
                break;
            }
        }
    });

    let writer = ConnectionTask::spawn(async move {
        while let Some(frame) = outgoing_rx.recv().await {
            let message = match frame {
                WorkerFrame::Text(text) => Message::Text(text.into()),
                WorkerFrame::Binary(bytes) => Message::Binary(bytes.into()),
                WorkerFrame::Close => {
                    let _ = sink.send(Message::Close(None)).await;
                    break;
                }
            };
            if sink.send(message).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    run_connection(state, incoming, outgoing).await;

    reader.abort().await;
    writer.settle(CLOSE_GRACE).await;
    Ok(connected_at.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_from(
        url: Option<&str>,
        token: Option<&str>,
        worker_id: Option<&str>,
        fallback_worker_id: Option<&str>,
    ) -> Result<Option<GatewaySettings>, String> {
        GatewaySettings::from_lookup(
            |name| match name {
                GATEWAY_URL_ENV => url.map(str::to_string),
                WORKER_TOKEN_ENV => token.map(str::to_string),
                WORKER_ID_ENV => worker_id.map(str::to_string),
                FALLBACK_WORKER_ID_ENV => worker_id.or(fallback_worker_id).map(str::to_string),
                _ => None,
            },
            fallback_worker_id,
        )
    }

    #[test]
    fn a_bare_gateway_host_gets_the_worker_path_and_id() {
        assert_eq!(
            worker_endpoint("wss://gateway.example", "worker-1").unwrap(),
            "wss://gateway.example/ws/worker?id=worker-1"
        );
        assert_eq!(
            worker_endpoint("wss://gateway.example/", "worker-1").unwrap(),
            "wss://gateway.example/ws/worker?id=worker-1"
        );
        assert_eq!(
            worker_endpoint("ws://127.0.0.1:8788", "worker-1").unwrap(),
            "ws://127.0.0.1:8788/ws/worker?id=worker-1"
        );
    }

    #[test]
    fn an_existing_worker_path_is_kept() {
        assert_eq!(
            worker_endpoint("wss://gateway.example/ws/worker", "worker-1").unwrap(),
            "wss://gateway.example/ws/worker?id=worker-1"
        );
        assert_eq!(
            worker_endpoint("wss://gateway.example/mounted/worker", "worker-1").unwrap(),
            "wss://gateway.example/mounted/worker?id=worker-1"
        );
    }

    #[test]
    fn an_existing_query_is_extended_not_replaced() {
        assert_eq!(
            worker_endpoint("wss://gateway.example/ws/worker?region=eu", "worker-1").unwrap(),
            "wss://gateway.example/ws/worker?region=eu&id=worker-1"
        );
        assert_eq!(
            worker_endpoint(
                "wss://gateway.example/ws/worker?id=already-there",
                "worker-1"
            )
            .unwrap(),
            "wss://gateway.example/ws/worker?id=already-there"
        );
    }

    #[test]
    fn the_worker_id_is_url_encoded() {
        assert_eq!(
            worker_endpoint("wss://gateway.example", "goble goble/2fbf&x=1").unwrap(),
            "wss://gateway.example/ws/worker?id=goble%20goble%2F2fbf%26x%3D1"
        );
    }

    #[test]
    fn http_urls_are_refused_with_a_hint() {
        for url in [
            "http://gateway.example",
            "https://gateway.example/ws/worker",
        ] {
            let error = worker_endpoint(url, "worker-1").unwrap_err();
            assert!(
                error.contains("ws:// or wss://"),
                "unexpected error for {url}: {error}"
            );
        }
        assert!(worker_endpoint("gateway.example", "worker-1")
            .unwrap_err()
            .contains("ws:// or wss://"));
        assert!(worker_endpoint("wss://gateway.example/#frag", "worker-1")
            .unwrap_err()
            .contains("fragment"));
    }

    #[test]
    fn a_url_without_a_token_is_an_error() {
        let error = settings_from(Some("wss://gateway.example"), None, None, None).unwrap_err();
        assert!(error.contains(WORKER_TOKEN_ENV), "{error}");
    }

    #[test]
    fn no_url_disables_the_feature_even_with_a_token() {
        assert!(settings_from(None, Some("token"), None, None)
            .unwrap()
            .is_none());
    }

    #[test]
    fn the_worker_id_falls_back_from_env_to_state_to_generated() {
        let from_env = settings_from(
            Some("wss://gateway.example"),
            Some("token"),
            Some("from-env"),
            Some("from-state"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(from_env.worker_id, "from-env");
        assert!(from_env.url.ends_with("?id=from-env"));

        let from_goblin_env = GatewaySettings::from_lookup(
            |name| match name {
                GATEWAY_URL_ENV => Some("wss://gateway.example".to_string()),
                WORKER_TOKEN_ENV => Some("token".to_string()),
                FALLBACK_WORKER_ID_ENV => Some("from-goblin-env".to_string()),
                _ => None,
            },
            Some("from-state"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(from_goblin_env.worker_id, "from-goblin-env");

        let from_state = settings_from(
            Some("wss://gateway.example"),
            Some("token"),
            None,
            Some("from-state"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(from_state.worker_id, "from-state");

        let generated = settings_from(Some("wss://gateway.example"), Some("token"), None, None)
            .unwrap()
            .unwrap();
        assert!(!generated.worker_id.is_empty());
        assert!(generated
            .url
            .contains(urlencoding::encode(&generated.worker_id).as_ref()));
    }

    #[test]
    fn backoff_doubles_up_to_the_cap() {
        let mut delay = INITIAL_BACKOFF;
        let mut seen = Vec::new();
        for _ in 0..7 {
            seen.push(delay.as_secs());
            delay = next_backoff(delay);
        }
        assert_eq!(seen, vec![1, 2, 4, 8, 16, 30, 30]);
    }
}
