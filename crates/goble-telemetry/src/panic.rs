//! Turning a panic into a report.
//!
//! The hook here is additive: whatever was installed before still runs, so the
//! process keeps printing a panic the way it always did. All this adds is a
//! file on disk and an entry in the upload queue.
//!
//! Two constraints shape the code:
//!
//! - it runs while the process is unwinding, so it allocates as little as it
//!   reasonably can and never unwraps;
//! - a panic inside a panic hook aborts the process, so a flag makes a
//!   reentrant call a no-op instead of a crash.

use std::backtrace::Backtrace;
use std::panic::PanicHookInfo;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Telemetry;
use crate::report::CrashKind;

/// Set while a report is being written, so a second panic does not recurse.
static CAPTURING: AtomicBool = AtomicBool::new(false);

/// Install the capture hook in front of the existing one.
pub fn install_panic_hook(telemetry: Arc<Telemetry>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        capture(&telemetry, info);
        previous(info);
    }));
}

/// Build a report from a panic and hand it to the telemetry layer.
fn capture(telemetry: &Telemetry, info: &PanicHookInfo<'_>) {
    // `swap` returns the old value: if it was already set, a panic happened
    // inside a panic hook and this one must not try again.
    if CAPTURING.swap(true, Ordering::SeqCst) {
        return;
    }

    // A panic while building the report would be caught here rather than
    // aborting the process.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let report = telemetry
            .crash_report(CrashKind::Panic, message_of(info))
            .with_location(info.location().map(|location| {
                format!("{}:{}:{}", location.file(), location.line(), location.column())
            }))
            .with_thread(thread_name())
            .with_backtrace(Some(Backtrace::force_capture().to_string()));
        telemetry.capture_crash(report);
    }));

    CAPTURING.store(false, Ordering::SeqCst);
}

/// A panic payload is usually a `&str` or a `String`, but it does not have to
/// be.
fn message_of(info: &PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(message) = payload.downcast_ref::<&'static str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "panic with a payload that is not a string".to_owned()
    }
}

fn thread_name() -> Option<String> {
    let current = std::thread::current();
    match current.name() {
        Some(name) => Some(name.to_owned()),
        None => Some(format!("{:?}", current.id())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Channel, TelemetryConfig, TelemetryPaths};
    use crate::report::CrashReport;

    /// The panic hook is process-global, so the crate has exactly one test that
    /// installs one. It also restores a silent hook afterwards so the test
    /// output is not filled with backtraces.
    #[test]
    fn a_panic_is_archived_and_the_previous_hook_still_runs() {
        use std::sync::atomic::AtomicUsize;

        static PREVIOUS_RUNS: AtomicUsize = AtomicUsize::new(0);

        let dir = tempfile::tempdir().unwrap();
        let config = TelemetryConfig::new("9.9.9", Channel::Dev, TelemetryPaths::under(dir.path()))
            .without_upload();
        let telemetry = Telemetry::start(config).unwrap();

        // Stand in for the default hook so the test output stays quiet.
        let quiet = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {
            PREVIOUS_RUNS.fetch_add(1, Ordering::SeqCst);
        }));

        install_panic_hook(telemetry.clone());

        let caught = std::panic::catch_unwind(|| panic!("boom from a test"));
        assert!(caught.is_err());
        assert_eq!(PREVIOUS_RUNS.load(Ordering::SeqCst), 1, "the previous hook must still run");

        // A payload that is not a string must not lose the report.
        let caught = std::panic::catch_unwind(|| std::panic::panic_any(42_u32));
        assert!(caught.is_err());

        let reports = telemetry.local_reports();
        assert_eq!(reports.len(), 2, "still-on-disk reports: {reports:?}");

        let mut parsed: Vec<CrashReport> = reports
            .iter()
            .map(|path| serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap())
            .collect();

        let panicked = parsed
            .pop()
            .expect("a report for the string panic");
        assert!(panicked.message.contains("boom from a test"), "{}", panicked.message);
        assert_eq!(panicked.kind, CrashKind::Panic);
        assert!(panicked.location.as_deref().is_some_and(|location| location.contains("panic.rs")));
        assert!(panicked.backtrace.is_some(), "a backtrace is what makes a report actionable");
        assert_eq!(panicked.context.app_version, "9.9.9");
        assert_eq!(panicked.context.channel, Channel::Dev);

        let anonymous = parsed.pop().expect("a report for the non-string panic");
        assert_eq!(anonymous.message, "panic with a payload that is not a string");

        std::panic::set_hook(quiet);
    }
}
