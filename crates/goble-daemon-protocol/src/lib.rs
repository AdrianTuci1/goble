//! Wire types for the GUI<->daemon boundary.
//!
//! This is the transport-agnostic seam between the GUI (a thin client) and the
//! daemon (embedded in the GUI binary or headless/remote). It owns only
//! serializable, pure data — ids, harness turns, event mirrors. There is no
//! transport, no execution, and no dependency on `goble-core` or `app/`.
//!
//! The natural framing is newline-delimited JSON: each message is one line.

use goble_harness_types::{
    CommandDecision, HarnessTurn, MediumId, ProjectId, RemoteScreenConfig, SessionId,
};
use goble_workflow::WorkflowHostRequest;
use serde::{Deserialize, Serialize};

/// Requests the GUI sends to the daemon.
///
/// Mirrors the daemon's run/resume/cancel entry points plus the read-only
/// introspection (harness list, snapshot) the GUI needs to drive its views.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DaemonRequest {
    /// Start one harness turn whose run is identified by the turn's session.
    RunHarness {
        turn: HarnessTurn,
    },
    /// Resume a turn that suspended on a user answer.
    Resume {
        session_id: SessionId,
        response: String,
    },
    /// Answer a turn that suspended on a command proposal with the user's
    /// decision (approve, edit or reject).
    ResumeCommand {
        session_id: SessionId,
        decision: CommandDecision,
    },
    /// Cancel a running turn.
    Cancel {
        session_id: SessionId,
    },
    /// Enumerate the harnesses the daemon can run.
    ListHarnesses,
    /// Ask the daemon to snapshot the current execution state.
    Snapshot,
    /// Rewind a settled session's transcript to keep `at` turns.
    Rewind {
        session_id: SessionId,
        at: usize,
    },
    /// Fork a fresh session from a checkpoint index, carrying the recorded prefix.
    Fork {
        session_id: SessionId,
        at: usize,
        new_session_id: SessionId,
    },
    /// Re-emit a session's recorded transcript from checkpoint `at` onward.
    Replay {
        session_id: SessionId,
        at: usize,
    },
    /// List a session's per-turn checkpoints.
    Checkpoints {
        session_id: SessionId,
    },
    /// Mark checkpoint turn `at` as a session's settlement candidate.
    Select {
        session_id: SessionId,
        at: usize,
    },
    /// Commit the selected candidate at `at`, keeping it and its prefix.
    Apply {
        session_id: SessionId,
        at: usize,
    },
    /// Drop a pending candidate at `at` without rewinding.
    Release {
        session_id: SessionId,
        at: usize,
    },
    /// Discard the candidate at `at` and its whole tail.
    Discard {
        session_id: SessionId,
        at: usize,
    },
    /// Run a scripted workflow: journal the request and drive it through the
    /// daemon's workflow host, persisting the resulting [`WorkflowRun`].
    RunWorkflow {
        request: WorkflowHostRequest,
    },
}

/// Events the daemon streams back to the GUI.
///
/// Two groups:
/// - a *tail mirror* of the harness event sub-stream, scoped by `session_id`,
///   so the GUI can render inline state as a run progresses; and
/// - the daemon trace lifecycle (`TraceStarted` / `TraceFinished`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", content = "payload")]
pub enum DaemonEvent {
    // --- tail mirror of the harness sub-stream ---
    /// A completion delta was produced for the turn.
    AssistantDelta {
        session_id: SessionId,
        delta: String,
    },
    ToolCallStarted {
        session_id: SessionId,
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ToolCallFinished {
        session_id: SessionId,
        id: String,
        result: String,
    },
    ToolCallError {
        session_id: SessionId,
        id: String,
        message: String,
    },
    AskUser {
        session_id: SessionId,
        question: String,
        quick_replies: Vec<String>,
    },
    /// A command tool is suspended waiting on the user's approval before it
    /// runs. Mirrors the harness-level `CommandProposed`; answer it with a
    /// [`DaemonRequest::ResumeCommand`].
    CommandProposed {
        session_id: SessionId,
        id: String,
        candidates: Vec<String>,
        cwd: String,
    },
    /// A sub-agent child was spawned on the session's conversation. Mirrors
    /// the harness-level `SubAgentSpawned`; plain data, the same fields as
    /// `goble-core`'s lifecycle event.
    SubAgentSpawned {
        session_id: SessionId,
        chat_id: String,
        subagent_id: String,
        subagent_type: String,
        description: String,
        parent_call_id: String,
        run_in_background: bool,
    },
    /// A live sub-agent's record moved forward: status kind, activity,
    /// budget counters and elapsed time. Mirrors `SubAgentProgress`.
    SubAgentProgress {
        session_id: SessionId,
        chat_id: String,
        subagent_id: String,
        status: String,
        activity: String,
        turns: u32,
        tool_calls: u32,
        tokens: u64,
        duration_ms: u64,
    },
    /// A sub-agent reached a terminal status: `output` for `completed`,
    /// `error` for `failed`/`cancelled`. Mirrors `SubAgentFinished`.
    SubAgentFinished {
        session_id: SessionId,
        chat_id: String,
        subagent_id: String,
        status: String,
        output: Option<String>,
        error: Option<String>,
        duration_ms: u64,
        turns: u32,
        tool_calls: u32,
        tokens: u64,
    },
    MissionUpdated {
        session_id: SessionId,
        mission_id: String,
        status: String,
    },
    /// A model call's token accounting, mirrored from the harness level so a
    /// client can total a conversation without re-reading the store. `cached`
    /// is the part of `input` the provider served from its prompt cache.
    TokenUsage {
        session_id: SessionId,
        chat_id: String,
        input: u64,
        cached: Option<u64>,
        output: u64,
    },
    /// The model began a reasoning (thinking) step, identified by `step`, in the
    /// current thinking `mode`. The GUI renders the thinking as its own rows.
    ReasoningStarted {
        session_id: SessionId,
        step: usize,
        mode: String,
    },
    /// A chunk of the current reasoning step's text; deltas stream in order
    /// between the step's `ReasoningStarted` and `ReasoningDone`.
    ReasoningDelta {
        session_id: SessionId,
        delta: String,
    },
    /// The reasoning step finished: `content` is the step's full text and
    /// `decision` the tool-call decision it settled on.
    ReasoningDone {
        session_id: SessionId,
        step: usize,
        mode: String,
        content: String,
        decision: String,
    },
    Done {
        session_id: SessionId,
    },
    Error {
        session_id: SessionId,
        message: String,
    },
    /// The harness handed off to an interactive remote desktop: the GUI should
    /// open the stream (via `goble-screen-sdk`) in a screen pane and route input
    /// back through it. Mirrors the harness-level [`HarnessServerEvent::ScreenHandoff`].
    ScreenHandoff {
        session_id: SessionId,
        config: RemoteScreenConfig,
    },
    // --- daemon trace lifecycle ---
    TraceStarted {
        session_id: SessionId,
        trace_id: String,
        project_id: ProjectId,
        medium_id: MediumId,
    },
    TraceFinished {
        session_id: SessionId,
        trace_id: String,
        status: String,
        project_id: ProjectId,
        medium_id: MediumId,
    },
}

/// Typed framing error for a [`DaemonMessage`] on the GUI<->daemon wire.
#[derive(Debug, thiserror::Error)]
pub enum DaemonMessageError {
    #[error("serde error while framing a daemon message: {0}")]
    Serde(#[from] serde_json::Error),
}

/// One frame on the GUI<->daemon wire: either a request or an event.
///
/// Frames are transport-agnostic: the daemon client (in-process or over
/// WebSocket) encodes/decodes them, so the GUI never sees transport details.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "direction", rename_all = "snake_case")]
pub enum DaemonMessage {
    Request(DaemonRequest),
    Event(DaemonEvent),
}

impl DaemonMessage {
    /// Encode one frame as a single newline-terminated JSON line.
    pub fn to_line(&self) -> anyhow::Result<String> {
        let mut line = serde_json::to_string(self)?;
        line.push('\n');
        Ok(line)
    }

    /// Decode a single line of JSON into a frame.
    pub fn from_line(line: &str) -> anyhow::Result<Self> {
        Ok(serde_json::from_str::<Self>(line.trim())?)
    }

    /// Encode one frame as raw JSON (no framing newline).
    pub fn encode(&self) -> Result<String, DaemonMessageError> {
        Ok(serde_json::to_string(self)?)
    }

    /// Decode a frame from raw JSON (no framing newline).
    pub fn decode(line: &str) -> Result<Self, DaemonMessageError> {
        Ok(serde_json::from_str::<Self>(line.trim())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_harness_types::{HarnessId, HarnessTurn};

    fn sample_turn() -> HarnessTurn {
        HarnessTurn::new(HarnessId::new("cli"), SessionId::new("s1"), "fix bug")
    }

    #[test]
    fn daemon_request_run_roundtrip() {
        let req = DaemonRequest::RunHarness {
            turn: sample_turn(),
        };
        let msg = DaemonMessage::Request(req.clone());

        // serde round-trip
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: DaemonMessage = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, msg);

        // line framing round-trip
        let line = msg.to_line().unwrap();
        let decoded = DaemonMessage::from_line(&line).unwrap();
        assert_eq!(decoded, msg);

        // typed framing round-trip
        let json = msg.encode().unwrap();
        let decoded = DaemonMessage::decode(&json).unwrap();
        assert_eq!(decoded, msg);

        assert!(matches!(decoded, DaemonMessage::Request(DaemonRequest::RunHarness { .. })));
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn daemon_request_resume_cancel_roundtrip() {
        let resume = DaemonRequest::Resume {
            session_id: SessionId::new("s1"),
            response: "yes".into(),
        };
        let cancel = DaemonRequest::Cancel {
            session_id: SessionId::new("s1"),
        };
        let req = DaemonMessage::Request(resume);
        let cancel_msg = DaemonMessage::Request(cancel);

        let line = req.to_line().unwrap();
        let decoded = DaemonMessage::from_line(&line).unwrap();
        assert_eq!(decoded, req);

        let line = cancel_msg.to_line().unwrap();
        let decoded = DaemonMessage::from_line(&line).unwrap();
        assert_eq!(decoded, cancel_msg);
    }

    #[test]
    fn daemon_request_resume_command_roundtrip() {
        let req = DaemonRequest::ResumeCommand {
            session_id: SessionId::new("s1"),
            decision: CommandDecision::Approve("git status".into()),
        };
        let msg = DaemonMessage::Request(req.clone());
        let line = msg.to_line().unwrap();
        assert_eq!(DaemonMessage::from_line(&line).unwrap(), msg);
    }

    #[test]
    fn daemon_event_command_proposed_roundtrip() {
        let ev = DaemonEvent::CommandProposed {
            session_id: SessionId::new("s1"),
            id: "call-1".into(),
            candidates: vec!["git status".into(), "git diff --stat".into()],
            cwd: "/workspace".into(),
        };
        let msg = DaemonMessage::Event(ev.clone());
        let line = msg.to_line().unwrap();
        assert_eq!(DaemonMessage::from_line(&line).unwrap(), msg);
    }

    #[test]
    fn daemon_request_introspection_roundtrip() {
        for req in [DaemonRequest::ListHarnesses, DaemonRequest::Snapshot] {
            let msg = DaemonMessage::Request(req.clone());
            let line = msg.to_line().unwrap();
            let decoded = DaemonMessage::from_line(&line).unwrap();
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn daemon_request_reversibility_roundtrip() {
        let sid = SessionId::new("s1");
        let reqs = vec![
            DaemonRequest::Rewind {
                session_id: sid.clone(),
                at: 2,
            },
            DaemonRequest::Fork {
                session_id: sid.clone(),
                at: 1,
                new_session_id: SessionId::new("s1-fork"),
            },
            DaemonRequest::Replay {
                session_id: sid.clone(),
                at: 0,
            },
            DaemonRequest::Checkpoints {
                session_id: sid.clone(),
            },
        ];

        for req in reqs {
            let msg = DaemonMessage::Request(req.clone());
            let line = msg.to_line().unwrap();
            let decoded = DaemonMessage::from_line(&line).unwrap();
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn daemon_request_settlement_roundtrip() {
        let sid = SessionId::new("s1");
        let reqs = vec![
            DaemonRequest::Select {
                session_id: sid.clone(),
                at: 1,
            },
            DaemonRequest::Apply {
                session_id: sid.clone(),
                at: 2,
            },
            DaemonRequest::Release {
                session_id: sid.clone(),
                at: 1,
            },
            DaemonRequest::Discard {
                session_id: sid.clone(),
                at: 3,
            },
        ];

        for req in reqs {
            let msg = DaemonMessage::Request(req.clone());
            let line = msg.to_line().unwrap();
            let decoded = DaemonMessage::from_line(&line).unwrap();
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn daemon_request_run_workflow_roundtrip() {
        use goble_workflow::{Trigger, WorkflowHostRequest, WorkflowId};
        let req = DaemonRequest::RunWorkflow {
            request: WorkflowHostRequest::new(
                0,
                "req-wf-1",
                WorkflowId::new("build"),
                Trigger::Manual,
                serde_json::json!({}),
            ),
        };
        let msg = DaemonMessage::Request(req.clone());
        let line = msg.to_line().unwrap();
        let decoded = DaemonMessage::from_line(&line).unwrap();
        assert_eq!(decoded, msg);
        assert!(matches!(
            decoded,
            DaemonMessage::Request(DaemonRequest::RunWorkflow { .. })
        ));
    }

    #[test]
    fn daemon_event_harness_mirror_roundtrip() {
        let events = vec![
            DaemonEvent::AssistantDelta {
                session_id: SessionId::new("s1"),
                delta: "hello".into(),
            },
            DaemonEvent::ToolCallStarted {
                session_id: SessionId::new("s1"),
                id: "t1".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path": "src/lib.rs"}),
            },
            DaemonEvent::ToolCallFinished {
                session_id: SessionId::new("s1"),
                id: "t1".into(),
                result: "ok".into(),
            },
            DaemonEvent::ToolCallError {
                session_id: SessionId::new("s1"),
                id: "t2".into(),
                message: "boom".into(),
            },
            DaemonEvent::AskUser {
                session_id: SessionId::new("s1"),
                question: "continue?".into(),
                quick_replies: vec!["yes".into(), "no".into()],
            },
            DaemonEvent::MissionUpdated {
                session_id: SessionId::new("s1"),
                mission_id: "m1".into(),
                status: "running".into(),
            },
            DaemonEvent::ReasoningStarted {
                session_id: SessionId::new("s1"),
                step: 0,
                mode: "contemplating".into(),
            },
            DaemonEvent::ReasoningDelta {
                session_id: SessionId::new("s1"),
                delta: "weighing options".into(),
            },
            DaemonEvent::ReasoningDone {
                session_id: SessionId::new("s1"),
                step: 0,
                mode: "contemplating".into(),
                content: "weighing options".into(),
                decision: "\"execute\"".into(),
            },
            DaemonEvent::Done {
                session_id: SessionId::new("s1"),
            },
            DaemonEvent::Error {
                session_id: SessionId::new("s1"),
                message: "failed".into(),
            },
            DaemonEvent::ScreenHandoff {
                session_id: SessionId::new("s1"),
                config: RemoteScreenConfig::new("vm.example.com", "u", "p"),
            },
        ];

        for event in events {
            let msg = DaemonMessage::Event(event.clone());
            let line = msg.to_line().unwrap();
            let decoded = DaemonMessage::from_line(&line).unwrap();
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn daemon_event_trace_lifecycle_roundtrip() {
        let events = vec![
            DaemonEvent::TraceStarted {
                session_id: SessionId::new("s1"),
                trace_id: "tr1".into(),
                project_id: ProjectId::new("p1"),
                medium_id: MediumId::new("remote"),
            },
            DaemonEvent::TraceFinished {
                session_id: SessionId::new("s1"),
                trace_id: "tr1".into(),
                status: "success".into(),
                project_id: ProjectId::new("p1"),
                medium_id: MediumId::new("remote"),
            },
        ];

        for event in events {
            let msg = DaemonMessage::Event(event.clone());
            let line = msg.to_line().unwrap();
            let decoded = DaemonMessage::from_line(&line).unwrap();
            assert_eq!(decoded, msg);

            let json = msg.encode().unwrap();
            let decoded = DaemonMessage::decode(&json).unwrap();
            assert_eq!(decoded, msg);
        }
    }

    #[test]
    fn line_framing_ignores_trailing_whitespace() {
        let msg = DaemonMessage::Request(DaemonRequest::ListHarnesses);
        let line = msg.to_line().unwrap();
        let padded = format!("{line}   ");
        let decoded = DaemonMessage::from_line(&padded).unwrap();
        assert_eq!(decoded, msg);
    }
}
