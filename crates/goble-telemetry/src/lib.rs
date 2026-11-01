//! Crash reporting and telemetry for Goble.
//!
//! # What this is
//!
//! A user runs into a bug, the app closes, and nothing is known about it. This
//! crate makes that observable without asking anyone to log in and without
//! turning an open-source desktop app into a surveillance surface:
//!
//! - **A crash is written to disk first.** Every panic produces a JSON report
//!   under `~/.goble/crashes/`, whether or not anything is ever uploaded. A
//!   user can attach that file to an issue, which is the only path that works
//!   when a collector is unreachable.
//! - **Uploading is a separate, consent-gated decision.** Local capture is
//!   always on; sending is opt-in on dev builds and on by default for released
//!   channels, and `DO_NOT_TRACK` forces it off everywhere.
//! - **Reports carry a fixed field set.** Version, channel, platform, an
//!   anonymous install id, a fingerprint, the panic text, a backtrace and the
//!   recent log tail. No user content by construction, and every free-text
//!   field is scrubbed by [`redact::Redactor`] as a second net.
//! - **Nothing is lost when the network is not there.** Items wait in a bounded
//!   on-disk queue and are retried on the next run.
//!
//! # Wiring
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use goble_telemetry::{Channel, Telemetry, TelemetryConfig, TelemetryPaths};
//!
//! let home = std::path::PathBuf::from(std::env::var("HOME")?).join(".goble");
//! let config = TelemetryConfig::new("0.1.0", Channel::Stable, TelemetryPaths::under(home))
//!     .resolved(&goble_telemetry::ProcessEnv);
//!
//! let telemetry = Telemetry::start(config)?;
//! telemetry.install_logger(
//!     Box::new(env_logger_stub()),
//!     log::LevelFilter::Info,
//! )?;
//! telemetry.clone().install_panic_hook();
//! telemetry.app_started(None);
//!
//! // … on the way out:
//! telemetry.app_exited(Some(0));
//! telemetry.shutdown(std::time::Duration::from_secs(2));
//! # Ok(())
//! # }
//! # fn env_logger_stub() -> impl log::Log {
//! #     struct Nop;
//! #     impl log::Log for Nop {
//! #         fn enabled(&self, _: &log::Metadata<'_>) -> bool { false }
//! #         fn log(&self, _: &log::Record<'_>) {}
//! #         fn flush(&self) {}
//! #     }
//! #     Nop
//! # }
//! ```
//!
//! # Rules for callers
//!
//! Adding a field to a report means it will be uploaded. Before doing that, ask
//! whether a maintainer could fix the bug without it — and remember the rule in
//! `.agents/07-observability/logs.md`: do not log secrets in the first place,
//! because redaction is a net and not a guarantee.

mod breadcrumbs;
mod config;
mod features;
mod panic;
mod queue;
mod redact;
mod report;
mod upload;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

pub use breadcrumbs::{
    Breadcrumb, BreadcrumbLogger, Breadcrumbs, DEFAULT_LIMIT as BREADCRUMB_LIMIT,
};
pub use config::{
    Channel, Consent, ConsentSource, Environment, ProcessEnv, Role, TelemetryConfig,
    TelemetryPaths, TelemetryState, DEFAULT_ENDPOINT, ENV_DO_NOT_TRACK, ENV_ENABLED, ENV_ENDPOINT,
    ENV_ROLE,
};
pub use features::{Feature, Outcome};
pub use panic::install_panic_hook;
pub use queue::{CrashQueue, QueueEntry};
pub use redact::{Redactor, REDACTED};
pub use report::{
    ClientContext, CrashKind, CrashReport, EventEnvelope, LegacyEventV1, QueuedItem, ReportArchive,
    TelemetryEvent, LEGACY_EVENT_PREFIX, LEGACY_SCHEMA, SCHEMA,
};
pub use upload::{
    drain, drain_if_due, HttpUploader, UploadConfig, UploadError, UploadErrorKind, UploadOutcome,
    UploadWorker, Uploader,
};

/// Crash reports kept on this machine.
pub const ARCHIVE_LIMIT: usize = 20;
/// How often a run checks the queue. Events are batched, so a minute of
/// latency costs one request instead of one per event.
pub const DRAIN_INTERVAL: Duration = Duration::from_secs(60);
/// Fewest pending items an automatic drain sends. A queue below this waits for
/// the next check, so a quiet client writes fewer, fuller objects.
pub const MIN_BATCH: usize = 20;
/// Longest an item waits for a batch to fill before the automatic drain sends
/// it anyway.
pub const MAX_WAIT: Duration = Duration::from_secs(10 * 60);
/// Longest the uploader will wait between retries.
pub const RETRY_CEILING: Duration = Duration::from_secs(10 * 60);

/// Build the upload thread from the collector settings. A client that cannot
/// be built leaves uploading off for this run rather than failing startup.
fn upload_worker(config: &TelemetryConfig, queue: &Arc<CrashQueue>) -> Option<UploadWorker> {
    match HttpUploader::new(
        format!("goble/{}", config.app_version),
        config.request_timeout,
    ) {
        Ok(uploader) => Some(UploadWorker::spawn(
            Arc::clone(queue),
            Arc::new(uploader),
            UploadConfig {
                base: config.collector().to_owned(),
                max_attempts: config.max_attempts,
                max_batch: config.max_batch,
                min_batch: MIN_BATCH,
                max_wait: MAX_WAIT,
                backoff: RETRY_CEILING,
                interval: DRAIN_INTERVAL,
            },
        )),
        Err(err) => {
            log::warn!("telemetry upload is off: {err}");
            None
        }
    }
}

/// Everything that could go wrong while starting telemetry.
///
/// Deliberately short: telemetry must not be able to stop the app from running,
/// so anything that can fail is turned into "telemetry is off" by the caller.
#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("could not prepare {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// The process-wide handle, for code that cannot be handed one.
static GLOBAL: OnceLock<Arc<Telemetry>> = OnceLock::new();

/// Publish the handle for the process. A second call is ignored.
pub fn set_global(telemetry: Arc<Telemetry>) {
    let _ = GLOBAL.set(telemetry);
}

/// The handle, if telemetry was started in this process.
pub fn global() -> Option<Arc<Telemetry>> {
    GLOBAL.get().cloned()
}

/// Record one feature against the process-wide handle, if there is one.
///
/// Instrumentation sits in code that must run the same whether telemetry
/// started or not, so the call has to be a no-op when it did not.
pub fn feature(feature: Feature, outcome: Outcome, duration: Option<Duration>) {
    if let Some(telemetry) = global() {
        telemetry.feature(feature, outcome, duration);
    }
}

/// Record a once-per-installation feature against the process-wide handle.
///
/// Returns whether this call was the one that recorded it.
pub fn feature_once(feature: Feature, outcome: Outcome, duration: Option<Duration>) -> bool {
    global().is_some_and(|telemetry| telemetry.feature_once(feature, outcome, duration))
}

/// Crash reporting and telemetry, as the rest of the app sees it.
#[derive(Debug)]
pub struct Telemetry {
    config: TelemetryConfig,
    /// The upload switch in force. Behind a lock because onboarding and the
    /// settings page can change it while the app is running.
    consent: RwLock<Consent>,
    /// The identity every event carries. Behind a lock because a pairing
    /// handshake can adopt a link id while events are being recorded.
    context: RwLock<ClientContext>,
    /// What survives a restart: install id, link id, one-shot flags.
    state: Mutex<TelemetryState>,
    state_path: PathBuf,
    redactor: Redactor,
    breadcrumbs: Arc<Breadcrumbs>,
    archive: ReportArchive,
    queue: Arc<CrashQueue>,
    worker: Mutex<Option<UploadWorker>>,
    started_at: Instant,
}

impl Telemetry {
    /// Prepare the directories, the identity and (when consent allows) the
    /// upload thread.
    pub fn start(config: TelemetryConfig) -> Result<Arc<Self>, TelemetryError> {
        config.paths.ensure().map_err(|source| TelemetryError::Io {
            path: config.paths.root().to_path_buf(),
            source,
        })?;

        let state_path = config.paths.state_path();
        let mut state = TelemetryState::load(&state_path);
        let install_id = state.ensure_install_id(&state_path);

        let mut redactor = Redactor::new();
        for secret in &config.secrets {
            redactor = redactor.with_secret(secret.clone());
        }

        if let Some(link_id) = config.link_id.clone() {
            if !state.set_link_id(&state_path, &link_id) {
                log::warn!("ignoring an empty telemetry link id");
            }
        }

        let context = ClientContext {
            app_version: config.app_version.clone(),
            channel: config.channel,
            os: config
                .os
                .clone()
                .unwrap_or_else(|| std::env::consts::OS.to_owned()),
            os_version: config.os_version.clone().or_else(detect_os_version),
            arch: config
                .arch
                .clone()
                .unwrap_or_else(|| std::env::consts::ARCH.to_owned()),
            install_id,
            link_id: state.link_id.clone(),
            role: config.role,
            session_id: config
                .session_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()),
        };

        let archive = ReportArchive::new(config.paths.reports_dir(), ARCHIVE_LIMIT);
        let queue = Arc::new(CrashQueue::new(
            config.paths.queue_dir(),
            config.max_queued,
            config.max_age,
        ));

        let consent = config.consent;
        let worker = if consent.upload {
            upload_worker(&config, &queue)
        } else {
            None
        };

        Ok(Arc::new(Self {
            consent: RwLock::new(consent),
            config,
            context: RwLock::new(context),
            state: Mutex::new(state),
            state_path,
            redactor,
            breadcrumbs: Arc::new(Breadcrumbs::new(BREADCRUMB_LIMIT)),
            archive,
            queue,
            worker: Mutex::new(worker),
            started_at: Instant::now(),
        }))
    }

    pub fn config(&self) -> &TelemetryConfig {
        &self.config
    }

    /// The anonymous identity attached to every report and event.
    pub fn context(&self) -> ClientContext {
        match self.context.read() {
            Ok(context) => context.clone(),
            // A poisoned read still holds valid data, and a telemetry lock must
            // not take the app down over an identity.
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// The link id shared with the installations this one is paired with, if
    /// any. Absent until a pairing handshake or an explicit adoption sets one.
    pub fn link_id(&self) -> Option<String> {
        self.context().link_id
    }

    /// Adopt a link id, for example one that arrived over the authenticated
    /// pairing handshake. Returns whether it was accepted.
    pub fn link_with(&self, link_id: impl AsRef<str>) -> bool {
        let link_id = link_id.as_ref().trim();
        if link_id.is_empty() {
            return false;
        }
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if !state.set_link_id(&self.state_path, link_id) {
            return false;
        }
        if let Ok(mut context) = self.context.write() {
            context.link_id = Some(link_id.to_owned());
        }
        true
    }

    /// The link id, minted and persisted on first use.
    ///
    /// This is what a desktop shares when it pairs a worker: the link exists
    /// from the moment there is somewhere to link to, and is stable after.
    pub fn ensure_link(&self) -> Option<String> {
        let Ok(mut state) = self.state.lock() else {
            return None;
        };
        let link_id = state.ensure_link_id(&self.state_path);
        if let Ok(mut context) = self.context.write() {
            context.link_id = Some(link_id.clone());
        }
        Some(link_id)
    }

    /// Forget the link id. Nothing is merged automatically, so the old link
    /// simply stops being carried; a later pairing starts a new one.
    pub fn clear_link(&self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.clear_link_id(&self.state_path);
        if let Ok(mut context) = self.context.write() {
            context.link_id = None;
        }
    }

    pub fn consent(&self) -> Consent {
        match self.consent.read() {
            Ok(consent) => *consent,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    /// Turn uploading on or off for this run and for later runs, returning the
    /// consent actually in force.
    ///
    /// The stored choice is always written, so a later launch starts from what
    /// the user picked. `DO_NOT_TRACK` and `GOBLE_TELEMETRY` still win for the
    /// current process: when one of them decided, the preference is recorded
    /// but the switch does not move.
    pub fn set_upload_consent(&self, enabled: bool) -> bool {
        if let Ok(mut state) = self.state.lock() {
            state.upload = Some(enabled);
            state.notice_shown = true;
            // Best effort: the in-memory switch below still applies when the
            // file cannot be written.
            let _ = state.store(&self.state_path);
        }

        let current = self.consent();
        if matches!(
            current.source,
            ConsentSource::Environment | ConsentSource::DoNotTrack
        ) {
            return current.upload;
        }

        let next = Consent {
            upload: enabled,
            source: ConsentSource::UserSetting,
        };
        if let Ok(mut consent) = self.consent.write() {
            *consent = next;
        }

        // Start or stop the upload thread with the switch. Taking the worker
        // out before shutting it down keeps the join off the lock.
        let stopped = self.worker.lock().ok().and_then(|mut worker| worker.take());
        if let Some(worker) = stopped {
            worker.shutdown(Duration::from_secs(2));
        }
        if enabled {
            if let Ok(mut worker) = self.worker.lock() {
                if worker.is_none() {
                    *worker = upload_worker(&self.config, &self.queue);
                }
            }
        }
        enabled
    }

    pub fn breadcrumbs(&self) -> &Arc<Breadcrumbs> {
        &self.breadcrumbs
    }

    pub fn queue(&self) -> &Arc<CrashQueue> {
        &self.queue
    }

    /// Crash reports written on this machine, newest first.
    pub fn local_reports(&self) -> Vec<PathBuf> {
        self.archive.list()
    }

    pub fn archive_dir(&self) -> &Path {
        self.archive.dir()
    }

    pub fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Install the app's logger, wrapped so its output also becomes breadcrumbs.
    ///
    /// This replaces `log::set_boxed_logger`; the app must hand its configured
    /// logger here instead of installing it directly.
    pub fn install_logger(
        &self,
        inner: Box<dyn log::Log>,
        max_level: log::LevelFilter,
    ) -> Result<(), log::SetLoggerError> {
        log::set_boxed_logger(Box::new(BreadcrumbLogger::new(
            inner,
            Arc::clone(&self.breadcrumbs),
        )))?;
        log::set_max_level(max_level);
        Ok(())
    }

    /// Capture panics. Safe to call twice: the second call chains behind the
    /// first.
    pub fn install_panic_hook(self: &Arc<Self>) {
        panic::install_panic_hook(Arc::clone(self));
    }

    /// Build a report for this process, with the recent log tail attached.
    /// Nothing is stored until [`Self::crash`] or [`Self::capture_crash`] is
    /// called.
    pub fn crash_report(&self, kind: CrashKind, message: impl Into<String>) -> CrashReport {
        CrashReport::new(&self.context(), kind, message)
            .with_breadcrumbs(self.breadcrumbs.snapshot())
    }

    /// Record a crash reported from a normal error path.
    ///
    /// Always written to the archive; queued for upload only when the user
    /// allows it.
    pub fn crash(&self, report: CrashReport) {
        self.store(report, false);
    }

    /// Record a crash from inside a panic hook.
    ///
    /// Identical to [`Self::crash`] except that it never logs: the logger's lock
    /// may be the reason the panic happened, and a deadlock there would hang the
    /// process instead of letting it die.
    pub fn capture_crash(&self, report: CrashReport) {
        self.store(report, true);
    }

    fn store(&self, report: CrashReport, in_panic: bool) {
        let report = report.redacted(&self.redactor);

        let write = || {
            if let Err(err) = self.archive.write(&report) {
                if in_panic {
                    eprintln!("goble: could not write a crash report: {err}");
                } else {
                    log::warn!("telemetry: could not write a crash report: {err}");
                }
            }

            if !self.consent().upload {
                return;
            }
            if let Err(err) = self.queue.enqueue(QueuedItem::crash(report.clone())) {
                if in_panic {
                    eprintln!("goble: could not queue a crash report: {err}");
                } else {
                    log::warn!("telemetry: could not queue a crash report: {err}");
                }
            }
            // The crash counts as a feature too, so "which crashes hurt most"
            // and "how many turns ended in a crash" come from one table.
            self.feature(Feature::Crash, Outcome::Error, None);
        };

        // A second panic while unwinding would abort, so this is the last net.
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(write)).is_err() && in_panic {
            eprintln!("goble: crash reporting failed");
        }
    }

    /// Record an event. Dropped silently when uploading is off, because an
    /// event that is never sent has no reason to be on disk.
    pub fn record(&self, event: TelemetryEvent) {
        if !self.consent().upload {
            log::debug!("telemetry: dropping an event because uploading is off");
            return;
        }
        self.enqueue_event(EventEnvelope::new(self.context(), event));
    }

    /// Record an already-built envelope. Used by callers that must keep the id
    /// and clock they were given, such as a queued item being replayed.
    pub fn record_envelope(&self, envelope: EventEnvelope) {
        if !self.consent().upload {
            log::debug!("telemetry: dropping an event because uploading is off");
            return;
        }
        self.enqueue_event(envelope);
    }

    fn enqueue_event(&self, envelope: EventEnvelope) {
        if let Err(err) = self.queue.enqueue(QueuedItem::event(envelope)) {
            log::warn!("telemetry: could not queue an event: {err}");
        }
    }

    pub fn app_started(&self, locale: Option<String>) {
        let locale = locale.or_else(|| self.config.locale.clone());
        self.record(TelemetryEvent::AppStarted { locale });
    }

    pub fn app_exited(&self, exit_code: Option<i32>) {
        let uptime = self.started_at.elapsed();
        self.record(TelemetryEvent::AppExited {
            uptime_secs: uptime.as_secs(),
            exit_code,
        });
        // How long a session lasted, in the same table as every other feature.
        self.feature(Feature::SessionSummary, Outcome::Success, Some(uptime));
    }

    /// Record one feature with its outcome and, when measured, its duration.
    ///
    /// Sampling is applied here rather than by the caller, so a rate changing
    /// in config changes one decision instead of every call site.
    pub fn feature(&self, feature: Feature, outcome: Outcome, duration: Option<Duration>) {
        if !self.sampled(feature) {
            return;
        }
        self.record(TelemetryEvent::FeatureUsed {
            feature,
            outcome,
            duration_ms: duration.map(|value| u64::try_from(value.as_millis()).unwrap_or(u64::MAX)),
        });
    }

    /// Record a feature that may happen once per installation.
    ///
    /// The flag is persisted before the event is queued, so a crash between
    /// the two loses one event instead of sending it every launch. Returns
    /// whether this call recorded it.
    pub fn feature_once(
        &self,
        feature: Feature,
        outcome: Outcome,
        duration: Option<Duration>,
    ) -> bool {
        if !self.consent().upload {
            return false;
        }
        if !self.sampled(feature) {
            return false;
        }
        let first = match self.state.lock() {
            Ok(mut state) => state.mark_once(&self.state_path, feature),
            Err(_) => false,
        };
        if !first {
            return false;
        }
        self.record(TelemetryEvent::FeatureUsed {
            feature,
            outcome,
            duration_ms: duration.map(|value| u64::try_from(value.as_millis()).unwrap_or(u64::MAX)),
        });
        true
    }

    /// Whether the configured sample rate keeps this event. A rate of 1.0 is
    /// every event; anything less is decided by a random draw, because a
    /// deterministic slice would follow installation order and be biased.
    fn sampled(&self, feature: Feature) -> bool {
        let rate = self.config.sample_rate(feature);
        if rate >= 1.0 {
            return true;
        }
        if rate <= 0.0 {
            return false;
        }
        // The uuid crate is the only randomness already in the dependency
        // graph; a v4 id is 122 random bits, and its low bits are as good a
        // draw as any for this.
        let draw = uuid::Uuid::new_v4().as_u128() as u64;
        (draw as f64 / u64::MAX as f64) < rate
    }

    /// Ask the uploader to drain the queue now, waiting up to `timeout`.
    ///
    /// `None` means uploading is off, or the worker did not answer in time.
    /// Either way the items stay on disk.
    pub fn flush(&self, timeout: Duration) -> Option<UploadOutcome> {
        let worker = self.worker.lock().ok()?;
        worker.as_ref()?.flush(timeout)
    }

    /// Drain one last time and stop the upload thread.
    pub fn shutdown(&self, timeout: Duration) -> Option<UploadOutcome> {
        let worker = { self.worker.lock().ok()?.take() };
        worker?.shutdown(timeout)
    }
}

/// Write a file in one step, so a crash in the middle cannot leave a half
/// written report behind.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path)
}

/// Best-effort OS version, used to interpret a crash. Failing is fine.
#[cfg(target_os = "macos")]
fn detect_os_version() -> Option<String> {
    let output = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (!version.is_empty()).then_some(version)
}

#[cfg(target_os = "linux")]
fn detect_os_version() -> Option<String> {
    let release = std::fs::read_to_string("/etc/os-release").ok()?;
    release.lines().find_map(|line| {
        line.strip_prefix("PRETTY_NAME=")
            .map(|value| value.trim().trim_matches('"').to_owned())
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn detect_os_version() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(dir: &Path) -> TelemetryConfig {
        TelemetryConfig::new("1.0.0", Channel::Dev, TelemetryPaths::under(dir)).without_upload()
    }

    #[test]
    fn starting_telemetry_prepares_its_directories() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(config(dir.path())).unwrap();

        assert!(dir.path().join("crashes").is_dir());
        assert!(dir.path().join("telemetry-queue").is_dir());
        assert!(dir.path().join("telemetry.json").is_file());
        assert!(!telemetry.context().install_id.is_empty());
        assert!(!telemetry.context().session_id.is_empty());
        assert_eq!(telemetry.context().os, std::env::consts::OS);
    }

    #[test]
    fn the_install_id_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let first = Telemetry::start(config(dir.path())).unwrap();
        let install_id = first.context().install_id.clone();
        drop(first);

        let second = Telemetry::start(config(dir.path())).unwrap();
        assert_eq!(second.context().install_id, install_id);
        assert_ne!(second.context().session_id, install_id);
    }

    #[test]
    fn upload_consent_can_be_turned_on_and_off_while_running() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = config(dir.path());
        config.endpoint = "http://127.0.0.1:9".into();
        config.request_timeout = Duration::from_millis(50);
        let telemetry = Telemetry::start(config).unwrap();
        assert!(!telemetry.consent().upload);

        assert!(telemetry.set_upload_consent(true));
        assert!(telemetry.consent().upload);
        assert_eq!(
            TelemetryState::load(&telemetry.config.paths.state_path()).upload,
            Some(true),
            "the choice is written for the next launch"
        );

        assert!(!telemetry.set_upload_consent(false));
        assert!(!telemetry.consent().upload);
        assert_eq!(
            TelemetryState::load(&telemetry.config.paths.state_path()).upload,
            Some(false)
        );
        telemetry.shutdown(Duration::from_millis(50));
    }

    #[test]
    fn a_crash_is_archived_even_when_uploading_is_off() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(config(dir.path())).unwrap();

        telemetry.crash(telemetry.crash_report(CrashKind::Reported, "something went wrong"));

        assert_eq!(telemetry.local_reports().len(), 1);
        assert!(
            telemetry.queue().is_empty(),
            "nothing may be uploaded without consent"
        );

        let report = std::fs::read_to_string(&telemetry.local_reports()[0]).unwrap();
        assert!(report.contains("something went wrong"), "{report}");
    }

    #[test]
    fn a_recorded_event_is_not_kept_when_uploading_is_off() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(config(dir.path())).unwrap();

        telemetry.app_started(None);
        telemetry.feature(
            Feature::TerminalCommandRun,
            Outcome::Success,
            Some(Duration::from_millis(5)),
        );
        telemetry.app_exited(Some(0));

        assert!(telemetry.queue().is_empty());
        assert!(telemetry.local_reports().is_empty());
    }

    /// The config a test uses when it wants the queue filled.
    fn uploading(dir: &Path) -> TelemetryConfig {
        let mut config = config(dir);
        config.consent = Consent {
            upload: true,
            source: ConsentSource::Default,
        };
        config.endpoint = "http://127.0.0.1:9".into();
        config
    }

    fn queued_events(telemetry: &Telemetry) -> Vec<EventEnvelope> {
        telemetry
            .queue()
            .entries()
            .into_iter()
            .filter_map(|(_, entry)| match entry.item {
                QueuedItem::Event(envelope) => Some(*envelope),
                QueuedItem::Crash(_) => None,
            })
            .collect()
    }

    #[test]
    fn every_event_is_queued_in_its_own_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(uploading(dir.path())).unwrap();

        telemetry.app_started(Some("en-GB".into()));
        telemetry.feature(Feature::ToolCallFinished, Outcome::Success, None);
        telemetry.app_exited(Some(0));
        telemetry.feature(Feature::SessionSummary, Outcome::Success, None);

        let events = queued_events(&telemetry);
        let ids: std::collections::BTreeSet<_> =
            events.iter().map(|e| e.event_id.clone()).collect();
        assert_eq!(ids.len(), events.len(), "ids are unique");
        assert!(
            events.iter().all(|event| event.schema == SCHEMA),
            "{events:?}"
        );
        assert!(events
            .iter()
            .all(|event| event.context.install_id == telemetry.context().install_id));
        assert_eq!(events[0].context.session_id, telemetry.context().session_id);
        assert!(events.iter().all(|event| !event.is_legacy()));
    }

    #[test]
    fn the_crash_feature_rides_with_the_crash() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(uploading(dir.path())).unwrap();
        telemetry.crash(telemetry.crash_report(CrashKind::Reported, "boom"));

        let events = queued_events(&telemetry);
        assert_eq!(events.len(), 1, "{events:?}");
        assert!(matches!(
            events[0].event,
            TelemetryEvent::FeatureUsed {
                feature: Feature::Crash,
                outcome: Outcome::Error,
                ..
            }
        ));
    }

    #[test]
    fn a_one_shot_feature_is_recorded_once_per_installation() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(uploading(dir.path())).unwrap();

        assert!(telemetry.feature_once(Feature::FirstTurnCompleted, Outcome::Success, None));
        assert!(!telemetry.feature_once(Feature::FirstTurnCompleted, Outcome::Success, None));
        // A different one-shot is its own flag.
        assert!(telemetry.feature_once(Feature::OnboardingCompleted, Outcome::Success, None));
        assert_eq!(queued_events(&telemetry).len(), 2);

        // The flag survives a restart, so a second launch does not resend it.
        drop(telemetry);
        let restarted = Telemetry::start(uploading(dir.path())).unwrap();
        let before = queued_events(&restarted).len();
        assert!(!restarted.feature_once(Feature::FirstTurnCompleted, Outcome::Success, None));
        assert_eq!(queued_events(&restarted).len(), before);
    }

    #[test]
    fn a_sample_rate_of_zero_records_nothing_and_one_records_everything() {
        let dir = tempfile::tempdir().unwrap();
        let mut sampled = uploading(dir.path());
        sampled.sample_rates.insert(Feature::TurnCompleted, 0.0);

        let telemetry = Telemetry::start(sampled).unwrap();
        for _ in 0..5 {
            telemetry.feature(Feature::TurnCompleted, Outcome::Success, None);
        }
        assert!(queued_events(&telemetry).is_empty());

        // Every other feature keeps its default of 1.0.
        telemetry.feature(Feature::TerminalCommandRun, Outcome::Success, None);
        assert_eq!(queued_events(&telemetry).len(), 1);
    }

    #[test]
    fn a_link_id_is_absent_until_it_is_set_then_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(config(dir.path())).unwrap();
        assert_eq!(
            telemetry.link_id(),
            None,
            "a machine that never linked has none"
        );
        assert_eq!(telemetry.context().link_id, None);

        let link = telemetry.ensure_link().expect("a link id is minted");
        assert!(!link.is_empty());
        assert_eq!(telemetry.link_id().as_deref(), Some(link.as_str()));
        drop(telemetry);

        let restarted = Telemetry::start(config(dir.path())).unwrap();
        assert_eq!(restarted.link_id().as_deref(), Some(link.as_str()));

        restarted.clear_link();
        assert_eq!(restarted.link_id(), None);
        drop(restarted);
        let third = Telemetry::start(config(dir.path())).unwrap();
        assert_eq!(third.link_id(), None, "a reset link does not come back");
    }

    #[test]
    fn a_link_can_be_adopted_before_the_first_event() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(uploading(dir.path())).unwrap();

        assert!(telemetry.link_with("linked-installation"));
        telemetry.app_started(None);

        let events = queued_events(&telemetry);
        assert_eq!(
            events[0].context.link_id.as_deref(),
            Some("linked-installation")
        );

        // An empty link is not an identity, so it is refused.
        assert!(!telemetry.link_with("  "));
        assert_eq!(telemetry.link_id().as_deref(), Some("linked-installation"));
    }

    #[test]
    fn the_role_from_config_travels_on_every_event() {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = uploading(dir.path());
        worker.role = Role::Worker;
        let telemetry = Telemetry::start(worker).unwrap();
        telemetry.feature(Feature::TurnCompleted, Outcome::Success, None);

        assert_eq!(telemetry.context().role, Role::Worker);
        assert_eq!(queued_events(&telemetry)[0].context.role, Role::Worker);
    }

    #[test]
    fn an_archived_report_has_the_recent_log_tail() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(config(dir.path())).unwrap();
        telemetry
            .breadcrumbs()
            .record(log::Level::Warn, "app::service", "the worker restarted");

        let report = telemetry.crash_report(CrashKind::Reported, "boom");
        assert_eq!(report.breadcrumbs.len(), 1);
        assert_eq!(report.breadcrumbs[0].message, "the worker restarted");
    }

    #[test]
    fn consent_decides_whether_the_upload_thread_exists() {
        let dir = tempfile::tempdir().unwrap();
        let mut with_upload = config(dir.path());
        with_upload.consent = Consent {
            upload: true,
            source: ConsentSource::Default,
        };
        with_upload.endpoint = "http://127.0.0.1:9".into();

        let telemetry = Telemetry::start(with_upload).unwrap();
        telemetry.app_started(None);
        assert_eq!(telemetry.queue().len(), 1, "an allowed event waits on disk");
        assert!(telemetry.local_reports().is_empty());

        // The collector in this test does not exist, so the item must survive
        // the attempt: this is the "endpoint not deployed yet" path.
        let outcome = telemetry
            .shutdown(Duration::from_secs(5))
            .expect("the worker answered");
        assert_eq!(outcome.uploaded, 0);
        assert!(outcome.remaining >= 1);
    }

    #[test]
    fn flushing_without_an_uploader_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(config(dir.path())).unwrap();
        assert!(telemetry.flush(Duration::from_millis(10)).is_none());
        assert!(telemetry.shutdown(Duration::from_millis(10)).is_none());
    }

    #[test]
    fn a_secret_never_reaches_the_archive() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(config(dir.path()).with_secret("s3cr3t-value")).unwrap();

        telemetry.crash(telemetry.crash_report(
            CrashKind::Reported,
            "the vault rejected s3cr3t-value while unlocking",
        ));

        let report = std::fs::read_to_string(&telemetry.local_reports()[0]).unwrap();
        assert!(!report.contains("s3cr3t-value"), "{report}");
        assert!(report.contains(REDACTED), "{report}");
    }
}
