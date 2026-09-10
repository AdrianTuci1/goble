//! The recent history that travels with a crash report.
//!
//! A panic message alone rarely explains a crash; the last few log lines
//! usually do. Breadcrumbs are that tail: a bounded ring buffer fed by the
//! application's own logger, so nothing has to be instrumented twice.
//!
//! The buffer is bounded twice over — by entry count and by message length —
//! because a report is uploaded, and a program that logs a megabyte a second
//! should not turn into a megabyte-size crash report.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Entries kept per process.
pub const DEFAULT_LIMIT: usize = 200;
/// Longest message kept, in bytes.
pub const DEFAULT_MESSAGE_CAP: usize = 512;

/// One line of recent history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breadcrumb {
    pub at: DateTime<Utc>,
    pub level: String,
    pub target: String,
    pub message: String,
    /// How many times this line repeated back to back. Log spam collapses into
    /// one entry instead of pushing the useful history out of the buffer.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub repeats: u32,
}

fn one() -> u32 {
    1
}

fn is_one(count: &u32) -> bool {
    *count <= 1
}

/// A bounded, thread-safe ring buffer of recent log lines.
#[derive(Debug)]
pub struct Breadcrumbs {
    entries: Mutex<VecDeque<Breadcrumb>>,
    limit: usize,
    message_cap: usize,
}

impl Default for Breadcrumbs {
    fn default() -> Self {
        Self::new(DEFAULT_LIMIT)
    }
}

impl Breadcrumbs {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            limit: limit.max(1),
            message_cap: DEFAULT_MESSAGE_CAP,
        }
    }

    pub fn with_message_cap(mut self, cap: usize) -> Self {
        self.message_cap = cap.max(1);
        self
    }

    /// Record one line, collapsing an immediate repeat into a counter.
    pub fn record(&self, level: log::Level, target: &str, message: &str) {
        let message = truncate(message.trim(), self.message_cap);
        let target = target.to_owned();

        // A poisoned lock must not take the process down: telemetry is
        // observability, not correctness.
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };

        if let Some(last) = entries.back_mut() {
            if last.level == level.as_str() && last.target == target && last.message == message {
                last.repeats = last.repeats.saturating_add(1);
                last.at = Utc::now();
                return;
            }
        }

        entries.push_back(Breadcrumb {
            at: Utc::now(),
            level: level.as_str().to_owned(),
            target,
            message,
            repeats: 1,
        });
        while entries.len() > self.limit {
            entries.pop_front();
        }
    }

    /// Oldest first, so a report reads like a log tail.
    pub fn snapshot(&self) -> Vec<Breadcrumb> {
        self.entries.lock().map(|entries| entries.iter().cloned().collect()).unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|entries| entries.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }
}

/// Wraps the application's logger so every accepted line also becomes a
/// breadcrumb. The wrapped logger still does its own job — this only observes.
pub struct BreadcrumbLogger {
    inner: Box<dyn log::Log>,
    breadcrumbs: Arc<Breadcrumbs>,
}

impl std::fmt::Debug for BreadcrumbLogger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreadcrumbLogger")
            .field("breadcrumbs", &self.breadcrumbs.len())
            .finish()
    }
}

impl BreadcrumbLogger {
    pub fn new(inner: Box<dyn log::Log>, breadcrumbs: Arc<Breadcrumbs>) -> Self {
        Self { inner, breadcrumbs }
    }
}

impl log::Log for BreadcrumbLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.inner.enabled(record.metadata()) {
            return;
        }
        self.breadcrumbs.record(record.level(), record.target(), &record.args().to_string());
        self.inner.log(record);
    }

    fn flush(&self) {
        self.inner.flush();
    }
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
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buffer_keeps_the_most_recent_entries() {
        let breadcrumbs = Breadcrumbs::new(3);
        for index in 0..5 {
            breadcrumbs.record(log::Level::Info, "app", &format!("line {index}"));
        }
        let snapshot = breadcrumbs.snapshot();
        assert_eq!(snapshot.len(), 3);
        assert_eq!(snapshot[0].message, "line 2");
        assert_eq!(snapshot[2].message, "line 4");
    }

    #[test]
    fn an_immediate_repeat_collapses_into_a_counter() {
        let breadcrumbs = Breadcrumbs::new(10);
        for _ in 0..4 {
            breadcrumbs.record(log::Level::Warn, "app", "retrying");
        }
        let snapshot = breadcrumbs.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].repeats, 4);
    }

    #[test]
    fn a_repeat_after_a_different_line_starts_a_new_entry() {
        let breadcrumbs = Breadcrumbs::new(10);
        breadcrumbs.record(log::Level::Info, "app", "one");
        breadcrumbs.record(log::Level::Info, "app", "two");
        breadcrumbs.record(log::Level::Info, "app", "one");
        assert_eq!(breadcrumbs.snapshot().len(), 3);
    }

    #[test]
    fn a_different_level_is_a_different_entry() {
        let breadcrumbs = Breadcrumbs::new(10);
        breadcrumbs.record(log::Level::Info, "app", "same text");
        breadcrumbs.record(log::Level::Error, "app", "same text");
        assert_eq!(breadcrumbs.snapshot().len(), 2);
    }

    #[test]
    fn a_long_line_is_cut_to_the_budget() {
        let breadcrumbs = Breadcrumbs::new(10).with_message_cap(16);
        breadcrumbs.record(log::Level::Info, "app", &"é".repeat(40));
        let message = &breadcrumbs.snapshot()[0].message;
        assert!(message.len() <= 16 + "…".len(), "{message}");
        assert!(message.ends_with('…'));
    }

    #[test]
    fn a_single_entry_does_not_serialise_its_repeat_count() {
        let breadcrumbs = Breadcrumbs::new(10);
        breadcrumbs.record(log::Level::Info, "app", "once");
        let json = serde_json::to_string(&breadcrumbs.snapshot()).unwrap();
        assert!(!json.contains("repeats"), "{json}");
    }

    /// Counts what reached the wrapped logger.
    #[derive(Debug)]
    struct Counter(Arc<std::sync::atomic::AtomicUsize>);

    impl log::Log for Counter {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, _: &log::Record<'_>) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        fn flush(&self) {}
    }

    #[test]
    fn the_wrapped_logger_still_forwards() {
        use log::Log as _;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let count = Arc::new(AtomicUsize::new(0));
        let breadcrumbs = Arc::new(Breadcrumbs::new(10));
        let logger =
            BreadcrumbLogger::new(Box::new(Counter(count.clone())), breadcrumbs.clone());

        logger.log(
            &log::Record::builder()
                .args(format_args!("hello"))
                .level(log::Level::Info)
                .target("test")
                .build(),
        );

        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(breadcrumbs.snapshot()[0].message, "hello");
    }
}
