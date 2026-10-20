//! Newline framing for the control socket.
//!
//! Messages are bounded before they are read or written, so a peer cannot make
//! the server allocate an unbounded line and a handler cannot emit a message
//! the client will refuse.

use std::io::{BufRead, Write};

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;

use crate::protocol::MAX_MESSAGE_BYTES;

/// Framing failures.
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid JSON message: {0}")]
    Json(#[from] serde_json::Error),
    #[error("message exceeds {MAX_MESSAGE_BYTES} bytes")]
    TooLarge,
    #[error("the control socket closed the connection")]
    Closed,
}

/// Incremental, size-bounded newline framing.
///
/// A partial line survives across calls, so a read timeout never loses bytes
/// that had already arrived.
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
    use crate::protocol::{Command, Request, Response, Payload};
    use std::io::Cursor;

    #[test]
    fn a_message_round_trips_through_one_line() {
        let request = Request::from_command(
            2,
            &Command::PanesSend {
                pane_id: 1,
                text: "hello".into(),
                enter: false,
            },
        )
        .unwrap();
        let mut wire = Vec::new();
        write_message(&mut wire, &request).unwrap();
        assert_eq!(wire.last(), Some(&b'\n'));

        // The reader keeps the trailing newline out of the decoded message and
        // reports EOF once the line is spent.
        let mut reader = MessageReader::new(Cursor::new(wire.clone()));
        let first = reader.read::<Request>().unwrap().unwrap();
        assert_eq!(first, request);
        assert!(reader.read::<Request>().unwrap().is_none());
    }

    #[test]
    fn two_messages_are_two_lines() {
        let mut wire = Vec::new();
        write_message(&mut wire, &Response::ok(1, Payload::Ok)).unwrap();
        write_message(&mut wire, &Response::error(2, crate::protocol::ControlError::missing_pane(3)))
            .unwrap();
        assert_eq!(wire.iter().filter(|&&b| b == b'\n').count(), 2);
        let mut reader = MessageReader::new(Cursor::new(wire));
        assert_eq!(reader.read::<Response>().unwrap().unwrap().id, 1);
        assert_eq!(reader.read::<Response>().unwrap().unwrap().id, 2);
    }
}
