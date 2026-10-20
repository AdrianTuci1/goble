//! The daemon's wire protocol.
//!
//! One JSON object per line, in both directions. Requests are internally
//! tagged with `type` and carry an `id`; every response carries the same `id`
//! so a client can correlate it while ignoring events that interleave.
//!
//! PTY bytes are base64 (`bytes_b64`), never text: output may contain invalid
//! UTF-8 and must reach the app's parser unchanged. `seq` is the byte offset of
//! the first byte in an `output` event, so `attach { from_seq }` asks for
//! exactly the bytes from that offset onward.

use std::io::{BufRead, Write};

use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The largest accepted message, checked before parsing and before writing.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

const BASE64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

/// Requests a client sends. Each variant carries its request `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Identify this connection. `client_id` names its subscriptions.
    Hello { id: u64, client_id: String },
    /// List every session, alive or exited.
    List { id: u64 },
    /// Open a PTY and start `shell` in `cwd`.
    ///
    /// `args` and `env` are optional so an existing client keeps working; they
    /// are how the app hands a shell its integration script (`--rcfile`, or
    /// `$ZDOTDIR`) without the daemon learning what shell integration is.
    Create {
        id: u64,
        pane_id: String,
        cwd: String,
        cols: u16,
        rows: u16,
        #[serde(default)]
        shell: Option<String>,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: Vec<(String, String)>,
    },
    /// Subscribe from byte offset `from_seq`; replays what the ring still has.
    Attach {
        id: u64,
        session_id: String,
        #[serde(default)]
        from_seq: u64,
    },
    /// Unsubscribe. Never kills the session.
    Detach { id: u64, session_id: String },
    /// Write raw bytes to the PTY.
    Input {
        id: u64,
        session_id: String,
        bytes_b64: String,
    },
    /// Resize the PTY and the kernel window size. The pixel fields are
    /// optional so an older client's `resize` still parses; reporting them
    /// keeps a program's pixel-size queries truthful.
    Resize {
        id: u64,
        session_id: String,
        cols: u16,
        rows: u16,
        #[serde(default)]
        pixel_width: u16,
        #[serde(default)]
        pixel_height: u16,
    },
    /// Kill the child and drop the session.
    Kill { id: u64, session_id: String },
    /// Send a signal (`SIGTERM`, `TERM` or a number) to the child.
    Signal {
        id: u64,
        session_id: String,
        signal: String,
    },
}

impl Request {
    /// The request id this message must be answered with.
    pub fn id(&self) -> u64 {
        match self {
            Self::Hello { id, .. }
            | Self::List { id }
            | Self::Create { id, .. }
            | Self::Attach { id, .. }
            | Self::Detach { id, .. }
            | Self::Input { id, .. }
            | Self::Resize { id, .. }
            | Self::Kill { id, .. }
            | Self::Signal { id, .. } => *id,
        }
    }
}

/// One session's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub pane_id: String,
    pub alive: bool,
    pub cols: u16,
    pub rows: u16,
}

/// Responses and events the daemon sends.
///
/// `Ok`, `Sessions` and an error with an `id` are responses. `Session`,
/// `Output` and `Exit` are events, and `Error` without an id is an out-of-band
/// event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// A successful request. `session` carries the result of `create`/`attach`.
    Ok {
        id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<SessionInfo>,
    },
    /// The result of `list`.
    Sessions {
        id: u64,
        sessions: Vec<SessionInfo>,
    },
    /// A request failed, or (with no `id`) a connection-level error occurred.
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<u64>,
        message: String,
    },
    /// A session's shape or liveness changed.
    Session {
        session_id: String,
        pane_id: String,
        alive: bool,
        cols: u16,
        rows: u16,
    },
    /// A byte range of output; `seq` is its first byte's offset.
    Output {
        session_id: String,
        seq: u64,
        bytes_b64: String,
    },
    /// The child exited.
    Exit { session_id: String, code: i32 },
}

impl ServerMessage {
    /// Build the session event for `info`.
    pub fn session_event(info: &SessionInfo) -> Self {
        Self::Session {
            session_id: info.session_id.clone(),
            pane_id: info.pane_id.clone(),
            alive: info.alive,
            cols: info.cols,
            rows: info.rows,
        }
    }

    /// Build an output event for `bytes` starting at byte offset `seq`.
    pub fn output(session_id: impl Into<String>, seq: u64, bytes: &[u8]) -> Self {
        Self::Output {
            session_id: session_id.into(),
            seq,
            bytes_b64: encode_bytes(bytes),
        }
    }

    /// Decode this event's PTY bytes, when it is an `output`.
    pub fn output_bytes(&self) -> Option<Result<Vec<u8>, ProtocolError>> {
        match self {
            Self::Output { bytes_b64, .. } => Some(decode_bytes(bytes_b64)),
            _ => None,
        }
    }
}

/// Protocol-level failures.
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid message: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("message exceeds {MAX_MESSAGE_BYTES} bytes")]
    TooLarge,
    #[error("the daemon closed the connection")]
    Closed,
}

/// Base64-encode PTY bytes for the wire.
pub fn encode_bytes(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

/// Decode a wire `bytes_b64` field.
pub fn decode_bytes(encoded: &str) -> Result<Vec<u8>, ProtocolError> {
    Ok(BASE64.decode(encoded)?)
}

/// Incremental, size-bounded newline framing.
///
/// A partial line is kept across calls, so a read timeout does not lose the
/// bytes that had already arrived.
#[derive(Debug)]
pub struct MessageReader<R> {
    inner: R,
    line: Vec<u8>,
}

impl<R: BufRead> MessageReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            line: Vec::new(),
        }
    }

    /// Read one message; `Ok(None)` is EOF before any bytes of a new line.
    pub fn read<T: DeserializeOwned>(&mut self) -> Result<Option<T>, ProtocolError> {
        loop {
            if let Some(pos) = self.line.iter().position(|&b| b == b'\n') {
                let rest = self.line.split_off(pos + 1);
                let mut line = std::mem::replace(&mut self.line, rest);
                line.truncate(pos);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                return Ok(Some(serde_json::from_slice(&line)?));
            }

            let available = self.inner.fill_buf()?;
            if available.is_empty() {
                if self.line.is_empty() {
                    return Ok(None);
                }
                let line = std::mem::take(&mut self.line);
                if line.iter().all(u8::is_ascii_whitespace) {
                    return Ok(None);
                }
                return Ok(Some(serde_json::from_slice(&line)?));
            }

            let take = available
                .iter()
                .position(|&b| b == b'\n')
                .map_or(available.len(), |p| p + 1);
            if self.line.len() + take > MAX_MESSAGE_BYTES {
                return Err(ProtocolError::TooLarge);
            }
            self.line.extend_from_slice(&available[..take]);
            self.inner.consume(take);
        }
    }
}

/// Read one message from a `BufRead`, with the same bound as [`MessageReader`].
pub fn read_message<R: BufRead, T: DeserializeOwned>(
    reader: &mut R,
) -> Result<Option<T>, ProtocolError> {
    MessageReader::new(reader).read()
}

/// Write one message plus its newline.
pub fn write_message<W: Write, T: Serialize>(
    writer: &mut W,
    message: &T,
) -> Result<(), ProtocolError> {
    let mut bytes = serde_json::to_vec(message)?;
    if bytes.len() + 1 > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn requests() -> Vec<Request> {
        vec![
            Request::Hello {
                id: 1,
                client_id: "gui".into(),
            },
            Request::List { id: 2 },
            Request::Create {
                id: 3,
                pane_id: "pane-1".into(),
                cwd: "/tmp".into(),
                cols: 120,
                rows: 40,
                shell: Some("/bin/bash".into()),
                args: vec!["--rcfile".into(), "/tmp/goble.bashrc".into()],
                env: vec![("GOBLE_SKIP_USER_RC".into(), "1".into())],
            },
            Request::Attach {
                id: 4,
                session_id: "s1".into(),
                from_seq: 17,
            },
            Request::Detach {
                id: 5,
                session_id: "s1".into(),
            },
            Request::Input {
                id: 6,
                session_id: "s1".into(),
                bytes_b64: encode_bytes(b"\x1b[200~hi\x1b[201~"),
            },
            Request::Resize {
                id: 7,
                session_id: "s1".into(),
                cols: 80,
                rows: 24,
                pixel_width: 640,
                pixel_height: 480,
            },
            Request::Kill {
                id: 8,
                session_id: "s1".into(),
            },
            Request::Signal {
                id: 9,
                session_id: "s1".into(),
                signal: "SIGTERM".into(),
            },
        ]
    }

    #[test]
    fn every_request_round_trips_through_one_line() {
        for request in requests() {
            let mut wire = Vec::new();
            write_message(&mut wire, &request).unwrap();
            assert_eq!(wire.last(), Some(&b'\n'));
            assert_eq!(
                wire.iter().filter(|&&b| b == b'\n').count(),
                1,
                "one message is one line"
            );
            let mut cursor = Cursor::new(wire);
            let decoded: Option<Request> = read_message(&mut cursor).unwrap();
            assert_eq!(decoded, Some(request));
        }
    }

    #[test]
    fn wire_shape_is_internally_tagged_and_carries_the_request_id() {
        let value = serde_json::to_value(Request::Resize {
            id: 42,
            session_id: "s1".into(),
            cols: 80,
            rows: 24,
            pixel_width: 800,
            pixel_height: 480,
        })
        .unwrap();
        assert_eq!(value["type"], "resize");
        assert_eq!(value["id"], 42);
        assert_eq!(value["session_id"], "s1");
        assert_eq!(value["pixel_width"], 800);

        let create = serde_json::to_value(Request::Create {
            id: 1,
            pane_id: "p".into(),
            cwd: "/".into(),
            cols: 80,
            rows: 24,
            shell: None,
            args: Vec::new(),
            env: Vec::new(),
        })
        .unwrap();
        assert_eq!(create["type"], "create");
        assert!(create["shell"].is_null());
        assert_eq!(create["args"], serde_json::json!([]));
        assert_eq!(create["env"], serde_json::json!([]));
    }

    /// An older client's `create` carries no args or env; the missing fields
    /// default rather than failing the request.
    #[test]
    fn a_create_without_args_or_env_still_parses() {
        let request: Request = serde_json::from_value(serde_json::json!({
            "type": "create",
            "id": 4,
            "pane_id": "p",
            "cwd": "/tmp",
            "cols": 80,
            "rows": 24,
            "shell": "/bin/sh"
        }))
        .unwrap();
        match request {
            Request::Create {
                shell, args, env, ..
            } => {
                assert_eq!(shell.as_deref(), Some("/bin/sh"));
                assert!(args.is_empty());
                assert!(env.is_empty());
            }
            other => panic!("expected create, got {other:?}"),
        }
    }

    #[test]
    fn server_messages_round_trip_and_output_bytes_are_byte_exact() {
        let info = SessionInfo {
            session_id: "s1".into(),
            pane_id: "p1".into(),
            alive: true,
            cols: 80,
            rows: 24,
        };
        let raw = b"\x00\xff\xfeOSC133;A\x07\x1b]133;A\x07";
        let messages = vec![
            ServerMessage::Ok {
                id: 1,
                session: Some(info.clone()),
            },
            ServerMessage::Sessions {
                id: 2,
                sessions: vec![info.clone()],
            },
            ServerMessage::Error {
                id: Some(3),
                message: "no session".into(),
            },
            ServerMessage::session_event(&info),
            ServerMessage::output("s1", 4096, raw),
            ServerMessage::Exit {
                session_id: "s1".into(),
                code: 7,
            },
        ];
        for message in messages {
            let mut wire = Vec::new();
            write_message(&mut wire, &message).unwrap();
            let mut cursor = Cursor::new(wire);
            let decoded: Option<ServerMessage> = read_message(&mut cursor).unwrap();
            assert_eq!(decoded, Some(message.clone()));
            if let Some(bytes) = message.output_bytes() {
                assert_eq!(bytes.unwrap(), raw);
            }
        }
    }

    #[test]
    fn an_event_error_has_no_response_id() {
        let value = serde_json::to_value(ServerMessage::Error {
            id: None,
            message: "boom".into(),
        })
        .unwrap();
        assert_eq!(value["type"], "error");
        assert!(value.get("id").is_none());
        assert_eq!(value["message"], "boom");
    }

    #[test]
    fn partial_lines_survive_a_read_timeout_and_oversized_messages_are_refused() {
        use std::collections::VecDeque;
        use std::io::{BufReader, Read};

        struct Chunks(VecDeque<Option<Vec<u8>>>);
        impl Read for Chunks {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                let next = self.0.pop_front().expect("the test reads a fixed script");
                match next {
                    Some(bytes) => {
                        out[..bytes.len()].copy_from_slice(&bytes);
                        Ok(bytes.len())
                    }
                    None => Err(std::io::ErrorKind::WouldBlock.into()),
                }
            }
        }

        let request = requests().remove(3);
        let mut wire = Vec::new();
        write_message(&mut wire, &request).unwrap();
        let split = wire.len() / 2;
        let chunks = Chunks(VecDeque::from([
            Some(wire[..split].to_vec()),
            None,
            Some(wire[split..].to_vec()),
        ]));
        let mut reader = MessageReader::new(BufReader::new(chunks));
        assert!(matches!(
            reader.read::<Request>(),
            Err(ProtocolError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock
        ));
        assert_eq!(reader.read::<Request>().unwrap(), Some(request));

        let too_long = format!(
            "{{\"type\":\"list\",\"id\":1,\"pad\":\"{}\"}}",
            "x".repeat(MAX_MESSAGE_BYTES)
        );
        let mut cursor = Cursor::new(too_long.into_bytes());
        assert!(matches!(
            read_message::<_, Request>(&mut cursor),
            Err(ProtocolError::TooLarge)
        ));
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        let mut cursor = Cursor::new(b"{\"type\":\"nope\"}\n".to_vec());
        assert!(matches!(
            read_message::<_, Request>(&mut cursor),
            Err(ProtocolError::Json(_))
        ));
    }

    #[test]
    fn blank_lines_are_skipped() {
        let mut cursor = Cursor::new(b"\n\r\n{\"type\":\"list\",\"id\":9}\n".to_vec());
        let request: Option<Request> = read_message(&mut cursor).unwrap();
        assert_eq!(request, Some(Request::List { id: 9 }));
    }
}
