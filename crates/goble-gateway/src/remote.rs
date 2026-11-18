//! Remote-execution control plane for the gateway.
//!
//! The relay in [`crate::ws`] copies frames between browsers and workers
//! without parsing them. This module adds the second, smaller job the gateway
//! needs to be a control plane as well:
//!
//! - **session bindings**: a stable name maps to a worker id and to the thread
//!   (conversation) the worker stores turns under. Bindings live in a JSON file
//!   in the gateway's data directory so they survive a restart; runs do not.
//! - **runs**: `POST /api/v1/sessions/{id}/runs` sends one
//!   `run_agent_for_thread_reply` frame to the session's worker through the
//!   worker's normal outbound channel — exactly the frame a browser client
//!   would relay — and remembers the `trace_id` it used.
//! - **frame correlation**: every text frame a worker sends is parsed just far
//!   enough to read its `kind` and `trace_id`. A frame that names a live run is
//!   stored as a control-plane event (assistant text deltas, tool calls, tool
//!   results, status, usage) in addition to being relayed to the clients that
//!   were already listening.
//! - **settling**: an `agent_finished` frame (or a timeout, a cancel, or the
//!   worker's socket going away) ends the run. A run never stays `running` in
//!   the store without a live worker behind it.
//!
//! The control plane never opens a second worker socket: the gateway already
//! has one connection per worker, and the outbound channel is shared by every
//! attached client, so the session binding simply uses it.
//!
//! ## Storage
//!
//! Sessions are persisted as `sessions.json` in the gateway's data directory
//! (`--data-dir`, default `~/.goble/gateway`). The file is written atomically
//! (temporary file plus rename) after every mutation. Runs and their event logs
//! are process-local and capped, so a gateway restart leaves sessions idle with
//! their threads intact and answers `404 unknown_run` for old run ids.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ws::Message;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, SecondsFormat, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::watch;

use crate::auth;
use crate::http::json_unauthorized;
use crate::state::GatewayState;

/// A run that is not given an explicit timeout settles after ten minutes.
pub(crate) const DEFAULT_RUN_TIMEOUT_S: u64 = 600;
/// Upper bound on a caller-supplied `timeout_s`.
pub(crate) const MAX_RUN_TIMEOUT_S: u64 = 3600;
/// Largest accepted `task` string.
pub(crate) const MAX_TASK_BYTES: usize = 256 * 1024;
/// How long a `wait: true` request waits for a busy session to free up.
const QUEUE_WINDOW: Duration = Duration::from_secs(30);
/// How long a finished-but-unsuccessful run history is kept in memory.
const MAX_RUNS: usize = 256;
/// The worker sends `thread_agent_reply` after `agent_finished`; hold the run
/// open briefly so the reply becomes the run's `final_text`.
const SETTLE_GRACE: Duration = Duration::from_millis(400);
/// SSE keepalive cadence while a run has no new events.
const SSE_KEEPALIVE: Duration = Duration::from_secs(15);
/// File under the data directory that holds the session bindings.
const SESSIONS_FILE: &str = "sessions.json";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// An error a control-plane handler can answer with. The JSON shape matches the
/// rest of the gateway: `{"error": "<code>", "message": "<detail>"}`.
#[derive(Debug)]
pub(crate) enum ApiError {
    BadRequest(String),
    NotFound(&'static str, String),
    Conflict(&'static str, String),
    Service(&'static str, String),
}

impl ApiError {
    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    pub(crate) fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self::NotFound(code, message.into())
    }

    pub(crate) fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::Conflict(code, message.into())
    }

    pub(crate) fn service(code: &'static str, message: impl Into<String>) -> Self {
        Self::Service(code, message.into())
    }

    fn response(self) -> Response {
        let (status, code, message) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            Self::NotFound(code, message) => (StatusCode::NOT_FOUND, code, message),
            Self::Conflict(code, message) => (StatusCode::CONFLICT, code, message),
            Self::Service(code, message) => (StatusCode::SERVICE_UNAVAILABLE, code, message),
        };
        json_response(status, json!({ "error": code, "message": message }))
    }
}

impl From<ApiError> for Response {
    fn from(error: ApiError) -> Self {
        error.response()
    }
}

fn json_response(status: StatusCode, value: Value) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        value.to_string(),
    )
        .into_response()
}

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn rfc3339(timestamp: DateTime<Utc>) -> String {
    timestamp.to_rfc3339_opts(SecondsFormat::Millis, true)
}

// ---------------------------------------------------------------------------
// Views and persisted data
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct SessionView {
    pub id: String,
    pub name: Option<String>,
    pub worker_id: String,
    pub worker_connected: bool,
    pub thread_id: String,
    pub agent_id: String,
    pub status: &'static str,
    pub active_run_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub run_count: u64,
    pub last_run_id: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct CreateSessionResponse {
    #[serde(flatten)]
    pub session: SessionView,
    pub reused: bool,
}

#[derive(Serialize)]
pub(crate) struct SessionList {
    pub sessions: Vec<SessionView>,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredSession {
    id: String,
    name: Option<String>,
    worker_id: String,
    thread_id: String,
    agent_id: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    #[serde(default)]
    run_count: u64,
    #[serde(default)]
    last_run_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct StoredSessions {
    version: u32,
    sessions: Vec<StoredSession>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Timeout,
}

impl RunStatus {
    pub(crate) fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Timeout
        )
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
        }
    }
}

#[derive(Serialize)]
pub(crate) struct RunView {
    pub id: String,
    pub session_id: String,
    pub worker_id: String,
    pub thread_id: String,
    pub trace_id: String,
    pub task: String,
    pub status: RunStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub final_text: Option<String>,
    pub error: Option<String>,
    pub usage: Option<Value>,
    pub events_count: usize,
}

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

struct RunInner {
    status: RunStatus,
    events: Vec<Value>,
    /// Assistant text accumulated from `assistant_delta` frames. Used as
    /// `final_text` when the worker never sends `thread_agent_reply`.
    text: String,
    final_text: Option<String>,
    error: Option<String>,
    usage: Option<Value>,
    finished_at: Option<DateTime<Utc>>,
    /// An `agent_finished: Success` arrived and the reply is still on its way.
    settling: bool,
    /// Tool names by call id, so a result can name the call it belongs to.
    tools: HashMap<String, String>,
}

impl RunInner {
    fn new() -> Self {
        Self {
            status: RunStatus::Running,
            events: Vec::new(),
            text: String::new(),
            final_text: None,
            error: None,
            usage: None,
            finished_at: None,
            settling: false,
            tools: HashMap::new(),
        }
    }

    /// Append an event, assigning its index and timestamp. Returns the index.
    fn push(&mut self, mut event: Value, timestamp: DateTime<Utc>) -> usize {
        let index = self.events.len();
        if let Value::Object(object) = &mut event {
            object.insert("index".to_string(), json!(index));
            object.insert("at".to_string(), json!(rfc3339(timestamp)));
            match object.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = object.get("text").and_then(Value::as_str) {
                        self.text.push_str(text);
                    }
                }
                Some("usage") => {
                    self.usage = Some(event.clone());
                }
                _ => {}
            }
        }
        self.events.push(event);
        index
    }
}

pub(crate) struct Run {
    pub(crate) id: String,
    pub(crate) session_id: String,
    pub(crate) worker_id: String,
    pub(crate) thread_id: String,
    pub(crate) trace_id: String,
    pub(crate) task: String,
    pub(crate) timeout_s: u64,
    created_at: DateTime<Utc>,
    started_at: DateTime<Utc>,
    inner: Mutex<RunInner>,
    /// Bumped every time an event is appended; lets SSE subscribers and
    /// `wait: true` callers wait without polling.
    events_tx: watch::Sender<u64>,
}

impl Run {
    fn new(
        id: String,
        session: &StoredSession,
        task: String,
        timeout_s: u64,
        trace_id: String,
    ) -> Self {
        let started_at = now();
        let (events_tx, _) = watch::channel(0u64);
        Self {
            id,
            session_id: session.id.clone(),
            worker_id: session.worker_id.clone(),
            thread_id: session.thread_id.clone(),
            trace_id,
            task,
            timeout_s,
            created_at: started_at,
            started_at,
            inner: Mutex::new(RunInner::new()),
            events_tx,
        }
    }

    pub(crate) fn is_terminal(&self) -> bool {
        self.inner.lock().status.terminal()
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<u64> {
        self.events_tx.subscribe()
    }

    pub(crate) fn events_from(&self, from: usize) -> Vec<Value> {
        let inner = self.inner.lock();
        inner.events.iter().skip(from).cloned().collect()
    }

    fn event_count(&self) -> usize {
        self.inner.lock().events.len()
    }

    pub(crate) fn view(&self) -> RunView {
        let inner = self.inner.lock();
        RunView {
            id: self.id.clone(),
            session_id: self.session_id.clone(),
            worker_id: self.worker_id.clone(),
            thread_id: self.thread_id.clone(),
            trace_id: self.trace_id.clone(),
            task: self.task.clone(),
            status: inner.status,
            created_at: self.created_at,
            started_at: self.started_at,
            finished_at: inner.finished_at,
            final_text: inner.final_text.clone(),
            error: inner.error.clone(),
            usage: inner.usage.clone(),
            events_count: inner.events.len(),
        }
    }

    fn push_event(&self, event: Value) -> usize {
        let index = {
            let mut inner = self.inner.lock();
            inner.push(event, now())
        };
        let _ = self.events_tx.send(index as u64 + 1);
        index
    }

    fn push_text(&self, delta: &str) {
        self.push_event(json!({ "type": "text", "text": delta }));
    }

    fn push_thinking(&self, delta: &str) {
        self.push_event(json!({ "type": "thinking", "text": delta }));
    }

    fn tool_started(&self, call_id: &str, name: &str, arguments: &Value) {
        {
            let mut inner = self.inner.lock();
            inner.tools.insert(call_id.to_string(), name.to_string());
        }
        self.push_event(json!({
            "type": "tool_call",
            "callId": call_id,
            "tool": name,
            "input": arguments,
        }));
    }

    fn tool_finished(&self, call_id: &str, result: &str) {
        let tool = self.tool_name(call_id);
        self.push_event(json!({
            "type": "tool_result",
            "callId": call_id,
            "tool": tool,
            "status": "completed",
            "result": result,
        }));
    }

    fn tool_failed(&self, call_id: &str, message: &str) {
        let tool = self.tool_name(call_id);
        self.push_event(json!({
            "type": "tool_result",
            "callId": call_id,
            "tool": tool,
            "status": "failed",
            "result": message,
        }));
    }

    fn tool_name(&self, call_id: &str) -> Option<String> {
        self.inner.lock().tools.get(call_id).cloned()
    }

    fn set_final_text(&self, text: &str) {
        let mut inner = self.inner.lock();
        if !text.is_empty() {
            inner.final_text = Some(text.to_string());
        }
    }

    /// Note that a successful `agent_finished` arrived and the reply is still
    /// to come. Returns `false` when a settle is already in progress or done.
    fn begin_settle_grace(&self) -> bool {
        let mut inner = self.inner.lock();
        if inner.status.terminal() || inner.settling {
            return false;
        }
        inner.settling = true;
        true
    }

    /// Settle the run exactly once. Returns whether this call was the one that
    /// ended it.
    pub(crate) fn settle(
        &self,
        status: RunStatus,
        final_text: Option<String>,
        error: Option<String>,
    ) -> bool {
        let mut inner = self.inner.lock();
        if inner.status.terminal() {
            return false;
        }
        let finished_at = now();
        inner.status = status;
        inner.finished_at = Some(finished_at);
        inner.error = error.clone();

        let text = final_text.or_else(|| inner.final_text.clone()).or_else(|| {
            let accumulated = inner.text.trim();
            if accumulated.is_empty() {
                None
            } else {
                Some(inner.text.clone())
            }
        });
        inner.final_text = text.clone();

        inner.push(
            json!({ "type": "status", "status": status.as_str() }),
            finished_at,
        );
        let reason = match status {
            RunStatus::Succeeded => json!({ "kind": "completed" }),
            RunStatus::Cancelled => json!({ "kind": "cancelled" }),
            _ => json!({
                "kind": "error",
                "error": {
                    "message": error.clone().unwrap_or_else(|| status.as_str().to_string()),
                },
            }),
        };
        inner.push(
            json!({ "type": "final", "reason": reason, "text": text }),
            finished_at,
        );
        let version = inner.events.len() as u64;
        drop(inner);
        let _ = self.events_tx.send(version);
        true
    }

    /// Wait until the run settles (or its settlement channel closes).
    pub(crate) async fn wait_terminal(&self) {
        let mut rx = self.events_tx.subscribe();
        loop {
            if self.is_terminal() {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Session store
// ---------------------------------------------------------------------------

#[derive(Default)]
struct RemoteInner {
    sessions: BTreeMap<String, StoredSession>,
    names: HashMap<String, String>,
    /// session id -> run id currently holding the session's single run slot.
    active: HashMap<String, String>,
    runs: HashMap<String, Arc<Run>>,
    trace_runs: HashMap<String, String>,
    order: VecDeque<String>,
}

pub(crate) struct RemoteState {
    data_dir: Option<PathBuf>,
    inner: Mutex<RemoteInner>,
}

/// What [`RemoteState::cancel`] hands back so the caller can also tell the
/// worker to stop the turn.
pub(crate) struct CancelTarget {
    pub run: Arc<Run>,
    pub worker_id: String,
    pub trace_id: String,
}

/// The per-session fields a dispatch needs to build its frame.
pub(crate) struct SessionDispatch {
    pub session_name: Option<String>,
    pub worker_id: String,
    pub thread_id: String,
    pub agent_id: String,
    pub created_at: DateTime<Utc>,
}

enum FrameOutcome {
    Ignored,
    Settle {
        run: Arc<Run>,
        status: RunStatus,
        error: Option<String>,
    },
    Grace {
        run: Arc<Run>,
    },
}

impl RemoteState {
    pub(crate) fn new(data_dir: Option<PathBuf>) -> Self {
        let mut inner = RemoteInner::default();
        if let Some(dir) = &data_dir {
            match load_sessions(dir) {
                Ok(Some(stored)) => {
                    for session in stored.sessions {
                        if let Some(name) = &session.name {
                            inner.names.insert(name.clone(), session.id.clone());
                        }
                        inner.sessions.insert(session.id.clone(), session);
                    }
                    tracing::info!(
                        "control plane: restored {} session binding(s) from {}",
                        inner.sessions.len(),
                        dir.join(SESSIONS_FILE).display()
                    );
                }
                Ok(None) => {}
                Err(error) => tracing::warn!(
                    "control plane: could not read {}: {error}; starting with no session bindings",
                    dir.join(SESSIONS_FILE).display()
                ),
            }
        }
        Self {
            data_dir,
            inner: Mutex::new(inner),
        }
    }

    fn persist_locked(&self, inner: &RemoteInner) {
        let Some(dir) = &self.data_dir else {
            return;
        };
        let stored = StoredSessions {
            version: 1,
            sessions: inner.sessions.values().cloned().collect(),
        };
        if let Err(error) = write_sessions(dir, &stored) {
            tracing::warn!(
                "control plane: could not write {}: {error}",
                dir.join(SESSIONS_FILE).display()
            );
        }
    }

    pub(crate) fn create_or_reuse(
        &self,
        name: Option<String>,
        worker: Option<String>,
        resolve_worker: impl FnOnce() -> Result<String, ApiError>,
        connected: &[String],
    ) -> Result<(SessionView, bool), ApiError> {
        let mut inner = self.inner.lock();

        if let Some(name) = &name {
            if let Some(id) = inner.names.get(name).cloned() {
                let Some(mut session) = inner.sessions.get(&id).cloned() else {
                    return Err(ApiError::not_found(
                        "unknown_session",
                        format!("session binding {id} is gone"),
                    ));
                };
                if let Some(requested) = worker.as_deref().map(str::trim) {
                    if !requested.is_empty() && requested != session.worker_id {
                        if let Some(run_id) = inner.active.get(&id) {
                            return Err(ApiError::conflict(
                                "session_busy",
                                format!(
                                    "session {id} is running run {run_id}; wait for it to finish \
                                     before rebinding its worker"
                                ),
                            ));
                        }
                        session.worker_id = requested.to_string();
                        session.updated_at = now();
                        inner.sessions.insert(id.clone(), session.clone());
                        self.persist_locked(&inner);
                    }
                }
                let view = session_view(&inner, &session, connected);
                return Ok((view, true));
            }
        }

        let worker_id = match worker.map(|worker| worker.trim().to_string()) {
            Some(worker) if !worker.is_empty() => worker,
            _ => resolve_worker()?,
        };

        let timestamp = now();
        let session = StoredSession {
            id: format!("ses_{}", auth::generate_token()),
            name: name.clone(),
            worker_id,
            thread_id: format!("thr_{}", auth::generate_token()),
            agent_id: format!("agent_{}", auth::generate_token()),
            created_at: timestamp,
            updated_at: timestamp,
            run_count: 0,
            last_run_id: None,
        };
        if let Some(name) = &name {
            inner.names.insert(name.clone(), session.id.clone());
        }
        inner.sessions.insert(session.id.clone(), session.clone());
        self.persist_locked(&inner);
        let view = session_view(&inner, &session, connected);
        Ok((view, false))
    }

    pub(crate) fn list_sessions(&self, connected: &[String]) -> Vec<SessionView> {
        let inner = self.inner.lock();
        let mut sessions: Vec<SessionView> = inner
            .sessions
            .values()
            .map(|session| session_view(&inner, session, connected))
            .collect();
        sessions.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then(left.id.cmp(&right.id))
        });
        sessions
    }

    pub(crate) fn get_session(&self, id: &str, connected: &[String]) -> Option<SessionView> {
        let inner = self.inner.lock();
        inner
            .sessions
            .get(id)
            .map(|session| session_view(&inner, session, connected))
    }

    fn active_run_id(&self, session_id: &str) -> Option<String> {
        self.inner.lock().active.get(session_id).cloned()
    }

    /// Reserve the session's single run slot and record the run. The caller
    /// still has to send the frame; if that fails it must settle the run.
    pub(crate) fn begin_run(
        &self,
        session_id: &str,
        task: String,
        timeout_s: u64,
    ) -> Result<(Arc<Run>, SessionDispatch), ApiError> {
        let mut inner = self.inner.lock();
        let Some(session) = inner.sessions.get(session_id).cloned() else {
            return Err(ApiError::not_found(
                "unknown_session",
                format!("no session {session_id}"),
            ));
        };
        if let Some(run_id) = inner.active.get(session_id) {
            return Err(ApiError::conflict(
                "session_busy",
                format!("session {session_id} is already running run {run_id}"),
            ));
        }

        let id = format!("run_{}", auth::generate_token());
        let trace_id = format!("{}-{id}", session.thread_id);
        let run = Arc::new(Run::new(
            id.clone(),
            &session,
            task,
            timeout_s,
            trace_id.clone(),
        ));
        run.push_event(json!({ "type": "status", "status": "running" }));

        inner.active.insert(session_id.to_string(), id.clone());
        if let Some(stored) = inner.sessions.get_mut(session_id) {
            stored.run_count += 1;
            stored.last_run_id = Some(id.clone());
            stored.updated_at = now();
        }
        inner.runs.insert(id.clone(), run.clone());
        inner.trace_runs.insert(trace_id, id.clone());
        inner.order.push_back(id);
        inner.prune();
        self.persist_locked(&inner);

        Ok((
            run,
            SessionDispatch {
                session_name: session.name,
                worker_id: session.worker_id,
                thread_id: session.thread_id,
                agent_id: session.agent_id,
                created_at: session.created_at,
            },
        ))
    }

    /// Settle a run and free its session's slot. Returns `false` when another
    /// path already settled it.
    pub(crate) fn finish_run(
        &self,
        run: &Arc<Run>,
        status: RunStatus,
        final_text: Option<String>,
        error: Option<String>,
    ) -> bool {
        if !run.settle(status, final_text, error) {
            return false;
        }
        let mut inner = self.inner.lock();
        if inner
            .active
            .get(&run.session_id)
            .map(|run_id| run_id == &run.id)
            .unwrap_or(false)
        {
            inner.active.remove(&run.session_id);
        }
        inner.trace_runs.remove(&run.trace_id);
        if let Some(session) = inner.sessions.get_mut(&run.session_id) {
            session.updated_at = now();
        }
        self.persist_locked(&inner);
        true
    }

    /// Settle a run as successful if its grace window elapsed without one.
    pub(crate) fn finish_run_if_pending(&self, run: &Arc<Run>) {
        if !run.is_terminal() {
            self.finish_run(run, RunStatus::Succeeded, None, None);
        }
    }

    pub(crate) fn get_run(&self, id: &str) -> Option<RunView> {
        let run = self.inner.lock().runs.get(id).cloned();
        run.map(|run| run.view())
    }

    /// Events from `from_index` on, clamped to the stored range, plus the total
    /// count so a client can resume from `next_index`.
    pub(crate) fn run_events(
        &self,
        id: &str,
        from_index: usize,
    ) -> Option<(RunStatus, Vec<Value>, usize)> {
        let run = self.inner.lock().runs.get(id).cloned()?;
        let status = run.view().status;
        let total = run.event_count();
        let from = from_index.min(total);
        Some((status, run.events_from(from), total))
    }

    pub(crate) fn get_run_handle(&self, id: &str) -> Option<Arc<Run>> {
        self.inner.lock().runs.get(id).cloned()
    }

    pub(crate) fn cancel(&self, session_id: &str) -> Result<Option<CancelTarget>, ApiError> {
        let inner = self.inner.lock();
        if !inner.sessions.contains_key(session_id) {
            return Err(ApiError::not_found(
                "unknown_session",
                format!("no session {session_id}"),
            ));
        }
        let Some(run_id) = inner.active.get(session_id).cloned() else {
            return Ok(None);
        };
        let Some(run) = inner.runs.get(&run_id).cloned() else {
            return Ok(None);
        };
        Ok(Some(CancelTarget {
            worker_id: run.worker_id.clone(),
            trace_id: run.trace_id.clone(),
            run,
        }))
    }

    /// Runs currently holding a worker's connection. Used when that connection
    /// goes away for good.
    pub(crate) fn active_runs_for_worker(&self, worker_id: &str) -> Vec<Arc<Run>> {
        let inner = self.inner.lock();
        inner
            .runs
            .values()
            .filter(|run| !run.is_terminal() && run.worker_id == worker_id)
            .cloned()
            .collect()
    }

    /// Map one worker text frame onto the run it names. Frames that name no
    /// live run are ignored by the control plane (the relay already handled
    /// them for attached browser clients).
    fn handle_frame(&self, text: &str) -> FrameOutcome {
        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return FrameOutcome::Ignored;
        };
        let Some(trace_id) = frame.get("trace_id").and_then(Value::as_str) else {
            return FrameOutcome::Ignored;
        };
        let run = {
            let inner = self.inner.lock();
            inner
                .trace_runs
                .get(trace_id)
                .and_then(|run_id| inner.runs.get(run_id))
                .cloned()
        };
        let Some(run) = run else {
            return FrameOutcome::Ignored;
        };
        if run.is_terminal() {
            return FrameOutcome::Ignored;
        }

        let kind = frame
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "assistant_delta" => {
                if let Some(delta) = frame.get("delta").and_then(Value::as_str) {
                    run.push_text(delta);
                }
                FrameOutcome::Ignored
            }
            "thread_agent_reply" => {
                if let Some(content) = frame.get("content").and_then(Value::as_str) {
                    run.set_final_text(content);
                }
                FrameOutcome::Settle {
                    run,
                    status: RunStatus::Succeeded,
                    error: None,
                }
            }
            "agent_finished" => match finished_status(&frame) {
                FinishedStatus::Success => {
                    if run.begin_settle_grace() {
                        FrameOutcome::Grace { run }
                    } else {
                        FrameOutcome::Ignored
                    }
                }
                FinishedStatus::Failure(message) => FrameOutcome::Settle {
                    run,
                    status: RunStatus::Failed,
                    error: Some(message),
                },
                FinishedStatus::Cancelled => FrameOutcome::Settle {
                    run,
                    status: RunStatus::Cancelled,
                    error: None,
                },
                FinishedStatus::Pending => FrameOutcome::Ignored,
            },
            "tool_call_started" => {
                if let Some(id) = frame.get("id").and_then(Value::as_str) {
                    let name = frame
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let arguments = frame.get("arguments").cloned().unwrap_or(Value::Null);
                    run.tool_started(id, name, &arguments);
                }
                FrameOutcome::Ignored
            }
            "tool_call_finished" => {
                if let Some(id) = frame.get("id").and_then(Value::as_str) {
                    let result = frame
                        .get("result")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    run.tool_finished(id, result);
                }
                FrameOutcome::Ignored
            }
            "tool_call_error" => {
                if let Some(id) = frame.get("id").and_then(Value::as_str) {
                    let message = frame
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    run.tool_failed(id, message);
                }
                FrameOutcome::Ignored
            }
            "agent_tool_result" => {
                let call_id = frame
                    .get("step_id")
                    .or_else(|| frame.get("id"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let name = frame
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let result = frame
                    .get("result")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if !call_id.is_empty() {
                    {
                        let mut inner = run.inner.lock();
                        inner.tools.insert(call_id.clone(), name);
                    }
                    run.tool_finished(&call_id, &result);
                }
                FrameOutcome::Ignored
            }
            "agent_log" => {
                let step_id = frame
                    .get("step_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if step_id == "usage" {
                    // Remote-DSH runs report usage as a summary line on an
                    // `agent_log` frame, not as a numeric frame yet.
                    run.push_event(json!({
                        "type": "usage",
                        "text": frame.get("message").cloned().unwrap_or(Value::Null),
                        "frame": frame,
                    }));
                } else {
                    run.push_event(json!({
                        "type": "log",
                        "level": frame.get("level").cloned().unwrap_or(Value::Null),
                        "message": frame.get("message").cloned().unwrap_or(Value::Null),
                        "stepId": frame.get("step_id").cloned().unwrap_or(Value::Null),
                    }));
                }
                FrameOutcome::Ignored
            }
            "done" => {
                // A remote-DSH turn reported `turn_end`; the reply arrives right
                // after it and settles the run.
                run.push_event(json!({ "type": "status", "phase": "turn_end" }));
                FrameOutcome::Ignored
            }
            "ask_user" => {
                run.push_event(json!({
                    "type": "ask_user",
                    "question": frame.get("question").cloned().unwrap_or(Value::Null),
                    "quickReplies": frame.get("quick_replies").cloned().unwrap_or(Value::Null),
                }));
                FrameOutcome::Ignored
            }
            "command_proposed" => {
                run.push_event(json!({
                    "type": "command_proposed",
                    "callId": frame.get("id").cloned().unwrap_or(Value::Null),
                    "candidates": frame.get("candidates").cloned().unwrap_or(Value::Null),
                    "cwd": frame.get("cwd").cloned().unwrap_or(Value::Null),
                }));
                FrameOutcome::Ignored
            }
            "agent_state_update" => {
                run.push_event(json!({
                    "type": "status",
                    "phase": "agent_state",
                    "state": frame.get("state").cloned().unwrap_or(Value::Null),
                }));
                FrameOutcome::Ignored
            }
            _ => {
                let lower = kind.to_ascii_lowercase();
                if lower.contains("thinking") || lower.contains("reasoning") {
                    let delta = frame
                        .get("delta")
                        .or_else(|| frame.get("text"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    run.push_thinking(delta);
                } else if lower.contains("usage") {
                    let used = frame.get("used").cloned();
                    let size = frame.get("size").cloned();
                    run.push_event(json!({
                        "type": "usage",
                        "used": used,
                        "size": size,
                        "frame": frame.clone(),
                    }));
                } else {
                    // Forward-compatible: a frame kind this gateway build does
                    // not know yet is still stored and fanned out, so additive
                    // protocol frames from a newer worker reach clients.
                    run.push_event(json!({ "type": "event", "kind": kind, "frame": frame }));
                }
                FrameOutcome::Ignored
            }
        }
    }
}

impl RemoteInner {
    fn prune(&mut self) {
        while self.runs.len() > MAX_RUNS {
            let Some(id) = self
                .order
                .iter()
                .find(|id| {
                    self.runs
                        .get(*id)
                        .map(|run| run.is_terminal())
                        .unwrap_or(false)
                })
                .cloned()
            else {
                break;
            };
            self.order.retain(|candidate| candidate != &id);
            if let Some(run) = self.runs.remove(&id) {
                self.trace_runs.remove(&run.trace_id);
            }
        }
    }
}

fn session_view(inner: &RemoteInner, session: &StoredSession, connected: &[String]) -> SessionView {
    let active_run_id = inner.active.get(&session.id).cloned();
    SessionView {
        id: session.id.clone(),
        name: session.name.clone(),
        worker_id: session.worker_id.clone(),
        worker_connected: connected.iter().any(|worker| worker == &session.worker_id),
        thread_id: session.thread_id.clone(),
        agent_id: session.agent_id.clone(),
        status: if active_run_id.is_some() {
            "running"
        } else {
            "idle"
        },
        active_run_id,
        created_at: session.created_at,
        updated_at: session.updated_at,
        run_count: session.run_count,
        last_run_id: session.last_run_id.clone(),
    }
}

enum FinishedStatus {
    Success,
    Failure(String),
    Cancelled,
    Pending,
}

/// Read `agent_finished.status`, which serializes as `"Success"` /
/// `"Cancelled"` / `{"Failure": "message"}`.
fn finished_status(frame: &Value) -> FinishedStatus {
    let Some(status) = frame.get("status") else {
        return FinishedStatus::Pending;
    };
    if let Some(name) = status.as_str() {
        return match name {
            "Success" => FinishedStatus::Success,
            "Cancelled" => FinishedStatus::Cancelled,
            _ => FinishedStatus::Pending,
        };
    }
    if let Some(message) = status.get("Failure").and_then(Value::as_str) {
        return FinishedStatus::Failure(message.to_string());
    }
    FinishedStatus::Pending
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

fn sessions_path(dir: &Path) -> PathBuf {
    dir.join(SESSIONS_FILE)
}

fn load_sessions(dir: &Path) -> std::io::Result<Option<StoredSessions>> {
    let path = sessions_path(dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let stored: StoredSessions = serde_json::from_slice(&bytes).map_err(std::io::Error::other)?;
    if stored.version != 1 {
        return Err(std::io::Error::other(format!(
            "unsupported session store version {}",
            stored.version
        )));
    }
    Ok(Some(stored))
}

fn write_sessions(dir: &Path, stored: &StoredSessions) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = sessions_path(dir);
    let temporary = dir.join(format!("{SESSIONS_FILE}.tmp"));
    let bytes = serde_json::to_vec_pretty(stored).map_err(std::io::Error::other)?;
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, &path)
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

/// Build the `run_agent_for_thread_reply` frame for a run.
///
/// The gateway owns the agent identity of a session: a stable generated
/// `agent_id`, a generated `AgentSpec` naming the session, and no MCP servers
/// (a worker's own configured servers are what its runs use). A client only has
/// to supply the task text.
fn run_agent_frame(dispatch: &SessionDispatch, run: &Run) -> String {
    let name = dispatch
        .session_name
        .clone()
        .unwrap_or_else(|| "gateway".to_string());
    let created_at = rfc3339(dispatch.created_at);
    json!({
        "kind": "run_agent_for_thread_reply",
        "trace_id": run.trace_id,
        "thread_id": dispatch.thread_id,
        "agent_id": dispatch.agent_id,
        "prompt": run.task,
        "spec": {
            "id": dispatch.agent_id,
            "name": name,
            "description": format!("Goble gateway session {}", run.session_id),
            "prompt": "",
            "tools": [],
            "triggers": [],
            "mcp_ids": [],
            "created_at": created_at,
            "updated_at": created_at,
        },
        "mcp_servers": [],
    })
    .to_string()
}

fn cancel_frame(trace_id: &str) -> String {
    json!({ "kind": "cancel_run", "trace_id": trace_id }).to_string()
}

// ---------------------------------------------------------------------------
// Gateway integration
// ---------------------------------------------------------------------------

impl GatewayState {
    /// Store any events a worker frame carries for a live run, and settle the
    /// run when the frame ends the turn.
    pub(crate) fn tap_worker_text(self: &Arc<Self>, text: &str) {
        match self.remote.handle_frame(text) {
            FrameOutcome::Ignored => {}
            FrameOutcome::Settle { run, status, error } => {
                self.remote.finish_run(&run, status, None, error);
            }
            FrameOutcome::Grace { run } => {
                let state = Arc::clone(self);
                tokio::spawn(async move {
                    tokio::time::sleep(SETTLE_GRACE).await;
                    state.remote.finish_run_if_pending(&run);
                });
            }
        }
    }

    /// A worker socket ended. If no replacement connection took over its id,
    /// no run on it can finish: settle them instead of leaking `running`
    /// sessions.
    pub(crate) fn remote_worker_disconnected(self: &Arc<Self>, worker_id: &str) {
        if self.has_worker(worker_id) {
            // The id reconnected before this socket finished closing.
            return;
        }
        for run in self.remote.active_runs_for_worker(worker_id) {
            self.remote.finish_run(
                &run,
                RunStatus::Failed,
                None,
                Some("the worker disconnected before the run finished".to_string()),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct CreateSessionRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub worker: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct StartRunRequest {
    pub task: String,
    #[serde(default)]
    pub timeout_s: Option<u64>,
    #[serde(default)]
    pub wait: bool,
}

#[derive(Deserialize)]
pub(crate) struct FromIndexQuery {
    #[serde(default)]
    pub from_index: usize,
}

fn worker_ids(state: &GatewayState) -> Vec<String> {
    state
        .workers()
        .into_iter()
        .map(|worker| worker.id)
        .collect()
}

fn authorized(state: &GatewayState, headers: &HeaderMap) -> bool {
    state.client_authorized(headers)
}

pub(crate) async fn create_session(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    Json(body): Json<CreateSessionRequest>,
) -> Response {
    if !authorized(&state, &headers) {
        return json_unauthorized();
    }
    let name = body
        .name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    if let Some(name) = &name {
        if name.len() > 200 {
            return ApiError::bad_request("a session name is at most 200 bytes").into();
        }
    }
    let connected = worker_ids(&state);
    let resolved = state.remote.create_or_reuse(
        name,
        body.worker,
        || match connected.as_slice() {
            [only] => Ok(only.clone()),
            [] => Err(ApiError::service(
                "no_worker_connected",
                "no worker is connected; connect a worker or pass a worker id",
            )),
            _ => Err(ApiError::conflict(
                "worker_required",
                "more than one worker is connected; pass \"worker\" or GET /api/v1/workers",
            )),
        },
        &connected,
    );
    match resolved {
        Ok((session, reused)) => {
            let status = if reused {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };
            json_response(
                status,
                serde_json::to_value(CreateSessionResponse { session, reused })
                    .unwrap_or(Value::Null),
            )
        }
        Err(error) => error.into(),
    }
}

pub(crate) async fn list_sessions(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&state, &headers) {
        return json_unauthorized();
    }
    let connected = worker_ids(&state);
    let sessions = state.remote.list_sessions(&connected);
    json_response(
        StatusCode::OK,
        serde_json::to_value(SessionList { sessions }).unwrap_or(Value::Null),
    )
}

pub(crate) async fn get_session(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return json_unauthorized();
    }
    let connected = worker_ids(&state);
    match state.remote.get_session(&id, &connected) {
        Some(session) => json_response(
            StatusCode::OK,
            serde_json::to_value(session).unwrap_or(Value::Null),
        ),
        None => ApiError::not_found("unknown_session", format!("no session {id}")).into(),
    }
}

pub(crate) async fn cancel_session(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return json_unauthorized();
    }
    let target = match state.remote.cancel(&id) {
        Ok(target) => target,
        Err(error) => return error.into(),
    };
    let mut run_id = None;
    if let Some(target) = target {
        let settled = state
            .remote
            .finish_run(&target.run, RunStatus::Cancelled, None, None);
        if settled {
            // Best effort: a worker that predates the cancel frame logs an
            // error for the unknown frame but keeps the turn running, which is
            // why the run is settled gateway-side either way.
            let _ = state
                .send_to_worker(
                    &target.worker_id,
                    Message::Text(cancel_frame(&target.trace_id).into()),
                )
                .await;
            run_id = Some(target.run.id.clone());
        }
    }
    json_response(StatusCode::OK, json!({ "ok": true, "run_id": run_id }))
}

pub(crate) async fn start_run(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    UrlPath(session_id): UrlPath<String>,
    Json(body): Json<StartRunRequest>,
) -> Response {
    if !authorized(&state, &headers) {
        return json_unauthorized();
    }
    let task = body.task;
    if task.trim().is_empty() {
        return ApiError::bad_request("\"task\" must not be empty").into();
    }
    if task.len() > MAX_TASK_BYTES {
        return ApiError::bad_request(format!(
            "\"task\" is {} bytes; the limit is {MAX_TASK_BYTES}",
            task.len()
        ))
        .into();
    }
    let timeout_s = body
        .timeout_s
        .unwrap_or(DEFAULT_RUN_TIMEOUT_S)
        .clamp(1, MAX_RUN_TIMEOUT_S);

    // The session's single run slot: 409 immediately, or for `wait: true` wait
    // up to the queue window before giving up.
    let queue_deadline = std::time::Instant::now() + QUEUE_WINDOW;
    loop {
        match state.remote.active_run_id(&session_id) {
            None => break,
            Some(run_id) if !body.wait => {
                return ApiError::conflict(
                    "session_busy",
                    format!("session {session_id} is already running run {run_id}"),
                )
                .into()
            }
            Some(_) => {
                if std::time::Instant::now() >= queue_deadline {
                    return ApiError::conflict(
                        "session_busy",
                        format!("session {session_id} stayed busy for the whole queue window"),
                    )
                    .into();
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    let (run, dispatch) = match state.remote.begin_run(&session_id, task, timeout_s) {
        Ok(result) => result,
        Err(error) => return error.into(),
    };

    if !state.has_worker(&dispatch.worker_id) {
        state.remote.finish_run(
            &run,
            RunStatus::Failed,
            None,
            Some(format!("worker {} is not connected", dispatch.worker_id)),
        );
        return ApiError::service(
            "worker_offline",
            format!("worker {} is not connected", dispatch.worker_id),
        )
        .into();
    }

    let frame = run_agent_frame(&dispatch, &run);
    if !state
        .send_to_worker(&dispatch.worker_id, Message::Text(frame.into()))
        .await
    {
        state.remote.finish_run(
            &run,
            RunStatus::Failed,
            None,
            Some("the worker disconnected before the run started".to_string()),
        );
        return ApiError::service(
            "worker_offline",
            format!("worker {} disconnected", dispatch.worker_id),
        )
        .into();
    }

    spawn_timeout(Arc::clone(&state), Arc::clone(&run));

    if body.wait {
        run.wait_terminal().await;
    }
    json_response(
        if body.wait {
            StatusCode::OK
        } else {
            StatusCode::ACCEPTED
        },
        serde_json::to_value(run.view()).unwrap_or(Value::Null),
    )
}

fn spawn_timeout(state: Arc<GatewayState>, run: Arc<Run>) {
    let timeout_s = run.timeout_s;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(timeout_s)).await;
        state.remote.finish_run(
            &run,
            RunStatus::Timeout,
            None,
            Some(format!("run timed out after {timeout_s}s")),
        );
    });
}

pub(crate) async fn get_run(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<String>,
) -> Response {
    if !authorized(&state, &headers) {
        return json_unauthorized();
    }
    match state.remote.get_run(&id) {
        Some(run) => json_response(
            StatusCode::OK,
            serde_json::to_value(run).unwrap_or(Value::Null),
        ),
        None => ApiError::not_found("unknown_run", format!("no run {id}")).into(),
    }
}

pub(crate) async fn run_events(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<String>,
    Query(query): Query<FromIndexQuery>,
) -> Response {
    if !authorized(&state, &headers) {
        return json_unauthorized();
    }
    match state.remote.run_events(&id, query.from_index) {
        Some((status, events, next_index)) => json_response(
            StatusCode::OK,
            json!({
                "run_id": id,
                "status": status,
                "from_index": query.from_index.min(next_index),
                "next_index": next_index,
                "events": events,
            }),
        ),
        None => ApiError::not_found("unknown_run", format!("no run {id}")).into(),
    }
}

pub(crate) async fn run_stream(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<String>,
    Query(query): Query<FromIndexQuery>,
) -> Response {
    if !authorized(&state, &headers) {
        return json_unauthorized();
    }
    let Some(run) = state.remote.get_run_handle(&id) else {
        return ApiError::not_found("unknown_run", format!("no run {id}")).into();
    };
    let from = query.from_index.min(
        state
            .remote
            .get_run(&id)
            .map(|run| run.events_count)
            .unwrap_or(0),
    );
    let stream = sse_stream(run, from);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Accel-Buffering", "no")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Replay the stored events from `from_index`, then follow the run live.
///
/// Events are the only `data:` frames; a `: keepalive` comment is sent right
/// after the replay (so a browser's `EventSource` sees a flush even when the
/// run is quiet) and again after every idle keepalive interval. The stream ends
/// right after the terminal `final` event.
fn sse_stream(
    run: Arc<Run>,
    from_index: usize,
) -> impl futures::Stream<Item = Result<String, Infallible>> + Send + 'static {
    let receiver = run.subscribe();
    futures::stream::unfold(
        (run, receiver, from_index, false),
        |(run, mut receiver, cursor, sent_keepalive)| async move {
            loop {
                let events = run.events_from(cursor);
                if !events.is_empty() {
                    let mut chunk = String::new();
                    let mut next = cursor;
                    for event in &events {
                        chunk.push_str("data: ");
                        chunk.push_str(&serde_json::to_string(event).unwrap_or_default());
                        chunk.push_str("\n\n");
                        next += 1;
                    }
                    return Some((Ok(chunk), (run, receiver, next, sent_keepalive)));
                }
                if run.is_terminal() {
                    return None;
                }
                if !sent_keepalive {
                    return Some((
                        Ok(": keepalive\n\n".to_string()),
                        (run, receiver, cursor, true),
                    ));
                }
                match tokio::time::timeout(SSE_KEEPALIVE, receiver.changed()).await {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) => return None,
                    Err(_) => {
                        return Some((
                            Ok(": keepalive\n\n".to_string()),
                            (run, receiver, cursor, true),
                        ))
                    }
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_session() -> (RemoteState, String) {
        let state = RemoteState::new(None);
        let (session, reused) = state
            .create_or_reuse(
                Some("test".to_string()),
                Some("worker-1".to_string()),
                || unreachable!("worker is supplied"),
                &["worker-1".to_string()],
            )
            .unwrap();
        assert!(!reused);
        (state, session.id)
    }

    /// What the gateway does with a worker frame: record it, then settle the
    /// run when the frame ends the turn.
    fn tap(state: &RemoteState, text: &str) {
        match state.handle_frame(text) {
            FrameOutcome::Ignored => {}
            FrameOutcome::Settle { run, status, error } => {
                state.finish_run(&run, status, None, error);
            }
            FrameOutcome::Grace { run } => state.finish_run_if_pending(&run),
        }
    }

    #[test]
    fn a_frame_stream_settles_a_run_and_records_events() {
        let (state, session_id) = state_with_session();
        let (run, dispatch) = state
            .begin_run(&session_id, "do the thing".to_string(), 60)
            .unwrap();
        assert_eq!(dispatch.worker_id, "worker-1");
        assert_eq!(run.view().status, RunStatus::Running);
        assert_eq!(run.view().events_count, 1, "status running is event 0");

        tap(
            &state,
            &json!({"kind":"assistant_delta","trace_id":run.trace_id,"delta":"hello "}).to_string(),
        );
        tap(
            &state,
            &json!({"kind":"tool_call_started","trace_id":run.trace_id,"id":"c1","name":"bash","arguments":{"cmd":"ls"}}).to_string(),
        );
        tap(
            &state,
            &json!({"kind":"tool_call_finished","trace_id":run.trace_id,"id":"c1","result":"ok"})
                .to_string(),
        );
        tap(
            &state,
            &json!({"kind":"thread_agent_reply","trace_id":run.trace_id,"thread_id":run.thread_id,"content":"hello world"}).to_string(),
        );

        let view = run.view();
        assert_eq!(view.status, RunStatus::Succeeded);
        assert_eq!(view.final_text.as_deref(), Some("hello world"));
        let (_, events, total) = state.run_events(&run.id, 0).unwrap();
        assert_eq!(total, events.len());
        assert_eq!(events[0]["type"], "status");
        assert_eq!(events[1]["type"], "text");
        assert_eq!(events[1]["text"], "hello ");
        assert_eq!(events[2]["type"], "tool_call");
        assert_eq!(events[2]["callId"], "c1");
        assert_eq!(events[3]["type"], "tool_result");
        assert_eq!(events[3]["status"], "completed");
        assert_eq!(events.last().unwrap()["type"], "final");
        assert_eq!(events.last().unwrap()["reason"]["kind"], "completed");
        // The session is free again.
        assert!(state.active_run_id(&session_id).is_none());
    }

    #[test]
    fn success_without_a_thread_reply_uses_the_streamed_text_after_the_grace_window() {
        let (state, session_id) = state_with_session();
        let (run, _) = state
            .begin_run(&session_id, "task".to_string(), 60)
            .unwrap();
        tap(
            &state,
            &json!({"kind":"assistant_delta","trace_id":run.trace_id,"delta":"only deltas"})
                .to_string(),
        );
        let outcome = state.handle_frame(
            &json!({"kind":"agent_finished","trace_id":run.trace_id,"status":"Success"})
                .to_string(),
        );
        assert!(matches!(outcome, FrameOutcome::Grace { .. }));
        // A duplicate finish does not schedule a second grace.
        assert!(matches!(
            state.handle_frame(
                &json!({"kind":"agent_finished","trace_id":run.trace_id,"status":"Success"})
                    .to_string()
            ),
            FrameOutcome::Ignored
        ));
        assert_eq!(run.view().status, RunStatus::Running);
        state.finish_run_if_pending(&run);
        assert_eq!(run.view().status, RunStatus::Succeeded);
        assert_eq!(run.view().final_text.as_deref(), Some("only deltas"));
    }

    #[test]
    fn failure_and_timeout_settle_with_an_error_event() {
        let (state, session_id) = state_with_session();
        let (run, _) = state
            .begin_run(&session_id, "task".to_string(), 60)
            .unwrap();
        let failure = json!({
            "kind": "agent_finished",
            "trace_id": run.trace_id,
            "status": {"Failure": "provider refused"},
        });
        tap(&state, &failure.to_string());
        assert_eq!(run.view().status, RunStatus::Failed);
        assert_eq!(run.view().error.as_deref(), Some("provider refused"));
        let (_, events, _) = state.run_events(&run.id, 0).unwrap();
        let final_event = events.last().unwrap();
        assert_eq!(final_event["reason"]["kind"], "error");
        assert_eq!(
            final_event["reason"]["error"]["message"],
            "provider refused"
        );

        let (second, _) = state
            .begin_run(&session_id, "task".to_string(), 60)
            .unwrap();
        assert!(state.finish_run(
            &second,
            RunStatus::Timeout,
            None,
            Some("run timed out after 1s".to_string())
        ));
        assert_eq!(second.view().status, RunStatus::Timeout);
        assert!(!state.finish_run(&second, RunStatus::Succeeded, None, None));
    }

    #[test]
    fn a_busy_session_refuses_a_second_run_and_cancel_frees_it() {
        let (state, session_id) = state_with_session();
        let (run, _) = state
            .begin_run(&session_id, "first".to_string(), 60)
            .unwrap();
        let error = state.begin_run(&session_id, "second".to_string(), 60);
        assert!(matches!(error, Err(ApiError::Conflict("session_busy", _))));

        let target = state.cancel(&session_id).unwrap().expect("a running run");
        assert!(state.finish_run(&target.run, RunStatus::Cancelled, None, None));
        assert_eq!(run.view().status, RunStatus::Cancelled);
        assert!(state.active_run_id(&session_id).is_none());
        assert!(state.cancel(&session_id).unwrap().is_none());
        assert!(state
            .begin_run(&session_id, "third".to_string(), 60)
            .is_ok());
    }

    #[test]
    fn remote_dsh_turn_end_and_usage_frames_are_mapped() {
        let (state, session_id) = state_with_session();
        let (run, _) = state
            .begin_run(&session_id, "task".to_string(), 60)
            .unwrap();
        tap(
            &state,
            &json!({"kind":"done","trace_id":run.trace_id}).to_string(),
        );
        tap(
            &state,
            &json!({
                "kind": "agent_log",
                "trace_id": run.trace_id,
                "step_id": "usage",
                "level": "Info",
                "message": "input 10 output 5",
            })
            .to_string(),
        );
        tap(
            &state,
            &json!({
                "kind": "thread_agent_reply",
                "trace_id": run.trace_id,
                "thread_id": run.thread_id,
                "content": "ok",
            })
            .to_string(),
        );
        let view = run.view();
        assert_eq!(view.status, RunStatus::Succeeded);
        assert_eq!(
            view.usage.as_ref().and_then(|usage| usage["text"].as_str()),
            Some("input 10 output 5")
        );
        let (_, events, _) = state.run_events(&run.id, 0).unwrap();
        assert!(events
            .iter()
            .any(|event| event["type"] == "status" && event["phase"] == "turn_end"));
        assert!(events
            .iter()
            .any(|event| event["type"] == "usage" && event["text"] == "input 10 output 5"));
    }

    #[test]
    fn frames_for_other_traces_are_ignored() {
        let (state, session_id) = state_with_session();
        let (run, _) = state
            .begin_run(&session_id, "task".to_string(), 60)
            .unwrap();
        assert!(matches!(
            state.handle_frame(r#"{"kind":"assistant_delta","trace_id":"other","delta":"x"}"#),
            FrameOutcome::Ignored
        ));
        assert_eq!(run.view().events_count, 1);
        assert!(matches!(
            state.handle_frame("not json"),
            FrameOutcome::Ignored
        ));
    }

    #[test]
    fn sessions_are_stable_by_name_and_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let state = RemoteState::new(Some(dir.path().to_path_buf()));
        let (first, reused) = state
            .create_or_reuse(
                Some("named".to_string()),
                Some("worker-1".to_string()),
                || unreachable!(),
                &["worker-1".to_string()],
            )
            .unwrap();
        assert!(!reused);
        let (second, reused) = state
            .create_or_reuse(
                Some("named".to_string()),
                None,
                || unreachable!("reuse does not resolve a worker"),
                &["worker-1".to_string()],
            )
            .unwrap();
        assert!(reused);
        assert_eq!(first.id, second.id);
        assert_eq!(first.thread_id, second.thread_id);

        let restored = RemoteState::new(Some(dir.path().to_path_buf()));
        let restored_session = restored
            .get_session(&first.id, &["worker-1".to_string()])
            .expect("session should be persisted");
        assert_eq!(restored_session.thread_id, first.thread_id);
        assert_eq!(restored_session.worker_id, "worker-1");
        assert_eq!(restored_session.status, "idle");
        // And it can be reused by name after the restart.
        let (again, reused) = restored
            .create_or_reuse(Some("named".to_string()), None, || unreachable!(), &[])
            .unwrap();
        assert!(reused);
        assert_eq!(again.id, first.id);
    }
}
