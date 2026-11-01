//! Getting queued items to the collector.
//!
//! Uploading is a background concern: the app should never wait for the
//! network, and a crash must be written to disk before anyone tries to send it.
//! So there is one worker thread per process, it drains the on-disk queue
//! oldest-first, and it is told when to give up on an attempt.
//!
//! Failure handling is the point of this module:
//!
//! - a transport failure (offline, DNS, TLS, a collector that is not deployed
//!   yet) keeps the item for the next attempt, so nothing is lost;
//! - a retryable HTTP status (5xx, 408, 429) counts as an attempt and is retried
//!   later;
//! - a permanent rejection (4xx that is not a timeout or a rate limit) deletes
//!   the item, because sending it again cannot change the answer;
//! - an item that has used up its attempts stays on disk for the next launch
//!   rather than being deleted, so a future build can still upload it.

use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use chrono::Utc;

use crate::queue::{CrashQueue, QueueEntry};
use crate::report::QueuedItem;

/// Why an upload did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadErrorKind {
    /// The request never reached the collector: offline, DNS, TLS, refused.
    Transport,
    /// The collector answered, but asked us to come back later.
    Retryable,
    /// The collector answered and will keep answering the same way. Do not
    /// retry this item.
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadError {
    pub kind: UploadErrorKind,
    pub message: String,
}

impl UploadError {
    pub fn transport(message: impl Into<String>) -> Self {
        Self { kind: UploadErrorKind::Transport, message: message.into() }
    }

    pub fn retryable(message: impl Into<String>) -> Self {
        Self { kind: UploadErrorKind::Retryable, message: message.into() }
    }

    pub fn rejected(message: impl Into<String>) -> Self {
        Self { kind: UploadErrorKind::Rejected, message: message.into() }
    }

    /// Whether retrying can ever succeed.
    pub fn is_permanent(&self) -> bool {
        self.kind == UploadErrorKind::Rejected
    }
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for UploadError {}

/// Sends queued items to a collector. Implemented for HTTP, and for tests.
pub trait Uploader: Send + Sync + std::fmt::Debug {
    /// `base` is the collector root; `item.endpoint_path()` is appended.
    fn upload(&self, base: &str, item: &QueuedItem) -> Result<(), UploadError>;

    /// Send several events in one request. The default falls back to one
    /// request per item, which keeps a test double simple and remains a correct
    /// (if expensive) collector client.
    fn upload_events(&self, base: &str, items: &[QueuedItem]) -> Result<(), UploadError> {
        debug_assert!(items.iter().all(|item| !item.is_crash()));
        for item in items {
            self.upload(base, item)?;
        }
        Ok(())
    }
}

/// Posts items as JSON over HTTPS.
///
/// The runtime lives here rather than being borrowed from the app, because the
/// caller is a plain thread that may be running while the app is shutting down.
#[derive(Debug)]
pub struct HttpUploader {
    runtime: tokio::runtime::Runtime,
    client: reqwest::Client,
}

impl HttpUploader {
    pub fn new(user_agent: impl Into<String>, timeout: Duration) -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let client = reqwest::Client::builder()
            .user_agent(user_agent.into())
            .timeout(timeout)
            .build()
            .map_err(std::io::Error::other)?;
        Ok(Self { runtime, client })
    }
}

impl HttpUploader {
    /// Post one encoded body and turn the answer into the retry policy the
    /// drain loop works from.
    fn post(&self, url: String, body: Vec<u8>) -> Result<(), UploadError> {
        let client = self.client.clone();

        self.runtime.block_on(async move {
            let response = client
                .post(&url)
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body)
                .send()
                .await
                .map_err(|err| UploadError::transport(err.to_string()))?;

            let status = response.status();
            if status.is_success() {
                return Ok(());
            }
            // "Later" includes the collector asking to slow down, and any
            // server-side fault. Everything else is final.
            if status.is_server_error()
                || status == reqwest::StatusCode::REQUEST_TIMEOUT
                || status == reqwest::StatusCode::TOO_MANY_REQUESTS
            {
                return Err(UploadError::retryable(format!("collector answered {status}")));
            }
            Err(UploadError::rejected(format!("collector answered {status}")))
        })
    }
}

impl Uploader for HttpUploader {
    fn upload(&self, base: &str, item: &QueuedItem) -> Result<(), UploadError> {
        let url = format!("{}{}", base.trim_end_matches('/'), item.endpoint_path());
        let body = serde_json::to_vec(item)
            .map_err(|err| UploadError::rejected(format!("could not encode item: {err}")))?;
        self.post(url, body)
    }

    fn upload_events(&self, base: &str, items: &[QueuedItem]) -> Result<(), UploadError> {
        if items.is_empty() {
            return Ok(());
        }
        debug_assert!(items.iter().all(|item| !item.is_crash()));

        let url = format!("{}/v1/events", base.trim_end_matches('/'));
        let body = serde_json::to_vec(items)
            .map_err(|err| UploadError::rejected(format!("could not encode the batch: {err}")))?;
        self.post(url, body)
    }
}

/// How the worker behaves.
#[derive(Debug, Clone)]
pub struct UploadConfig {
    /// Collector root.
    pub base: String,
    /// Attempts per item within one run of the app.
    pub max_attempts: u32,
    /// Events carried by one request. Crashes always travel alone.
    pub max_batch: usize,
    /// Fewest pending items worth a request of their own. A queue below this
    /// waits for the next drain, unless a crash is waiting or the oldest item
    /// has waited `max_wait`.
    pub min_batch: usize,
    /// Longest an item waits for a batch to fill. Bounds telemetry latency and
    /// keeps a quiet client from writing one object per minute.
    pub max_wait: Duration,
    /// Longest the worker will sleep between drains. The sleep doubles after a
    /// drain that failed, and drops back to `interval` after one that did not,
    /// so a collector that is down is not hammered.
    pub backoff: Duration,
    /// How often the worker wakes up to drain the queue on its own.
    pub interval: Duration,
}

/// What one drain did. Counts are items, not requests: one batched request can
/// move a hundred events.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UploadOutcome {
    pub uploaded: usize,
    /// Deleted because the collector will never accept them.
    pub rejected: usize,
    /// Attempted and failed; still queued.
    pub failed: usize,
    /// Left alone: out of attempts, or behind a failure that stopped the drain.
    pub skipped: usize,
    /// Entries still on disk when the drain finished.
    pub remaining: usize,
}

impl UploadOutcome {
    pub fn is_clean(&self) -> bool {
        self.failed == 0 && self.rejected == 0
    }
}

/// Upload as much of the queue as a single pass allows, oldest first.
///
/// Stops at the first retryable failure: if the network is down, walking the
/// rest of the queue only produces more timeouts.
pub fn drain(queue: &CrashQueue, uploader: &dyn Uploader, config: &UploadConfig) -> UploadOutcome {
    let mut outcome = UploadOutcome::default();
    let entries = queue.entries();
    let batch_limit = config.max_batch.max(1);
    let mut index = 0;

    while index < entries.len() {
        if entries[index].1.attempts >= config.max_attempts {
            outcome.skipped += 1;
            index += 1;
            continue;
        }

        // A crash is rare and large: it travels alone and never shares a
        // request with events.
        if entries[index].1.item.is_crash() {
            let (path, entry) = &entries[index];
            match uploader.upload(&config.base, &entry.item) {
                Ok(()) => {
                    remove(queue, path);
                    outcome.uploaded += 1;
                    index += 1;
                }
                Err(err) if err.is_permanent() => {
                    remove(queue, path);
                    log::debug!("telemetry: collector rejected a {}: {}", entry.item.label(), err);
                    outcome.rejected += 1;
                    index += 1;
                }
                Err(err) => {
                    let mut entry = entry.clone();
                    record_attempt(queue, path, &mut entry);
                    log::debug!("telemetry: upload failed, keeping the item: {err}");
                    outcome.failed += 1;

                    // One failure means the rest of this pass is probably the
                    // same failure, so stop instead of hammering a dead
                    // connection.
                    outcome.skipped += entries.len() - index - 1;
                    break;
                }
            }
            continue;
        }

        // A run of events travels as one request. An item out of attempts ends
        // the run here; the outer loop then skips it.
        let start = index;
        while index < entries.len()
            && !entries[index].1.item.is_crash()
            && entries[index].1.attempts < config.max_attempts
            && index - start < batch_limit
        {
            index += 1;
        }

        let batch = &entries[start..index];
        let items: Vec<QueuedItem> = batch.iter().map(|(_, entry)| entry.item.clone()).collect();

        match uploader.upload_events(&config.base, &items) {
            Ok(()) => {
                for (path, _) in batch {
                    remove(queue, path);
                }
                outcome.uploaded += items.len();
            }
            Err(err) if err.is_permanent() => {
                for (path, _) in batch {
                    remove(queue, path);
                }
                log::debug!("telemetry: collector rejected a batch of {}: {err}", items.len());
                outcome.rejected += items.len();
            }
            Err(err) => {
                // Every item in the batch gets one attempt. A retry sends the
                // same batch, and the collector's idempotency key makes that
                // one row rather than two.
                for (path, entry) in batch {
                    let mut entry = entry.clone();
                    record_attempt(queue, path, &mut entry);
                }
                log::debug!(
                    "telemetry: batch upload failed, keeping {} items: {err}",
                    items.len()
                );
                outcome.failed += items.len();
                outcome.skipped += entries.len() - index;
                break;
            }
        }
    }

    outcome.remaining = queue.len();
    outcome
}

fn remove(queue: &CrashQueue, path: &Path) {
    if let Err(err) = queue.remove(path) {
        log::warn!("telemetry: could not delete {}: {err}", path.display());
    }
}

fn record_attempt(queue: &CrashQueue, path: &Path, entry: &mut QueueEntry) {
    if let Err(err) = queue.record_attempt(path, entry) {
        log::warn!("telemetry: could not record an attempt: {err}");
    }
}

/// Whether the queue has waited long enough for an automatic drain.
///
/// A request is cheap; an object is the part that adds up. Waiting for a batch
/// keeps a quiet client from writing one object a minute, and `max_wait` bounds
/// how long an event can sit on disk when the client really is quiet.
fn due(queue: &CrashQueue, config: &UploadConfig) -> bool {
    let entries = queue.entries();
    let pending: Vec<&QueueEntry> = entries
        .iter()
        .filter(|(_, entry)| entry.attempts < config.max_attempts)
        .map(|(_, entry)| entry)
        .collect();

    let Some(oldest) = pending.first() else {
        return false;
    };

    // A crash should not wait behind a batch that has not filled.
    if pending.iter().any(|entry| entry.item.is_crash()) {
        return true;
    }
    if pending.len() >= config.min_batch.max(1) {
        return true;
    }

    let age = Utc::now().signed_duration_since(oldest.queued_at);
    age.to_std().unwrap_or_default() >= config.max_wait
}

/// Drain only when the queue is worth a request. `None` means the worker looked
/// and decided to wait; `Some` is what a forced drain would have returned.
pub fn drain_if_due(
    queue: &CrashQueue,
    uploader: &dyn Uploader,
    config: &UploadConfig,
) -> Option<UploadOutcome> {
    due(queue, config).then(|| drain(queue, uploader, config))
}

/// How long to wait before the next automatic drain: doubles while drains keep
/// failing, up to the ceiling, and resets as soon as one succeeds.
fn advance_wait(current: Duration, config: &UploadConfig, outcome: &UploadOutcome) -> Duration {
    let base = config.interval.max(Duration::from_millis(1));
    let ceiling = config.backoff.max(base);
    if outcome.failed == 0 {
        base
    } else {
        current.saturating_mul(2).min(ceiling)
    }
}

enum WorkerRequest {
    Flush(Sender<UploadOutcome>),
    Shutdown(Sender<UploadOutcome>),
}

/// The background thread that owns uploading for the process.
#[derive(Debug)]
pub struct UploadWorker {
    requests: Sender<WorkerRequest>,
    handle: Option<JoinHandle<()>>,
}

impl UploadWorker {
    pub fn spawn(
        queue: Arc<CrashQueue>,
        uploader: Arc<dyn Uploader>,
        config: UploadConfig,
    ) -> Self {
        let (requests, receiver) = mpsc::channel::<WorkerRequest>();
        let handle = std::thread::Builder::new()
            .name("goble-telemetry-upload".to_owned())
            .spawn(move || {
                let mut wait = config.interval.max(Duration::from_millis(1));
                loop {
                    match receiver.recv_timeout(wait) {
                        Ok(WorkerRequest::Flush(reply)) => {
                            let outcome = drain(&queue, uploader.as_ref(), &config);
                            wait = advance_wait(wait, &config, &outcome);
                            let _ = reply.send(outcome);
                        }
                        Ok(WorkerRequest::Shutdown(reply)) => {
                            let _ = reply.send(drain(&queue, uploader.as_ref(), &config));
                            break;
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            if let Some(outcome) =
                                drain_if_due(&queue, uploader.as_ref(), &config)
                            {
                                wait = advance_wait(wait, &config, &outcome);
                            }
                        }
                        // Nothing holds the sender any more.
                        Err(RecvTimeoutError::Disconnected) => {
                            drain(&queue, uploader.as_ref(), &config);
                            break;
                        }
                    }
                }
            });

        let handle = match handle {
            Ok(handle) => Some(handle),
            Err(err) => {
                log::warn!("telemetry: could not start the upload thread: {err}");
                None
            }
        };

        Self { requests, handle }
    }

    /// Ask for a drain and wait up to `timeout`. `None` means the worker did
    /// not answer in time; the items stay on disk either way.
    pub fn flush(&self, timeout: Duration) -> Option<UploadOutcome> {
        let (reply, receiver) = mpsc::channel();
        self.requests.send(WorkerRequest::Flush(reply)).ok()?;
        receiver.recv_timeout(timeout).ok()
    }

    /// Drain one last time and stop the thread.
    pub fn shutdown(mut self, timeout: Duration) -> Option<UploadOutcome> {
        let outcome = {
            let (reply, receiver) = mpsc::channel();
            match self.requests.send(WorkerRequest::Shutdown(reply)) {
                Ok(()) => receiver.recv_timeout(timeout).ok(),
                Err(_) => None,
            }
        };
        if let Some(handle) = self.handle.take() {
            // The worker breaks out of its loop right after replying, so this
            // returns immediately; a detached join would leak the thread name.
            let _ = handle.join();
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Channel, Role};
    use crate::queue::QueueEntry;
    use crate::report::{ClientContext, CrashKind, CrashReport, TelemetryEvent};
    use chrono::Utc;
    use std::sync::Mutex;

    fn context() -> ClientContext {
        ClientContext {
            app_version: "0.1.0".into(),
            channel: Channel::Dev,
            os: "macos".into(),
            os_version: None,
            arch: "aarch64".into(),
            install_id: "install".into(),
            link_id: None,
            role: Role::Desktop,
            session_id: "session".into(),
        }
    }

    fn crash(message: &str) -> QueuedItem {
        QueuedItem::crash(CrashReport::new(&context(), CrashKind::Panic, message))
    }

    fn queue(dir: &std::path::Path) -> CrashQueue {
        CrashQueue::new(dir, 50, Duration::from_secs(60 * 60))
    }

    fn config() -> UploadConfig {
        UploadConfig {
            base: "http://collector.invalid".into(),
            max_attempts: 3,
            max_batch: 100,
            min_batch: 1,
            max_wait: Duration::from_secs(1),
            backoff: Duration::from_secs(1),
            interval: Duration::from_millis(20),
        }
    }

    fn event() -> QueuedItem {
        QueuedItem::event(crate::EventEnvelope::new(
            context(),
            TelemetryEvent::AppExited { uptime_secs: 1, exit_code: None },
        ))
    }

    /// Records what it was asked to send and answers from a script.
    #[derive(Debug, Default)]
    struct FakeUploader {
        seen: Mutex<Vec<QueuedItem>>,
        answers: Mutex<Vec<Result<(), UploadError>>>,
    }

    impl FakeUploader {
        fn always_ok() -> Self {
            Self::default()
        }

        fn answering(answers: Vec<Result<(), UploadError>>) -> Self {
            Self { seen: Mutex::new(Vec::new()), answers: Mutex::new(answers) }
        }

        fn seen(&self) -> Vec<QueuedItem> {
            self.seen.lock().unwrap().clone()
        }
    }

    impl Uploader for FakeUploader {
        fn upload(&self, _base: &str, item: &QueuedItem) -> Result<(), UploadError> {
            self.seen.lock().unwrap().push(item.clone());
            let mut answers = self.answers.lock().unwrap();
            if answers.is_empty() {
                Ok(())
            } else {
                answers.remove(0)
            }
        }
    }

    /// Records whole batches instead of individual items.
    #[derive(Debug, Default)]
    struct BatchUploader {
        batches: Mutex<Vec<Vec<QueuedItem>>>,
        answers: Mutex<Vec<Result<(), UploadError>>>,
    }

    impl BatchUploader {
        fn answering(answers: Vec<Result<(), UploadError>>) -> Self {
            Self { batches: Mutex::new(Vec::new()), answers: Mutex::new(answers) }
        }

        fn batches(&self) -> Vec<Vec<QueuedItem>> {
            self.batches.lock().unwrap().clone()
        }
    }

    impl Uploader for BatchUploader {
        fn upload(&self, _base: &str, _item: &QueuedItem) -> Result<(), UploadError> {
            panic!("an event must not travel as a single-item request");
        }

        fn upload_events(&self, _base: &str, items: &[QueuedItem]) -> Result<(), UploadError> {
            self.batches.lock().unwrap().push(items.to_vec());
            let mut answers = self.answers.lock().unwrap();
            if answers.is_empty() {
                Ok(())
            } else {
                answers.remove(0)
            }
        }
    }

    #[test]
    fn events_travel_in_batches() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        for _ in 0..3 {
            queue.enqueue(event()).unwrap();
        }

        let uploader = BatchUploader::default();
        let outcome = drain(&queue, &uploader, &UploadConfig { max_batch: 2, ..config() });

        assert_eq!(outcome.uploaded, 3);
        assert_eq!(outcome.remaining, 0);
        assert!(queue.is_empty());

        let batches = uploader.batches();
        assert_eq!(batches.len(), 2, "three events at two per request is two requests");
        assert_eq!(batches[0].len(), 2);
        assert_eq!(batches[1].len(), 1);
    }

    #[test]
    fn a_small_fresh_queue_waits_for_a_batch() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        queue.enqueue(event()).unwrap();

        let uploader = BatchUploader::default();
        let config = UploadConfig {
            min_batch: 20,
            max_wait: Duration::from_secs(3600),
            ..config()
        };

        assert!(drain_if_due(&queue, &uploader, &config).is_none());
        assert_eq!(queue.len(), 1);
        assert!(uploader.batches().is_empty());
    }

    #[test]
    fn a_queue_that_waited_long_enough_drains_anyway() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        let mut entry = QueueEntry::new(event());
        entry.queued_at = Utc::now() - chrono::Duration::hours(2);
        queue.enqueue_entry(entry).unwrap();

        let uploader = BatchUploader::default();
        let config = UploadConfig {
            min_batch: 20,
            max_wait: Duration::from_secs(600),
            ..config()
        };

        let outcome = drain_if_due(&queue, &uploader, &config).expect("an old item is due");
        assert_eq!(outcome.uploaded, 1);
        assert!(queue.is_empty());
    }

    #[test]
    fn a_crash_does_not_wait_for_a_batch() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        queue.enqueue(crash("boom")).unwrap();

        let uploader = FakeUploader::always_ok();
        let config = UploadConfig {
            min_batch: 20,
            max_wait: Duration::from_secs(3600),
            ..config()
        };

        let outcome = drain_if_due(&queue, &uploader, &config).expect("a crash is due");
        assert_eq!(outcome.uploaded, 1);
        assert!(queue.is_empty());
    }

    #[test]
    fn a_failed_batch_keeps_every_item_and_counts_one_attempt_each() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        for _ in 0..3 {
            queue.enqueue(event()).unwrap();
        }

        let uploader =
            BatchUploader::answering(vec![Err(UploadError::transport("offline"))]);
        let outcome = drain(&queue, &uploader, &config());

        assert_eq!(outcome.uploaded, 0);
        assert_eq!(outcome.failed, 3);
        assert_eq!(outcome.remaining, 3);
        assert_eq!(queue.len(), 3);
        assert!(queue.entries().iter().all(|(_, entry)| entry.attempts == 1));
    }

    #[test]
    fn a_clean_drain_empties_the_queue() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        queue.enqueue(crash("one")).unwrap();
        queue.enqueue(crash("two")).unwrap();

        let uploader = FakeUploader::always_ok();
        let outcome = drain(&queue, &uploader, &config());

        assert_eq!(outcome.uploaded, 2);
        assert_eq!(outcome.remaining, 0);
        assert!(outcome.is_clean());
        assert!(queue.is_empty());
        assert_eq!(uploader.seen().len(), 2);
    }

    #[test]
    fn an_empty_queue_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = drain(&queue(dir.path()), &FakeUploader::always_ok(), &config());
        assert_eq!(outcome, UploadOutcome::default());
    }

    #[test]
    fn a_transport_failure_keeps_the_item_and_counts_the_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        queue.enqueue(crash("boom")).unwrap();

        let uploader = FakeUploader::answering(vec![Err(UploadError::transport("offline"))]);
        let outcome = drain(&queue, &uploader, &config());

        assert_eq!(outcome.uploaded, 0);
        assert_eq!(outcome.failed, 1);
        assert_eq!(outcome.remaining, 1);
        assert_eq!(queue.entries()[0].1.attempts, 1);
    }

    #[test]
    fn a_permanent_rejection_deletes_the_item() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        queue.enqueue(crash("boom")).unwrap();

        let uploader = FakeUploader::answering(vec![Err(UploadError::rejected("HTTP 400"))]);
        let outcome = drain(&queue, &uploader, &config());

        assert_eq!(outcome.rejected, 1);
        assert_eq!(outcome.remaining, 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn an_item_out_of_attempts_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        let mut entry = QueueEntry::new(crash("boom"));
        entry.attempts = 3;
        queue.enqueue_entry(entry).unwrap();

        let uploader = FakeUploader::always_ok();
        let outcome = drain(&queue, &uploader, &config());

        assert_eq!(outcome.skipped, 1);
        assert_eq!(outcome.uploaded, 0);
        assert_eq!(queue.len(), 1, "an out-of-attempts item must survive for a later build");
        assert!(uploader.seen().is_empty());
    }

    #[test]
    fn a_failure_stops_the_pass_instead_of_hammering_the_collector() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        for index in 0..4 {
            let mut entry = QueueEntry::new(crash(&format!("boom {index}")));
            entry.queued_at = Utc::now() + chrono::Duration::seconds(index);
            queue.enqueue_entry(entry).unwrap();
        }

        let uploader = FakeUploader::answering(vec![Err(UploadError::transport("offline"))]);
        let outcome = drain(&queue, &uploader, &config());

        assert_eq!(outcome.failed, 1);
        assert_eq!(uploader.seen().len(), 1);
        assert_eq!(outcome.remaining, 4);
    }

    #[test]
    fn a_retryable_status_is_counted_and_kept() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        queue.enqueue(crash("boom")).unwrap();

        let uploader = FakeUploader::answering(vec![Err(UploadError::retryable("HTTP 503"))]);
        let outcome = drain(&queue, &uploader, &config());

        assert_eq!(outcome.failed, 1);
        assert_eq!(queue.len(), 1);
        assert!(!UploadError::retryable("x").is_permanent());
        assert!(UploadError::rejected("x").is_permanent());
    }

    #[test]
    fn events_are_uploaded_to_their_own_path() {
        let item = QueuedItem::event(crate::EventEnvelope::new(
            context(),
            TelemetryEvent::AppExited { uptime_secs: 1, exit_code: None },
        ));
        assert_eq!(item.endpoint_path(), "/v1/events");
    }

    #[test]
    fn the_worker_drains_on_request() {
        let dir = tempfile::tempdir().unwrap();
        let queue = Arc::new(queue(dir.path()));
        queue.enqueue(crash("boom")).unwrap();

        let uploader = Arc::new(FakeUploader::always_ok());
        let worker = UploadWorker::spawn(queue.clone(), uploader.clone(), config());

        let outcome = worker.flush(Duration::from_secs(5)).expect("the worker answered");
        assert_eq!(outcome.uploaded, 1);
        assert!(queue.is_empty());

        // Nothing left to send, so the shutdown drain is a no-op that still
        // reports back.
        let final_outcome = worker.shutdown(Duration::from_secs(5)).expect("the worker stopped");
        assert_eq!(final_outcome.uploaded, 0);
        assert_eq!(final_outcome.remaining, 0);
    }

    #[test]
    fn a_dropped_worker_stops_on_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let queue = Arc::new(queue(dir.path()));
        queue.enqueue(crash("boom")).unwrap();

        let worker = UploadWorker::spawn(
            queue.clone(),
            Arc::new(FakeUploader::always_ok()),
            UploadConfig { interval: Duration::from_millis(10), ..config() },
        );
        // The periodic drain should pick the item up without being asked.
        for _ in 0..100 {
            if queue.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(queue.is_empty(), "the periodic drain never ran");
        drop(worker);
    }
}
