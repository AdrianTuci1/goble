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
mod panic;
mod queue;
mod redact;
mod report;
mod upload;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub use breadcrumbs::{Breadcrumb, BreadcrumbLogger, Breadcrumbs, DEFAULT_LIMIT as BREADCRUMB_LIMIT};
pub use config::{
    Channel, Consent, ConsentSource, Environment, ProcessEnv, TelemetryConfig, TelemetryPaths,
    TelemetryState, DEFAULT_ENDPOINT, ENV_DO_NOT_TRACK, ENV_ENABLED, ENV_ENDPOINT,
};
pub use panic::install_panic_hook;
pub use queue::{CrashQueue, QueueEntry};
pub use redact::{Redactor, REDACTED};
pub use report::{
    ClientContext, CrashKind, CrashReport, QueuedItem, ReportArchive, TelemetryEvent, SCHEMA,
};
pub use upload::{
    drain, HttpUploader, UploadConfig, UploadError, UploadErrorKind, UploadOutcome, UploadWorker,
    Uploader,
};

/// Crash reports kept on this machine.
pub const ARCHIVE_LIMIT: usize = 20;
/// How often a run retries the queue on its own.
pub const DRAIN_INTERVAL: Duration = Duration::from_secs(30);
/// Longest the uploader will wait between retries.
pub const RETRY_CEILING: Duration = Duration::from_secs(10 * 60);

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

/// Crash reporting and telemetry, as the rest of the app sees it.
#[derive(Debug)]
pub struct Telemetry {
    config: TelemetryConfig,
    context: ClientContext,
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

        let context = ClientContext {
            app_version: config.app_version.clone(),
            channel: config.channel,
            os: config.os.clone().unwrap_or_else(|| std::env::consts::OS.to_owned()),
            os_version: config.os_version.clone().or_else(detect_os_version),
            arch: config.arch.clone().unwrap_or_else(|| std::env::consts::ARCH.to_owned()),
            install_id,
            session_id: config
                .session_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()),
        };

        let archive = ReportArchive::new(config.paths.reports_dir(), ARCHIVE_LIMIT);
        let queue =
            Arc::new(CrashQueue::new(config.paths.queue_dir(), config.max_queued, config.max_age));

        let worker = if config.consent.upload {
            match HttpUploader::new(
                format!("goble/{}", config.app_version),
                config.request_timeout,
            ) {
                Ok(uploader) => Some(UploadWorker::spawn(
                    Arc::clone(&queue),
                    Arc::new(uploader),
                    UploadConfig {
                        base: config.collector().to_owned(),
                        max_attempts: config.max_attempts,
                        backoff: RETRY_CEILING,
                        interval: DRAIN_INTERVAL,
                    },
                )),
                Err(err) => {
                    log::warn!("telemetry upload is off: {err}");
                    None
                }
            }
        } else {
            None
        };

        Ok(Arc::new(Self {
            config,
            context,
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
    pub fn context(&self) -> &ClientContext {
        &self.context
    }

    pub fn consent(&self) -> Consent {
        self.config.consent
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
    pub fn crash_report(
        &self,
        kind: CrashKind,
        message: impl Into<String>,
    ) -> CrashReport {
        CrashReport::new(&self.context, kind, message)
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

            if !self.config.consent.upload {
                return;
            }
            if let Err(err) = self.queue.enqueue(QueuedItem::crash(report.clone())) {
                if in_panic {
                    eprintln!("goble: could not queue a crash report: {err}");
                } else {
                    log::warn!("telemetry: could not queue a crash report: {err}");
                }
            }
        };

        // A second panic while unwinding would abort, so this is the last net.
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(write)).is_err() && in_panic {
            eprintln!("goble: crash reporting failed");
        }
    }

    /// Record an event. Dropped silently when uploading is off, because an
    /// event that is never sent has no reason to be on disk.
    pub fn record(&self, event: TelemetryEvent) {
        if !self.config.consent.upload {
            log::debug!("telemetry: dropping an event because uploading is off");
            return;
        }
        if let Err(err) = self.queue.enqueue(QueuedItem::event(event)) {
            log::warn!("telemetry: could not queue an event: {err}");
        }
    }

    pub fn app_started(&self, locale: Option<String>) {
        let locale = locale.or_else(|| self.config.locale.clone());
        self.record(TelemetryEvent::AppStarted { context: self.context.clone(), locale });
    }

    pub fn app_exited(&self, exit_code: Option<i32>) {
        self.record(TelemetryEvent::AppExited {
            uptime_secs: self.started_at.elapsed().as_secs(),
            exit_code,
        });
    }

    pub fn feature_used(&self, name: impl Into<String>, duration: Option<Duration>) {
        self.record(TelemetryEvent::FeatureUsed {
            name: name.into(),
            duration_ms: duration.map(|value| value.as_millis() as u64),
        });
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
    let output = std::process::Command::new("sw_vers").arg("-productVersion").output().ok()?;
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
        line.strip_prefix("PRETTY_NAME=").map(|value| value.trim().trim_matches('"').to_owned())
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
        TelemetryConfig::new("1.0.0", Channel::Dev, TelemetryPaths::under(dir))
            .without_upload()
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
    fn a_crash_is_archived_even_when_uploading_is_off() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(config(dir.path())).unwrap();

        telemetry.crash(telemetry.crash_report(CrashKind::Reported, "something went wrong"));

        assert_eq!(telemetry.local_reports().len(), 1);
        assert!(telemetry.queue().is_empty(), "nothing may be uploaded without consent");

        let report = std::fs::read_to_string(&telemetry.local_reports()[0]).unwrap();
        assert!(report.contains("something went wrong"), "{report}");
    }

    #[test]
    fn a_recorded_event_is_not_kept_when_uploading_is_off() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(config(dir.path())).unwrap();

        telemetry.app_started(None);
        telemetry.feature_used("terminal", Some(Duration::from_millis(5)));
        telemetry.app_exited(Some(0));

        assert!(telemetry.queue().is_empty());
        assert!(telemetry.local_reports().is_empty());
    }

    #[test]
    fn an_archived_report_has_the_recent_log_tail() {
        let dir = tempfile::tempdir().unwrap();
        let telemetry = Telemetry::start(config(dir.path())).unwrap();
        telemetry.breadcrumbs().record(log::Level::Warn, "app::service", "the worker restarted");

        let report = telemetry.crash_report(CrashKind::Reported, "boom");
        assert_eq!(report.breadcrumbs.len(), 1);
        assert_eq!(report.breadcrumbs[0].message, "the worker restarted");
    }

    #[test]
    fn consent_decides_whether_the_upload_thread_exists() {
        let dir = tempfile::tempdir().unwrap();
        let mut with_upload = config(dir.path());
        with_upload.consent = Consent { upload: true, source: ConsentSource::Default };
        with_upload.endpoint = "http://127.0.0.1:9".into();

        let telemetry = Telemetry::start(with_upload).unwrap();
        telemetry.app_started(None);
        assert_eq!(telemetry.queue().len(), 1, "an allowed event waits on disk");
        assert!(telemetry.local_reports().is_empty());

        // The collector in this test does not exist, so the item must survive
        // the attempt: this is the "endpoint not deployed yet" path.
        let outcome = telemetry.shutdown(Duration::from_secs(5)).expect("the worker answered");
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
