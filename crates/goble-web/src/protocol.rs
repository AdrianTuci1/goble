//! JSON frames exchanged with the gateway's worker relay.
//!
//! The gateway copies frames verbatim between the browser and the worker, and
//! fans every worker frame out to every attached client. A client therefore
//! has to ignore frames for panes and turns it does not own; [`ServerFrame`]
//! keeps the `pane_id` and `trace_id` so callers can filter before acting.
//!
//! The wire format is a serde tag named `kind` with snake_case variants. Do
//! not reorder fields: the gateway and worker serialize with the same layout
//! and the tests pin the exact JSON.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// The agent prompt a browser turn runs under. The user's message travels in
/// `prompt`, so this only says what the turn is, the way the phone's
/// `specFor()` does.
const WEB_SPEC_PROMPT: &str = "You are a coding agent working on the user's project through a remote worker. Answer the message you are given, use the workspace tools you need, and keep the reply short enough to read in a browser.";

/// The agent identity one browser turn runs under (`goble-core`'s
/// `AgentSpec`). `triggers` carries `["Manual"]`, the only trigger a browser
/// turn has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpec {
    pub id: String,
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub tools: Vec<String>,
    pub triggers: Vec<String>,
    pub mcp_ids: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// How a run ended, in `goble-core`'s serialization: `"Success"`,
/// `"Cancelled"`, or `{"Failure":"…"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnStatus {
    Pending,
    Running,
    Success,
    Failure(String),
    Cancelled,
}

/// Frames the browser sends to the worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Ask the worker to answer `prompt` in `thread_id`, streaming the turn
    /// back under `trace_id`.
    RunAgentForThreadReply {
        trace_id: String,
        thread_id: String,
        agent_id: String,
        prompt: String,
        spec: AgentSpec,
        mcp_servers: Vec<serde_json::Value>,
    },
    Ping,
}

/// Frames the worker sends to the browser.
///
/// `Unknown` absorbs worker message kinds the browser does not implement
/// (`status_report`, `agent_state_update`, …) so an unexpected frame never
/// tears down a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// The worker accepted a turn; a turn that never gets this never started.
    AgentStarted {
        trace_id: String,
        #[serde(default)]
        agent_id: String,
    },
    /// A streamed piece of the answer.
    AssistantDelta {
        trace_id: String,
        delta: String,
    },
    /// The run ended. `status` is `"Success"`, `"Cancelled"` or
    /// `{"Failure":"…"}`.
    AgentFinished {
        trace_id: String,
        status: TurnStatus,
    },
    AgentLog {
        trace_id: String,
        #[serde(default)]
        step_id: String,
        #[serde(default)]
        level: String,
        #[serde(default)]
        message: String,
    },
    ToolCallStarted {
        trace_id: String,
        id: String,
        name: String,
        #[serde(default)]
        arguments: serde_json::Value,
    },
    ToolCallFinished {
        trace_id: String,
        id: String,
        #[serde(default)]
        result: String,
    },
    ToolCallError {
        trace_id: String,
        id: String,
        message: String,
    },
    AskUser {
        trace_id: String,
        question: String,
        #[serde(default)]
        quick_replies: Vec<String>,
    },
    /// The thread's answer, sent once the run finished. A harness that only
    /// reports at the end carries the whole reply here.
    ThreadAgentReply {
        trace_id: String,
        #[serde(default)]
        thread_id: String,
        content: String,
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
        ServerFrame::AgentStarted { .. }
        | ServerFrame::AssistantDelta { .. }
        | ServerFrame::AgentFinished { .. }
        | ServerFrame::AgentLog { .. }
        | ServerFrame::ToolCallStarted { .. }
        | ServerFrame::ToolCallFinished { .. }
        | ServerFrame::ToolCallError { .. }
        | ServerFrame::AskUser { .. }
        | ServerFrame::ThreadAgentReply { .. } => None,
    }
}

/// The turn a server frame belongs to, if it names one.
#[must_use]
pub fn frame_trace(frame: &ServerFrame) -> Option<&str> {
    match frame {
        ServerFrame::AgentStarted { trace_id, .. }
        | ServerFrame::AssistantDelta { trace_id, .. }
        | ServerFrame::AgentFinished { trace_id, .. }
        | ServerFrame::AgentLog { trace_id, .. }
        | ServerFrame::ToolCallStarted { trace_id, .. }
        | ServerFrame::ToolCallFinished { trace_id, .. }
        | ServerFrame::ToolCallError { trace_id, .. }
        | ServerFrame::AskUser { trace_id, .. }
        | ServerFrame::ThreadAgentReply { trace_id, .. } => Some(trace_id),
        ServerFrame::PtyOpened { .. }
        | ServerFrame::PtyOutput { .. }
        | ServerFrame::PtyExit { .. }
        | ServerFrame::PtyError { .. }
        | ServerFrame::Pong
        | ServerFrame::Unknown => None,
    }
}

/// The `run_agent_for_thread_reply` frame one browser turn sends.
///
/// `stamp` makes the ids unique per turn (`web-<epoch ms>`); `now_rfc3339` is
/// the spec's `created_at`/`updated_at`. The spec mirrors the phone's
/// `specFor()`: the user's message travels in `prompt`, so the agent prompt
/// only says what the turn is.
#[must_use]
pub fn run_agent_for_thread_reply(
    trace_id: &str,
    thread_id: &str,
    prompt: &str,
    stamp: &str,
    now_rfc3339: &str,
) -> ClientFrame {
    let agent_id = format!("web-{stamp}");
    ClientFrame::RunAgentForThreadReply {
        trace_id: trace_id.to_string(),
        thread_id: thread_id.to_string(),
        agent_id: agent_id.clone(),
        prompt: prompt.to_string(),
        spec: AgentSpec {
            id: agent_id,
            name: "Browser session".to_string(),
            description: "A turn started from the web client".to_string(),
            prompt: WEB_SPEC_PROMPT.to_string(),
            tools: Vec::new(),
            triggers: vec!["Manual".to_string()],
            mcp_ids: Vec::new(),
            created_at: now_rfc3339.to_string(),
            updated_at: now_rfc3339.to_string(),
        },
        mcp_servers: Vec::new(),
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
    fn run_agent_for_thread_reply_json_is_exact() {
        let frame =
            run_agent_for_thread_reply("t1", "th1", "hi", "1700", "2026-11-15T05:00:00.000Z");
        assert_eq!(
            json(&frame),
            concat!(
                r#"{"kind":"run_agent_for_thread_reply","trace_id":"t1","thread_id":"th1","agent_id":"web-1700","prompt":"hi","spec":{"id":"web-1700","name":"Browser session","description":"A turn started from the web client","prompt":""#,
                "You are a coding agent working on the user's project through a remote worker. Answer the message you are given, use the workspace tools you need, and keep the reply short enough to read in a browser.",
                r#"","tools":[],"triggers":["Manual"],"mcp_ids":[],"created_at":"2026-11-15T05:00:00.000Z","updated_at":"2026-11-15T05:00:00.000Z"},"mcp_servers":[]}"#
            )
        );
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
        assert_eq!(
            server(r#"{"kind":"agent_state_update","trace_id":"t1"}"#),
            ServerFrame::Unknown
        );
        assert_eq!(
            server(r#"{"kind":"command_proposed","trace_id":"t1","id":"c"}"#),
            ServerFrame::Unknown
        );
    }

    #[test]
    fn agent_started_and_delta_round_trip() {
        let started = server(r#"{"kind":"agent_started","trace_id":"t1","agent_id":"a1"}"#);
        assert_eq!(
            started,
            ServerFrame::AgentStarted {
                trace_id: "t1".into(),
                agent_id: "a1".into(),
            }
        );
        assert_eq!(
            serde_json::to_string(&started).unwrap(),
            r#"{"kind":"agent_started","trace_id":"t1","agent_id":"a1"}"#
        );

        let delta = server(r#"{"kind":"assistant_delta","trace_id":"t1","delta":"hi"}"#);
        assert_eq!(
            delta,
            ServerFrame::AssistantDelta {
                trace_id: "t1".into(),
                delta: "hi".into(),
            }
        );
        assert_eq!(
            serde_json::to_string(&delta).unwrap(),
            r#"{"kind":"assistant_delta","trace_id":"t1","delta":"hi"}"#
        );
    }

    #[test]
    fn agent_finished_status_is_core_json() {
        let success = server(r#"{"kind":"agent_finished","trace_id":"t1","status":"Success"}"#);
        assert_eq!(
            success,
            ServerFrame::AgentFinished {
                trace_id: "t1".into(),
                status: TurnStatus::Success,
            }
        );
        assert_eq!(
            serde_json::to_string(&success).unwrap(),
            r#"{"kind":"agent_finished","trace_id":"t1","status":"Success"}"#
        );

        let failure = server(
            r#"{"kind":"agent_finished","trace_id":"t1","status":{"Failure":"no model key"}}"#,
        );
        assert_eq!(
            failure,
            ServerFrame::AgentFinished {
                trace_id: "t1".into(),
                status: TurnStatus::Failure("no model key".into()),
            }
        );
        assert_eq!(
            serde_json::to_string(&failure).unwrap(),
            r#"{"kind":"agent_finished","trace_id":"t1","status":{"Failure":"no model key"}}"#
        );

        let cancelled = server(r#"{"kind":"agent_finished","trace_id":"t1","status":"Cancelled"}"#);
        assert_eq!(
            cancelled,
            ServerFrame::AgentFinished {
                trace_id: "t1".into(),
                status: TurnStatus::Cancelled,
            }
        );
    }

    #[test]
    fn agent_log_json_is_exact_and_tolerant_of_missing_fields() {
        let frame = server(
            r#"{"kind":"agent_log","trace_id":"t1","step_id":"s1","level":"error","message":"boom"}"#,
        );
        assert_eq!(
            serde_json::to_string(&frame).unwrap(),
            r#"{"kind":"agent_log","trace_id":"t1","step_id":"s1","level":"error","message":"boom"}"#
        );
        assert_eq!(
            server(r#"{"kind":"agent_log","trace_id":"t1"}"#),
            ServerFrame::AgentLog {
                trace_id: "t1".into(),
                step_id: String::new(),
                level: String::new(),
                message: String::new(),
            }
        );
    }

    #[test]
    fn tool_call_frames_round_trip() {
        let started = server(
            r#"{"kind":"tool_call_started","trace_id":"t1","id":"c1","name":"read_file","arguments":{"path":"a"}}"#,
        );
        assert_eq!(
            started,
            ServerFrame::ToolCallStarted {
                trace_id: "t1".into(),
                id: "c1".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path": "a"}),
            }
        );
        assert_eq!(
            serde_json::to_string(&started).unwrap(),
            r#"{"kind":"tool_call_started","trace_id":"t1","id":"c1","name":"read_file","arguments":{"path":"a"}}"#
        );

        let finished =
            server(r#"{"kind":"tool_call_finished","trace_id":"t1","id":"c1","result":"ok"}"#);
        assert_eq!(
            finished,
            ServerFrame::ToolCallFinished {
                trace_id: "t1".into(),
                id: "c1".into(),
                result: "ok".into(),
            }
        );
        assert_eq!(
            server(r#"{"kind":"tool_call_error","trace_id":"t1","id":"c1","message":"nope"}"#),
            ServerFrame::ToolCallError {
                trace_id: "t1".into(),
                id: "c1".into(),
                message: "nope".into(),
            }
        );
    }

    #[test]
    fn ask_user_and_thread_reply_round_trip() {
        let ask = server(
            r#"{"kind":"ask_user","trace_id":"t1","question":"which branch?","quick_replies":["main","dev"]}"#,
        );
        assert_eq!(
            ask,
            ServerFrame::AskUser {
                trace_id: "t1".into(),
                question: "which branch?".into(),
                quick_replies: vec!["main".into(), "dev".into()],
            }
        );
        assert_eq!(
            serde_json::to_string(&ask).unwrap(),
            r#"{"kind":"ask_user","trace_id":"t1","question":"which branch?","quick_replies":["main","dev"]}"#
        );

        let reply = server(
            r#"{"kind":"thread_agent_reply","trace_id":"t1","thread_id":"th1","content":"done"}"#,
        );
        assert_eq!(
            reply,
            ServerFrame::ThreadAgentReply {
                trace_id: "t1".into(),
                thread_id: "th1".into(),
                content: "done".into(),
            }
        );
        assert_eq!(
            serde_json::to_string(&reply).unwrap(),
            r#"{"kind":"thread_agent_reply","trace_id":"t1","thread_id":"th1","content":"done"}"#
        );
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
        assert_eq!(
            frame_pane(&server(
                r#"{"kind":"assistant_delta","trace_id":"t1","delta":"x"}"#
            )),
            None
        );
    }

    #[test]
    fn turn_ids_are_exposed_for_filtering() {
        assert_eq!(
            frame_trace(&server(
                r#"{"kind":"assistant_delta","trace_id":"t1","delta":"x"}"#
            )),
            Some("t1")
        );
        assert_eq!(
            frame_trace(&server(r#"{"kind":"pty_opened","pane_id":"p1"}"#)),
            None
        );
        assert_eq!(frame_trace(&ServerFrame::Unknown), None);
    }
}
