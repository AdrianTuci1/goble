//! The collector end of [`goble_telemetry`].
//!
//! It is deliberately small, because the hard part of a crash collector is not
//! receiving reports — it is not becoming a liability:
//!
//! - **It stores what it receives and nothing else.** No cookies, no IP
//!   logging, no third-party analytics, no accounts.
//! - **It is authenticated when asked to be.** An open collector invites
//!   junk; setting `GOBLE_TELEMETRY_TOKEN` makes every ingest request carry a
//!   shared token.
//! - **It bounds every request.** A malformed or oversized body is rejected
//!   before it reaches the store, and a schema this build does not know is
//!   refused rather than written as a half-understood file.
//!
//! The wire types are the ones the client already uses, from the client crate:
//! there is one schema, defined once, and both sides deserialize the same
//! `QueuedItem`.
//!
//! Layout on disk:
//!
//! ```text
//! <root>/
//!   crashes/<fingerprint>/<report-id>.json   one file per report, grouped by crash
//!   events/<YYYY-MM-DD>.jsonl                append-only event stream
//! ```

use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use axum::Json;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;

use goble_telemetry::{CrashReport, QueuedItem, TelemetryEvent, SCHEMA};

/// Largest ingest body accepted. A report is capped well below this by the
/// client; the limit is here so a hand-written request cannot fill the disk.
pub const MAX_BODY: usize = 512 * 1024;

/// A summary of one crash, grouped the way reports are fingerprinted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FingerprintSummary {
    pub fingerprint: String,
    pub reports: usize,
    pub last_seen: DateTime<Utc>,
    pub app_version: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not write to {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not encode the report: {0}")]
    Encode(#[from] serde_json::Error),
}

/// A crash/event store rooted in one directory.
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn crashes_dir(&self) -> PathBuf {
        self.root.join("crashes")
    }

    pub fn events_dir(&self) -> PathBuf {
        self.root.join("events")
    }

    /// Store one report. Uploading the same report twice overwrites it, so a
    /// client that retries after a timeout does not create duplicates.
    pub fn put_crash(&self, report: &CrashReport) -> Result<PathBuf, StoreError> {
        let dir = self.crashes_dir().join(sanitise(&report.fingerprint));
        std::fs::create_dir_all(&dir).map_err(|source| StoreError::Io {
            path: dir.clone(),
            source,
        })?;

        let path = dir.join(format!("{}.json", sanitise(&report.id)));
        let text = serde_json::to_string_pretty(report)?;
        std::fs::write(&path, text).map_err(|source| StoreError::Io {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }

    /// Append one event to the day's stream.
    pub fn put_event(&self, event: &TelemetryEvent) -> Result<(), StoreError> {
        let dir = self.events_dir();
        std::fs::create_dir_all(&dir).map_err(|source| StoreError::Io {
            path: dir.clone(),
            source,
        })?;

        let path = dir.join(format!("{}.jsonl", Utc::now().format("%Y-%m-%d")));
        let mut line = serde_json::to_string(event)?;
        line.push('\n');

        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| StoreError::Io { path: path.clone(), source })?;
        file.write_all(line.as_bytes())
            .map_err(|source| StoreError::Io { path, source })
    }

    /// One entry per crash, newest crash first.
    pub fn summaries(&self) -> Result<Vec<FingerprintSummary>, StoreError> {
        let Ok(dirs) = std::fs::read_dir(self.crashes_dir()) else {
            return Ok(Vec::new());
        };

        let mut summaries = Vec::new();
        for entry in dirs.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(summary) = self.summarise(&dir) else {
                continue;
            };
            summaries.push(summary);
        }
        summaries.sort_by_key(|summary| Reverse(summary.last_seen));
        Ok(summaries)
    }

    fn summarise(&self, dir: &Path) -> Option<FingerprintSummary> {
        let mut reports = 0;
        let mut newest: Option<(DateTime<Utc>, CrashReport)> = None;

        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(report) = serde_json::from_str::<CrashReport>(&text) else {
                continue;
            };
            reports += 1;
            if newest.as_ref().is_none_or(|(at, _)| report.captured_at > *at) {
                newest = Some((report.captured_at, report));
            }
        }

        let (last_seen, report) = newest?;
        Some(FingerprintSummary {
            fingerprint: report.fingerprint,
            reports,
            last_seen,
            app_version: report.context.app_version,
            message: report.message,
        })
    }

    /// Crash reports captured on the given day, newest first.
    pub fn crashes_on(&self, day: NaiveDate) -> Result<Vec<CrashReport>, StoreError> {
        let mut reports = Vec::new();
        let Ok(dirs) = std::fs::read_dir(self.crashes_dir()) else {
            return Ok(reports);
        };

        for entry in dirs.flatten() {
            let Ok(files) = std::fs::read_dir(entry.path()) else {
                continue;
            };
            for file in files.flatten() {
                let Ok(text) = std::fs::read_to_string(file.path()) else {
                    continue;
                };
                let Ok(report) = serde_json::from_str::<CrashReport>(&text) else {
                    continue;
                };
                if report.captured_at.date_naive() == day {
                    reports.push(report);
                }
            }
        }
        reports.sort_by_key(|report| Reverse(report.captured_at));
        Ok(reports)
    }
}

/// Shared state of a running collector.
#[derive(Debug, Clone)]
pub struct ServerState {
    store: Store,
    /// When set, ingest requires this token.
    token: Option<String>,
}

impl ServerState {
    pub fn new(store: Store, token: Option<String>) -> Self {
        let token = token.filter(|value| !value.trim().is_empty());
        Self { store, token }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn requires_token(&self) -> bool {
        self.token.is_some()
    }

    fn authorised(&self, headers: &HeaderMap) -> bool {
        let Some(expected) = &self.token else {
            return true;
        };

        let presented = headers
            .get("x-goble-token")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
            .or_else(|| {
                headers
                    .get(header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.strip_prefix("Bearer "))
                    .map(str::to_owned)
            });

        // Constant-time-ish comparison: the token is shared, not a password,
        // but there is no reason to leak its length through timing either.
        match presented {
            Some(presented) => {
                let expected = expected.as_bytes();
                let presented = presented.as_bytes();
                expected.len() == presented.len()
                    && expected
                        .iter()
                        .zip(presented)
                        .fold(0_u8, |acc, (left, right)| acc | (left ^ right))
                        == 0
            }
            None => false,
        }
    }
}

/// The collector's routes.
pub fn router(state: ServerState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/crashes", post(ingest_crash))
        .route("/v1/events", post(ingest_event))
        .route("/v1/summary", get(summary))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok", "schema": SCHEMA }))
}

/// An ingest answer. Kept tiny: the client only needs to know whether to retry.
#[derive(Debug, Serialize)]
struct Ingested {
    accepted: usize,
}

fn rejected(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

async fn ingest_crash(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !state.authorised(&headers) {
        return rejected(StatusCode::UNAUTHORIZED, "a collector token is required");
    }

    let item: QueuedItem = match serde_json::from_slice(&body) {
        Ok(item) => item,
        Err(err) => return rejected(StatusCode::BAD_REQUEST, &format!("malformed report: {err}")),
    };

    let QueuedItem::Crash(report) = item else {
        return rejected(StatusCode::BAD_REQUEST, "this endpoint takes crash reports");
    };

    if report.schema != SCHEMA {
        return rejected(
            StatusCode::BAD_REQUEST,
            &format!("unsupported schema {}: this collector speaks {SCHEMA}", report.schema),
        );
    }

    match state.store.put_crash(&report) {
        Ok(_) => {
            tracing::info!(fingerprint = %report.fingerprint, version = %report.context.app_version, "stored a crash report");
            (StatusCode::ACCEPTED, Json(Ingested { accepted: 1 })).into_response()
        }
        Err(err) => {
            tracing::error!("could not store a crash report: {err}");
            rejected(StatusCode::INTERNAL_SERVER_ERROR, "could not store the report")
        }
    }
}

async fn ingest_event(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !state.authorised(&headers) {
        return rejected(StatusCode::UNAUTHORIZED, "a collector token is required");
    }

    let item: QueuedItem = match serde_json::from_slice(&body) {
        Ok(item) => item,
        Err(err) => return rejected(StatusCode::BAD_REQUEST, &format!("malformed event: {err}")),
    };

    let QueuedItem::Event(event) = item else {
        return rejected(StatusCode::BAD_REQUEST, "this endpoint takes events");
    };

    match state.store.put_event(&event) {
        Ok(()) => (StatusCode::ACCEPTED, Json(Ingested { accepted: 1 })).into_response(),
        Err(err) => {
            tracing::error!("could not store an event: {err}");
            rejected(StatusCode::INTERNAL_SERVER_ERROR, "could not store the event")
        }
    }
}

async fn summary(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    if !state.authorised(&headers) {
        return rejected(StatusCode::UNAUTHORIZED, "a collector token is required");
    }

    match state.store.summaries() {
        Ok(summaries) => Json(serde_json::json!({ "crashes": summaries })).into_response(),
        Err(err) => {
            tracing::error!("could not summarise the store: {err}");
            rejected(StatusCode::INTERNAL_SERVER_ERROR, "could not read the store")
        }
    }
}

/// Keep a fingerprint out of the path namespace it does not own.
fn sanitise(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(64)
        .collect();
    if cleaned.is_empty() {
        "unknown".to_owned()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request;
    use goble_telemetry::{Channel, ClientContext, CrashKind};
    use tower::ServiceExt as _;

    fn context() -> ClientContext {
        ClientContext {
            app_version: "0.1.0".into(),
            channel: Channel::Stable,
            os: "macos".into(),
            os_version: Some("15.0".into()),
            arch: "aarch64".into(),
            install_id: "install".into(),
            session_id: "session".into(),
        }
    }

    fn report(message: &str) -> CrashReport {
        CrashReport::new(&context(), CrashKind::Panic, message)
            .with_location(Some("app/src/main.rs:30".into()))
            .with_backtrace(Some("0: goble_app::main".into()))
    }

    fn state(dir: &Path, token: Option<&str>) -> ServerState {
        ServerState::new(Store::new(dir), token.map(str::to_owned))
    }

    async fn post(state: &ServerState, path: &str, body: Vec<u8>, token: Option<&str>) -> u16 {
        let mut builder = Request::builder().method("POST").uri(path);
        if let Some(token) = token {
            builder = builder.header("x-goble-token", token);
        }
        let request = builder.body(axum::body::Body::from(body)).unwrap();
        let response = router(state.clone()).oneshot(request).await.unwrap();
        response.status().as_u16()
    }

    async fn get(state: &ServerState, path: &str, token: Option<&str>) -> (u16, serde_json::Value) {
        let mut builder = Request::builder().method("GET").uri(path);
        if let Some(token) = token {
            builder = builder.header("x-goble-token", token);
        }
        let request = builder.body(axum::body::Body::empty()).unwrap();
        let response = router(state.clone()).oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let bytes = to_bytes(response.into_body(), MAX_BODY).await.unwrap();
        let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, value)
    }

    #[tokio::test]
    async fn a_report_is_stored_under_its_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);
        let report = report("boom");
        let body = serde_json::to_vec(&QueuedItem::crash(report.clone())).unwrap();

        assert_eq!(post(&state, "/v1/crashes", body, None).await, 202);

        let stored = state.store().crashes_on(Utc::now().date_naive()).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].message, "boom");
        assert!(state.store().crashes_dir().join(&report.fingerprint).is_dir());
    }

    #[tokio::test]
    async fn the_same_report_twice_does_not_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);
        let body =
            serde_json::to_vec(&QueuedItem::crash(report("boom"))).unwrap();

        assert_eq!(post(&state, "/v1/crashes", body.clone(), None).await, 202);
        assert_eq!(post(&state, "/v1/crashes", body, None).await, 202);

        assert_eq!(state.store().crashes_on(Utc::now().date_naive()).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn two_different_crashes_are_two_groups() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);
        for message in ["boom", "bang"] {
            let body = serde_json::to_vec(&QueuedItem::crash(report(message))).unwrap();
            assert_eq!(post(&state, "/v1/crashes", body, None).await, 202);
        }

        let summaries = state.store().summaries().unwrap();
        assert_eq!(summaries.len(), 2);
        assert!(summaries.iter().all(|summary| summary.reports == 1));
    }

    #[tokio::test]
    async fn an_event_is_appended_to_the_days_stream() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);
        let event = TelemetryEvent::AppStarted { context: context(), locale: None };
        let body = serde_json::to_vec(&QueuedItem::event(event)).unwrap();

        assert_eq!(post(&state, "/v1/events", body, None).await, 202);
        // The day rollover is not simulated; today's file must exist.
        let path = state.store().events_dir().join(format!("{}.jsonl", Utc::now().format("%Y-%m-%d")));
        assert!(path.is_file(), "{path:?}");
        assert!(std::fs::read_to_string(path).unwrap().contains("app_started"));
    }

    #[tokio::test]
    async fn the_wrong_endpoint_for_a_kind_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);
        let crash = serde_json::to_vec(&QueuedItem::crash(report("boom"))).unwrap();
        let event = serde_json::to_vec(&QueuedItem::event(TelemetryEvent::AppExited {
            uptime_secs: 1,
            exit_code: None,
        }))
        .unwrap();

        assert_eq!(post(&state, "/v1/events", crash, None).await, 400);
        assert_eq!(post(&state, "/v1/crashes", event, None).await, 400);
    }

    #[tokio::test]
    async fn a_malformed_body_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);
        assert_eq!(post(&state, "/v1/crashes", b"{ not json".to_vec(), None).await, 400);
        assert_eq!(post(&state, "/v1/crashes", b"{}".to_vec(), None).await, 400);
    }

    #[tokio::test]
    async fn an_unknown_schema_is_refused_rather_than_stored() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);

        let mut value = serde_json::to_value(QueuedItem::crash(report("boom"))).unwrap();
        value["value"]["schema"] = serde_json::json!(SCHEMA + 1);
        let body = serde_json::to_vec(&value).unwrap();

        assert_eq!(post(&state, "/v1/crashes", body, None).await, 400);
        assert!(state.store().summaries().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_configured_token_is_required() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), Some("let-me-in"));
        let body = serde_json::to_vec(&QueuedItem::crash(report("boom"))).unwrap();

        assert_eq!(post(&state, "/v1/crashes", body.clone(), None).await, 401);
        assert_eq!(post(&state, "/v1/crashes", body.clone(), Some("wrong")).await, 401);
        assert_eq!(post(&state, "/v1/crashes", body, Some("let-me-in")).await, 202);

        let (status, _) = get(&state, "/v1/summary", None).await;
        assert_eq!(status, 401);
        let (status, summary) = get(&state, "/v1/summary", Some("let-me-in")).await;
        assert_eq!(status, 200);
        assert_eq!(summary["crashes"][0]["message"], "boom");
    }

    #[tokio::test]
    async fn health_is_open_and_says_which_schema_is_supported() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), Some("let-me-in"));
        let (status, body) = get(&state, "/health", None).await;
        assert_eq!(status, 200);
        assert_eq!(body["schema"], SCHEMA);
    }

    #[tokio::test]
    async fn the_store_ignores_a_file_it_cannot_read() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);
        let body = serde_json::to_vec(&QueuedItem::crash(report("boom"))).unwrap();
        assert_eq!(post(&state, "/v1/crashes", body, None).await, 202);

        let junk = state.store().crashes_dir().join("deadbeef").join("notes.txt");
        std::fs::create_dir_all(junk.parent().unwrap()).unwrap();
        std::fs::write(&junk, "not a report").unwrap();
        std::fs::write(state.store().crashes_dir().join("deadbeef").join("bad.json"), "{").unwrap();

        let summaries = state.store().summaries().unwrap();
        assert_eq!(summaries.len(), 1, "only the readable group is summarised");
    }

    #[tokio::test]
    async fn a_fingerprint_cannot_escape_the_store_directory() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);

        let mut value = serde_json::to_value(QueuedItem::crash(report("boom"))).unwrap();
        value["value"]["fingerprint"] = serde_json::json!("../../etc");
        let body = serde_json::to_vec(&value).unwrap();
        assert_eq!(post(&state, "/v1/crashes", body, None).await, 202);

        // The sanitised name stays inside the crashes directory.
        let groups: Vec<_> = std::fs::read_dir(state.store().crashes_dir())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(groups, ["etc"]);
        assert!(!dir.path().join("etc").exists());
    }

    #[test]
    fn a_summary_sorts_the_newest_crash_first() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());

        let mut old = report("old");
        old.captured_at = Utc::now() - chrono::Duration::days(1);
        let mut new = report("new");
        new.captured_at = Utc::now();

        store.put_crash(&old).unwrap();
        store.put_crash(&new).unwrap();

        let summaries = store.summaries().unwrap();
        assert_eq!(summaries[0].message, "new");
        assert_eq!(summaries[1].message, "old");
    }
}
