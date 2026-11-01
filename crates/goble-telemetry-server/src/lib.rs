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
//! - **It is idempotent.** An event is stored under its own `event_id`, and a
//!   marker remembers that id, so a retry after a timeout is one row rather
//!   than two. Nothing a client can do short of lying about the id inflates a
//!   count.
//! - **It keeps the client's clock.** `occurred_at` says when the event
//!   happened and decides its day; `received_at` says when the collector got
//!   it. An event queued offline for two days lands in the day it happened.
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
//!   events/dt=YYYY-MM-DD/<event_id>.json     one file per event, day from occurred_at
//!   events/seen/<event_id>                   idempotency marker, one per id
//! ```
//!
//! When `GOBLE_TELEMETRY_S3_BUCKET` is set the same items are copied to the
//! bucket under `raw/` (see [`sink`]). Local disk stays the source of truth, and
//! a sink write that fails makes the request a retryable 5xx.

pub mod sink;

use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use chrono::{DateTime, NaiveDate, Utc};
use serde::de;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use goble_telemetry::{CrashReport, EventEnvelope, LegacyEventV1, LEGACY_SCHEMA, SCHEMA};

use sink::{Sink, SinkFailure};

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

/// One event as the collector stores it: the client's envelope, plus the
/// collector's own receipt stamp.
///
/// It serialises flat, with `received_at` alongside the envelope's fields, so a
/// row in the local store has the same shape as an object in the bucket and
/// one Athena table can read both.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvent {
    pub received_at: DateTime<Utc>,
    pub envelope: EventEnvelope,
}

impl StoredEvent {
    pub fn new(envelope: EventEnvelope, received_at: DateTime<Utc>) -> Self {
        Self { received_at, envelope }
    }

    pub fn occurred_at(&self) -> DateTime<Utc> {
        self.envelope.occurred_at
    }

    /// The day this event belongs to: the day it happened, not the day it
    /// arrived.
    pub fn day(&self) -> NaiveDate {
        self.occurred_at().date_naive()
    }
}

impl Serialize for StoredEvent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error as _;

        let envelope = serde_json::to_value(&self.envelope).map_err(S::Error::custom)?;
        let serde_json::Value::Object(fields) = envelope else {
            return Err(S::Error::custom("an envelope serialises to an object"));
        };

        let mut map = serializer.serialize_map(Some(fields.len() + 1))?;
        map.serialize_entry("received_at", &self.received_at)?;
        for (key, value) in fields {
            map.serialize_entry(&key, &value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for StoredEvent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let received_at = value
            .get("received_at")
            .cloned()
            .ok_or_else(|| de::Error::missing_field("received_at"))?;
        let received_at = serde_json::from_value(received_at).map_err(de::Error::custom)?;
        // `received_at` rides along in the envelope's fields; the envelope
        // ignores what it does not know, exactly as it does on the wire.
        let envelope = serde_json::from_value(value).map_err(de::Error::custom)?;
        Ok(Self { received_at, envelope })
    }
}

/// What storing an event found: a new row, or one that was already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stored {
    New,
    Duplicate,
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

    /// The directory holding one day's events, named by `occurred_at`.
    pub fn event_day_dir(&self, day: NaiveDate) -> PathBuf {
        self.events_dir().join(format!("dt={day}"))
    }

    /// The directory of idempotency markers, one file per `event_id` seen.
    pub fn seen_dir(&self) -> PathBuf {
        self.events_dir().join("seen")
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

    /// Store one event under the day it happened.
    ///
    /// The first write of an `event_id` creates its marker and its file. A
    /// second write of the same id is a [`Stored::Duplicate`]: the file is left
    /// alone, so the day holds one row per event even if the client retried.
    pub fn put_event(&self, event: &StoredEvent) -> Result<Stored, StoreError> {
        let seen = self.seen_dir();
        std::fs::create_dir_all(&seen).map_err(|source| StoreError::Io {
            path: seen.clone(),
            source,
        })?;

        let id = sanitise(&event.envelope.event_id);
        let marker = seen.join(&id);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&marker) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Ok(Stored::Duplicate);
            }
            Err(source) => return Err(StoreError::Io { path: marker, source }),
        }

        let dir = self.event_day_dir(event.day());
        let path = dir.join(format!("{id}.json"));
        let body = serde_json::to_vec(event)?;
        let written = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&path, &body))
            .map_err(|source| StoreError::Io { path: path.clone(), source });
        if let Err(error) = written {
            // The marker must not outlive a write that did not happen, or the
            // retry would look like a duplicate and the event would be lost.
            let _ = std::fs::remove_file(&marker);
            return Err(error);
        }
        Ok(Stored::New)
    }

    /// Every event stored for one day, in id order. Unreadable files are
    /// skipped the way unreadable crash files are.
    pub fn events_on(&self, day: NaiveDate) -> Result<Vec<StoredEvent>, StoreError> {
        let dir = self.event_day_dir(day);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Ok(Vec::new());
        };

        let mut events: Vec<StoredEvent> = entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
            .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
            .filter_map(|text| serde_json::from_str(&text).ok())
            .collect();
        events.sort_by(|left, right| left.envelope.event_id.cmp(&right.envelope.event_id));
        Ok(events)
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
    /// A copy destination beyond the local store; `None` means local only.
    sink: Option<Arc<dyn Sink>>,
}

impl ServerState {
    pub fn new(store: Store, token: Option<String>) -> Self {
        Self::with_sink(store, token, None)
    }

    pub fn with_sink(store: Store, token: Option<String>, sink: Option<Arc<dyn Sink>>) -> Self {
        let token = token.filter(|value| !value.trim().is_empty());
        Self { store, token, sink }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn sink(&self) -> Option<&dyn Sink> {
        self.sink.as_deref()
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
        // `/health` stays for the probes that already point at it.
        .route("/health", get(health))
        .route("/v1/health", get(health))
        .route("/v1/crashes", post(ingest_crash))
        .route("/v1/events", post(ingest_event))
        .route("/v1/summary", get(summary))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(state)
}

/// Liveness and sink reachability.
///
/// It never authenticates and never names a credential: an unconfigured sink is
/// `configured: false` with no probe, a configured one is probed and reduced to
/// a boolean plus a fixed reason label.
async fn health(State(state): State<ServerState>) -> Response {
    let s3 = match state.sink() {
        None => serde_json::json!({ "configured": false, "reachable": null }),
        Some(sink) => match sink.probe().await {
            Ok(()) => serde_json::json!({ "configured": true, "reachable": true }),
            Err(failure) => serde_json::json!({
                "configured": true,
                "reachable": false,
                "reason": failure.as_str(),
            }),
        },
    };

    Json(serde_json::json!({ "status": "ok", "schema": SCHEMA, "s3": s3 })).into_response()
}

/// An ingest answer. Kept tiny: the client only needs to know whether to retry.
#[derive(Debug, Serialize)]
struct Ingested {
    accepted: usize,
}

/// One item as it arrives, before its schema is known.
///
/// The event payload stays untyped until its `schema` says which shape it is:
/// a schema-2 envelope, or the bare event a schema-1 client sent.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum IncomingItem {
    Crash(Box<CrashReport>),
    Event(Box<serde_json::Value>),
}

/// Bring an event payload up to the schema-2 envelope.
///
/// `Err` is a body this collector cannot read and a 400 to the client.
/// `Ok(None)` is a schema-1 event that cannot be represented honestly (a
/// free-form feature name outside the catalog): it is accepted and logged, so a
/// released v1 client stops retrying something no build can store.
fn normalise_event(
    value: serde_json::Value,
    received_at: DateTime<Utc>,
) -> Result<Option<EventEnvelope>, String> {
    let schema = value.get("schema").and_then(serde_json::Value::as_u64);

    match schema {
        // A schema-1 event has no envelope at all.
        Some(schema) if schema == u64::from(LEGACY_SCHEMA) => {
            let legacy: LegacyEventV1 = serde_json::from_value(value)
                .map_err(|err| format!("malformed schema-1 event: {err}"))?;
            Ok(legacy.into_envelope(received_at))
        }
        Some(schema) if schema == u64::from(SCHEMA) => {
            let envelope: EventEnvelope = serde_json::from_value(value)
                .map_err(|err| format!("malformed schema-2 event: {err}"))?;
            Ok(Some(envelope))
        }
        Some(schema) => Err(format!(
            "unsupported schema {schema}: this collector speaks {SCHEMA} and accepts {LEGACY_SCHEMA}"
        )),
        // Nothing said "schema", so this is a bare event from a v1 client.
        None => {
            let legacy: LegacyEventV1 = serde_json::from_value(value)
                .map_err(|err| format!("malformed schema-1 event: {err}"))?;
            Ok(legacy.into_envelope(received_at))
        }
    }
}

fn rejected(status: StatusCode, message: &str) -> Response {
    (status, Json(serde_json::json!({ "error": message }))).into_response()
}

/// A sink write that did not happen.
///
/// The status is retryable on purpose: the client keeps the item and sends it
/// again, and the second attempt overwrites the same object. Acknowledging now
/// would lose the copy until somebody noticed the bucket was thin.
fn sink_failed(failure: SinkFailure) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::RETRY_AFTER, "5")],
        Json(serde_json::json!({
            "error": "the collector could not copy the item to the object store",
            "reason": failure.as_str(),
        })),
    )
        .into_response()
}

async fn ingest_crash(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !state.authorised(&headers) {
        return rejected(StatusCode::UNAUTHORIZED, "a collector token is required");
    }

    let item: IncomingItem = match serde_json::from_slice(&body) {
        Ok(item) => item,
        Err(err) => return rejected(StatusCode::BAD_REQUEST, &format!("malformed report: {err}")),
    };

    let IncomingItem::Crash(report) = item else {
        return rejected(StatusCode::BAD_REQUEST, "this endpoint takes crash reports");
    };

    // A schema-1 client's report is still a real crash; refusing it would throw
    // away the reports the release that shipped first is sending.
    if !(LEGACY_SCHEMA..=SCHEMA).contains(&report.schema) {
        return rejected(
            StatusCode::BAD_REQUEST,
            &format!("unsupported schema {}: this collector speaks {SCHEMA}", report.schema),
        );
    }

    match state.store.put_crash(&report) {
        Ok(_) => {}
        Err(err) => {
            tracing::error!("could not store a crash report: {err}");
            return rejected(StatusCode::INTERNAL_SERVER_ERROR, "could not store the report");
        }
    }

    if let Some(sink) = state.sink() {
        if let Err(failure) = sink.put_crash(&report).await {
            return sink_failed(failure);
        }
    }

    tracing::info!(fingerprint = %report.fingerprint, version = %report.context.app_version, "stored a crash report");
    (StatusCode::ACCEPTED, Json(Ingested { accepted: 1 })).into_response()
}

async fn ingest_event(
    State(state): State<ServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !state.authorised(&headers) {
        return rejected(StatusCode::UNAUTHORIZED, "a collector token is required");
    }

    let item: IncomingItem = match serde_json::from_slice(&body) {
        Ok(item) => item,
        Err(err) => return rejected(StatusCode::BAD_REQUEST, &format!("malformed event: {err}")),
    };

    let IncomingItem::Event(value) = item else {
        return rejected(StatusCode::BAD_REQUEST, "this endpoint takes events");
    };

    let received_at = Utc::now();
    let envelope = match normalise_event(*value, received_at) {
        Ok(Some(envelope)) => envelope,
        Ok(None) => {
            tracing::debug!("dropped a schema-1 event with no catalog feature name");
            return (StatusCode::ACCEPTED, Json(Ingested { accepted: 0 })).into_response();
        }
        Err(err) => return rejected(StatusCode::BAD_REQUEST, &err),
    };

    let event = StoredEvent::new(envelope, received_at);
    let stored = match state.store.put_event(&event) {
        Ok(stored) => stored,
        Err(err) => {
            tracing::error!("could not store an event: {err}");
            return rejected(StatusCode::INTERNAL_SERVER_ERROR, "could not store the event");
        }
    };

    if stored == Stored::Duplicate {
        // A retry of an event already on disk: one row, and the sink is asked
        // again because the retry may be finishing a copy that failed.
        tracing::debug!(event_id = %event.envelope.event_id, "an event was already stored");
    } else if event.envelope.is_legacy() {
        tracing::debug!(event_id = %event.envelope.event_id, "normalised a schema-1 event");
    }

    if let Some(sink) = state.sink() {
        if let Err(failure) = sink.put_event(&event).await {
            return sink_failed(failure);
        }
    }

    (StatusCode::ACCEPTED, Json(Ingested { accepted: 1 })).into_response()
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
    use goble_telemetry::{
        Channel, ClientContext, CrashKind, QueuedItem, Role, TelemetryEvent, LEGACY_EVENT_PREFIX,
    };
    use tower::ServiceExt as _;

    fn context() -> ClientContext {
        ClientContext {
            app_version: "0.1.0".into(),
            channel: Channel::Stable,
            os: "macos".into(),
            os_version: Some("15.0".into()),
            arch: "aarch64".into(),
            install_id: "install".into(),
            link_id: None,
            role: Role::Desktop,
            session_id: "session".into(),
        }
    }

    fn report(message: &str) -> CrashReport {
        CrashReport::new(&context(), CrashKind::Panic, message)
            .with_location(Some("app/src/main.rs:30".into()))
            .with_backtrace(Some("0: goble_app::main".into()))
    }

    fn event() -> EventEnvelope {
        EventEnvelope::new(context(), TelemetryEvent::AppStarted { locale: Some("en-GB".into()) })
    }

    fn state(dir: &Path, token: Option<&str>) -> ServerState {
        ServerState::new(Store::new(dir), token.map(str::to_owned))
    }

    /// A sink that always fails, standing in for an unreachable bucket.
    #[derive(Debug)]
    struct FailingSink;

    impl sink::Sink for FailingSink {
        fn put_event<'a>(&'a self, _: &'a StoredEvent) -> sink::SinkFuture<'a, ()> {
            Box::pin(async { Err(SinkFailure::Unavailable) })
        }

        fn put_crash<'a>(&'a self, _: &'a CrashReport) -> sink::SinkFuture<'a, ()> {
            Box::pin(async { Err(SinkFailure::Unavailable) })
        }

        fn probe<'a>(&'a self) -> sink::SinkFuture<'a, ()> {
            Box::pin(async { Err(SinkFailure::Unavailable) })
        }
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
    async fn an_event_is_stored_under_the_day_it_happened() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);
        let body = serde_json::to_vec(&QueuedItem::event(event())).unwrap();

        assert_eq!(post(&state, "/v1/events", body, None).await, 202);

        let today = Utc::now().date_naive();
        let stored = state.store().events_on(today).unwrap();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].received_at >= stored[0].occurred_at());
        // The file is named by the event, under the day it happened.
        let path = state
            .store()
            .event_day_dir(today)
            .join(format!("{}.json", stored[0].envelope.event_id));
        assert!(path.is_file(), "{path:?}");
    }

    #[tokio::test]
    async fn an_event_queued_offline_lands_in_the_day_it_happened() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);

        let mut envelope = event();
        envelope.occurred_at = Utc::now() - chrono::Duration::days(2);
        let day = envelope.occurred_at.date_naive();
        let body = serde_json::to_vec(&QueuedItem::event(envelope)).unwrap();

        assert_eq!(post(&state, "/v1/events", body, None).await, 202);

        assert_eq!(state.store().events_on(day).unwrap().len(), 1);
        assert!(state.store().events_on(Utc::now().date_naive()).unwrap().is_empty());
        assert!(state.store().event_day_dir(day).is_dir());
    }

    #[tokio::test]
    async fn the_same_event_twice_is_one_logical_row() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);
        let envelope = event();
        let event_id = envelope.event_id.clone();
        let body = serde_json::to_vec(&QueuedItem::event(envelope)).unwrap();

        assert_eq!(post(&state, "/v1/events", body.clone(), None).await, 202);
        assert_eq!(post(&state, "/v1/events", body, None).await, 202);

        let stored = state.store().events_on(Utc::now().date_naive()).unwrap();
        assert_eq!(stored.len(), 1, "a retry is not a second row");
        assert_eq!(stored[0].envelope.event_id, event_id);
        assert!(state.store().seen_dir().join(&event_id).is_file());
    }

    #[tokio::test]
    async fn a_schema_1_event_is_accepted_and_stored_as_a_legacy_row() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);
        // Exactly what a released v1 client sends: no envelope at all.
        let body = serde_json::to_vec(&serde_json::json!({
            "kind": "event",
            "value": {
                "event": "app_started",
                "context": {
                    "app_version": "0.1.0",
                    "channel": "stable",
                    "os": "macos",
                    "arch": "aarch64",
                    "install_id": "install-1",
                    "session_id": "session-1"
                },
                "locale": "en-GB"
            }
        }))
        .unwrap();

        assert_eq!(post(&state, "/v1/events", body, None).await, 202);

        let stored = state.store().events_on(Utc::now().date_naive()).unwrap();
        assert_eq!(stored.len(), 1);
        let row = &stored[0];
        assert!(row.envelope.event_id.starts_with(LEGACY_EVENT_PREFIX), "{}", row.envelope.event_id);
        assert_eq!(row.envelope.context.install_id, "install-1");
        assert_eq!(row.envelope.arrived_as_schema(), LEGACY_SCHEMA);
        assert_eq!(row.envelope.schema, SCHEMA);
        // A v1 client sent no clock, so the collector's receipt is both stamps.
        assert_eq!(row.envelope.occurred_at, row.received_at);
    }

    #[tokio::test]
    async fn a_v1_feature_name_outside_the_catalog_is_accepted_and_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);
        let body = serde_json::to_vec(&serde_json::json!({
            "kind": "event",
            "value": { "event": "feature_used", "name": "terminal", "duration_ms": 5 }
        }))
        .unwrap();

        assert_eq!(post(&state, "/v1/events", body, None).await, 202);
        assert!(!state.store().events_dir().exists(), "nothing is stored under an invented name");
    }

    #[tokio::test]
    async fn a_v2_event_is_stored_with_its_own_id_and_clock() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);

        let mut envelope = event();
        envelope.occurred_at = Utc::now() - chrono::Duration::days(3);
        let event_id = envelope.event_id.clone();
        let occurred_at = envelope.occurred_at;
        let body = serde_json::to_vec(&QueuedItem::event(envelope)).unwrap();

        assert_eq!(post(&state, "/v1/events", body, None).await, 202);

        let stored = state.store().events_on(occurred_at.date_naive()).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].envelope.event_id, event_id);
        assert_eq!(stored[0].envelope.occurred_at, occurred_at);
        assert!(!stored[0].envelope.is_legacy());
    }

    #[tokio::test]
    async fn the_wrong_endpoint_for_a_kind_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), None);
        let crash = serde_json::to_vec(&QueuedItem::crash(report("boom"))).unwrap();
        let event = serde_json::to_vec(&QueuedItem::event(EventEnvelope::new(
            context(),
            TelemetryEvent::AppExited { uptime_secs: 1, exit_code: None },
        )))
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

        let (status, body) = get(&state, "/v1/health", None).await;
        assert_eq!(status, 200);
        assert_eq!(body["s3"]["configured"], false);
        assert!(body["s3"]["reachable"].is_null());
    }

    #[tokio::test]
    async fn health_reports_a_sink_that_cannot_be_reached_without_naming_why_in_detail() {
        let dir = tempfile::tempdir().unwrap();
        let state = ServerState::with_sink(
            Store::new(dir.path()),
            None,
            Some(Arc::new(FailingSink)),
        );

        let (status, body) = get(&state, "/v1/health", None).await;
        assert_eq!(status, 200);
        assert_eq!(body["s3"]["configured"], true);
        assert_eq!(body["s3"]["reachable"], false);
        assert_eq!(body["s3"]["reason"], "unavailable");
        let text = body.to_string();
        for leak in ["aws", "secret", "token", "AKIA"] {
            assert!(!text.to_ascii_lowercase().contains(leak), "{text}");
        }
    }

    #[tokio::test]
    async fn a_sink_failure_is_retryable_and_leaves_the_local_row() {
        let dir = tempfile::tempdir().unwrap();
        let state =
            ServerState::with_sink(Store::new(dir.path()), None, Some(Arc::new(FailingSink)));
        let body = serde_json::to_vec(&QueuedItem::event(event())).unwrap();

        // The local store has the event, so the response is a retry rather
        // than a loss, and the retry does not add a second row.
        assert_eq!(post(&state, "/v1/events", body.clone(), None).await, 503);
        assert_eq!(post(&state, "/v1/events", body, None).await, 503);
        assert_eq!(state.store().events_on(Utc::now().date_naive()).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_sink_failure_on_a_crash_is_retryable_too() {
        let dir = tempfile::tempdir().unwrap();
        let state =
            ServerState::with_sink(Store::new(dir.path()), None, Some(Arc::new(FailingSink)));
        let body = serde_json::to_vec(&QueuedItem::crash(report("boom"))).unwrap();

        assert_eq!(post(&state, "/v1/crashes", body, None).await, 503);
        assert_eq!(state.store().summaries().unwrap().len(), 1);
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
