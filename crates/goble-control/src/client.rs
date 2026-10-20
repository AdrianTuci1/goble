//! The client side: connect to the app, send one command, read its reply.

use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use crate::codec::{write_message, MessageReader, ProtocolError};
use crate::protocol::{Command, ControlError, Payload, Request, Response};

/// A connection to the Goble app's control socket.
///
/// Reads and writes are synchronous, and one request is one reply: the
/// protocol carries no unsolicited events.
#[derive(Debug)]
pub struct Client {
    stream: UnixStream,
    reader: MessageReader<BufReader<UnixStream>>,
    next_id: u64,
}

impl Client {
    /// Connect to `socket`.
    pub fn connect(socket: impl AsRef<Path>) -> Result<Self, ControlError> {
        let socket = socket.as_ref();
        let stream = UnixStream::connect(socket).map_err(|e| {
            ControlError::unavailable(format!(
                "could not connect to the Goble control socket at {}: {e}; is the Goble app running?",
                socket.display()
            ))
        })?;
        let read_half = stream.try_clone().map_err(|e| {
            ControlError::io(format!(
                "could not read from the control socket at {}: {e}",
                socket.display()
            ))
        })?;
        Ok(Self {
            stream,
            reader: MessageReader::new(BufReader::new(read_half)),
            next_id: 1,
        })
    }

    /// Connect to [`resolve_socket_path`](crate::resolve_socket_path).
    pub fn connect_default() -> Result<Self, ControlError> {
        Self::connect(crate::resolve_socket_path())
    }

    /// Set the read timeout. A timeout leaves a partial line buffered, so
    /// retrying [`Self::call`] is safe.
    pub fn set_read_timeout(
        &self,
        timeout: Option<std::time::Duration>,
    ) -> Result<(), ControlError> {
        self.stream
            .set_read_timeout(timeout)
            .map_err(|e| ControlError::io(format!("set the control socket timeout: {e}")))
    }

    /// Allocate a request id. Ids are per connection and start at 1.
    pub fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Send a typed command and return its typed result.
    pub fn call(&mut self, command: Command) -> Result<Payload, ControlError> {
        let id = self.next_id();
        let response = self.request(Request::from_command(id, &command)?)?;
        if let Some(error) = response.error {
            return Err(error);
        }
        response
            .result
            .ok_or_else(|| ControlError::internal("the control server answered without a result"))
    }

    /// Send a raw request and return the whole response, including a structured
    /// error. This is the surface tests and diagnostic tools use to prove an
    /// unknown verb is refused rather than dropped.
    pub fn request(&mut self, request: Request) -> Result<Response, ControlError> {
        write_message(&mut self.stream, &request)
            .and_then(|()| self.stream.flush().map_err(ProtocolError::Io))
            .map_err(|e| ControlError::io(format!("send control request: {e}")))?;

        let response = self
            .reader
            .read::<Response>()
            .map_err(|e| ControlError::io(format!("read control response: {e}")))?
            .ok_or_else(|| ControlError::io("the control socket closed before replying"))?;
        if response.id != request.id {
            return Err(ControlError::internal(format!(
                "control reply id {} does not match request id {}",
                response.id, request.id
            )));
        }
        Ok(response)
    }
}

/// The socket the app serves: `$GOBLE_CONTROL_SOCKET`, else
/// `$HOME/.goble/control.sock`.
pub fn resolve_socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("GOBLE_CONTROL_SOCKET").filter(|p| !p.is_empty()) {
        return PathBuf::from(path);
    }
    let home = std::env::var_os("HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".goble").join("control.sock")
}
