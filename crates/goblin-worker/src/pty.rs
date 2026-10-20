//! PTY sessions for the worker's WebSocket transport.
//!
//! A browser client opens a terminal with `OpenPty`; the worker owns a real
//! child shell (`portable-pty`) for it and streams its output back as base64
//! `PtyOutput` frames. The worker is the PTY daemon here, so unlike
//! `goble-pty` there is no separate process: panes live and die with the
//! connection that opened them (see [`PtyManager::close_owned`]).
//!
//! Concurrency:
//! * `spawn` runs the openpty/spawn synchronously; the `panes` lock serializes
//!   opens so two connections cannot open the same `pane_id`.
//! * A blocking reader pumps 8 KiB chunks into a bounded queue. The queue is
//!   the backpressure: a child that outruns the WebSocket blocks on its own
//!   pty instead of growing an unbounded buffer in the worker.
//! * An async task coalesces queued chunks (up to [`MAX_OUTPUT_EVENT`]) and
//!   emits one `PtyOutput` per batch, yielding between batches so the
//!   WebSocket writer is scheduled.
//! * A blocking waiter reaps the child and emits `PtyExit`; the entry is
//!   removed first, so a pane id can be reopened as soon as the child is gone.
//!
//! Ownership: writes, resizes and closes are refused for a connection that did
//! not open the pane. Output travels on the worker-wide event channel like
//! every other `WorkerMessage`, so a second connection to the same worker can
//! see another connection's `PtyOutput`; per-connection fan-out would need the
//! socket to filter the event stream.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use goble_core::protocol::{decode_pty_bytes, encode_pty_bytes, WorkerMessage};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, Child, ChildKiller, CommandBuilder, MasterPty, PtySize};
use tokio::sync::{broadcast, mpsc, watch};

/// Bytes read from the pty at a time.
const READ_CHUNK: usize = 8 * 1024;

/// Largest raw byte count carried by a single `PtyOutput` frame.
const MAX_OUTPUT_EVENT: usize = 32 * 1024;

/// Chunks the reader may queue before it blocks the child on its pty.
const OUTPUT_QUEUE: usize = 32;

/// Owns the worker's PTY children, one per `pane_id`.
pub struct PtyManager {
    event_tx: broadcast::Sender<WorkerMessage>,
    panes: Mutex<HashMap<String, Arc<Pane>>>,
    next_generation: AtomicU64,
    /// Test-only shell override; production resolves `$SHELL`.
    shell: Option<String>,
}

struct Pane {
    /// Distinguishes a pane from a later pane that reuses the same `pane_id`,
    /// so a stale exit task cannot remove the new one.
    generation: u64,
    owner: String,
    pane_id: String,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    size: Mutex<(u16, u16)>,
    /// Set when the pane is closed so the output task stops promptly instead
    /// of waiting for the child's death to close the read side.
    cancel: watch::Sender<bool>,
}

impl std::fmt::Debug for PtyManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtyManager")
            .field("panes", &self.panes.lock().len())
            .finish_non_exhaustive()
    }
}

impl PtyManager {
    pub fn new(event_tx: broadcast::Sender<WorkerMessage>) -> Arc<Self> {
        Self::with_shell(event_tx, None)
    }

    fn with_shell(event_tx: broadcast::Sender<WorkerMessage>, shell: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            event_tx,
            panes: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(1),
            shell,
        })
    }

    /// Open `pane_id` for `owner` and return the events for the caller to emit.
    ///
    /// Failures — a duplicate pane, an unusable `cwd`, a shell that will not
    /// spawn — are `PtyError` events rather than errors, because they cross the
    /// wire the same way successes do. A missing `cwd` is created (a browser
    /// may open a terminal in a directory that does not exist yet); anything
    /// that cannot be created is a `PtyError`.
    pub fn spawn(
        self: &Arc<Self>,
        owner: &str,
        pane_id: &str,
        cwd: &str,
        cols: u16,
        rows: u16,
    ) -> Vec<WorkerMessage> {
        if cols == 0 || rows == 0 {
            return vec![pty_error(
                pane_id,
                "cols and rows must be greater than zero",
            )];
        }

        let cwd = if cwd.trim().is_empty() {
            default_cwd()
        } else {
            PathBuf::from(cwd)
        };
        if !cwd.is_dir() {
            if let Err(error) = std::fs::create_dir_all(&cwd) {
                return vec![pty_error(
                    pane_id,
                    format!("cwd {}: {error}", cwd.display()),
                )];
            }
        }

        // Held across the spawn so the duplicate check and the insert are one
        // step. Spawning a child takes milliseconds; opens therefore serialize.
        let mut panes = self.panes.lock();
        if panes.contains_key(pane_id) {
            return vec![pty_error(pane_id, "a pane with this id is already open")];
        }

        let shell = self
            .shell
            .clone()
            .or_else(|| std::env::var("SHELL").ok().filter(|s| !s.trim().is_empty()))
            .unwrap_or_else(|| "/bin/bash".to_string());

        let pty_system = native_pty_system();
        let pair = match pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            Ok(pair) => pair,
            Err(error) => return vec![pty_error(pane_id, format!("open pty: {error}"))],
        };

        // `CommandBuilder::new` starts from the worker's environment, which is
        // what a terminal inside the sandbox should see.
        let mut command = CommandBuilder::new(&shell);
        command.cwd(&cwd);
        let child = match pair.slave.spawn_command(command) {
            Ok(child) => child,
            Err(error) => {
                return vec![pty_error(
                    pane_id,
                    format!("spawn {shell} in {}: {error}", cwd.display()),
                )]
            }
        };
        drop(pair.slave);

        let reader = match pair.master.try_clone_reader() {
            Ok(reader) => reader,
            Err(error) => {
                let mut killer = child.clone_killer();
                let _ = killer.kill();
                return vec![pty_error(pane_id, format!("pty reader: {error}"))];
            }
        };
        let writer = match pair.master.take_writer() {
            Ok(writer) => writer,
            Err(error) => {
                let mut killer = child.clone_killer();
                let _ = killer.kill();
                return vec![pty_error(pane_id, format!("pty writer: {error}"))];
            }
        };

        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        let (cancel, cancel_rx) = watch::channel(false);
        let pane = Arc::new(Pane {
            generation,
            owner: owner.to_string(),
            pane_id: pane_id.to_string(),
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            killer: Mutex::new(child.clone_killer()),
            size: Mutex::new((cols, rows)),
            cancel,
        });
        panes.insert(pane_id.to_string(), Arc::clone(&pane));
        drop(panes);

        self.spawn_reader(pane_id.to_string(), reader, cancel_rx);
        self.spawn_waiter(generation, pane_id.to_string(), child, &pane);

        vec![WorkerMessage::PtyOpened {
            pane_id: pane_id.to_string(),
        }]
    }

    /// Write base64 keystrokes to the pane's stdin.
    pub fn input(&self, owner: &str, pane_id: &str, data: &str) -> Vec<WorkerMessage> {
        let pane = match self.owned(owner, pane_id) {
            Ok(pane) => pane,
            Err(events) => return events,
        };
        let bytes = match decode_pty_bytes(data) {
            Ok(bytes) => bytes,
            Err(error) => {
                return vec![pty_error(pane_id, format!("invalid base64 input: {error}"))]
            }
        };
        let mut writer = pane.writer.lock();
        match writer.write_all(&bytes).and_then(|_| writer.flush()) {
            Ok(()) => Vec::new(),
            Err(error) => vec![pty_error(pane_id, format!("write to pty: {error}"))],
        }
    }

    /// Apply a new window size to the pane.
    ///
    /// There is no protocol ack: the child observes the size through its tty
    /// (`stty size`), which is what the test asserts.
    pub fn resize(&self, owner: &str, pane_id: &str, cols: u16, rows: u16) -> Vec<WorkerMessage> {
        if cols == 0 || rows == 0 {
            return vec![pty_error(
                pane_id,
                "cols and rows must be greater than zero",
            )];
        }
        let pane = match self.owned(owner, pane_id) {
            Ok(pane) => pane,
            Err(events) => return events,
        };
        let resized = pane.master.lock().resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        match resized {
            Ok(()) => {
                *pane.size.lock() = (cols, rows);
                Vec::new()
            }
            Err(error) => vec![pty_error(pane_id, format!("resize pty: {error}"))],
        }
    }

    /// Close one pane: remove the entry, signal cancellation, kill the child.
    ///
    /// The child is reaped by the waiter task, which is what emits the
    /// `PtyExit`. Closing a pane that already exited (or was never opened) is
    /// a no-op, so a client racing the child's exit sees no error.
    pub fn close(&self, owner: &str, pane_id: &str) -> Vec<WorkerMessage> {
        let pane = {
            let mut panes = self.panes.lock();
            match panes.get(pane_id) {
                Some(pane) if pane.owner != owner => {
                    return vec![pty_error(pane_id, "pane belongs to another connection")]
                }
                _ => panes.remove(pane_id),
            }
        };
        let Some(pane) = pane else {
            return Vec::new();
        };
        let _ = pane.cancel.send(true);
        let killed = pane.killer.lock().kill();
        match killed {
            Ok(()) => Vec::new(),
            Err(error) => vec![pty_error(pane_id, format!("kill pty: {error}"))],
        }
    }

    /// Close every pane `owner` opened. Called when its WebSocket disconnects:
    /// panes are connection-scoped, so nothing else may outlive the socket.
    pub fn close_owned(&self, owner: &str) -> Vec<WorkerMessage> {
        self.close_where(|pane| pane.owner == owner)
    }

    /// Close every pane.
    ///
    /// Panes are children of the worker process and survive its exit (a pty
    /// child is a session leader), so a shutdown path should call this. The
    /// worker has no graceful-shutdown hook yet; connection teardown
    /// ([`Self::close_owned`]) is what releases panes today.
    pub fn close_all(&self) -> Vec<WorkerMessage> {
        self.close_where(|_| true)
    }

    fn close_where(&self, matches: impl Fn(&Pane) -> bool) -> Vec<WorkerMessage> {
        let closed: Vec<Arc<Pane>> = {
            let mut panes = self.panes.lock();
            let ids: Vec<String> = panes
                .iter()
                .filter(|(_, pane)| matches(pane))
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter().filter_map(|id| panes.remove(&id)).collect()
        };
        let mut events = Vec::new();
        for pane in closed {
            let _ = pane.cancel.send(true);
            if let Err(error) = pane.killer.lock().kill() {
                events.push(pty_error(&pane.pane_id, format!("kill pty: {error}")));
            }
        }
        events
    }

    /// Whether `owner` has a live pane named `pane_id`.
    pub fn is_open(&self, owner: &str, pane_id: &str) -> bool {
        self.panes
            .lock()
            .get(pane_id)
            .is_some_and(|pane| pane.owner == owner)
    }

    /// Live pane count, regardless of owner.
    pub fn pane_count(&self) -> usize {
        self.panes.lock().len()
    }

    /// The size last applied to `pane_id`, as `(cols, rows)`.
    pub fn size(&self, owner: &str, pane_id: &str) -> Option<(u16, u16)> {
        self.owned(owner, pane_id)
            .ok()
            .map(|pane| *pane.size.lock())
    }

    /// Resolve a pane this `owner` may act on.
    ///
    /// A pane belongs to the connection that opened it; another connection
    /// gets a `PtyError` rather than control of the terminal.
    fn owned(&self, owner: &str, pane_id: &str) -> Result<Arc<Pane>, Vec<WorkerMessage>> {
        match self.panes.lock().get(pane_id) {
            Some(pane) if pane.owner == owner => Ok(Arc::clone(pane)),
            Some(_) => Err(vec![pty_error(
                pane_id,
                "pane belongs to another connection",
            )]),
            None => Err(vec![pty_error(pane_id, "no such pane")]),
        }
    }

    fn emit(&self, message: WorkerMessage) {
        let _ = self.event_tx.send(message);
    }

    /// Pump the pty into a bounded queue on a blocking thread.
    fn spawn_reader(
        self: &Arc<Self>,
        pane_id: String,
        mut reader: Box<dyn Read + Send>,
        mut cancel: watch::Receiver<bool>,
    ) {
        let (chunks_tx, mut chunks_rx) = mpsc::channel::<Vec<u8>>(OUTPUT_QUEUE);
        tokio::task::spawn_blocking(move || {
            let mut buffer = vec![0u8; READ_CHUNK];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if chunks_tx.blocking_send(buffer[..read].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        });

        let manager = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                let first = tokio::select! {
                    biased;
                    // Closing a pane stops its stream even if the child is slow
                    // to die; the reader ends when the pty does.
                    changed = cancel.changed() => {
                        let _ = changed;
                        break;
                    }
                    chunk = chunks_rx.recv() => match chunk {
                        Some(chunk) => chunk,
                        None => break,
                    },
                };
                // Coalesce what the queue already holds into one frame. Bounded
                // by MAX_OUTPUT_EVENT so a fast producer cannot make the worker
                // spend an unbounded time on one message.
                let mut batch = first;
                while batch.len() < MAX_OUTPUT_EVENT {
                    match chunks_rx.try_recv() {
                        Ok(next) => batch.extend_from_slice(&next),
                        Err(_) => break,
                    }
                }
                manager.emit(WorkerMessage::PtyOutput {
                    pane_id: pane_id.clone(),
                    data: encode_pty_bytes(&batch),
                });
                tokio::task::yield_now().await;
            }
        });
    }

    /// Reap the child on a blocking thread, then remove the pane and report
    /// `PtyExit`. Removal happens before the event so a reopened pane id is not
    /// clobbered by this stale task.
    fn spawn_waiter(
        self: &Arc<Self>,
        generation: u64,
        pane_id: String,
        mut child: Box<dyn Child + Send + Sync>,
        pane: &Arc<Pane>,
    ) {
        // The waiter needs the pane's cancel sender so a child that exits on
        // its own also stops the output stream.
        let cancel = pane.cancel.clone();
        let manager = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            let code = match child.wait() {
                Ok(status) => Some(status.exit_code() as i32),
                Err(_) => None,
            };
            let _ = cancel.send(true);
            manager.forget(generation, &pane_id);
            manager.emit(WorkerMessage::PtyExit { pane_id, code });
        });
    }

    fn forget(&self, generation: u64, pane_id: &str) {
        let mut panes = self.panes.lock();
        if panes
            .get(pane_id)
            .is_some_and(|pane| pane.generation == generation)
        {
            panes.remove(pane_id);
        }
    }
}

/// Where an empty `OpenPty.cwd` lands. A sandbox is provisioned with the
/// project's shared workspace path, so terminals open there; local development
/// falls back to the worker's own directory.
fn default_cwd() -> PathBuf {
    ["GOBLE_WORKSPACE_PATH", "GOBLIN_WORKSPACE_ROOT"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(PathBuf::from)
        .find(|path| path.is_dir())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn pty_error(pane_id: &str, message: impl std::fmt::Display) -> WorkerMessage {
    WorkerMessage::PtyError {
        pane_id: pane_id.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    const TEST_TIMEOUT: Duration = Duration::from_secs(10);

    /// `/bin/sh` exists on both the macOS test host and the Linux sandbox, so
    /// the tests do not depend on the runner's `$SHELL`.
    fn test_manager() -> (Arc<PtyManager>, broadcast::Receiver<WorkerMessage>) {
        let (tx, rx) = broadcast::channel(256);
        (PtyManager::with_shell(tx, Some("/bin/sh".to_string())), rx)
    }

    /// Kills every child even when a test fails before its explicit cleanup,
    /// so a panic cannot leave a shell or a blocked waiter behind.
    struct Cleanup(Arc<PtyManager>);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.0.close_all();
        }
    }

    async fn next_matching(
        events: &mut broadcast::Receiver<WorkerMessage>,
        matches: impl Fn(&WorkerMessage) -> bool,
    ) -> WorkerMessage {
        timeout(TEST_TIMEOUT, async {
            loop {
                let message = events.recv().await.expect("event channel closed");
                if matches(&message) {
                    return message;
                }
            }
        })
        .await
        .expect("timed out waiting for a worker message")
    }

    /// Collect decoded output until it contains `needle`. The terminal echoes
    /// input, so callers pick a needle that only the child can produce.
    async fn wait_for_output(
        events: &mut broadcast::Receiver<WorkerMessage>,
        needle: &str,
    ) -> String {
        timeout(TEST_TIMEOUT, async {
            let mut seen = String::new();
            loop {
                match events.recv().await.expect("event channel closed") {
                    WorkerMessage::PtyOutput { data, .. } => {
                        seen.push_str(&String::from_utf8_lossy(
                            &decode_pty_bytes(&data).expect("output is base64"),
                        ));
                        if seen.contains(needle) {
                            return seen;
                        }
                    }
                    WorkerMessage::PtyError { message, .. } => panic!("pty error: {message}"),
                    _ => {}
                }
            }
        })
        .await
        .expect("timed out waiting for pty output")
    }

    #[tokio::test]
    async fn test_open_input_output_and_close() {
        let (manager, mut events) = test_manager();
        let _cleanup = Cleanup(Arc::clone(&manager));
        let dir = tempfile::tempdir().unwrap();

        let opened = manager.spawn("conn-1", "pane-1", dir.path().to_str().unwrap(), 80, 24);
        assert_eq!(
            opened,
            vec![WorkerMessage::PtyOpened {
                pane_id: "pane-1".to_string()
            }]
        );

        // `hel%s` + `lo` prints `hello`, while the terminal echo of the command
        // does not contain `hello`: a match proves the child's output.
        let input = manager.input(
            "conn-1",
            "pane-1",
            &encode_pty_bytes(b"printf 'hel%s\\n' lo\n"),
        );
        assert!(
            input.is_empty(),
            "a successful write emits nothing: {input:?}"
        );
        wait_for_output(&mut events, "hello").await;

        assert!(manager.close("conn-1", "pane-1").is_empty());
        assert!(
            !manager.is_open("conn-1", "pane-1"),
            "close removes the pane before the child is reaped"
        );
        let exit = next_matching(
            &mut events,
            |m| matches!(m, WorkerMessage::PtyExit { pane_id, .. } if pane_id == "pane-1"),
        )
        .await;
        assert!(matches!(exit, WorkerMessage::PtyExit { code: Some(_), .. }));
        assert_eq!(manager.pane_count(), 0);
    }

    #[tokio::test]
    async fn test_resize_changes_the_size_the_child_sees() {
        let (manager, mut events) = test_manager();
        let _cleanup = Cleanup(Arc::clone(&manager));
        let dir = tempfile::tempdir().unwrap();
        manager.spawn("conn", "pane", dir.path().to_str().unwrap(), 80, 24);

        manager.input("conn", "pane", &encode_pty_bytes(b"stty size\n"));
        wait_for_output(&mut events, "24 80").await;

        assert!(manager.resize("conn", "pane", 100, 30).is_empty());
        assert_eq!(manager.size("conn", "pane"), Some((100, 30)));

        manager.input("conn", "pane", &encode_pty_bytes(b"stty size\n"));
        wait_for_output(&mut events, "30 100").await;

        assert!(manager.close("conn", "pane").is_empty());
    }

    #[tokio::test]
    async fn test_duplicate_open_is_a_pty_error() {
        let (manager, _events) = test_manager();
        let _cleanup = Cleanup(Arc::clone(&manager));
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();

        manager.spawn("conn", "pane", cwd, 80, 24);
        let duplicate = manager.spawn("conn", "pane", cwd, 80, 24);

        assert_eq!(
            duplicate,
            vec![WorkerMessage::PtyError {
                pane_id: "pane".to_string(),
                message: "a pane with this id is already open".to_string(),
            }]
        );
        assert_eq!(manager.pane_count(), 1);
        manager.close("conn", "pane");
    }

    #[tokio::test]
    async fn test_closing_the_owner_removes_only_its_panes() {
        let (manager, mut events) = test_manager();
        let _cleanup = Cleanup(Arc::clone(&manager));
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap();

        manager.spawn("conn-a", "pane-a", cwd, 80, 24);
        manager.spawn("conn-b", "pane-b", cwd, 80, 24);

        // A pane belongs to the connection that opened it.
        let stray = manager.input("conn-b", "pane-a", &encode_pty_bytes(b"x"));
        assert!(
            matches!(&stray[..], [WorkerMessage::PtyError { pane_id, .. }] if pane_id == "pane-a"),
            "another connection must not write to the pane: {stray:?}"
        );

        assert!(manager.close_owned("conn-a").is_empty());
        assert!(!manager.is_open("conn-a", "pane-a"));
        assert!(
            manager.is_open("conn-b", "pane-b"),
            "another owner's pane survives"
        );
        assert_eq!(manager.pane_count(), 1);

        next_matching(
            &mut events,
            |m| matches!(m, WorkerMessage::PtyExit { pane_id, .. } if pane_id == "pane-a"),
        )
        .await;

        assert!(manager.close_owned("conn-b").is_empty());
        assert_eq!(manager.pane_count(), 0);
    }

    #[tokio::test]
    async fn test_unknown_pane_is_a_pty_error_and_close_is_idempotent() {
        let (manager, _events) = test_manager();
        let missing = manager.input("conn", "nope", &encode_pty_bytes(b"x"));
        assert_eq!(
            missing,
            vec![WorkerMessage::PtyError {
                pane_id: "nope".to_string(),
                message: "no such pane".to_string(),
            }]
        );
        assert!(manager.close("conn", "nope").is_empty());
        assert_eq!(manager.pane_count(), 0);
    }

    #[tokio::test]
    async fn test_missing_cwd_is_created() {
        let (manager, _events) = test_manager();
        let _cleanup = Cleanup(Arc::clone(&manager));
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("new").join("deeper");

        let opened = manager.spawn("conn", "pane", nested.to_str().unwrap(), 80, 24);
        assert!(matches!(&opened[..], [WorkerMessage::PtyOpened { .. }]));
        assert!(nested.is_dir(), "the pane's cwd is created when missing");
        manager.close("conn", "pane");
    }

    #[tokio::test]
    async fn test_output_frames_are_capped() {
        let (manager, mut events) = test_manager();
        let _cleanup = Cleanup(Arc::clone(&manager));
        let dir = tempfile::tempdir().unwrap();
        manager.spawn("conn", "pane", dir.path().to_str().unwrap(), 80, 24);

        // 64 KiB from a fast producer: every frame must stay within the cap.
        manager.input(
            "conn",
            "pane",
            &encode_pty_bytes(b"yes x | head -c 65536\n"),
        );

        let produced = timeout(TEST_TIMEOUT, async {
            let mut produced = 0usize;
            while produced < 65536 {
                match events.recv().await.expect("event channel closed") {
                    WorkerMessage::PtyOutput { data, .. } => {
                        let bytes = decode_pty_bytes(&data).unwrap();
                        assert!(
                            bytes.len() <= MAX_OUTPUT_EVENT,
                            "a frame carried {} bytes",
                            bytes.len()
                        );
                        produced += bytes.len();
                    }
                    WorkerMessage::PtyError { message, .. } => panic!("pty error: {message}"),
                    _ => {}
                }
            }
            produced
        })
        .await
        .expect("timed out waiting for the producer");

        assert!(produced >= 65536);
        manager.close("conn", "pane");
    }
}
