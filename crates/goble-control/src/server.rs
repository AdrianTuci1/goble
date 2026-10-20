//! One Unix socket, one thread per connection, one reply per request.
//!
//! The server is generic over [`Handler`] so the app can forward requests to
//! its winit thread while a test (or F3b's tooling) plugs in a handler of its
//! own. Handlers are shared across connections with an `Arc`; a handler that
//! needs mutable state owns a lock.

use std::io::{BufReader, BufWriter};
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::codec::{write_message, MessageReader, ProtocolError};
use crate::protocol::{Command, ControlError, ErrorCode, Request, Response};

/// How often the accept loop checks the shutdown flag. The listener is
/// nonblocking so dropping a server never waits for a connection.
const ACCEPT_POLL: Duration = Duration::from_millis(10);

/// The control surface's handler. It receives a typed [`Command`] and answers
/// with a typed payload or a structured error. It must never panic: a panic in
/// one connection's thread is caught by the client as a closed socket.
pub trait Handler: Send + Sync + 'static {
    fn handle(&self, command: Command) -> Result<crate::protocol::Payload, ControlError>;
}

/// A running control server bound to `socket_path`.
pub struct Server<H: Handler> {
    handler: Arc<H>,
    socket_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl<H: Handler> std::fmt::Debug for Server<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("socket", &self.socket_path)
            .finish_non_exhaustive()
    }
}

impl<H: Handler> Server<H> {
    /// Bind `socket` and start accepting clients.
    ///
    /// An existing file is reused only when it is a Unix socket nobody is
    /// listening on; a live socket or a non-socket is refused rather than
    /// deleted.
    pub fn start(socket: impl AsRef<Path>, handler: H) -> Result<Self, ControlError> {
        let socket_path = socket.as_ref().to_path_buf();
        prepare_socket_dir(&socket_path)?;

        if socket_path.exists() {
            if UnixStream::connect(&socket_path).is_ok() {
                return Err(ControlError::new(
                    ErrorCode::Unavailable,
                    format!(
                        "another Goble control server is already listening on {}",
                        socket_path.display()
                    ),
                ));
            }
            let meta = std::fs::symlink_metadata(&socket_path).map_err(|e| {
                ControlError::io(format!("inspect {}: {e}", socket_path.display()))
            })?;
            if !meta.file_type().is_socket() {
                return Err(ControlError::io(format!(
                    "refusing to remove {}: not a Unix socket",
                    socket_path.display()
                )));
            }
            std::fs::remove_file(&socket_path).map_err(|e| {
                ControlError::io(format!("remove stale {}: {e}", socket_path.display()))
            })?;
        }

        let listener = UnixListener::bind(&socket_path).map_err(|e| {
            ControlError::io(format!("bind {}: {e}", socket_path.display()))
        })?;
        let _ = std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600));
        listener
            .set_nonblocking(true)
            .map_err(|e| ControlError::io(format!("make the listener nonblocking: {e}")))?;

        let handler = Arc::new(handler);
        let shutdown = Arc::new(AtomicBool::new(false));
        let accept_handler = Arc::clone(&handler);
        let accept_shutdown = Arc::clone(&shutdown);
        let accept = thread::Builder::new()
            .name("goble-control".to_string())
            .spawn(move || accept_loop(listener, accept_handler, accept_shutdown))
            .map_err(|e| ControlError::io(format!("start the accept thread: {e}")))?;

        Ok(Self {
            handler,
            socket_path,
            shutdown,
            accept: Some(accept),
        })
    }

    /// The socket this server bound.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Stop accepting, join the accept thread, and remove the socket.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        remove_if_socket(&self.socket_path);
    }

    /// The handler, for tests that observe what the server dispatched.
    pub fn handler(&self) -> &H {
        &self.handler
    }
}

impl<H: Handler> Drop for Server<H> {
    fn drop(&mut self) {
        self.stop();
    }
}

fn prepare_socket_dir(socket: &Path) -> Result<(), ControlError> {
    let Some(parent) = socket.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    let existed = parent.exists();
    std::fs::create_dir_all(parent)
        .map_err(|e| ControlError::io(format!("create {}: {e}", parent.display())))?;
    if !existed {
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn remove_if_socket(path: &Path) {
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_socket() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn accept_loop<H: Handler>(
    listener: UnixListener,
    handler: Arc<H>,
    shutdown: Arc<AtomicBool>,
) {
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let handler = Arc::clone(&handler);
                // Detached: a handler may be waiting on the app's UI thread,
                // and shutdown must not wait for that. The request's reply
                // channel closes when the app drops, so the thread exits then.
                let _ = thread::Builder::new()
                    .name("goble-control-client".to_string())
                    .spawn(move || handle_client(stream, handler.as_ref()));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(ACCEPT_POLL),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

fn handle_client<H: Handler>(stream: UnixStream, handler: &H) {
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = MessageReader::new(BufReader::new(read_half));
    let mut writer = BufWriter::new(stream);

    loop {
        let request = match reader.read::<Request>() {
            Ok(Some(request)) => request,
            Ok(None) => break,
            Err(ProtocolError::Json(e)) => {
                let response = Response::error(
                    0,
                    ControlError::invalid_request(format!("invalid JSON request: {e}")),
                );
                if write_response(&mut writer, &response).is_err() {
                    break;
                }
                continue;
            }
            Err(ProtocolError::TooLarge) => {
                let response = Response::error(
                    0,
                    ControlError::invalid_request("request exceeds the message size limit"),
                );
                let _ = write_response(&mut writer, &response);
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
            Err(_) => break,
        };

        let id = request.id;
        let response = match request.command() {
            Ok(command) => match handler.handle(command) {
                Ok(payload) => Response::ok(id, payload),
                Err(error) => Response::error(id, error),
            },
            Err(error) => Response::error(id, error),
        };
        if write_response(&mut writer, &response).is_err() {
            break;
        }
    }
}

fn write_response(
    writer: &mut BufWriter<UnixStream>,
    response: &Response,
) -> Result<(), ProtocolError> {
    write_message(writer, response)?;
    use std::io::Write;
    writer.flush()?;
    Ok(())
}
