//! A small typed client for a goble gateway.
//!
//! The desktop service uses this to drive Settings → Gateway: probe `/health`,
//! exchange a PIN for a session at `/api/v1/session`, list `/api/workers`, and
//! forget the session with `/logout`. It has no UI and keeps no state on disk;
//! the bearer token lives in memory only and persistence is the caller's job.
//!
//! Wire shapes (see `crates/goble-gateway/src/http.rs`):
//!
//! - `GET /health` → `{"status":"ok","workers":N}`, unauthenticated.
//! - `POST /api/v1/session` with `{"pin":"..."}` → `{"token":"...","expires_in":N}`;
//!   401 with `{"error":"wrong_pin"}` refuses the PIN, and 429 is retried with
//!   bounded backoff.
//! - `GET /api/workers` with `Authorization: Bearer <token>` →
//!   `{"workers":[{"id","connected_at","last_seen"}]}`; 401 means the session
//!   has expired or was revoked.
//! - `POST /logout` with `Authorization: Bearer <token>` destroys the session.
//!
//! URL handling follows the Settings contract: a base may be `http(s)://` or
//! `ws(s)://`, may carry a path prefix, and may end in `/ws/worker`, which is
//! stripped because the worker endpoint is not part of the base. Query strings
//! and fragments are rejected. The client WebSocket shares the same base:
//! `<base>/ws/client`.

use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, RETRY_AFTER};
use reqwest::{Response, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// How long a gateway call may take when the caller does not override it.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// How many times a PIN exchange is attempted when the gateway answers 429.
const MAX_PAIR_ATTEMPTS: u32 = 3;
/// Upper bound on an honoured `Retry-After`, so a hostile gateway cannot park
/// the desktop service for hours.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(5);

/// Typed failures the Settings page maps onto its states.
///
/// No variant carries a token, a PIN, or a request body, so an error can be
/// shown, logged, or wrapped without leaking a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GatewayError {
    /// The URL was not an acceptable gateway base, or the gateway answered
    /// with a body this build cannot understand.
    #[error("the gateway URL or its answer was invalid")]
    BadRequest,
    /// The connection could not be established (refused, DNS, TLS, ...).
    #[error("the gateway could not be reached")]
    Unreachable,
    /// The gateway refused the PIN (HTTP 401 on `/api/v1/session`).
    #[error("the gateway refused the PIN")]
    Unauthorized,
    /// The bearer session is gone (HTTP 401 on `/api/workers`).
    #[error("the gateway session expired")]
    Expired,
    /// The gateway answered with a status this client does not special-case.
    #[error("the gateway answered HTTP {0}")]
    Http(u16),
    /// The request exceeded the configured timeout.
    #[error("the gateway did not answer before the timeout")]
    Timeout,
}

/// The gateway `/health` payload.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Health {
    pub status: String,
    pub workers: usize,
}

/// A paired gateway session.
///
/// `token` is a bearer secret. `Debug` redacts it deliberately; do not add a
/// derived implementation.
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct Session {
    pub token: String,
    pub expires_in: u64,
}

impl fmt::Debug for Session {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Session")
            .field("token", &"<redacted>")
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

/// One connected worker, as returned by `/api/workers`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerSummary {
    pub id: String,
    pub connected_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

/// A validated, normalised gateway base.
///
/// `as_str()` is the HTTP origin plus the optional path prefix, with no
/// trailing slash and no `/ws/worker`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseUrl {
    url: Url,
    text: String,
    tls: bool,
}

impl BaseUrl {
    /// The normalised base, e.g. `https://gateway.example.ts.net/prefix`.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The client WebSocket URL for this gateway, e.g.
    /// `wss://gateway.example.ts.net/prefix/ws/client`.
    pub fn socket_url(&self) -> String {
        let Ok(mut url) = Url::parse(self.url.as_str()) else {
            return String::new();
        };
        let _ = url.set_scheme(if self.tls { "wss" } else { "ws" });
        let path = format!("{}/ws/client", url.path().trim_end_matches('/'));
        url.set_path(&path);
        url.set_query(None);
        url.set_fragment(None);
        url.to_string()
    }

    /// The client WebSocket URL for one worker. The id is percent-encoded.
    pub fn client_socket_url(&self, worker: &str) -> String {
        let Ok(mut url) = Url::parse(&self.socket_url()) else {
            return String::new();
        };
        url.query_pairs_mut().append_pair("worker", worker);
        url.to_string()
    }
}

impl fmt::Display for BaseUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text)
    }
}

/// Normalise a gateway URL for both HTTP calls and the client WebSocket.
///
/// Accepted forms are `http(s)://host[:port][/prefix]` and
/// `ws(s)://host[:port][/prefix][/ws/worker]`. A trailing `/ws/worker` is
/// removed; a path prefix is kept. Query strings, fragments, user info, and
/// every other scheme are rejected.
pub fn normalize_base(input: &str) -> Result<BaseUrl, GatewayError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(GatewayError::BadRequest);
    }

    let mut url = Url::parse(trimmed).map_err(|_| GatewayError::BadRequest)?;
    let (scheme, tls) = match url.scheme() {
        "http" | "ws" => ("http", false),
        "https" | "wss" => ("https", true),
        _ => return Err(GatewayError::BadRequest),
    };
    if url.host_str().is_none() {
        return Err(GatewayError::BadRequest);
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(GatewayError::BadRequest);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(GatewayError::BadRequest);
    }

    // `/ws/worker` is a worker endpoint, not part of a base. Strip it, with or
    // without a trailing slash, and keep whatever prefix precedes it.
    let without_trailing = url.path().trim_end_matches('/');
    let path = if without_trailing == "/ws/worker" {
        String::new()
    } else if let Some(prefix) = without_trailing.strip_suffix("/ws/worker") {
        prefix.to_string()
    } else {
        without_trailing.to_string()
    };

    url.set_scheme(scheme)
        .map_err(|_| GatewayError::BadRequest)?;
    url.set_path(&path);
    url.set_query(None);
    url.set_fragment(None);

    let text = url.as_str().trim_end_matches('/').to_string();
    Ok(BaseUrl { url, text, tls })
}

/// A reusable gateway client. Cloning is cheap; the underlying `reqwest`
/// client pools connections.
#[derive(Debug, Clone)]
pub struct GatewayClient {
    base: BaseUrl,
    http: reqwest::Client,
}

impl GatewayClient {
    /// Build a client with the default ten-second timeout.
    pub fn new(url: &str) -> Result<Self, GatewayError> {
        Self::with_timeout(url, DEFAULT_TIMEOUT)
    }

    /// Build a client with an explicit timeout. Callers normally use [`new`].
    pub fn with_timeout(url: &str, timeout: Duration) -> Result<Self, GatewayError> {
        let base = normalize_base(url)?;
        let http = reqwest::Client::builder()
            .timeout(timeout)
            // A gateway redirect is never a success for these calls. Not
            // following it also keeps the bearer header on the intended host.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| GatewayError::Unreachable)?;
        Ok(Self { base, http })
    }

    pub fn base(&self) -> &BaseUrl {
        &self.base
    }

    /// `GET /health`.
    pub async fn health(&self) -> Result<Health, GatewayError> {
        let response = self.send(self.http.get(self.endpoint("health"))).await?;
        let status = response.status();
        if status.is_success() {
            decode(response).await
        } else {
            Err(classify(status))
        }
    }

    /// `POST /api/v1/session` — exchange a PIN for a bearer session.
    ///
    /// A 401 is [`GatewayError::Unauthorized`] (the gateway's `wrong_pin`).
    /// A 429 is retried up to [`MAX_PAIR_ATTEMPTS`] with the `Retry-After`
    /// delay, capped by [`MAX_RETRY_AFTER`]; a persistent 429 becomes
    /// [`GatewayError::Http`].
    pub async fn pair(&self, pin: &str) -> Result<Session, GatewayError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let request = self
                .http
                .post(self.endpoint("api/v1/session"))
                .json(&PairRequest { pin });
            let response = self.send(request).await?;
            let status = response.status();

            if status.is_success() {
                return decode(response).await;
            }
            if status == StatusCode::UNAUTHORIZED {
                return Err(GatewayError::Unauthorized);
            }
            if status == StatusCode::TOO_MANY_REQUESTS && attempt < MAX_PAIR_ATTEMPTS {
                // The gateway uses this for login backoff. Honour its hint but
                // never sleep for an unbounded time or panic on a bad header.
                tokio::time::sleep(retry_delay(response.headers(), attempt)).await;
                continue;
            }
            return Err(classify(status));
        }
    }

    /// `GET /api/workers` with the session bearer.
    ///
    /// A 401 means the session was revoked, expired, or belongs to a
    /// restarted gateway; it is reported as [`GatewayError::Expired`] rather
    /// than a bare HTTP status so Settings can ask for the PIN again.
    pub async fn workers(&self, token: &str) -> Result<Vec<WorkerSummary>, GatewayError> {
        let authorization = bearer(token)?;
        let response = self
            .send(
                self.http
                    .get(self.endpoint("api/workers"))
                    .header(AUTHORIZATION, authorization),
            )
            .await?;
        let status = response.status();
        if status.is_success() {
            let body: WorkersResponse = decode(response).await?;
            Ok(body.workers)
        } else if status == StatusCode::UNAUTHORIZED {
            Err(GatewayError::Expired)
        } else {
            Err(classify(status))
        }
    }

    /// `POST /logout`, best effort.
    ///
    /// Unpair must clear local state even when the gateway is unreachable, so
    /// this method deliberately returns nothing and ignores every failure. It
    /// never logs or retries, and it never sends the token to another host.
    pub async fn logout(&self, token: &str) {
        let Ok(authorization) = bearer(token) else {
            return;
        };
        let _ = self
            .http
            .post(self.endpoint("logout"))
            .header(AUTHORIZATION, authorization)
            .send()
            .await;
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{path}", self.base.as_str())
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Response, GatewayError> {
        request.send().await.map_err(|error| {
            if error.is_timeout() {
                GatewayError::Timeout
            } else {
                GatewayError::Unreachable
            }
        })
    }
}

/// `GET /health` with the default timeout.
pub async fn health(url: &str) -> Result<Health, GatewayError> {
    GatewayClient::new(url)?.health().await
}

/// `POST /api/v1/session` with the default timeout.
pub async fn pair(url: &str, pin: &str) -> Result<Session, GatewayError> {
    GatewayClient::new(url)?.pair(pin).await
}

/// `GET /api/workers` with the default timeout.
pub async fn workers(url: &str, token: &str) -> Result<Vec<WorkerSummary>, GatewayError> {
    GatewayClient::new(url)?.workers(token).await
}

/// `POST /logout`, best effort. Invalid URLs and transport failures are ignored.
pub async fn logout(url: &str, token: &str) {
    if let Ok(client) = GatewayClient::new(url) {
        client.logout(token).await;
    }
}

#[derive(Serialize)]
struct PairRequest<'a> {
    pin: &'a str,
}

#[derive(Deserialize)]
struct WorkersResponse {
    workers: Vec<WorkerSummary>,
}

async fn decode<T: DeserializeOwned>(response: Response) -> Result<T, GatewayError> {
    response.json().await.map_err(|_| GatewayError::BadRequest)
}

fn classify(status: StatusCode) -> GatewayError {
    match status.as_u16() {
        400 => GatewayError::BadRequest,
        401 => GatewayError::Unauthorized,
        other => GatewayError::Http(other),
    }
}

fn bearer(token: &str) -> Result<HeaderValue, GatewayError> {
    HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| GatewayError::BadRequest)
}

fn retry_delay(headers: &HeaderMap, attempt: u32) -> Duration {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| Duration::from_secs(seconds).min(MAX_RETRY_AFTER))
        .unwrap_or_else(|| Duration::from_millis(250 * u64::from(attempt)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(input: &str) -> String {
        normalize_base(input).unwrap().as_str().to_string()
    }

    #[test]
    fn http_and_https_bases_keep_their_prefix() {
        assert_eq!(
            base("https://gateway.example.ts.net"),
            "https://gateway.example.ts.net"
        );
        assert_eq!(
            base("https://gateway.example.ts.net:8443/prefix"),
            "https://gateway.example.ts.net:8443/prefix"
        );
        assert_eq!(base("http://127.0.0.1:8788"), "http://127.0.0.1:8788");
        assert_eq!(
            base("https://gateway.example.ts.net/prefix/"),
            "https://gateway.example.ts.net/prefix"
        );
        assert_eq!(
            base("  https://gateway.example.ts.net  "),
            "https://gateway.example.ts.net"
        );
    }

    #[test]
    fn worker_urls_are_stripped_and_map_to_http() {
        assert_eq!(
            base("wss://gateway.example.ts.net/ws/worker"),
            "https://gateway.example.ts.net"
        );
        assert_eq!(
            base("wss://gateway.example.ts.net/prefix/ws/worker"),
            "https://gateway.example.ts.net/prefix"
        );
        assert_eq!(
            base("ws://127.0.0.1:8788/ws/worker/"),
            "http://127.0.0.1:8788"
        );
    }

    #[test]
    fn socket_urls_share_the_normalised_base() {
        let tls = normalize_base("wss://gateway.example.ts.net/prefix/ws/worker").unwrap();
        assert_eq!(
            tls.socket_url(),
            "wss://gateway.example.ts.net/prefix/ws/client"
        );
        assert_eq!(
            tls.client_socket_url("worker-a"),
            "wss://gateway.example.ts.net/prefix/ws/client?worker=worker-a"
        );

        let plain = normalize_base("http://127.0.0.1:8788").unwrap();
        assert_eq!(plain.socket_url(), "ws://127.0.0.1:8788/ws/client");
    }

    #[test]
    fn unsupported_shapes_are_bad_requests() {
        for input in [
            "",
            "   ",
            "ftp://gateway.example.ts.net",
            "file:///tmp/gateway",
            "gateway.example.ts.net:8788",
            "gateway.example.ts.net",
            "https://gateway.example.ts.net?x=1",
            "https://gateway.example.ts.net#fragment",
            "https://",
            "http://user:pass@gateway.example.ts.net",
        ] {
            assert_eq!(
                normalize_base(input).unwrap_err(),
                GatewayError::BadRequest,
                "expected {input:?} to be rejected"
            );
        }
    }

    #[test]
    fn secrets_are_redacted_in_debug_output() {
        let session = Session {
            token: "super-secret-token".into(),
            expires_in: 60,
        };
        let shown = format!("{session:?}");
        assert!(!shown.contains("super-secret-token"));
        assert!(shown.contains("<redacted>"));
    }

    #[test]
    fn retry_delay_is_bounded_and_parses_safely() {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_static("0"));
        assert_eq!(retry_delay(&headers, 1), Duration::ZERO);

        headers.insert(RETRY_AFTER, HeaderValue::from_static("9999"));
        assert_eq!(retry_delay(&headers, 1), MAX_RETRY_AFTER);

        // A non-numeric or non-UTF-8 header falls back to the attempt delay.
        headers.insert(RETRY_AFTER, HeaderValue::from_static("later"));
        assert_eq!(retry_delay(&headers, 2), Duration::from_millis(500));
    }
}
