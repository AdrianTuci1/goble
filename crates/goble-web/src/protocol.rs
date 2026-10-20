//! JSON frames exchanged with the gateway's worker relay.
//!
//! The gateway copies frames verbatim between the browser and the worker, and
//! fans every worker frame out to every attached client. A client therefore
//! has to ignore frames for panes it does not own; [`ServerFrame`] keeps the
//! `pane_id` so callers can filter before acting.
//!
//! The wire format is a serde tag named `kind` with snake_case variants. Do
//! not reorder fields: the gateway and worker serialize with the same layout
//! and the tests pin the exact JSON.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// Frames the browser sends to the worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClientFrame {
    OpenPty {
        pane_id: String,
        cwd: String,
        cols: u16,
        rows: u16,
    },
    PtyInput {
        pane_id: String,
        data: String,
    },
    PtyResize {
        pane_id: String,
        cols: u16,
        rows: u16,
    },
    ClosePty {
        pane_id: String,
    },
    Ping,
}

/// Frames the worker sends to the browser.
///
/// `Unknown` absorbs worker message kinds the browser does not implement
/// (`status_report`, `agent_log`, …) so an unexpected frame never tears down
/// a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServerFrame {
    PtyOpened {
        pane_id: String,
    },
    PtyOutput {
        pane_id: String,
        data: String,
    },
    PtyExit {
        pane_id: String,
        #[serde(default)]
        code: Option<i32>,
    },
    PtyError {
        pane_id: String,
        message: String,
    },
    Pong,
    #[serde(other)]
    Unknown,
}

/// Build a `pty_input` frame from raw bytes.
#[must_use]
pub fn input_frame(pane_id: &str, bytes: &[u8]) -> ClientFrame {
    ClientFrame::PtyInput {
        pane_id: pane_id.to_string(),
        data: BASE64.encode(bytes),
    }
}

/// Raw bytes carried by an output frame, or `None` for every other kind.
#[must_use]
pub fn output_bytes(frame: &ServerFrame) -> Option<Vec<u8>> {
    match frame {
        ServerFrame::PtyOutput { data, .. } => BASE64.decode(data).ok(),
        _ => None,
    }
}

/// The pane a server frame belongs to, if it names one.
#[must_use]
pub fn frame_pane(frame: &ServerFrame) -> Option<&str> {
    match frame {
        ServerFrame::PtyOpened { pane_id }
        | ServerFrame::PtyOutput { pane_id, .. }
        | ServerFrame::PtyExit { pane_id, .. }
        | ServerFrame::PtyError { pane_id, .. } => Some(pane_id),
        ServerFrame::Pong | ServerFrame::Unknown => None,
    }
}

/// Serialize a client frame for `WebSocket::send`.
pub fn encode(frame: &ClientFrame) -> Result<String, serde_json::Error> {
    serde_json::to_string(frame)
}

/// Parse one relayed worker frame. Malformed JSON and unknown kinds are both
/// non-fatal; malformed input returns the error so the caller can log it.
pub fn decode(text: &str) -> Result<ServerFrame, serde_json::Error> {
    serde_json::from_str(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(frame: &ClientFrame) -> String {
        encode(frame).unwrap()
    }

    fn server(text: &str) -> ServerFrame {
        decode(text).unwrap()
    }

    #[test]
    fn open_pty_json_is_exact() {
        assert_eq!(
            json(&ClientFrame::OpenPty {
                pane_id: "p1".into(),
                cwd: "/workspace".into(),
                cols: 120,
                rows: 40,
            }),
            r#"{"kind":"open_pty","pane_id":"p1","cwd":"/workspace","cols":120,"rows":40}"#
        );
    }

    #[test]
    fn input_frame_json_is_exact_and_base64() {
        assert_eq!(
            json(&input_frame("p1", b"hi\n")),
            r#"{"kind":"pty_input","pane_id":"p1","data":"aGkK"}"#
        );
    }

    #[test]
    fn resize_json_is_exact() {
        assert_eq!(
            json(&ClientFrame::PtyResize {
                pane_id: "p1".into(),
                cols: 80,
                rows: 24,
            }),
            r#"{"kind":"pty_resize","pane_id":"p1","cols":80,"rows":24}"#
        );
    }

    #[test]
    fn close_json_is_exact() {
        assert_eq!(
            json(&ClientFrame::ClosePty {
                pane_id: "p1".into(),
            }),
            r#"{"kind":"close_pty","pane_id":"p1"}"#
        );
    }

    #[test]
    fn ping_json_is_exact() {
        assert_eq!(json(&ClientFrame::Ping), r#"{"kind":"ping"}"#);
    }

    #[test]
    fn opened_json_is_exact() {
        assert_eq!(
            serde_json::to_string(&server(r#"{"kind":"pty_opened","pane_id":"p1"}"#)).unwrap(),
            r#"{"kind":"pty_opened","pane_id":"p1"}"#
        );
    }

    #[test]
    fn output_json_is_exact() {
        let frame = server(r#"{"kind":"pty_output","pane_id":"p1","data":"aGkK"}"#);
        assert_eq!(output_bytes(&frame).unwrap(), b"hi\n");
        assert_eq!(
            serde_json::to_string(&frame).unwrap(),
            r#"{"kind":"pty_output","pane_id":"p1","data":"aGkK"}"#
        );
    }

    #[test]
    fn exit_json_is_exact() {
        let frame = server(r#"{"kind":"pty_exit","pane_id":"p1","code":0}"#);
        assert_eq!(
            frame,
            ServerFrame::PtyExit {
                pane_id: "p1".into(),
                code: Some(0)
            }
        );
        assert_eq!(
            serde_json::to_string(&frame).unwrap(),
            r#"{"kind":"pty_exit","pane_id":"p1","code":0}"#
        );
        let null = server(r#"{"kind":"pty_exit","pane_id":"p1","code":null}"#);
        assert_eq!(
            null,
            ServerFrame::PtyExit {
                pane_id: "p1".into(),
                code: None
            }
        );
    }

    #[test]
    fn exit_without_code_is_tolerated() {
        assert_eq!(
            server(r#"{"kind":"pty_exit","pane_id":"p1"}"#),
            ServerFrame::PtyExit {
                pane_id: "p1".into(),
                code: None
            }
        );
    }

    #[test]
    fn error_json_is_exact() {
        let frame = server(r#"{"kind":"pty_error","pane_id":"p1","message":"nope"}"#);
        assert_eq!(
            serde_json::to_string(&frame).unwrap(),
            r#"{"kind":"pty_error","pane_id":"p1","message":"nope"}"#
        );
    }

    #[test]
    fn pong_json_is_exact() {
        assert_eq!(server(r#"{"kind":"pong"}"#), ServerFrame::Pong);
        assert_eq!(
            serde_json::to_string(&ServerFrame::Pong).unwrap(),
            r#"{"kind":"pong"}"#
        );
    }

    #[test]
    fn unknown_kinds_are_ignored() {
        assert_eq!(
            server(r#"{"kind":"status_report","worker_id":"w","load":3}"#),
            ServerFrame::Unknown
        );
        assert_eq!(server(r#"{"kind":"agent_log"}"#), ServerFrame::Unknown);
    }

    #[test]
    fn malformed_json_is_an_error() {
        assert!(decode("not json").is_err());
        assert!(decode(r#"{"kind":"pty_output"}"#).is_err());
    }

    #[test]
    fn non_utf8_bytes_round_trip_through_base64() {
        let raw = [0u8, 159, 146, 150, b'\n'];
        let ClientFrame::PtyInput { data, .. } = input_frame("p", &raw) else {
            panic!("wrong frame");
        };
        let frame = ServerFrame::PtyOutput {
            pane_id: "p".into(),
            data,
        };
        assert_eq!(output_bytes(&frame).unwrap(), raw);
        assert_eq!(output_bytes(&ServerFrame::Pong), None);
    }

    #[test]
    fn pane_ids_are_exposed_for_filtering() {
        assert_eq!(
            frame_pane(&server(r#"{"kind":"pty_opened","pane_id":"p1"}"#)),
            Some("p1")
        );
        assert_eq!(frame_pane(&ServerFrame::Pong), None);
        assert_eq!(frame_pane(&ServerFrame::Unknown), None);
    }
}
