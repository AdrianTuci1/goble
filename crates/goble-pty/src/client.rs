//! The client side: connect, auto-start, send requests and read events.

use std::collections::VecDeque;
use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::protocol::{write_message, MessageReader, ProtocolError, Request, ServerMessage};
use crate::{
    cleanup_stale, log_path_for, pid_alive, pid_path_for, read_pidfile, resolve_socket_path,
    wait_for_socket,
};

/// A connection to the PTY daemon.
///
/// Reads and writes are synchronous; events may arrive between a request and
/// its response, so [`Self::request`] buffers them and [`Self::next_message`]
/// returns them first.
pub struct Client {
    stream: UnixStream,
    reader: MessageReader<BufReader<UnixStream>>,
    buffered: VecDeque<ServerMessage>,
    next_id: u64,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").finish_non_exhaustive()
    }
}

impl Client {
    /// Connect to `socket`.
    pub fn connect(socket: impl AsRef<Path>) -> std::io::Result<Self> {
        let stream = UnixStream::connect(socket)?;
        let read_half = stream.try_clone()?;
        Ok(Self {
            stream,
            reader: MessageReader::new(BufReader::new(read_half)),
            buffered: VecDeque::new(),
            next_id: 1,
        })
    }

    /// Connect to [`resolve_socket_path`].
    pub fn connect_default() -> std::io::Result<Self> {
        Self::connect(resolve_socket_path())
    }

    /// Connect, starting the daemon if it is not running.
    pub fn connect_or_start() -> Result<Self> {
        let socket = resolve_socket_path();
        let exe = resolve_daemon_exe()?;
        Self::connect_or_start_at(&socket, &exe)
    }

    /// Connect to `socket`, starting `exe --daemon` when nothing is listening.
    ///
    /// A stale socket and pidfile are removed only when the pidfile names a
    /// dead process; a live pid is waited for (bounded) rather than duplicated.
    pub fn connect_or_start_at(socket: &Path, exe: &Path) -> Result<Self> {
        match Self::connect(socket) {
            Ok(client) => return Ok(client),
            Err(first) => {
                let pidfile = pid_path_for(socket);
                if let Some(pid) = read_pidfile(&pidfile) {
                    if pid_alive(pid) {
                        wait_for_socket(socket, Duration::from_secs(5)).with_context(|| {
                            format!(
                                "a daemon (pid {pid}) owns {} but did not answer: {first}",
                                socket.display()
                            )
                        })?;
                        return Self::connect(socket).map_err(anyhow::Error::from);
                    }
                }
                cleanup_stale(socket, &pidfile)
                    .with_context(|| format!("clean stale socket {}", socket.display()))?;
            }
        }

        spawn_daemon_at(exe, socket)?;
        wait_for_socket(socket, Duration::from_secs(10))
            .with_context(|| format!("wait for the daemon at {}", socket.display()))?;
        Self::connect(socket).map_err(anyhow::Error::from)
    }

    /// Allocate a request id. Ids are per connection and start at 1.
    pub fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Set the read timeout used by [`Self::next_message`]. A timeout leaves
    /// any partial message buffered, so it is safe to retry.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }

    /// Send one request.
    pub fn send(&mut self, request: &Request) -> Result<(), ProtocolError> {
        write_message(&mut self.stream, request)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Read the next message: a buffered event first, then the socket.
    pub fn next_message(&mut self) -> Result<ServerMessage, ProtocolError> {
        if let Some(message) = self.buffered.pop_front() {
            return Ok(message);
        }
        self.read_socket()
    }

    fn read_socket(&mut self) -> Result<ServerMessage, ProtocolError> {
        self.reader.read()?.ok_or(ProtocolError::Closed)
    }

    /// Send `request` and read until its response, buffering interleaved
    /// events for [`Self::next_message`].
    pub fn request(&mut self, request: Request) -> Result<ServerMessage, ProtocolError> {
        let id = request.id();
        self.send(&request)?;
        loop {
            // Read past the buffer: it holds older events, never this
            // response.
            let message = self.read_socket()?;
            let answered = match &message {
                ServerMessage::Ok { id: got, .. } | ServerMessage::Sessions { id: got, .. } => {
                    *got == id
                }
                ServerMessage::Error {
                    id: Some(got), ..
                } => *got == id,
                _ => false,
            };
            if answered {
                return Ok(message);
            }
            self.buffered.push_back(message);
        }
    }
}

/// Start `exe --daemon` and wait for the short-lived launcher to exit.
pub fn spawn_daemon_at(exe: &Path, socket: &Path) -> Result<()> {
    let log_path = log_path_for(socket);
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))?;
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("open {}", log_path.display()))?;

    let mut child = Command::new(exe)
        .arg("--daemon")
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .spawn()
        .with_context(|| format!("spawn {}", exe.display()))?;
    let _ = child.wait();
    Ok(())
}

fn daemon_binary_name() -> String {
    format!("goble-pty-daemon{}", std::env::consts::EXE_SUFFIX)
}

/// The places a daemon binary is looked for, in order, for an app at
/// `current_exe`: beside the app, one directory up (the `target/<profile>/deps`
/// layout used by test binaries), then bare `goble-pty-daemon` (PATH).
fn daemon_exe_candidates(current_exe: &Path) -> Vec<PathBuf> {
    let name = daemon_binary_name();
    let mut candidates = Vec::new();
    if let Some(dir) = current_exe.parent() {
        candidates.push(dir.join(&name));
        if let Some(profile_dir) = dir.parent() {
            candidates.push(profile_dir.join(&name));
        }
    }
    candidates.push(PathBuf::from(name));
    candidates
}

/// Resolve `GOBLE_PTY_DAEMON`, the app's sibling, or PATH. The env override
/// wins because it is the explicit escape hatch; everything else prefers the
/// copy that was installed with the app.
fn resolve_daemon_exe_from(current_exe: &Path, override_path: Option<PathBuf>) -> PathBuf {
    if let Some(path) = override_path {
        return path;
    }
    let name = daemon_binary_name();
    if current_exe
        .file_name()
        .is_some_and(|file| file.to_string_lossy() == name)
    {
        return current_exe.to_path_buf();
    }
    for candidate in daemon_exe_candidates(current_exe) {
        if candidate.is_file() {
            return candidate;
        }
    }
    // Nothing was installed beside the app; let the OS search PATH on spawn.
    PathBuf::from(name)
}

fn resolve_daemon_exe() -> Result<PathBuf> {
    let override_path = std::env::var_os("GOBLE_PTY_DAEMON")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from);
    let current = std::env::current_exe().context("resolve the current executable")?;
    Ok(resolve_daemon_exe_from(&current, override_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn the_current_process_is_alive() {
        assert!(pid_alive(std::process::id()));
    }

    #[test]
    fn cleanup_refuses_a_non_socket_file() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("pty.sock");
        let pidfile = pid_path_for(&socket);
        std::fs::write(&socket, b"not a socket").unwrap();
        std::fs::write(&pidfile, b"999999\n").unwrap();
        assert!(cleanup_stale(&socket, &pidfile).is_err());
        assert!(socket.exists(), "the file must not be deleted");
    }

    #[test]
    fn a_stale_socket_and_pidfile_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("pty.sock");
        let pidfile = pid_path_for(&socket);
        {
            let _listener = UnixListener::bind(&socket).unwrap();
        }
        std::fs::write(&pidfile, b"999999\n").unwrap();
        assert!(socket.exists() && pidfile.exists());
        assert!(cleanup_stale(&socket, &pidfile).unwrap());
        assert!(!socket.exists());
        assert!(!pidfile.exists());
    }

    #[test]
    fn a_live_pid_is_not_cleaned() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("pty.sock");
        let pidfile = pid_path_for(&socket);
        let _listener = UnixListener::bind(&socket).unwrap();
        std::fs::write(&pidfile, format!("{}\n", std::process::id())).unwrap();
        assert!(!cleanup_stale(&socket, &pidfile).unwrap());
        assert!(socket.exists() && pidfile.exists());
    }

    #[test]
    fn daemon_lookup_prefers_the_app_sibling_over_path() {
        let dir = tempfile::tempdir().unwrap();
        let macos_dir = dir.path().join("Contents").join("MacOS");
        std::fs::create_dir_all(&macos_dir).unwrap();
        let app = macos_dir.join(format!("goble-app{}", std::env::consts::EXE_SUFFIX));
        let daemon = macos_dir.join(daemon_binary_name());
        std::fs::write(&daemon, b"").unwrap();

        let candidates = daemon_exe_candidates(&app);
        assert_eq!(candidates.first(), Some(&daemon), "the sibling comes first");
        assert_eq!(
            candidates.get(1),
            Some(&macos_dir.parent().unwrap().join(daemon_binary_name())),
            "then the profile directory (target/<profile>/deps)"
        );
        assert_eq!(
            candidates.last(),
            Some(&PathBuf::from(daemon_binary_name())),
            "PATH is the final fallback"
        );

        assert_eq!(resolve_daemon_exe_from(&app, None), daemon);
    }

    #[test]
    fn the_env_override_wins_over_an_installed_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir
            .path()
            .join(format!("goble-app{}", std::env::consts::EXE_SUFFIX));
        let daemon = dir.path().join(daemon_binary_name());
        std::fs::write(&daemon, b"").unwrap();
        let override_path = dir.path().join("custom-daemon");

        assert_eq!(
            resolve_daemon_exe_from(&app, Some(override_path.clone())),
            override_path
        );
    }
}
