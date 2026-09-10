//! The types that travel: a crash report, a lifecycle event, and the archive of
//! reports kept on this machine.
//!
//! The field set is fixed on purpose. A report is assembled from values the
//! runtime already knows — version, channel, platform, an anonymous install id
//! — never from whatever text happened to be on screen. The only variable parts
//! are a panic message, a backtrace, a note and the recent log tail, and every
//! one of those passes through [`crate::redact::Redactor`] before it is written
//! or sent.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::breadcrumbs::Breadcrumb;
use crate::config::Channel;
use crate::redact::Redactor;

/// Bumped whenever a field is added, removed or changes meaning.
pub const SCHEMA: u32 = 1;

/// How much free text one report may carry.
const MESSAGE_CAP: usize = 4 * 1024;
const BACKTRACE_CAP: usize = 64 * 1024;
const NOTE_CAP: usize = 1024;

/// Facts about this installation that every report and event carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientContext {
    pub app_version: String,
    pub channel: Channel,
    pub os: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    pub arch: String,
    /// Random per installation, generated on first run. Not derived from the
    /// machine, the account or the user.
    pub install_id: String,
    /// Random per process.
    pub session_id: String,
}

/// Where a crash came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashKind {
    /// A Rust panic.
    Panic,
    /// The process was aborted, or died without unwinding.
    Abort,
    /// Reported deliberately from an error path.
    Reported,
}

impl CrashKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Panic => "panic",
            Self::Abort => "abort",
            Self::Reported => "reported",
        }
    }
}

/// One crash, as it will be stored and sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashReport {
    pub schema: u32,
    /// Unique per report.
    pub id: String,
    /// Stable across occurrences of the same crash, so a collector can group
    /// them without reading the message.
    pub fingerprint: String,
    pub captured_at: DateTime<Utc>,
    pub kind: CrashKind,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backtrace: Option<String>,
    /// A sentence the user chose to add.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub context: ClientContext,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breadcrumbs: Vec<Breadcrumb>,
}

impl CrashReport {
    /// Build a report. Nothing is redacted yet; call [`Self::redacted`] before
    /// the report is written or sent.
    pub fn new(context: &ClientContext, kind: CrashKind, message: impl Into<String>) -> Self {
        let message = truncate(message.into().trim(), MESSAGE_CAP);
        let id = uuid::Uuid::new_v4().simple().to_string();
        Self {
            schema: SCHEMA,
            fingerprint: fingerprint(&message),
            id,
            captured_at: Utc::now(),
            kind,
            message,
            location: None,
            thread: None,
            backtrace: None,
            note: None,
            context: context.clone(),
            breadcrumbs: Vec::new(),
        }
    }

    pub fn with_location(mut self, location: Option<String>) -> Self {
        self.location = location.map(|value| truncate(&value, MESSAGE_CAP));
        self
    }

    pub fn with_thread(mut self, thread: Option<String>) -> Self {
        self.thread = thread;
        self
    }

    pub fn with_backtrace(mut self, backtrace: Option<String>) -> Self {
        self.backtrace = backtrace.map(|value| truncate(&value, BACKTRACE_CAP));
        self
    }

    pub fn with_note(mut self, note: Option<String>) -> Self {
        self.note = note.map(|value| truncate(&value, NOTE_CAP));
        self
    }

    pub fn with_breadcrumbs(mut self, breadcrumbs: Vec<Breadcrumb>) -> Self {
        self.breadcrumbs = breadcrumbs;
        self
    }

    /// Scrub every free-text field.
    pub fn redacted(mut self, redactor: &Redactor) -> Self {
        self.message = redactor.scrub(&self.message);
        self.location = redactor.scrub_opt(self.location.as_deref());
        self.thread = redactor.scrub_opt(self.thread.as_deref());
        self.backtrace = redactor.scrub_opt(self.backtrace.as_deref());
        self.note = redactor.scrub_opt(self.note.as_deref());
        for breadcrumb in &mut self.breadcrumbs {
            breadcrumb.message = redactor.scrub(&breadcrumb.message);
            breadcrumb.target = redactor.scrub(&breadcrumb.target);
        }
        self
    }
}

/// A lifecycle event. Deliberately coarse: knowing that a feature was used is
/// useful, knowing what a user typed into it is not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TelemetryEvent {
    AppStarted {
        context: ClientContext,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        locale: Option<String>,
    },
    AppExited {
        uptime_secs: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
    },
    FeatureUsed {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
}

/// One thing waiting to be uploaded.
///
/// Adjacently tagged rather than flattened: a flattened report would put its own
/// `kind` field in the same map as this enum's tag, and a duplicate key is a
/// silent way to lose a crash report on the way back in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum QueuedItem {
    Crash(Box<CrashReport>),
    Event(Box<TelemetryEvent>),
}

impl QueuedItem {
    pub fn crash(report: CrashReport) -> Self {
        Self::Crash(Box::new(report))
    }

    pub fn event(event: TelemetryEvent) -> Self {
        Self::Event(Box::new(event))
    }

    /// Collector path for this kind of item.
    pub fn endpoint_path(&self) -> &'static str {
        match self {
            Self::Crash(_) => "/v1/crashes",
            Self::Event(_) => "/v1/events",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Crash(_) => "crash",
            Self::Event(_) => "event",
        }
    }

    pub fn is_crash(&self) -> bool {
        matches!(self, Self::Crash(_))
    }
}

/// The copy of a crash report kept on this machine.
///
/// Written even when uploading is turned off, because the user — not the
/// collector — is the first person who may need to send it somewhere.
#[derive(Debug, Clone)]
pub struct ReportArchive {
    dir: PathBuf,
    limit: usize,
}

impl ReportArchive {
    pub fn new(dir: impl Into<PathBuf>, limit: usize) -> Self {
        Self { dir: dir.into(), limit: limit.max(1) }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Write one report, then keep the archive within its bound.
    pub fn write(&self, report: &CrashReport) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!(
            "crash-{:013}-{}.json",
            report.captured_at.timestamp_millis(),
            &report.id[..report.id.len().min(8)]
        ));
        let text = serde_json::to_string_pretty(report).map_err(std::io::Error::other)?;
        crate::write_atomic(&path, text.as_bytes())?;
        if let Err(err) = self.prune() {
            log::warn!("could not prune the crash archive: {err}");
        }
        Ok(path)
    }

    /// Newest first.
    pub fn list(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
            .collect();
        paths.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
        paths
    }

    /// Drop the oldest reports past the limit. Returns how many went.
    pub fn prune(&self) -> std::io::Result<usize> {
        let mut removed = 0;
        for path in self.list().into_iter().skip(self.limit) {
            match std::fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
        }
        Ok(removed)
    }

    pub fn len(&self) -> usize {
        self.list().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Group reports by what went wrong, not by where.
///
/// The message is the stable part: file paths, line numbers and column numbers
/// all shift between builds and checkouts, and a crash that moves down a file
/// is still the same crash. Numbers inside the message are folded to `N` for
/// the same reason — "the len is 3 but the index is 5" and "the len is 7 but the
/// index is 9" are one bug, not two.
fn fingerprint(message: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut normalized = String::with_capacity(message.len());
    let mut last_was_space = false;
    let mut in_number = false;
    for character in message.trim().chars() {
        if character.is_ascii_digit() {
            if !in_number {
                normalized.push('N');
                in_number = true;
            }
            last_was_space = false;
        } else {
            in_number = false;
            if character.is_whitespace() {
                if !last_was_space {
                    normalized.push(' ');
                    last_was_space = true;
                }
            } else {
                normalized.push(character.to_ascii_lowercase());
                last_was_space = false;
            }
        }
    }

    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    hex::encode(&hasher.finalize()[..8])
}

/// Cut to a byte budget without splitting a character.
fn truncate(text: &str, cap: usize) -> String {
    if text.len() <= cap {
        return text.to_owned();
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = text[..end].to_owned();
    out.push_str("…[truncated]");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Channel;

    fn context() -> ClientContext {
        ClientContext {
            app_version: "0.1.0".into(),
            channel: Channel::Stable,
            os: "macos".into(),
            os_version: Some("15.0".into()),
            arch: "aarch64".into(),
            install_id: "abc".into(),
            session_id: "def".into(),
        }
    }

    fn report(message: &str) -> CrashReport {
        CrashReport::new(&context(), CrashKind::Panic, message)
    }

    #[test]
    fn the_same_crash_gets_the_same_fingerprint() {
        let first = report("index out of bounds").with_location(Some("app/src/main.rs:30".into()));
        let second = report("index out of bounds").with_location(Some("app/src/main.rs:30".into()));
        assert_eq!(first.fingerprint, second.fingerprint);
    }

    #[test]
    fn a_different_message_gets_a_different_fingerprint() {
        let base = report("index out of bounds").with_location(Some("app/src/main.rs:30".into()));
        let other = report("unwrap on None").with_location(Some("app/src/main.rs:30".into()));
        assert_ne!(base.fingerprint, other.fingerprint);
    }

    #[test]
    fn a_fingerprint_ignores_where_it_happened() {
        // Grouping by message means a crash that moves down a file, or is built
        // from a different checkout path, stays one group.
        let here = report("boom").with_location(Some("/Users/ana/goble/app/src/main.rs:30:9".into()));
        let elsewhere = report("boom").with_location(Some("/ci/build/app/src/main.rs:55:2".into()));
        assert_eq!(here.fingerprint, elsewhere.fingerprint);
    }

    #[test]
    fn a_fingerprint_folds_the_numbers_in_the_message() {
        // The same bug with different values is one group, not one per value.
        let small = report("index out of bounds: the len is 3 but the index is 5");
        let large = report("index out of bounds: the len is 70 but the index is 900");
        assert_eq!(small.fingerprint, large.fingerprint);
    }

    #[test]
    fn reports_carry_no_identifying_field_beyond_the_install_id() {
        let json = serde_json::to_string(&report("boom")).unwrap();
        for forbidden in ["user", "email", "hostname", "username", "path", "cwd"] {
            assert!(!json.contains(forbidden), "{forbidden} appears in {json}");
        }
    }

    #[test]
    fn a_redacted_message_loses_its_token() {
        let report = report("request failed: Authorization: Bearer abcdef1234567890")
            .with_breadcrumbs(vec![Breadcrumb {
                at: Utc::now(),
                level: "info".into(),
                target: "http".into(),
                message: "token sk-proj-abcdefghijklmnop rejected".into(),
                repeats: 1,
            }])
            .redacted(&Redactor::new().with_home("/Users/ana"));

        assert!(!report.message.contains("abcdef1234567890"), "{}", report.message);
        assert!(!report.breadcrumbs[0].message.contains("sk-proj-"), "{:?}", report.breadcrumbs);
    }

    #[test]
    fn a_giant_message_is_cut() {
        let report = report(&"x".repeat(MESSAGE_CAP * 2));
        assert!(report.message.len() < MESSAGE_CAP + 32, "{}", report.message.len());
        assert!(report.message.ends_with("…[truncated]"));
    }

    #[test]
    fn a_giant_backtrace_is_cut() {
        let report = report("boom").with_backtrace(Some("y".repeat(BACKTRACE_CAP * 2)));
        assert!(report.backtrace.unwrap().len() < BACKTRACE_CAP + 32);
    }

    #[test]
    fn absent_fields_are_not_serialised() {
        let json = serde_json::to_string(&report("boom")).unwrap();
        assert!(!json.contains("backtrace"), "{json}");
        assert!(!json.contains("note"), "{json}");
    }

    #[test]
    fn a_report_survives_a_round_trip() {
        let original = report("boom")
            .with_location(Some("app/src/main.rs:30".into()))
            .with_thread(Some("main".into()))
            .with_backtrace(Some("0: goble_app::main".into()))
            .with_note(Some("I pressed the button twice".into()));
        let json = serde_json::to_string(&original).unwrap();
        let parsed: CrashReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn a_queued_item_knows_where_it_goes() {
        assert_eq!(QueuedItem::crash(report("boom")).endpoint_path(), "/v1/crashes");
        let event = TelemetryEvent::FeatureUsed { name: "terminal".into(), duration_ms: None };
        assert_eq!(QueuedItem::event(event).endpoint_path(), "/v1/events");
        assert!(QueuedItem::crash(report("boom")).is_crash());
    }

    #[test]
    fn a_queued_crash_survives_a_round_trip() {
        // The report has a `kind` of its own; the envelope must not steal it.
        let item = QueuedItem::crash(
            report("boom").with_location(Some("app/src/main.rs:30".into())),
        );
        let json = serde_json::to_string(&item).unwrap();
        let parsed: QueuedItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, item);
    }

    #[test]
    fn a_queued_event_survives_a_round_trip() {
        let item = QueuedItem::event(TelemetryEvent::AppStarted {
            context: context(),
            locale: Some("en-GB".into()),
        });
        let json = serde_json::to_string(&item).unwrap();
        assert_eq!(serde_json::from_str::<QueuedItem>(&json).unwrap(), item);
    }

    #[test]
    fn the_envelope_keeps_the_report_in_its_own_object() {
        let item = QueuedItem::crash(report("boom"));
        let json: serde_json::Value = serde_json::to_value(&item).unwrap();

        assert_eq!(json["kind"], "crash");
        // The report's own `kind` lives inside the payload. Flattening the two
        // together would put one key at the same level twice, and the crash
        // would come back as an unreadable variant.
        assert_eq!(json["value"]["kind"], "panic");
    }

    #[test]
    fn an_event_round_trips_through_its_tag() {
        let event = TelemetryEvent::AppExited { uptime_secs: 42, exit_code: Some(0) };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"app_exited\""), "{json}");
        assert_eq!(serde_json::from_str::<TelemetryEvent>(&json).unwrap(), event);
    }

    #[test]
    fn the_archive_keeps_the_newest_and_drops_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let archive = ReportArchive::new(dir.path(), 2);

        for index in 0..4 {
            let mut report = report(&format!("boom {index}"));
            report.captured_at = Utc::now() + chrono::Duration::seconds(index);
            archive.write(&report).unwrap();
        }

        let kept = archive.list();
        assert_eq!(kept.len(), 2);
        // Newest first: the last written report is at the top.
        let newest = std::fs::read_to_string(&kept[0]).unwrap();
        assert!(newest.contains("boom 3"), "{newest}");
    }

    #[test]
    fn a_stray_file_in_the_archive_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let archive = ReportArchive::new(dir.path(), 5);
        archive.write(&report("boom")).unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not a report").unwrap();
        assert_eq!(archive.len(), 1);
    }

    #[test]
    fn an_archive_documented_as_human_readable_is_human_readable() {
        let dir = tempfile::tempdir().unwrap();
        let archive = ReportArchive::new(dir.path(), 5);
        let path = archive.write(&report("boom")).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("\n  \""), "expected pretty printing: {text}");
    }
}
