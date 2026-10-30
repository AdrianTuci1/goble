//! An attachable PTY daemon.
//!
//! The daemon owns real PTYs (`portable-pty`) and outlives any one client, so
//! terminal sessions survive a GUI restart or a dropped connection. Clients
//! speak newline-delimited JSON over a Unix domain socket; output bytes are
//! carried base64-encoded and are never interpreted here, which keeps the
//! app's OSC 133 / DCS shell-integration parsing in the app.
//!
//! Layers:
//! * [`protocol`] — request/event wire types and bounded line framing.
//! * [`session`] — the bounded, byte-exact replay ring.
//! * [`server`] — the socket listener, sessions and broadcast.
//! * [`client`] — connect, auto-start, request/event reading.
//!
//! The daemon is Unix-only: a Unix socket and a pty are the transport.

#![cfg(unix)]
#![warn(missing_debug_implementations)]

pub mod client;
pub mod protocol;
pub mod server;
pub mod session;

use std::path::{Path, PathBuf};
use std::time::Duration;

pub use client::Client;
pub use protocol::{
    decode_bytes, encode_bytes, Request, ServerMessage, SessionInfo, MAX_MESSAGE_BYTES,
};
pub use server::Server;
pub use session::{ReplayRing, REPLAY_CAPACITY};

/// The socket the daemon binds: `$GOBLE_PTY_SOCKET`, else
/// `$XDG_RUNTIME_DIR/goble/pty.sock`, else `$HOME/.goble/pty.sock`.
pub fn resolve_socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("GOBLE_PTY_SOCKET").filter(|p| !p.is_empty()) {
        return PathBuf::from(path);
    }
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|p| !p.is_empty()) {
        return PathBuf::from(dir).join("goble").join("pty.sock");
    }
    let home = std::env::var_os("HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".goble").join("pty.sock")
}

/// The pidfile beside a socket: `pty.sock` → `pty.pid`.
pub fn pid_path_for(socket: &Path) -> PathBuf {
    socket.with_extension("pid")
}

/// The daemon's log beside a socket: `pty.sock` → `pty.log`.
pub fn log_path_for(socket: &Path) -> PathBuf {
    socket.with_extension("log")
}

/// Whether `pid` names a live process. `EPERM` counts as alive: the process is
/// there, we simply may not signal it.
pub fn pid_alive(pid: u32) -> bool {
    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None) {
        Ok(()) => true,
        Err(nix::errno::Errno::ESRCH) => false,
        Err(_) => true,
    }
}

/// Read a pidfile, if it holds a pid.
pub fn read_pidfile(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.trim().parse().ok())
}

/// Whether the pidfile beside `socket` names a live process.
pub fn daemon_alive(socket: &Path) -> bool {
    read_pidfile(&pid_path_for(socket)).is_some_and(pid_alive)
}

/// Remove a socket and pidfile only when they are stale.
///
/// Returns `Ok(true)` when something was removed, `Ok(false)` when the pid in
/// the pidfile is still alive, and an error when `socket` exists but is not a
/// socket — never delete a file we did not create.
pub fn cleanup_stale(socket: &Path, pidfile: &Path) -> std::io::Result<bool> {
    if let Some(pid) = read_pidfile(pidfile) {
        if pid_alive(pid) {
            return Ok(false);
        }
    }
    remove_stale_record(socket, pidfile)
}

/// Remove a socket file and pidfile without consulting the pid.
///
/// This is the recovery path for a record whose pid names a live process that
/// is not the daemon: after a crash the pid can be reused by an unrelated
/// process, and `cleanup_stale` would then refuse to clean a record nobody can
/// answer on. The caller must already know the daemon is unreachable; with no
/// socket file there is nothing that could answer, so the record is stale
/// whatever the pid says. Like [`cleanup_stale`], it refuses to delete a
/// non-socket file at `socket`.
pub fn remove_stale_record(socket: &Path, pidfile: &Path) -> std::io::Result<bool> {
    if socket.exists() {
        use std::os::unix::fs::FileTypeExt;
        let meta = std::fs::symlink_metadata(socket)?;
        if !meta.file_type().is_socket() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "refusing to remove {}: not a Unix socket",
                    socket.display()
                ),
            ));
        }
        std::fs::remove_file(socket)?;
    }

    match std::fs::remove_file(pidfile) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(e),
    }
}

/// Wait (bounded, with backoff) until something accepts connections at
/// `socket`. Each probe is closed immediately; the daemon treats it as a
/// client that left without a hello.
pub fn wait_for_socket(socket: &Path, timeout: Duration) -> std::io::Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    let mut delay = Duration::from_millis(10);
    let mut last;
    loop {
        match std::os::unix::net::UnixStream::connect(socket) {
            Ok(_) => return Ok(()),
            Err(e) => last = e,
        }
        if std::time::Instant::now() >= deadline {
            return Err(last);
        }
        std::thread::sleep(delay);
        delay = (delay * 2).min(Duration::from_millis(250));
    }
}
