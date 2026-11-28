//! The settings-file watcher: external edits are noticed and reported as change
//! events, our own writes are not.
//!
//! One `notify` watcher watches the Goble home directory (non-recursive) and is
//! filtered to the three settings file names. A burst of events — an editor
//! saving, or a `fsync`-heavy write — is debounced into one event per file
//! ([`CONFIG_WATCH_DEBOUNCE`], 250 ms). For every candidate file the disk bytes
//! are hashed and compared with the revision the editor last read or wrote:
//!
//! - equal to the last read revision → no change (a touch, a metadata event);
//! - equal to the last *written* hash → our own write, ignored;
//! - different → an external edit, emitted with the new revision.
//!
//! [`poll_for_changes`] is the same comparison without the watcher, for the
//! focus-time fallback on filesystems where inotify/FSEvents is unreliable.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};

use super::document::{sha256_hex, ConfigFile, ConfigRevision};
use super::editor::ConfigEditor;

/// How long the watcher waits for a burst of file events to go quiet before it
/// reports one change. One debounce covers a save that writes a temp file and
/// renames it.
pub const CONFIG_WATCH_DEBOUNCE: Duration = Duration::from_millis(250);

/// One external settings-file change: which file, its new revision and the
/// revision the UI last knew.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigChange {
    pub file: ConfigFile,
    pub path: String,
    pub revision: ConfigRevision,
    pub previous: Option<ConfigRevision>,
    /// Always `true` today: a change this process wrote is filtered out before
    /// it becomes an event. Kept explicit so a future writer can emit its own.
    pub external: bool,
}

impl ConfigChange {
    /// A one-line description for a log line or the Advanced page.
    pub fn line(&self) -> String {
        match &self.previous {
            Some(previous) => format!(
                "{} changed on disk ({} → {})",
                self.file.file_name(),
                previous.short(),
                self.revision.short()
            ),
            None => format!(
                "{} changed on disk ({})",
                self.file.file_name(),
                self.revision.short()
            ),
        }
    }
}

/// A running settings-file watcher. Dropping it stops the thread.
pub struct ConfigWatcher {
    // Field order matters: the watcher (and the callback holding the event
    // sender) is dropped before the thread is joined, so the thread sees the
    // channel disconnect and exits.
    watcher: Option<RecommendedWatcher>,
    events: Receiver<ConfigChange>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ConfigWatcher {
    /// Start watching the editor's home directory.
    pub fn start(editor: Arc<ConfigEditor>) -> Result<Self, String> {
        Self::start_with_debounce(editor, CONFIG_WATCH_DEBOUNCE)
    }

    /// Start watching with an explicit debounce (tests use a shorter one).
    pub fn start_with_debounce(
        editor: Arc<ConfigEditor>,
        debounce: Duration,
    ) -> Result<Self, String> {
        let root = editor.root().to_path_buf();
        fs::create_dir_all(&root)
            .map_err(|error| format!("create watch root {}: {error}", root.display()))?;

        let (raw_tx, raw_rx) = mpsc::channel::<PathBuf>();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else {
                    return;
                };
                for path in event.paths {
                    // A closed receiver just means the watcher is shutting down.
                    let _ = raw_tx.send(path);
                }
            })
            .map_err(|error| format!("start file watcher: {error}"))?;
        watcher
            .watch(&root, RecursiveMode::NonRecursive)
            .map_err(|error| format!("watch {}: {error}", root.display()))?;

        let (change_tx, change_rx) = mpsc::channel::<ConfigChange>();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("goble-config-watch".to_string())
            .spawn(move || {
                watch_loop(raw_rx, change_tx, editor, debounce, thread_stop);
            })
            .map_err(|error| format!("spawn config watcher: {error}"))?;

        Ok(Self {
            watcher: Some(watcher),
            events: change_rx,
            stop,
            thread: Some(thread),
        })
    }

    /// The next queued change, if one has arrived.
    pub fn try_recv(&self) -> Option<ConfigChange> {
        self.events.try_recv().ok()
    }

    /// Wait up to `timeout` for the next change. `None` on timeout or shutdown.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<ConfigChange> {
        self.events.recv_timeout(timeout).ok()
    }

    /// Every change queued right now.
    pub fn drain(&self) -> Vec<ConfigChange> {
        let mut changes = Vec::new();
        while let Some(change) = self.try_recv() {
            changes.push(change);
        }
        changes
    }
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Dropping the watcher drops the event sender, which disconnects the
        // thread's channel; it also stops new events.
        self.watcher.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn watch_loop(
    raw: Receiver<PathBuf>,
    out: mpsc::Sender<ConfigChange>,
    editor: Arc<ConfigEditor>,
    debounce: Duration,
    stop: Arc<AtomicBool>,
) {
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let mut touched: BTreeSet<ConfigFile> = BTreeSet::new();
        // Block for the first event of a burst, waking occasionally to notice a
        // shutdown.
        match raw.recv_timeout(Duration::from_millis(200)) {
            Ok(path) => collect(&mut touched, &path),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // Then keep draining until the burst has been quiet for `debounce`.
        loop {
            match raw.recv_timeout(debounce) {
                Ok(path) => collect(&mut touched, &path),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        for change in changes_for(&editor, &touched) {
            if out.send(change).is_err() {
                return;
            }
        }
    }
}

fn collect(touched: &mut BTreeSet<ConfigFile>, path: &Path) {
    if let Some(file) = file_for_path(path) {
        touched.insert(file);
    }
}

fn file_for_path(path: &Path) -> Option<ConfigFile> {
    let name = path.file_name()?.to_str()?;
    ConfigFile::ALL
        .into_iter()
        .find(|file| file.file_name() == name)
}

/// Compare the current disk bytes of each touched file against the editor's
/// revision and the last-written hash, and return only the real external
/// changes.
fn changes_for(editor: &ConfigEditor, touched: &BTreeSet<ConfigFile>) -> Vec<ConfigChange> {
    let mut changes = Vec::new();
    for file in touched {
        let path = editor.path(*file);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => continue,
        };
        let hash = sha256_hex(&bytes);
        let previous = editor.revision(*file);
        if bytes.is_empty() && previous.is_none() {
            // A file this process has never read and that does not exist: there
            // is no change to report.
            continue;
        }
        if previous
            .as_ref()
            .is_some_and(|previous| previous.hash == hash)
        {
            // Nothing changed, or it is the revision we last read/wrote.
            continue;
        }
        if editor.is_own_revision(*file, &ConfigRevision::of(&bytes, 0)) {
            // Our own write raced the revision cache update: ignore it.
            continue;
        }
        changes.push(ConfigChange {
            file: *file,
            path: path.display().to_string(),
            revision: ConfigRevision::of(&bytes, editor.generation(*file)),
            previous,
            external: true,
        });
    }
    changes
}

/// The focus/focus-time fallback: the same external-edit comparison without a
/// watcher. Cheap enough to call on every window focus and on a two-second
/// timer, and the only reload trigger on a filesystem whose watcher is
/// unreliable.
pub fn poll_for_changes(editor: &ConfigEditor) -> Vec<ConfigChange> {
    let touched: BTreeSet<ConfigFile> = ConfigFile::ALL.into_iter().collect();
    changes_for(editor, &touched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConfigOp, ConfigValue};

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("goble-config-watch-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wait_for_change(watcher: &ConfigWatcher, timeout: Duration) -> Option<ConfigChange> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            if let Some(change) = watcher.recv_timeout(remaining) {
                return Some(change);
            }
        }
    }

    #[test]
    fn a_watcher_reports_an_external_edit_and_ignores_our_own_write() {
        let dir = scratch("external");
        let path = dir.join("config.toml");
        fs::write(&path, "[app]\n").unwrap();
        let editor = Arc::new(ConfigEditor::new(dir.clone()));
        editor.load(ConfigFile::Config).unwrap();
        let watcher =
            ConfigWatcher::start_with_debounce(Arc::clone(&editor), Duration::from_millis(120))
                .expect("watcher starts");

        // Our own write must not come back as an external change.
        editor
            .apply_ops(
                ConfigFile::Config,
                &[ConfigOp::set(
                    &["app", "friendly_name"],
                    ConfigValue::Str("Ada".to_string()),
                )],
                None,
            )
            .expect("own write");
        assert!(
            wait_for_change(&watcher, Duration::from_millis(700)).is_none(),
            "the watcher reported our own write"
        );

        // An external append is one change, with the new revision.
        let mut external = fs::read_to_string(&path).unwrap();
        external.push_str("# written by another editor\n");
        fs::write(&path, &external).unwrap();
        let change =
            wait_for_change(&watcher, Duration::from_secs(10)).expect("external edit is reported");
        assert_eq!(change.file, ConfigFile::Config);
        assert!(change.external);
        assert_eq!(
            change.revision.hash,
            sha256_hex(external.as_bytes()),
            "the event carries the bytes on disk"
        );
        // A second event for the same edit is not reported.
        assert!(
            wait_for_change(&watcher, Duration::from_millis(700)).is_none(),
            "the same edit was reported twice"
        );
    }

    #[test]
    fn a_burst_of_saves_is_debounced_into_one_event() {
        let dir = scratch("debounce");
        let path = dir.join("config.toml");
        fs::write(&path, "[app]\n").unwrap();
        let editor = Arc::new(ConfigEditor::new(dir.clone()));
        editor.load(ConfigFile::Config).unwrap();
        let watcher =
            ConfigWatcher::start_with_debounce(Arc::clone(&editor), Duration::from_millis(250))
                .expect("watcher starts");

        for index in 0..6 {
            fs::write(&path, format!("[app]\nfriendly_name = \"{index}\"\n")).unwrap();
            std::thread::sleep(Duration::from_millis(15));
        }

        let first = wait_for_change(&watcher, Duration::from_secs(10)).expect("one event");
        assert_eq!(first.file, ConfigFile::Config);
        let extra = wait_for_change(&watcher, Duration::from_millis(800));
        assert!(
            extra.is_none(),
            "the burst produced a second event: {extra:?}"
        );
    }

    #[test]
    fn poll_reports_external_edits_and_not_our_own() {
        let dir = scratch("poll");
        let path = dir.join("config.toml");
        fs::write(&path, "[app]\n").unwrap();
        let editor = ConfigEditor::new(dir.clone());
        editor.load(ConfigFile::Config).unwrap();

        assert!(poll_for_changes(&editor).is_empty(), "nothing changed yet");

        editor
            .apply_ops(
                ConfigFile::Config,
                &[ConfigOp::set(&["app", "vim_mode"], ConfigValue::Bool(true))],
                None,
            )
            .expect("own write");
        assert!(
            poll_for_changes(&editor).is_empty(),
            "our own write is not external"
        );

        let mut external = fs::read_to_string(&path).unwrap();
        external.push_str("# external\n");
        fs::write(&path, &external).unwrap();
        let changes = poll_for_changes(&editor);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert_eq!(changes[0].file, ConfigFile::Config);
        assert_eq!(changes[0].revision.hash, sha256_hex(external.as_bytes()));

        // Once reloaded, the poll is quiet again.
        editor.load(ConfigFile::Config).unwrap();
        assert!(poll_for_changes(&editor).is_empty());
    }

    #[test]
    fn a_change_carries_the_previous_revision() {
        let dir = scratch("previous");
        let path = dir.join("config.toml");
        fs::write(&path, "[app]\n").unwrap();
        let editor = ConfigEditor::new(dir.clone());
        let loaded = editor.load(ConfigFile::Config).unwrap();
        let original = loaded.revision().clone();

        fs::write(&path, "[app]\nvim_mode = true\n").unwrap();
        let changes = poll_for_changes(&editor);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].previous.as_ref(), Some(&original));
    }

    #[test]
    fn a_deleted_file_is_a_change_to_the_missing_revision() {
        let dir = scratch("deleted");
        let path = dir.join("config.toml");
        fs::write(&path, "[app]\n").unwrap();
        let editor = ConfigEditor::new(dir.clone());
        editor.load(ConfigFile::Config).unwrap();
        fs::remove_file(&path).unwrap();

        let changes = poll_for_changes(&editor);
        assert_eq!(changes.len(), 1, "{changes:?}");
        assert!(changes[0].revision.is_missing());
    }
}
