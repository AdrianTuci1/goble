//! The daemon: one Unix socket, many PTY sessions, many attached clients.
//!
//! Sessions live independently of connections. A per-session reader thread
//! appends every byte to the replay ring and broadcasts the same `output`
//! event to each subscriber; `detach` only removes a subscription. The child is
//! killed on `kill` (or when the server drops), never on a disconnect.

use std::collections::HashMap;
use std::io::{BufReader, BufWriter, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use portable_pty::{
    native_pty_system, Child, ChildKiller, CommandBuilder, MasterPty, PtySize,
};
use uuid::Uuid;

use crate::protocol::{
    decode_bytes, write_message, MessageReader, ProtocolError, Request, ServerMessage, SessionInfo,
};
use crate::session::{ReplayRing, REPLAY_CAPACITY};

use crate::{cleanup_stale, log_path_for, pid_path_for, read_pidfile};

type SharedState = Arc<Mutex<Inner>>;

/// How many queued messages a client may fall behind before it is detached.
const CLIENT_QUEUE: usize = 4096;

/// How often the accept loop polls for shutdown (it uses a nonblocking
/// listener so `Drop` never hangs joining the thread).
const ACCEPT_POLL: Duration = Duration::from_millis(10);

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, Session>,
}

struct Session {
    info: SessionInfo,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pid: Option<u32>,
    ring: ReplayRing,
    subscribers: Vec<Subscriber>,
}

struct Subscriber {
    client_id: String,
    tx: SyncSender<ServerMessage>,
}

/// A running daemon bound to `socket_path`.
pub struct Server {
    state: SharedState,
    socket_path: PathBuf,
    pid_path: PathBuf,
    pid: u32,
    shutdown: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("socket", &self.socket_path)
            .field("pid", &self.pid)
            .finish_non_exhaustive()
    }
}

impl Server {
    /// Bind `socket`, write the pidfile, and start accepting clients.
    ///
    /// An existing socket is only removed when its pidfile names a dead
    /// process; a live daemon or a non-socket file is refused.
    pub fn start(socket: impl AsRef<Path>) -> Result<Self> {
        let socket_path = socket.as_ref().to_path_buf();
        let pid_path = pid_path_for(&socket_path);
        prepare_socket_dir(&socket_path)?;

        if socket_path.exists() {
            if UnixStream::connect(&socket_path).is_ok() {
                bail!(
                    "another PTY daemon is already listening on {}",
                    socket_path.display()
                );
            }
            if !cleanup_stale(&socket_path, &pid_path).map_err(anyhow::Error::from)? {
                bail!(
                    "{} is owned by a live process (pidfile {})",
                    socket_path.display(),
                    pid_path.display()
                );
            }
        }

        let listener = UnixListener::bind(&socket_path)
            .with_context(|| format!("bind {}", socket_path.display()))?;
        let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600));
        write_pidfile(&pid_path, std::process::id())
            .with_context(|| format!("write {}", pid_path.display()))?;
        listener
            .set_nonblocking(true)
            .context("make the listener nonblocking")?;

        let state: SharedState = Arc::new(Mutex::new(Inner::default()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let accept_state = Arc::clone(&state);
        let accept_shutdown = Arc::clone(&shutdown);
        let accept = thread::spawn(move || accept_loop(listener, accept_state, accept_shutdown));

        Ok(Self {
            state,
            socket_path,
            pid_path,
            pid: std::process::id(),
            shutdown,
            accept: Some(accept),
        })
    }

    /// The socket this daemon bound.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// The pidfile beside it.
    pub fn pid_path(&self) -> &Path {
        &self.pid_path
    }

    /// Block until the accept loop stops. In the foreground daemon this is the
    /// whole lifetime of the process.
    pub fn wait(mut self) -> Result<()> {
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        Ok(())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }

        let sessions = {
            let mut inner = lock(&self.state);
            std::mem::take(&mut inner.sessions)
        };
        for (_, mut session) in sessions {
            let _ = session.killer.kill();
        }

        if read_pidfile(&self.pid_path) == Some(self.pid) {
            let _ = std::fs::remove_file(&self.pid_path);
        }
        remove_if_socket(&self.socket_path);
    }
}

/// Run the daemon in the foreground until it is killed.
pub fn run_foreground(socket: impl AsRef<Path>) -> Result<()> {
    Server::start(socket)?.wait()
}

/// Re-exec ourselves in the background, with stdio on the log beside the
/// socket.
///
/// The first child only opens a new session (`setsid`) and then starts the
/// real daemon before exiting. The daemon is therefore orphaned by the
/// launcher rather than by the client, and it is not a session leader, which
/// is the property a double fork normally provides. No `unsafe` is involved.
pub fn daemonize(socket: &Path) -> Result<()> {
    let exe = std::env::current_exe().context("resolve the daemon executable")?;
    let log_path = log_path_for(socket);
    prepare_socket_dir(socket)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("open {}", log_path.display()))?;

    let mut launcher = Command::new(&exe)
        .arg("--new-session")
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .spawn()
        .with_context(|| format!("spawn {}", exe.display()))?;
    let status = launcher.wait().context("wait for the session launcher")?;
    if !status.success() {
        bail!("the session launcher exited with {status}");
    }
    Ok(())
}

/// Start a new session and launch the foreground daemon as its child.
///
/// Called from `--new-session`; the child inherits the log stdio and is
/// orphaned when this process exits.
pub fn detach_session(socket: &Path) -> Result<()> {
    nix::unistd::setsid().context("start a new session")?;
    let exe = std::env::current_exe().context("resolve the daemon executable")?;
    Command::new(&exe)
        .arg("--foreground")
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn {}", exe.display()))?;
    Ok(())
}

fn prepare_socket_dir(socket: &Path) -> Result<()> {
    let Some(parent) = socket.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    let existed = parent.exists();
    std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    if !existed {
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn write_pidfile(pidfile: &Path, pid: u32) -> std::io::Result<()> {
    let tmp = pidfile.with_extension("pid.tmp");
    std::fs::write(&tmp, format!("{pid}\n"))?;
    std::fs::rename(&tmp, pidfile)
}

fn remove_if_socket(path: &Path) {
    use std::os::unix::fs::FileTypeExt;
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_socket() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn lock(state: &SharedState) -> MutexGuard<'_, Inner> {
    state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn accept_loop(listener: UnixListener, state: SharedState, shutdown: Arc<AtomicBool>) {
    let mut client_seq = 0u64;
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                client_seq += 1;
                let state = Arc::clone(&state);
                thread::spawn(move || handle_client(stream, state, client_seq));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(ACCEPT_POLL),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

fn handle_client(stream: UnixStream, state: SharedState, connection: u64) {
    let Ok(write_half) = stream.try_clone() else {
        return;
    };
    let (tx, rx) = mpsc::sync_channel(CLIENT_QUEUE);
    let writer = thread::spawn(move || write_loop(write_half, rx));

    let mut reader = MessageReader::new(BufReader::new(stream));
    let mut client_id = format!("anon-{connection}");
    loop {
        match reader.read::<Request>() {
            Ok(Some(request)) => {
                if let Request::Hello { client_id: named, .. } = &request {
                    client_id.clone_from(named);
                }
                let response = dispatch(&state, &client_id, request, &tx);
                if tx.send(response).is_err() {
                    break;
                }
            }
            Ok(None) => break,
            Err(ProtocolError::Json(e)) => {
                let _ = tx.send(ServerMessage::Error {
                    id: None,
                    message: format!("invalid request: {e}"),
                });
            }
            Err(ProtocolError::TooLarge) => {
                let _ = tx.send(ServerMessage::Error {
                    id: None,
                    message: "request exceeds the message size limit".into(),
                });
                break;
            }
            Err(ProtocolError::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(ProtocolError::Base64(_)) | Err(ProtocolError::Closed) => break,
            Err(ProtocolError::Io(_)) => break,
        }
    }

    remove_client(&state, &client_id);
    drop(tx);
    let _ = writer.join();
}

fn write_loop(stream: UnixStream, rx: mpsc::Receiver<ServerMessage>) {
    let mut writer = BufWriter::new(stream);
    for message in rx {
        if write_message(&mut writer, &message).is_err() || writer.flush().is_err() {
            break;
        }
    }
}

fn dispatch(
    state: &SharedState,
    client_id: &str,
    request: Request,
    tx: &SyncSender<ServerMessage>,
) -> ServerMessage {
    let id = request.id();
    match request {
        Request::Hello { .. } => ok(id, None),
        Request::List { .. } => {
            let inner = lock(state);
            let mut sessions: Vec<SessionInfo> =
                inner.sessions.values().map(|s| s.info.clone()).collect();
            sessions.sort_by(|a, b| a.session_id.cmp(&b.session_id));
            ServerMessage::Sessions { id, sessions }
        }
        Request::Create {
            pane_id,
            cwd,
            cols,
            rows,
            shell,
            args,
            env,
            ..
        } => {
            let new_session = NewSession {
                pane_id,
                cwd,
                cols,
                rows,
                shell,
                args,
                env,
            };
            match create_session(state, new_session) {
                Ok(info) => ok(id, Some(info)),
                Err(e) => error(Some(id), e),
            }
        }
        Request::Attach {
            session_id,
            from_seq,
            ..
        } => attach(state, client_id, id, &session_id, from_seq, tx),
        Request::Detach { session_id, .. } => detach(state, client_id, id, &session_id),
        Request::Input {
            session_id,
            bytes_b64,
            ..
        } => write_input(state, id, &session_id, &bytes_b64),
        Request::Resize {
            session_id,
            cols,
            rows,
            pixel_width,
            pixel_height,
            ..
        } => resize(
            state,
            id,
            &session_id,
            cols,
            rows,
            pixel_width,
            pixel_height,
        ),
        Request::Kill { session_id, .. } => kill(state, id, &session_id),
        Request::Signal {
            session_id,
            signal: name,
            ..
        } => send_signal(state, id, &session_id, &name),
    }
}

fn ok(id: u64, session: Option<SessionInfo>) -> ServerMessage {
    ServerMessage::Ok { id, session }
}

fn error(id: Option<u64>, message: impl std::fmt::Display) -> ServerMessage {
    ServerMessage::Error {
        id,
        message: message.to_string(),
    }
}

/// The pane and command a `create` request asks for.
struct NewSession {
    pane_id: String,
    cwd: String,
    cols: u16,
    rows: u16,
    shell: Option<String>,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

fn create_session(state: &SharedState, request: NewSession) -> Result<SessionInfo> {
    let NewSession {
        pane_id,
        cwd,
        cols,
        rows,
        shell,
        args,
        env,
    } = request;
    if cols == 0 || rows == 0 {
        bail!("cols and rows must be greater than zero");
    }
    let shell = shell
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("SHELL").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "/bin/sh".to_string());
    let cwd = if cwd.is_empty() {
        std::env::current_dir().context("resolve the current directory")?
    } else {
        PathBuf::from(cwd)
    };

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("open a pty")?;

    let mut command = CommandBuilder::new(&shell);
    command.cwd(&cwd);
    for arg in &args {
        command.arg(arg);
    }
    for (key, value) in &env {
        command.env(key, value);
    }
    let mut child = pair
        .slave
        .spawn_command(command)
        .with_context(|| format!("spawn {shell} in {}", cwd.display()))?;
    drop(pair.slave);

    let reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(e) => {
            let _ = child.kill();
            return Err(e).context("clone the pty reader");
        }
    };
    let writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(e) => {
            let _ = child.kill();
            return Err(e).context("take the pty writer");
        }
    };

    let killer = child.clone_killer();
    let pid = child.process_id();
    let info = SessionInfo {
        session_id: Uuid::new_v4().to_string(),
        pane_id,
        alive: true,
        cols,
        rows,
    };
    let session = Session {
        info: info.clone(),
        writer: Arc::new(Mutex::new(writer)),
        master: Arc::new(Mutex::new(pair.master)),
        killer,
        pid,
        ring: ReplayRing::new(REPLAY_CAPACITY),
        subscribers: Vec::new(),
    };
    lock(state)
        .sessions
        .insert(info.session_id.clone(), session);
    spawn_session_io(Arc::clone(state), info.session_id.clone(), reader, child);
    Ok(info)
}

fn spawn_session_io(
    state: SharedState,
    session_id: String,
    mut reader: Box<dyn Read + Send>,
    mut child: Box<dyn Child + Send + Sync>,
) {
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => deliver_output(&state, &session_id, &buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        // Read the status only after the pty reaches EOF, so the exit event
        // follows every output byte.
        let code = child.wait().map(|s| s.exit_code() as i32).unwrap_or(-1);
        finish_session(&state, &session_id, code);
    });
}

fn deliver_output(state: &SharedState, session_id: &str, bytes: &[u8]) {
    let mut inner = lock(state);
    let Some(session) = inner.sessions.get_mut(session_id) else {
        return;
    };
    let seq = session.ring.push(bytes);
    let event = ServerMessage::output(session_id, seq, bytes);
    // A slow or departed client must never block the pty; it is dropped from
    // the subscription list instead.
    session
        .subscribers
        .retain(|subscriber| subscriber.tx.try_send(event.clone()).is_ok());
}

fn finish_session(state: &SharedState, session_id: &str, code: i32) {
    let mut inner = lock(state);
    let Some(session) = inner.sessions.get_mut(session_id) else {
        return;
    };
    if !session.info.alive {
        return;
    }
    session.info.alive = false;
    let event = ServerMessage::Exit {
        session_id: session_id.to_string(),
        code,
    };
    session
        .subscribers
        .retain(|subscriber| subscriber.tx.try_send(event.clone()).is_ok());
    // The shell is gone and its pty is at EOF. Keeping the session would hold
    // the master and writer descriptors for the life of the daemon — one leak
    // per closed pane, until no pty can be opened. The exit event is the last
    // thing the session owes its clients; the replay ring goes with it.
    let removed = inner.sessions.remove(session_id);
    drop(inner);
    drop(removed);
}

fn attach(
    state: &SharedState,
    client_id: &str,
    id: u64,
    session_id: &str,
    from_seq: u64,
    tx: &SyncSender<ServerMessage>,
) -> ServerMessage {
    let mut inner = lock(state);
    let Some(session) = inner.sessions.get_mut(session_id) else {
        return error(Some(id), format!("no session {session_id}"));
    };

    session
        .subscribers
        .retain(|subscriber| subscriber.client_id != client_id);
    session.subscribers.push(Subscriber {
        client_id: client_id.to_string(),
        tx: tx.clone(),
    });

    let info = session.info.clone();
    let _ = tx.try_send(ServerMessage::session_event(&info));
    for (seq, bytes) in session.ring.chunks_from(from_seq) {
        let _ = tx.try_send(ServerMessage::output(session_id, seq, &bytes));
    }
    ok(id, Some(info))
}

fn detach(state: &SharedState, client_id: &str, id: u64, session_id: &str) -> ServerMessage {
    let mut inner = lock(state);
    let Some(session) = inner.sessions.get_mut(session_id) else {
        return error(Some(id), format!("no session {session_id}"));
    };
    session
        .subscribers
        .retain(|subscriber| subscriber.client_id != client_id);
    ok(id, None)
}

fn remove_client(state: &SharedState, client_id: &str) {
    let mut inner = lock(state);
    for session in inner.sessions.values_mut() {
        session
            .subscribers
            .retain(|subscriber| subscriber.client_id != client_id);
    }
}

fn write_input(
    state: &SharedState,
    id: u64,
    session_id: &str,
    bytes_b64: &str,
) -> ServerMessage {
    let bytes = match decode_bytes(bytes_b64) {
        Ok(bytes) => bytes,
        Err(e) => return error(Some(id), e),
    };
    let writer = {
        let inner = lock(state);
        let Some(session) = inner.sessions.get(session_id) else {
            return error(Some(id), format!("no session {session_id}"));
        };
        if !session.info.alive {
            return error(Some(id), format!("session {session_id} is not alive"));
        }
        Arc::clone(&session.writer)
    };

    let mut writer = writer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match writer.write_all(&bytes).and_then(|_| writer.flush()) {
        Ok(()) => ok(id, None),
        Err(e) => error(Some(id), format!("write to session {session_id}: {e}")),
    }
}

fn resize(
    state: &SharedState,
    id: u64,
    session_id: &str,
    cols: u16,
    rows: u16,
    pixel_width: u16,
    pixel_height: u16,
) -> ServerMessage {
    if cols == 0 || rows == 0 {
        return error(Some(id), "cols and rows must be greater than zero");
    }
    let master = {
        let inner = lock(state);
        let Some(session) = inner.sessions.get(session_id) else {
            return error(Some(id), format!("no session {session_id}"));
        };
        Arc::clone(&session.master)
    };

    let master = master.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Err(e) = master.resize(PtySize {
        rows,
        cols,
        pixel_width,
        pixel_height,
    }) {
        return error(Some(id), format!("resize session {session_id}: {e}"));
    }
    drop(master);

    let mut inner = lock(state);
    let Some(session) = inner.sessions.get_mut(session_id) else {
        return error(Some(id), format!("no session {session_id}"));
    };
    session.info.cols = cols;
    session.info.rows = rows;
    let event = ServerMessage::session_event(&session.info);
    session
        .subscribers
        .retain(|subscriber| subscriber.tx.try_send(event.clone()).is_ok());
    ok(id, Some(session.info.clone()))
}

fn kill(state: &SharedState, id: u64, session_id: &str) -> ServerMessage {
    let removed = lock(state).sessions.remove(session_id);
    let Some(mut session) = removed else {
        return error(Some(id), format!("no session {session_id}"));
    };
    let was_alive = session.info.alive;
    let _ = session.killer.kill();
    if was_alive {
        session.info.alive = false;
        let event = ServerMessage::Exit {
            session_id: session_id.to_string(),
            code: -1,
        };
        for subscriber in &session.subscribers {
            let _ = subscriber.tx.try_send(event.clone());
        }
    }
    ok(id, None)
}

fn send_signal(state: &SharedState, id: u64, session_id: &str, name: &str) -> ServerMessage {
    let pid = {
        let inner = lock(state);
        let Some(session) = inner.sessions.get(session_id) else {
            return error(Some(id), format!("no session {session_id}"));
        };
        if !session.info.alive {
            return error(Some(id), format!("session {session_id} is not alive"));
        }
        session.pid
    };
    let Some(pid) = pid else {
        return error(Some(id), "the child has no process id");
    };
    let signal = match parse_signal(name) {
        Ok(signal) => signal,
        Err(e) => return error(Some(id), e),
    };
    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), signal) {
        Ok(()) => ok(id, None),
        Err(e) => error(Some(id), format!("signal {name} to {pid}: {e}")),
    }
}

fn parse_signal(name: &str) -> Result<nix::sys::signal::Signal, String> {
    if let Ok(number) = name.parse::<i32>() {
        return nix::sys::signal::Signal::try_from(number)
            .map_err(|_| format!("unknown signal number {number}"));
    }
    let full = if name.starts_with("SIG") {
        name.to_string()
    } else {
        format!("SIG{name}")
    };
    full.parse()
        .map_err(|_| format!("unknown signal {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_accept_names_and_numbers() {
        assert_eq!(
            parse_signal("TERM").unwrap(),
            nix::sys::signal::Signal::SIGTERM
        );
        assert_eq!(
            parse_signal("SIGINT").unwrap(),
            nix::sys::signal::Signal::SIGINT
        );
        assert_eq!(
            parse_signal("15").unwrap(),
            nix::sys::signal::Signal::SIGTERM
        );
        assert!(parse_signal("NOT-A-SIGNAL").is_err());
        assert!(parse_signal("9999").is_err());
    }
}
