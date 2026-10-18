//! One pane's shell session, attached through the PTY daemon.
//!
//! This module owns the only blocking I/O in the terminal feature. Each pane
//! gets a [`TerminalSession`] that asks [`goble_pty`](goble_pty) for a session
//! in the pane's working directory and attaches a background reader to it. The
//! reader interprets every output event into the pane's
//! [`Emulator`](crate::emulator::Emulator) — a real VT screen, not a line
//! buffer — behind an `Arc<Mutex<..>>`; the UI thread takes the lock for the
//! few microseconds it needs to clone the visible rows, the cursor and the
//! input mode. Blocking reads therefore never touch the UI/window thread, and
//! the shell is never a cursor of the winit event loop.
//!
//! The daemon owns the PTY, so dropping a session only detaches: the shell
//! keeps running and a later app start can reattach to the same session id.
//! Closing a pane is explicit and calls [`TerminalSession::kill`].
//!
//! The `TerminalSession` impl is cut at method boundaries across the submodules:
//! the pump ([`pump`]), the paint state ([`view`]), the read accessors
//! ([`access`]) and the session side of the claim protocol ([`claims`]).

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;

use goble_terminal::{HookEvent, OscEvent, Palette, ToolResult};

use crate::emulator::Emulator;

use super::claim::{ClaimOutcome, PaneCommands, PendingCommandClaim, CLAIM_TIMEOUT};
use super::shell::{default_shell, resolve_cwd, shell_command};

#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(unix)]
use goble_pty::protocol::{decode_bytes, ProtocolError, Request, ServerMessage};
#[cfg(unix)]
use goble_pty::{resolve_socket_path, Client};

mod access;
mod claims;
mod pump;
mod view;

pub use view::TerminalViewState;

/// The daemon connection and session this pane is attached to.
///
/// The control client is shared with the writer so writes, resizes and the
/// pane's explicit kill all travel over one connection. The reader has its own
/// connection but the same `client_id`, because the daemon keys subscriptions
/// by that id: detaching on the control connection therefore releases the
/// reader's subscription too.
#[cfg(unix)]
struct DaemonLink {
    control: Arc<Mutex<Client>>,
    session_id: String,
    alive: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

/// The pane's writer, as a [`Write`] over the daemon's `input` request.
#[cfg(unix)]
struct DaemonWriter {
    control: Arc<Mutex<Client>>,
    session_id: String,
}

#[cfg(unix)]
impl Write for DaemonWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut client = self
            .control
            .lock()
            .map_err(|_| std::io::Error::other("the PTY daemon client lock is poisoned"))?;
        let id = client.next_id();
        let response = client
            .request(Request::Input {
                id,
                session_id: self.session_id.clone(),
                bytes_b64: goble_pty::encode_bytes(buf),
            })
            .map_err(std::io::Error::other)?;
        match response {
            ServerMessage::Ok { .. } => Ok(buf.len()),
            ServerMessage::Error { message, .. } => Err(std::io::Error::other(message)),
            other => Err(std::io::Error::other(format!(
                "unexpected input response from the PTY daemon: {other:?}"
            ))),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// One PTY-backed shell session for a terminal pane.
///
/// The reader thread owns the blocking read end and interprets bytes into the
/// shared [`Emulator`]; the UI thread only ever takes a short-lived lock to
/// clone what it paints. Writing input goes to the daemon.
pub struct TerminalSession {
    pub(super) state: Arc<Mutex<Emulator>>,
    pub(super) writer: Option<Box<dyn Write + Send>>,
    /// The daemon session behind this pane, while it is attached.
    #[cfg(unix)]
    link: Option<DaemonLink>,
    /// The grid size the screen and the daemon were last told about.
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
    /// The report the app has already been offered, so a directory the shell
    /// said once is never offered as a move twice.
    cwd_handed: Option<String>,
    /// Whether a shell of its own was spawned behind this session. A shell
    /// starts where it is put — by the app, or by itself when the app named no
    /// directory — so its start is never a move. A session with no shell behind
    /// it (a failed launch, a screen a test drives) has no start to discount.
    spawned: bool,
    /// Scratch directory holding this session's integration script; removed on
    /// drop. `None` when the shell has no integration or the file could not be
    /// written, and on a reattach (the running shell already sourced it).
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
            .field("alive", &self.is_alive())
            .field("session_id", &self.session_id())
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
            #[cfg(unix)]
            link: None,
            size: (24, 80),
            cell: (0, 0),
            palette: Palette::xterm(),
            title: None,
            bell: false,
            hooks: VecDeque::new(),
            osc: VecDeque::new(),
            cwd: None,
            cwd_handed: None,
            spawned: false,
            integration_dir: None,
            pending_claim: None,
            claim_outcomes: VecDeque::new(),
            claim_timeout: CLAIM_TIMEOUT,
            commands: Arc::new(Mutex::new(PaneCommands::default())),
            awaiting: HashMap::new(),
            tool_results: Vec::new(),
        }
    }

    /// A session around a screen with a shell started behind it, the shape
    /// [`Self::spawn`] builds: a unit test has no pty to start a shell in.
    #[cfg(test)]
    pub(crate) fn started(emulator: Emulator) -> Self {
        let mut session = Self::with_emulator(emulator);
        session.spawned = true;
        session
    }

    /// Ask the daemon for a shell in `cwd` and attach to it.
    ///
    /// A daemon that cannot be reached or started is an error: the pane shows
    /// [`Self::failed`]'s report rather than silently running a shell of its
    /// own outside the daemon.
    pub fn spawn(cwd: &str) -> Result<Self, String> {
        Self::spawn_for(0, cwd, None)
    }

    /// Spawn or reattach one pane's session.
    ///
    /// `daemon_session_id` is the id the pane persisted last run. It is reused
    /// only while the daemon still reports it alive; a session the daemon no
    /// longer has gets a fresh shell instead of a stale id.
    #[cfg(unix)]
    pub(crate) fn spawn_for(
        pane_id: u64,
        cwd: &str,
        daemon_session_id: Option<&str>,
    ) -> Result<Self, String> {
        let socket = resolve_socket_path();
        let control = Client::connect_or_start()
            .map_err(|e| format!("connect to the PTY daemon at {}: {e:#}", socket.display()))?;
        Self::spawn_with_control(control, socket, pane_id, cwd, daemon_session_id)
    }

    /// As [`Self::spawn_for`], against a daemon already listening on `socket`.
    #[cfg(unix)]
    pub(crate) fn spawn_at(
        socket: &Path,
        pane_id: u64,
        cwd: &str,
        daemon_session_id: Option<&str>,
    ) -> Result<Self, String> {
        let control = Client::connect(socket)
            .map_err(|e| format!("connect to the PTY daemon at {}: {e}", socket.display()))?;
        Self::spawn_with_control(control, socket.to_path_buf(), pane_id, cwd, daemon_session_id)
    }

    #[cfg(not(unix))]
    pub(crate) fn spawn_for(
        _pane_id: u64,
        _cwd: &str,
        _daemon_session_id: Option<&str>,
    ) -> Result<Self, String> {
        Err("terminal sessions need the PTY daemon, which is Unix-only".to_string())
    }

    #[cfg(not(unix))]
    pub(crate) fn spawn_at(
        _socket: &Path,
        _pane_id: u64,
        _cwd: &str,
        _daemon_session_id: Option<&str>,
    ) -> Result<Self, String> {
        Err("terminal sessions need the PTY daemon, which is Unix-only".to_string())
    }

    #[cfg(unix)]
    fn spawn_with_control(
        mut control: Client,
        socket: PathBuf,
        pane_id: u64,
        cwd: &str,
        wanted: Option<&str>,
    ) -> Result<Self, String> {
        control
            .set_read_timeout(Some(Duration::from_secs(10)))
            .map_err(|e| format!("set the PTY daemon read timeout: {e}"))?;

        // Both connections share this id so a `detach` on the control
        // connection releases the reader's subscription on the daemon, while a
        // second session for the same pane cannot detach the first. The pid is
        // part of the id because two app instances are two processes: a
        // pane-local counter alone would collide and let one instance detach
        // the other's subscription.
        static NEXT_CLIENT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let client_id = format!(
            "pane-{pane_id}-{}-{}",
            std::process::id(),
            NEXT_CLIENT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let id = control.next_id();
        match control
            .request(Request::Hello {
                id,
                client_id: client_id.clone(),
            })
            .map_err(|e| format!("hello the PTY daemon: {e}"))?
        {
            ServerMessage::Ok { .. } => {}
            other => return Err(format!("the PTY daemon refused the hello: {other:?}")),
        }

        // A persisted id is reused only when the daemon still has it alive.
        let reuse = wanted.and_then(|wanted| {
            let id = control.next_id();
            match control.request(Request::List { id }) {
                Ok(ServerMessage::Sessions { sessions, .. }) => sessions
                    .into_iter()
                    .find(|session| session.session_id == wanted && session.alive),
                _ => None,
            }
        });

        let (info, integration_dir, created) = match reuse {
            Some(info) => (info, None, false),
            None => {
                let launch = shell_command(&default_shell());
                let create_id = control.next_id();
                let result = control.request(Request::Create {
                    id: create_id,
                    pane_id: format!("pane-{pane_id}"),
                    cwd: resolve_cwd(cwd).display().to_string(),
                    cols: 80,
                    rows: 24,
                    shell: Some(launch.program),
                    args: launch.args,
                    env: launch.env,
                });
                match result {
                    Ok(ServerMessage::Ok {
                        session: Some(info),
                        ..
                    }) if info.alive => (info, launch.integration_dir, true),
                    Ok(ServerMessage::Error { message, .. }) => {
                        remove_integration_dir(launch.integration_dir);
                        return Err(message);
                    }
                    Ok(other) => {
                        remove_integration_dir(launch.integration_dir);
                        return Err(format!("the PTY daemon could not start a shell: {other:?}"));
                    }
                    Err(e) => {
                        remove_integration_dir(launch.integration_dir);
                        return Err(format!("create a PTY daemon session: {e}"));
                    }
                }
            }
        };

        // The reader has its own connection; `request` buffers the replayed
        // events until its `attach` response arrives, then the loop feeds them.
        let reader = match Client::connect(&socket) {
            Ok(reader) => reader,
            Err(e) => {
                remove_integration_dir(integration_dir);
                if created {
                    let id = control.next_id();
                    let _ = control.send(&Request::Kill {
                        id,
                        session_id: info.session_id,
                    });
                }
                return Err(format!("connect a reader to the PTY daemon: {e}"));
            }
        };
        let state = Arc::new(Mutex::new(Emulator::new(
            info.cols as usize,
            info.rows as usize,
        )));
        let alive = Arc::new(AtomicBool::new(info.alive));
        let stop = Arc::new(AtomicBool::new(false));
        let session_id = info.session_id.clone();
        {
            let out = Arc::clone(&state);
            let alive = Arc::clone(&alive);
            let stop = Arc::clone(&stop);
            let session_id = session_id.clone();
            let client_id = client_id.clone();
            // Detached: it owns the blocking read end and feeds the screen until
            // the session ends or the pane detaches. The UI thread never joins.
            std::thread::spawn(move || {
                read_events(reader, client_id, session_id, out, alive, stop)
            });
        }

        let control = Arc::new(Mutex::new(control));
        let writer = Box::new(DaemonWriter {
            control: Arc::clone(&control),
            session_id: session_id.clone(),
        }) as Box<dyn Write + Send>;

        Ok(Self {
            state,
            writer: Some(writer),
            link: Some(DaemonLink {
                control,
                session_id,
                alive,
                stop,
            }),
            size: (info.rows, info.cols),
            cell: (0, 0),
            palette: Palette::xterm(),
            title: None,
            bell: false,
            hooks: VecDeque::new(),
            osc: VecDeque::new(),
            cwd: None,
            cwd_handed: None,
            spawned: true,
            integration_dir,
            pending_claim: None,
            claim_outcomes: VecDeque::new(),
            claim_timeout: CLAIM_TIMEOUT,
            commands: Arc::new(Mutex::new(PaneCommands::default())),
            awaiting: HashMap::new(),
            tool_results: Vec::new(),
        })
    }

    /// The daemon session this pane is attached to, for persisting and
    /// reattaching. `None` for a failed or test-only session.
    pub fn session_id(&self) -> Option<&str> {
        #[cfg(unix)]
        {
            self.link
                .as_ref()
                .map(|link| link.session_id.as_str())
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    /// Close the daemon session behind this pane. This is the pane-close path;
    /// dropping a session only detaches.
    pub fn kill(&mut self) {
        #[cfg(unix)]
        if let Some(link) = self.link.take() {
            link.stop.store(true, Ordering::SeqCst);
            if let Ok(mut client) = link.control.lock() {
                let id = client.next_id();
                let _ = client.request(Request::Kill {
                    id,
                    session_id: link.session_id,
                });
            }
        }
        self.writer = None;
    }

    /// Tell the daemon the pane's new grid size. The pixel report is what a
    /// program asking the terminal its text-area size in pixels sees.
    #[cfg(unix)]
    pub(super) fn resize_daemon(&self, rows: u16, cols: u16, cell_width: u16, cell_height: u16) {
        if let Some(link) = self.link.as_ref() {
            if let Ok(mut client) = link.control.lock() {
                let id = client.next_id();
                let _ = client.request(Request::Resize {
                    id,
                    session_id: link.session_id.clone(),
                    cols,
                    rows,
                    pixel_width: cols.saturating_mul(cell_width),
                    pixel_height: rows.saturating_mul(cell_height),
                });
            }
        }
    }
}

#[cfg(unix)]
fn read_events(
    mut client: Client,
    client_id: String,
    session_id: String,
    state: Arc<Mutex<Emulator>>,
    alive: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) {
    if client
        .set_read_timeout(Some(Duration::from_secs(10)))
        .is_err()
    {
        alive.store(false, Ordering::SeqCst);
        return;
    }
    let id = client.next_id();
    match client.request(Request::Hello { id, client_id }) {
        Ok(ServerMessage::Ok { .. }) => {}
        _ => {
            alive.store(false, Ordering::SeqCst);
            return;
        }
    }
    let id = client.next_id();
    match client.request(Request::Attach {
        id,
        session_id,
        from_seq: 0,
    }) {
        Ok(ServerMessage::Ok {
            session: Some(info),
            ..
        }) => alive.store(info.alive, Ordering::SeqCst),
        Ok(ServerMessage::Error { message, .. }) => {
            log::warn!("attach to PTY daemon session failed: {message}");
            alive.store(false, Ordering::SeqCst);
            return;
        }
        Ok(other) => {
            log::warn!("unexpected attach response from the PTY daemon: {other:?}");
            alive.store(false, Ordering::SeqCst);
            return;
        }
        Err(e) => {
            log::warn!("attach to the PTY daemon failed: {e}");
            alive.store(false, Ordering::SeqCst);
            return;
        }
    }

    let _ = client.set_read_timeout(Some(Duration::from_millis(200)));
    while !stop.load(Ordering::Relaxed) {
        match client.next_message() {
            Ok(ServerMessage::Output { bytes_b64, .. }) => {
                if let Ok(bytes) = decode_bytes(&bytes_b64) {
                    if let Ok(mut screen) = state.lock() {
                        screen.feed(&bytes);
                    }
                }
            }
            Ok(ServerMessage::Session { alive: now, .. }) => {
                alive.store(now, Ordering::SeqCst);
            }
            Ok(ServerMessage::Exit { .. }) => {
                alive.store(false, Ordering::SeqCst);
                break;
            }
            Ok(_) => {}
            Err(ProtocolError::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => break,
        }
    }
}

fn remove_integration_dir(dir: Option<PathBuf>) {
    if let Some(dir) = dir {
        let _ = std::fs::remove_dir_all(dir);
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        // Detach rather than kill: the daemon owns the shell, so it survives a
        // closed pane's session being dropped and an app restart can reattach.
        #[cfg(unix)]
        if let Some(link) = self.link.take() {
            link.stop.store(true, Ordering::SeqCst);
            if let Ok(mut client) = link.control.lock() {
                let id = client.next_id();
                let _ = client.send(&Request::Detach {
                    id,
                    session_id: link.session_id,
                });
            }
        }
        remove_integration_dir(self.integration_dir.take());
    }
}
