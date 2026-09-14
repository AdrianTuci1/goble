//! One pane's PTY-backed shell session.
//!
//! This module owns the only blocking I/O in the terminal feature. Each pane
//! gets a [`TerminalSession`] that spawns a shell in the pane's working
//! directory and reads its output on a dedicated background thread. The reader
//! interprets that output into the pane's [`Emulator`](crate::emulator::Emulator)
//! — a real VT screen, not a line buffer — behind an `Arc<Mutex<..>>`; the UI
//! thread takes the lock for the few microseconds it needs to clone the visible
//! rows, the cursor and the input mode. Blocking reads therefore never touch the
//! UI/window thread, and the shell is never a cursor of the winit event loop.
//!
//! The `TerminalSession` impl is cut at method boundaries across the submodules:
//! the pty pump ([`pump`]), the paint state ([`view`]), the read accessors
//! ([`access`]) and the session side of the claim protocol ([`claims`]).

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use portable_pty::{native_pty_system, PtySize};
use tokio::sync::oneshot;

use goble_terminal::{HookEvent, OscEvent, Palette, ToolResult};

use crate::emulator::Emulator;

use super::claim::{ClaimOutcome, PaneCommands, PendingCommandClaim, CLAIM_TIMEOUT};
use super::shell::{default_shell, resolve_cwd, shell_command};

mod access;
mod claims;
mod pump;
mod view;

pub use view::TerminalViewState;

/// One PTY-backed shell session for a terminal pane.
///
/// The reader thread owns the blocking read end and interprets bytes into the
/// shared [`Emulator`]; the UI thread only ever takes a short-lived lock to
/// clone what it paints. Writing input goes straight to the pty master.
pub struct TerminalSession {
    pub(super) state: Arc<Mutex<Emulator>>,
    pub(super) writer: Option<Box<dyn Write + Send>>,
    /// Kept so the pty stays open for the lifetime of the session.
    _master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    child: Option<Box<dyn portable_pty::Child + Send>>,
    /// The grid size the screen and the pty were last told about.
    size: (u16, u16),
    /// Font metrics, for the reports that are measured in pixels. The pane owns
    /// the font and hands them over as it lays out.
    cell: (u16, u16),
    palette: Palette,
    title: Option<String>,
    /// Set when the program rang the bell; the pane takes it and plays it.
    bell: bool,
    /// Shell-integration events, bounded: the pane reads them, and a pane that
    /// never does must not grow the heap.
    hooks: VecDeque<HookEvent>,
    osc: VecDeque<OscEvent>,
    /// The last working directory the shell reported (`OSC 7`).
    cwd: Option<String>,
    /// Scratch directory holding this session's integration script; removed on
    /// drop. `None` when the shell has no integration or the file could not be
    /// written.
    integration_dir: Option<PathBuf>,
    /// A command the app claimed, waiting for its `Preexec` to resolve it.
    pending_claim: Option<PendingCommandClaim>,
    /// Claim verdicts the pane has not read yet, bounded like the hooks.
    claim_outcomes: VecDeque<ClaimOutcome>,
    /// The bounded wait a new claim gets; overridden in tests.
    pub(super) claim_timeout: Duration,
    /// The harness's side of the shell-tool route for this pane. Commands are
    /// submitted here and answered from the block list's tool results.
    pub(super) commands: Arc<Mutex<PaneCommands>>,
    /// Tool calls waiting for their claimed block's result, keyed by call id.
    awaiting: HashMap<String, oneshot::Sender<Result<String, String>>>,
    /// Claimed blocks whose tool result has arrived but whose caller has not
    /// been answered yet.
    tool_results: Vec<ToolResult>,
}

/// How many shell-integration events a session holds before dropping the
/// oldest. The pane consumes them every frame; this is only a bound.
pub(super) const EVENT_BACKLOG: usize = 256;

impl std::fmt::Debug for TerminalSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalSession")
            .field("alive", &self.child.is_some())
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl TerminalSession {
    /// A session that failed to launch; renders the error rather than panicking.
    pub fn failed(message: String) -> Self {
        let mut emulator = Emulator::new(80, 24);
        emulator.feed(format!("(terminal error: {message})\r\n").as_bytes());
        Self::with_emulator(emulator)
    }

    /// A session around a screen with no pty behind it: how a failed launch and
    /// a test both build one.
    pub(crate) fn with_emulator(emulator: Emulator) -> Self {
        Self {
            state: Arc::new(Mutex::new(emulator)),
            writer: None,
            _master: None,
            child: None,
            size: (24, 80),
            cell: (0, 0),
            palette: Palette::xterm(),
            title: None,
            bell: false,
            hooks: VecDeque::new(),
            osc: VecDeque::new(),
            cwd: None,
            integration_dir: None,
            pending_claim: None,
            claim_outcomes: VecDeque::new(),
            claim_timeout: CLAIM_TIMEOUT,
            commands: Arc::new(Mutex::new(PaneCommands::default())),
            awaiting: HashMap::new(),
            tool_results: Vec::new(),
        }
    }

    /// Spawn `$SHELL` (or `/bin/zsh`) in `cwd` and start a background reader.
    pub fn spawn(cwd: &str) -> Result<Self, String> {
        let columns = 80;
        let screen_lines = 24;
        let state = Arc::new(Mutex::new(Emulator::new(columns, screen_lines)));

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: screen_lines as u16,
                cols: columns as u16,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let (cmd, integration_dir) = shell_command(&default_shell(), resolve_cwd(cwd));

        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| {
            let _ = child.kill();
            e.to_string()
        })?;
        let writer = pair.master.take_writer().map_err(|e| {
            let _ = child.kill();
            e.to_string()
        })?;

        let out = Arc::clone(&state);
        // Detached reader: it owns the blocking read end and interprets bytes
        // into the shared screen until EOF (the child is killed on `drop`). We
        // never join it so the UI thread is never blocked.
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Ok(mut o) = out.lock() {
                            o.feed(&buf[..n]);
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            state,
            writer: Some(writer),
            _master: Some(pair.master),
            child: Some(child),
            size: (screen_lines as u16, columns as u16),
            cell: (0, 0),
            palette: Palette::xterm(),
            title: None,
            bell: false,
            hooks: VecDeque::new(),
            osc: VecDeque::new(),
            cwd: None,
            integration_dir,
            pending_claim: None,
            claim_outcomes: VecDeque::new(),
            claim_timeout: CLAIM_TIMEOUT,
            commands: Arc::new(Mutex::new(PaneCommands::default())),
            awaiting: HashMap::new(),
            tool_results: Vec::new(),
        })
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        // Kill the child so the pty closes; the detached reader thread sees EOF
        // and exits on its own. We never join it (that would block the UI
        // thread).
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
        }
        if let Some(dir) = self.integration_dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
