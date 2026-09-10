//! The on-disk upload queue.
//!
//! A crash happens at the worst possible moment — while the process is dying —
//! so the queue never holds anything only in memory. An item is written to a
//! file when it is captured, marked when an attempt fails, and deleted when it
//! is either accepted or permanently rejected. A machine that is offline, or a
//! collector that is not deployed yet, loses nothing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::report::QueuedItem;

/// One queued item, with the bookkeeping needed to retry it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueueEntry {
    pub id: String,
    pub queued_at: DateTime<Utc>,
    /// Failed attempts so far. Persisted so a permanently broken item is not
    /// retried forever across launches.
    #[serde(default)]
    pub attempts: u32,
    pub item: QueuedItem,
}

impl QueueEntry {
    pub fn new(item: QueuedItem) -> Self {
        Self {
            id: uuid::Uuid::new_v4().simple().to_string(),
            queued_at: Utc::now(),
            attempts: 0,
            item,
        }
    }
}

/// A directory of pending items, oldest first.
#[derive(Debug, Clone)]
pub struct CrashQueue {
    dir: PathBuf,
    max_queued: usize,
    max_age: Duration,
}

impl CrashQueue {
    pub fn new(dir: impl Into<PathBuf>, max_queued: usize, max_age: Duration) -> Self {
        Self { dir: dir.into(), max_queued: max_queued.max(1), max_age }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Write an item, then prune the queue back to its bounds.
    pub fn enqueue(&self, item: QueuedItem) -> std::io::Result<PathBuf> {
        self.enqueue_entry(QueueEntry::new(item))
    }

    pub fn enqueue_entry(&self, entry: QueueEntry) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.path_for(&entry);
        let text = serde_json::to_string(&entry).map_err(std::io::Error::other)?;
        crate::write_atomic(&path, text.as_bytes())?;

        if let Err(err) = self.prune() {
            log::warn!("could not prune the telemetry queue: {err}");
        }
        Ok(path)
    }

    /// Pending items, oldest first. Unreadable entries are dropped: a file that
    /// cannot be parsed can never be uploaded, and keeping it would block the
    /// queue forever.
    pub fn entries(&self) -> Vec<(PathBuf, QueueEntry)> {
        let Ok(read_dir) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };

        let mut paths: Vec<PathBuf> = read_dir
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
            .collect();
        paths.sort();

        let mut entries = Vec::with_capacity(paths.len());
        for path in paths {
            match std::fs::read_to_string(&path).map_err(|err| err.to_string()).and_then(|text| {
                serde_json::from_str::<QueueEntry>(&text).map_err(|err| err.to_string())
            }) {
                Ok(entry) => entries.push((path, entry)),
                Err(err) => {
                    log::warn!("dropping unreadable telemetry entry {}: {err}", path.display());
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        entries
    }

    /// Record a failed attempt, so the count survives a restart.
    pub fn record_attempt(&self, path: &Path, entry: &mut QueueEntry) -> std::io::Result<()> {
        entry.attempts = entry.attempts.saturating_add(1);
        let text = serde_json::to_string(entry).map_err(std::io::Error::other)?;
        crate::write_atomic(path, text.as_bytes())
    }

    /// Drop an item that has been accepted, or permanently rejected.
    pub fn remove(&self, path: &Path) -> std::io::Result<()> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// Remove entries that are too old or past the count bound.
    pub fn prune(&self) -> std::io::Result<usize> {
        let entries = self.entries();
        let now = Utc::now();
        let mut removed = 0;

        // By age first, then by count, so a burst of crashes cannot evict the
        // queue's bound and leave the oldest items behind forever.
        let mut keep: Vec<(PathBuf, QueueEntry)> = Vec::with_capacity(entries.len());
        for (path, entry) in entries {
            let age = now.signed_duration_since(entry.queued_at);
            let too_old = age.num_seconds() > 0 && age.to_std().unwrap_or_default() > self.max_age;
            if too_old {
                self.remove(&path)?;
                removed += 1;
            } else {
                keep.push((path, entry));
            }
        }

        let overflow = keep.len().saturating_sub(self.max_queued);
        for (path, _) in keep.into_iter().take(overflow) {
            self.remove(&path)?;
            removed += 1;
        }

        Ok(removed)
    }

    pub fn len(&self) -> usize {
        self.entries().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) -> std::io::Result<()> {
        for (path, _) in self.entries() {
            self.remove(&path)?;
        }
        Ok(())
    }

    /// Queued crash reports, for a caller that wants to know what is local.
    pub fn crash_count(&self) -> usize {
        self.entries().iter().filter(|(_, entry)| entry.item.is_crash()).count()
    }

    fn path_for(&self, entry: &QueueEntry) -> PathBuf {
        // The timestamp leads so that a plain name sort is a time sort.
        self.dir.join(format!(
            "{:013}-{}.json",
            entry.queued_at.timestamp_millis(),
            &entry.id[..entry.id.len().min(8)]
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Channel;
    use crate::report::{ClientContext, CrashKind, CrashReport, TelemetryEvent};

    fn context() -> ClientContext {
        ClientContext {
            app_version: "0.1.0".into(),
            channel: Channel::Dev,
            os: "macos".into(),
            os_version: None,
            arch: "aarch64".into(),
            install_id: "install".into(),
            session_id: "session".into(),
        }
    }

    fn crash(message: &str) -> QueuedItem {
        QueuedItem::crash(CrashReport::new(&context(), CrashKind::Panic, message))
    }

    fn queue(dir: &Path) -> CrashQueue {
        CrashQueue::new(dir, 10, Duration::from_secs(60 * 60))
    }

    #[test]
    fn an_enqueued_item_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        queue.enqueue(crash("boom")).unwrap();

        let entries = queue.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.attempts, 0);
        assert!(entries[0].1.item.is_crash());
    }

    #[test]
    fn items_come_back_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());

        for index in 0..3 {
            let mut entry = QueueEntry::new(crash("boom"));
            entry.queued_at = Utc::now() + chrono::Duration::seconds(index);
            queue.enqueue_entry(entry).unwrap();
        }

        let entries = queue.entries();
        let times: Vec<_> = entries.iter().map(|(_, entry)| entry.queued_at).collect();
        let mut sorted = times.clone();
        sorted.sort();
        assert_eq!(times, sorted, "queue must drain oldest first");
    }

    #[test]
    fn an_attempt_survives_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        queue.enqueue(crash("boom")).unwrap();

        let (path, mut entry) = queue.entries().remove(0);
        queue.record_attempt(&path, &mut entry).unwrap();

        let reloaded = CrashQueue::new(dir.path(), 10, Duration::from_secs(60 * 60));
        assert_eq!(reloaded.entries()[0].1.attempts, 1);
    }

    #[test]
    fn a_removed_item_stays_removed() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        let path = queue.enqueue(crash("boom")).unwrap();
        queue.remove(&path).unwrap();
        assert!(queue.is_empty());
        // Removing twice is not an error: a retry loop may race itself.
        queue.remove(&path).unwrap();
    }

    #[test]
    fn the_count_bound_drops_the_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let queue = CrashQueue::new(dir.path(), 3, Duration::from_secs(60 * 60));
        for index in 0..5 {
            let mut entry = QueueEntry::new(crash(&format!("boom {index}")));
            entry.queued_at = Utc::now() + chrono::Duration::seconds(index);
            queue.enqueue_entry(entry).unwrap();
        }

        let entries = queue.entries();
        assert_eq!(entries.len(), 3);
        let kept: Vec<String> = entries
            .iter()
            .map(|(_, entry)| match &entry.item {
                QueuedItem::Crash(report) => report.message.clone(),
                QueuedItem::Event(_) => String::new(),
            })
            .collect();
        assert_eq!(kept, ["boom 2", "boom 3", "boom 4"]);
    }

    #[test]
    fn the_age_bound_drops_the_stale() {
        let dir = tempfile::tempdir().unwrap();
        let queue = CrashQueue::new(dir.path(), 10, Duration::from_secs(60));

        let mut stale = QueueEntry::new(crash("old"));
        stale.queued_at = Utc::now() - chrono::Duration::hours(1);
        queue.enqueue_entry(stale).unwrap();
        queue.enqueue(crash("fresh")).unwrap();

        let entries = queue.entries();
        assert_eq!(entries.len(), 1);
        match &entries[0].1.item {
            QueuedItem::Crash(report) => assert_eq!(report.message, "fresh"),
            QueuedItem::Event(_) => panic!("expected a crash"),
        }
    }

    #[test]
    fn a_future_timestamp_is_not_treated_as_ancient() {
        // A clock that jumped backwards must not throw away fresh reports.
        let dir = tempfile::tempdir().unwrap();
        let queue = CrashQueue::new(dir.path(), 10, Duration::from_secs(60));
        let mut ahead = QueueEntry::new(crash("ahead"));
        ahead.queued_at = Utc::now() + chrono::Duration::minutes(5);
        queue.enqueue_entry(ahead).unwrap();
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn an_unreadable_entry_is_dropped_rather_than_blocking_the_queue() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        queue.enqueue(crash("boom")).unwrap();
        std::fs::write(dir.path().join("0000000000000-broken.json"), "{ not json").unwrap();
        std::fs::write(dir.path().join("ignore-me.txt"), "not an entry").unwrap();

        let entries = queue.entries();
        assert_eq!(entries.len(), 1);
        assert!(!dir.path().join("0000000000000-broken.json").exists());
    }

    #[test]
    fn events_and_crashes_share_the_queue() {
        let dir = tempfile::tempdir().unwrap();
        let queue = queue(dir.path());
        queue.enqueue(crash("boom")).unwrap();
        queue
            .enqueue(QueuedItem::event(TelemetryEvent::FeatureUsed {
                name: "terminal".into(),
                duration_ms: Some(120),
            }))
            .unwrap();

        assert_eq!(queue.len(), 2);
        assert_eq!(queue.crash_count(), 1);

        let round_tripped = queue.entries();
        assert!(round_tripped.iter().any(|(_, entry)| entry.item.is_crash()));
        assert!(round_tripped.iter().any(|(_, entry)| !entry.item.is_crash()));
    }
}
