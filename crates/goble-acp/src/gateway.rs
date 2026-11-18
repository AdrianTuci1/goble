//! HTTP/SSE client for the Goble gateway remote-execution API.
//!
//! Contract consumed (documented in `docs/gateway-acp.md`):
//!
//! ```text
//! POST /api/v1/session                 {"pin"}            -> {"token"}
//! POST /api/v1/sessions                {"name","worker"?} -> {"id","name",…}
//! POST /api/v1/sessions/{id}/runs      {"task","timeout_s","wait":false} -> {"id",…}
//! GET  /api/v1/runs/{id}                                  -> {"id","status","final_text",…}
//! GET  /api/v1/runs/{id}/stream?from_index=N              -> SSE `data: {...}`
//! POST /api/v1/sessions/{id}/cancel                       -> {"ok":true}
//! ```
//!
//! Auth is `Authorization: Bearer <token>`; the token is either configured
//! directly or paired from a PIN. Parsing is tolerant (aliases for both
//! snake_case and camelCase ids) because the gateway side of the contract is
//! implemented in parallel; the shapes are pinned by the axum mock in
//! `tests/e2e.rs`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{Mutex, Notify};

use crate::config::{AcpConfig, PAIR_TIMEOUT, REQUEST_TIMEOUT};

/// Idle wait for the next SSE chunk before treating the stream as dead and
/// reconnecting (the gateway sends `: keepalive` comments, so this only fires
/// on a genuinely stuck connection).
const STREAM_READ_TIMEOUT: Duration = Duration::from_secs(120);
/// First reconnect delay; doubles on each retry.
const RECONNECT_BASE_DELAY: Duration = Duration::from_millis(250);

/// How a settled gateway run ends, as far as the ACP turn is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunTerminal {
    /// The run finished successfully: ACP `stopReason: "end_turn"`.
    Completed,
    /// The run was cancelled: ACP `stopReason: "cancelled"`.
    Cancelled,
    /// The run failed: surfaced as a JSON-RPC error, matching DSH's behaviour
    /// for a failed turn (never a silent `end_turn`).
    Failed { message: String },
}

/// Normalised run status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    Timeout,
    Unknown(String),
}

impl RunStatus {
    pub fn from_raw(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "queued" | "pending" | "created" => Self::Queued,
            "running" | "in_progress" | "started" => Self::Running,
            "done" | "completed" | "complete" | "succeeded" | "success" | "ok" => Self::Completed,
            "failed" | "error" => Self::Failed,
            "cancelled" | "canceled" | "stopped" => Self::Cancelled,
            "timeout" | "timed_out" => Self::Timeout,
            other => Self::Unknown(other.to_string()),
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Timeout
        )
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::Unknown(raw) => raw,
        }
    }
}

/// A gateway session binding (a durable session, not a process).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GatewaySession {
    #[serde(alias = "sessionId", alias = "session_id")]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, alias = "workerId", alias = "worker_id")]
    pub worker_id: Option<String>,
    #[serde(default, alias = "threadId", alias = "thread_id")]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

/// A gateway run: the dispatch of one prompt into a session's worker.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GatewayRun {
    #[serde(alias = "runId", alias = "run_id")]
    pub id: String,
    #[serde(default, alias = "sessionId", alias = "session_id")]
    pub session_id: Option<String>,
    #[serde(default, alias = "state")]
    pub status: String,
    #[serde(default, alias = "finalText", alias = "final_text")]
    pub final_text: Option<String>,
    #[serde(default)]
    pub error: Option<Value>,
    #[serde(default)]
    pub usage: Option<Value>,
    #[serde(default, alias = "exitCode", alias = "exit_code")]
    pub exit_code: Option<i64>,
}

impl GatewayRun {
    pub fn status(&self) -> RunStatus {
        RunStatus::from_raw(&self.status)
    }

    /// The failure message, flattening string and `{"message": …}` shapes.
    pub fn error_message(&self) -> Option<String> {
        let error = self.error.as_ref()?;
        if let Some(text) = error.as_str() {
            return (!text.is_empty()).then(|| text.to_string());
        }
        if let Some(text) = error.get("message").and_then(Value::as_str) {
            return Some(text.to_string());
        }
        Some(error.to_string())
    }
}

/// Errors from the gateway API. The JSON-RPC mapping is deliberate:
/// busy → `-32000`, bad session/run → `-32002`, everything else → `-32603`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GatewayError {
    #[error("gateway transport error: {0}")]
    Transport(String),
    #[error("gateway authentication failed: {0}")]
    Unauthorized(String),
    #[error("gateway returned 404: {0}")]
    NotFound(String),
    #[error("gateway is busy: {0}")]
    Conflict(String),
    #[error("gateway returned HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("gateway protocol error: {0}")]
    Protocol(String),
}

impl GatewayError {
    /// JSON-RPC error code this gateway failure maps to.
    pub fn rpc_code(&self) -> i64 {
        match self {
            Self::Conflict(_) => -32000,
            Self::NotFound(_) => -32002,
            Self::Transport(_) | Self::Unauthorized(_) | Self::Http { .. } | Self::Protocol(_) => {
                -32603
            }
        }
    }

    /// The gateway could not be reached at all (no HTTP response).
    pub fn is_transport(&self) -> bool {
        matches!(self, Self::Transport(_))
    }
}

/// A cooperative cancellation flag for one prompt turn.
///
/// Cloning shares the flag. `cancel()` wakes whatever is awaiting
/// [`CancelToken::cancelled`], so an SSE read blocked on a silent run settles
/// immediately when the client sends `session/cancel`.
#[derive(Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Whether two handles refer to the same turn's flag.
    pub fn is_same(&self, other: &CancelToken) -> bool {
        Arc::ptr_eq(&self.flag, &other.flag)
    }

    /// Resolves once the token is cancelled (immediately if it already is).
    pub async fn cancelled(&self) {
        loop {
            if self.is_cancelled() {
                return;
            }
            let notified = self.notify.notified();
            // Registering the waiter before the second check closes the race
            // with a `cancel()` that lands between the check and the await.
            if self.is_cancelled() {
                return;
            }
            notified.await;
            if self.is_cancelled() {
                return;
            }
        }
    }
}

/// What the caller wants after seeing one streamed event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamControl {
    Continue,
    Stop,
}

/// Progress of one `stream_run` call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamProgress {
    /// Number of data events consumed, i.e. the `from_index` to resume at.
    pub events: u64,
    /// How many times the stream had to be reconnected.
    pub dropped_connections: u32,
    /// The event callback asked to stop (a terminal event was mapped).
    pub stopped: bool,
}

/// Incremental Server-Sent-Events parser for `data:` frames.
///
/// Handles arbitrary chunk boundaries, `\r\n`, `: keepalive` comments,
/// `event:`/`id:`/`retry:` fields, and multi-line `data:` payloads. Only frames
/// separated by a blank line are dispatched, per the SSE spec.
#[derive(Debug, Default)]
pub struct SseParser {
    buffer: Vec<u8>,
    data: Vec<String>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a network chunk. Returns every complete event decoded so far.
    /// A payload that is not JSON yields `Err` and is skipped; the connection is
    /// not torn down over one malformed frame.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Result<Value, String>> {
        self.buffer.extend_from_slice(chunk);
        let mut out = Vec::new();
        let mut consumed = 0usize;
        while let Some(offset) = self.buffer[consumed..]
            .iter()
            .position(|byte| *byte == b'\n')
        {
            let line_end = consumed + offset;
            let line = String::from_utf8_lossy(&self.buffer[consumed..line_end])
                .trim_end_matches('\r')
                .to_string();
            consumed = line_end + 1;
            self.process_line(&line, &mut out);
        }
        self.buffer.drain(..consumed);
        out
    }

    fn process_line(&mut self, line: &str, out: &mut Vec<Result<Value, String>>) {
        if line.is_empty() {
            return self.dispatch(out);
        }
        if line.starts_with(':') {
            // Comment frame, e.g. ": keepalive".
            return;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            let rest = rest.strip_prefix(' ').unwrap_or(rest);
            self.data.push(rest.to_string());
        }
        // `event:`, `id:`, `retry:` and unknown fields carry no payload we need
        // (the event type is inside the JSON object), so they are ignored.
    }

    fn dispatch(&mut self, out: &mut Vec<Result<Value, String>>) {
        if self.data.is_empty() {
            return;
        }
        let payload = self.data.join("\n");
        self.data.clear();
        if payload.trim().is_empty() {
            return;
        }
        match serde_json::from_str::<Value>(&payload) {
            Ok(value) => out.push(Ok(value)),
            Err(error) => out.push(Err(format!("malformed SSE data frame: {error}"))),
        }
    }
}

/// Auth strategy for the gateway.
enum Auth {
    None,
    Token(String),
    Pin(String),
}

/// HTTP client for one gateway.
pub struct GatewayClient {
    http: reqwest::Client,
    base: String,
    auth: Auth,
    cached_token: Mutex<Option<String>>,
    reconnect_attempts: u32,
}

impl GatewayClient {
    pub fn new(config: &AcpConfig) -> Result<Self, GatewayError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("goble-acp/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| GatewayError::Transport(error.to_string()))?;
        let auth = match (&config.token, &config.pin) {
            (Some(token), _) => Auth::Token(token.clone()),
            (None, Some(pin)) => Auth::Pin(pin.clone()),
            (None, None) => Auth::None,
        };
        Ok(Self {
            http,
            base: config.gateway_url.trim_end_matches('/').to_string(),
            auth,
            cached_token: Mutex::new(None),
            reconnect_attempts: config.reconnect_attempts,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// The bearer token to use, pairing with the configured PIN if needed.
    async fn current_token(&self) -> Result<Option<String>, GatewayError> {
        match &self.auth {
            Auth::None => Ok(None),
            Auth::Token(token) => Ok(Some(token.clone())),
            Auth::Pin(pin) => {
                let mut cached = self.cached_token.lock().await;
                if let Some(token) = cached.as_ref() {
                    return Ok(Some(token.clone()));
                }
                let token = self.pair(pin).await?;
                *cached = Some(token.clone());
                Ok(Some(token))
            }
        }
    }

    async fn invalidate_token(&self) {
        *self.cached_token.lock().await = None;
    }

    /// Exchange a PIN for a bearer token via `POST /api/v1/session`.
    async fn pair(&self, pin: &str) -> Result<String, GatewayError> {
        let response = self
            .http
            .post(self.url("/api/v1/session"))
            .timeout(PAIR_TIMEOUT)
            .json(&json!({ "pin": pin }))
            .send()
            .await
            .map_err(|error| GatewayError::Transport(error.to_string()))?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(GatewayError::Unauthorized(format!(
                "PIN pairing rejected ({status})"
            )));
        }
        if !status.is_success() {
            return Err(GatewayError::Http {
                status: status.as_u16(),
                body: clean_error_body(&body),
            });
        }
        let value: Value = serde_json::from_str(&body).map_err(|error| {
            GatewayError::Protocol(format!("pairing response is not JSON: {error}"))
        })?;
        token_from_value(&value).ok_or_else(|| {
            GatewayError::Protocol("pairing response has no token field".to_string())
        })
    }

    /// Send a JSON request and decode a JSON response, retrying once through a
    /// fresh PIN pairing when the cached token expired (`401`).
    async fn send_json(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, GatewayError> {
        let mut repairement_attempted = false;
        loop {
            let token = self.current_token().await?;
            let mut request = self
                .http
                .request(method.clone(), self.url(path))
                .timeout(REQUEST_TIMEOUT);
            if let Some(token) = token.as_deref() {
                request = request.bearer_auth(token);
            }
            if let Some(body) = body.as_ref() {
                request = request.json(body);
            }
            let response = request
                .send()
                .await
                .map_err(|error| GatewayError::Transport(error.to_string()))?;
            let status = response.status();
            let text = response
                .text()
                .await
                .map_err(|error| GatewayError::Transport(error.to_string()))?;
            if status.is_success() {
                if text.trim().is_empty() {
                    return Ok(Value::Null);
                }
                return serde_json::from_str(&text).map_err(|error| {
                    GatewayError::Protocol(format!("response is not JSON: {error}"))
                });
            }
            if status == reqwest::StatusCode::UNAUTHORIZED {
                if matches!(self.auth, Auth::Pin(_)) && !repairement_attempted {
                    repairement_attempted = true;
                    self.invalidate_token().await;
                    continue;
                }
                return Err(GatewayError::Unauthorized(clean_error_body(&text)));
            }
            return Err(status_error(status.as_u16(), clean_error_body(&text)));
        }
    }

    /// Create or reuse a gateway session by name.
    pub async fn create_session(
        &self,
        name: &str,
        worker: Option<&str>,
    ) -> Result<GatewaySession, GatewayError> {
        let mut body = json!({ "name": name });
        if let Some(worker) = worker {
            body["worker"] = json!(worker);
        }
        let value = self
            .send_json(reqwest::Method::POST, "/api/v1/sessions", Some(body))
            .await?;
        serde_json::from_value(value)
            .map_err(|error| GatewayError::Protocol(format!("invalid session object: {error}")))
    }

    /// Dispatch a prompt as a run into a gateway session.
    pub async fn create_run(
        &self,
        session_id: &str,
        task: &str,
        timeout_s: u64,
    ) -> Result<GatewayRun, GatewayError> {
        let path = format!("/api/v1/sessions/{}/runs", encode_segment(session_id));
        let body = json!({
            "task": task,
            "timeout_s": timeout_s,
            "wait": false,
        });
        let value = self
            .send_json(reqwest::Method::POST, &path, Some(body))
            .await?;
        serde_json::from_value(value)
            .map_err(|error| GatewayError::Protocol(format!("invalid run object: {error}")))
    }

    /// Read the current state of a run.
    pub async fn get_run(&self, run_id: &str) -> Result<GatewayRun, GatewayError> {
        let path = format!("/api/v1/runs/{}", encode_segment(run_id));
        let value = self.send_json(reqwest::Method::GET, &path, None).await?;
        serde_json::from_value(value)
            .map_err(|error| GatewayError::Protocol(format!("invalid run object: {error}")))
    }

    /// Ask the gateway to cancel the session's running run. Best effort: the
    /// caller treats any failure as "the remote run may still continue".
    pub async fn cancel_session(&self, session_id: &str) -> Result<Value, GatewayError> {
        let path = format!("/api/v1/sessions/{}/cancel", encode_segment(session_id));
        self.send_json(reqwest::Method::POST, &path, Some(json!({})))
            .await
    }

    /// Open the SSE stream at `from_index`, reading `data:` frames until the
    /// callback stops, the stream ends, or the token is cancelled.
    pub async fn stream_run<F>(
        &self,
        run_id: &str,
        from_index: u64,
        cancel: &CancelToken,
        mut on_event: F,
    ) -> Result<StreamProgress, GatewayError>
    where
        F: FnMut(&Value) -> StreamControl,
    {
        let mut progress = StreamProgress {
            events: from_index,
            ..StreamProgress::default()
        };
        let mut attempts = 0u32;
        loop {
            let mut response = match self.open_stream(run_id, progress.events).await {
                Ok(response) => response,
                Err(GatewayError::NotFound(message)) => {
                    return Err(GatewayError::NotFound(message))
                }
                Err(error) => {
                    if cancel.is_cancelled() {
                        return Ok(progress);
                    }
                    if attempts >= self.reconnect_attempts {
                        return Err(error);
                    }
                    attempts += 1;
                    if !self.reconnect_delay(attempts, cancel).await {
                        return Ok(progress);
                    }
                    continue;
                }
            };

            let mut parser = SseParser::new();
            // Read frames until EOF, transport error, idle timeout, or cancel.
            'read: loop {
                let chunk = tokio::select! {
                    biased;
                    _ = cancel.cancelled() => return Ok(progress),
                    timed = tokio::time::timeout(STREAM_READ_TIMEOUT, response.chunk()) => {
                        match timed {
                            Err(_) => {
                                tracing::warn!(
                                    "gateway stream for run {run_id} sent nothing for {}s; reconnecting",
                                    STREAM_READ_TIMEOUT.as_secs()
                                );
                                break 'read;
                            }
                            Ok(chunk) => chunk,
                        }
                    }
                };
                match chunk {
                    Ok(Some(bytes)) => {
                        for frame in parser.push(&bytes) {
                            match frame {
                                Ok(event) => {
                                    // The gateway's data events carry a globally
                                    // contiguous `index`, so resume from
                                    // `last_index + 1`. Fall back to counting
                                    // for gateways that omit the field.
                                    progress.events = event
                                        .get("index")
                                        .and_then(Value::as_u64)
                                        .map_or(progress.events + 1, |index| index + 1);
                                    if on_event(&event) == StreamControl::Stop {
                                        progress.stopped = true;
                                        return Ok(progress);
                                    }
                                }
                                Err(message) => tracing::warn!("{message}"),
                            }
                        }
                    }
                    Ok(None) => break 'read,
                    Err(error) => {
                        tracing::warn!("gateway stream for run {run_id} dropped: {error}");
                        break 'read;
                    }
                }
            }

            if cancel.is_cancelled() {
                return Ok(progress);
            }
            if attempts >= self.reconnect_attempts {
                // Give the caller a chance to settle from `GET /api/v1/runs`.
                return Ok(progress);
            }
            attempts += 1;
            progress.dropped_connections += 1;
            if !self.reconnect_delay(attempts, cancel).await {
                return Ok(progress);
            }
        }
    }

    async fn open_stream(
        &self,
        run_id: &str,
        from_index: u64,
    ) -> Result<reqwest::Response, GatewayError> {
        let token = self.current_token().await?;
        let url = self.url(&format!(
            "/api/v1/runs/{}/stream?from_index={}",
            encode_segment(run_id),
            from_index
        ));
        let mut request = self.http.get(url);
        if let Some(token) = token.as_deref() {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .map_err(|error| GatewayError::Transport(error.to_string()))?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let body = response.text().await.unwrap_or_default();
        Err(status_error(status.as_u16(), clean_error_body(&body)))
    }

    /// Sleep before reconnecting; returns false if the token was cancelled.
    async fn reconnect_delay(&self, attempt: u32, cancel: &CancelToken) -> bool {
        let factor = 1u64 << (attempt - 1).min(3);
        let delay = RECONNECT_BASE_DELAY * u32::try_from(factor).unwrap_or(8);
        tokio::select! {
            _ = cancel.cancelled() => false,
            _ = tokio::time::sleep(delay) => true,
        }
    }
}

fn status_error(status: u16, body: String) -> GatewayError {
    match status {
        401 | 403 => GatewayError::Unauthorized(body),
        404 => GatewayError::NotFound(body),
        409 => GatewayError::Conflict(body),
        _ => GatewayError::Http { status, body },
    }
}

/// Keep gateway error bodies short and JSON `detail`-style useful.
fn clean_error_body(body: &str) -> String {
    let trimmed = body.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        // `message` before `error`: the gateway's errors are
        // `{"error": "<code>", "message": "<human text>"}`.
        for key in ["detail", "message", "error"] {
            if let Some(text) = value.get(key).and_then(Value::as_str) {
                return text.to_string();
            }
        }
        return trimmed.chars().take(300).collect();
    }
    trimmed.chars().take(300).collect()
}

fn token_from_value(value: &Value) -> Option<String> {
    for key in ["token", "access_token", "accessToken", "session_token"] {
        if let Some(token) = value.get(key).and_then(Value::as_str) {
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Percent-encode a path segment so gateway ids with unusual characters cannot
/// break the URL.
fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_parser_handles_comments_and_crlf() {
        let mut parser = SseParser::new();
        let frames =
            parser.push(b": keepalive\r\ndata: {\"type\":\"text\",\"text\":\"hi\"}\r\n\r\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(
            frames[0].as_ref().unwrap(),
            &json!({"type": "text", "text": "hi"})
        );
    }

    #[test]
    fn sse_parser_handles_arbitrary_chunk_boundaries() {
        let wire = "data: {\"type\":\"text\",\"text\":\"hello world\"}\n\ndata: {\"type\":\"end\",\"status\":\"done\"}\n\n";
        for split in 0..wire.len() {
            let mut parser = SseParser::new();
            let mut frames = Vec::new();
            frames.extend(parser.push(&wire.as_bytes()[..split]));
            frames.extend(parser.push(&wire.as_bytes()[split..]));
            assert_eq!(frames.len(), 2, "split at {split}");
            assert_eq!(
                frames[1].as_ref().unwrap(),
                &json!({"type": "end", "status": "done"})
            );
        }
    }

    #[test]
    fn sse_parser_joins_multiline_data_and_skips_malformed_json() {
        let mut parser = SseParser::new();
        let frames = parser.push(b"data: {\"type\":\ndata: \"text\"}\n\ndata: not json\n\n");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].as_ref().unwrap(), &json!({"type": "text"}));
        assert!(frames[1].is_err());
    }

    #[test]
    fn run_status_normalisation_accepts_gateway_and_reference_spelling() {
        assert_eq!(RunStatus::from_raw("done"), RunStatus::Completed);
        assert_eq!(RunStatus::from_raw("completed"), RunStatus::Completed);
        assert_eq!(RunStatus::from_raw("failed"), RunStatus::Failed);
        assert_eq!(RunStatus::from_raw("cancelled"), RunStatus::Cancelled);
        assert_eq!(RunStatus::from_raw("running"), RunStatus::Running);
        assert_eq!(RunStatus::from_raw("queued"), RunStatus::Queued);
        assert!(RunStatus::from_raw("timeout").is_terminal());
        assert_eq!(
            RunStatus::from_raw("weird"),
            RunStatus::Unknown("weird".to_string())
        );
    }

    #[test]
    fn gateway_errors_map_to_json_rpc_codes() {
        assert_eq!(GatewayError::Conflict("busy".into()).rpc_code(), -32000);
        assert_eq!(GatewayError::NotFound("gone".into()).rpc_code(), -32002);
        assert_eq!(GatewayError::Transport("refused".into()).rpc_code(), -32603);
        assert_eq!(
            GatewayError::Unauthorized("no token".into()).rpc_code(),
            -32603
        );
    }

    #[test]
    fn run_error_shapes_flatten_to_a_message() {
        let run: GatewayRun = serde_json::from_value(json!({
            "id": "r-1",
            "status": "failed",
            "error": {"message": "sandbox died", "code": "E_SANDBOX"},
        }))
        .unwrap();
        assert_eq!(run.error_message().as_deref(), Some("sandbox died"));

        let run: GatewayRun =
            serde_json::from_value(json!({"id": "r-2", "status": "failed", "error": "boom"}))
                .unwrap();
        assert_eq!(run.error_message().as_deref(), Some("boom"));
    }

    #[test]
    fn session_accepts_snake_and_camel_ids() {
        let session: GatewaySession =
            serde_json::from_value(json!({"id": "s-1", "name": "acp/x/y"})).unwrap();
        assert_eq!(session.id, "s-1");
        let session: GatewaySession = serde_json::from_value(json!({"sessionId": "s-2"})).unwrap();
        assert_eq!(session.id, "s-2");
        let session: GatewaySession = serde_json::from_value(json!({"session_id": "s-3"})).unwrap();
        assert_eq!(session.id, "s-3");
    }

    #[tokio::test]
    async fn cancel_token_wakes_a_waiter() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        let waiter = token.clone();
        let handle = tokio::spawn(async move {
            waiter.cancelled().await;
            true
        });
        token.cancel();
        assert!(handle.await.unwrap());
    }

    #[test]
    fn url_segments_are_percent_encoded() {
        assert_eq!(encode_segment("s-abc_123"), "s-abc_123");
        assert_eq!(encode_segment("a/b c"), "a%2Fb%20c");
    }
}
