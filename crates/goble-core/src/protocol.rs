use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::agent::{AgentId, AgentSpec, McpServer};
use crate::worker::WorkerId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesktopMessage {
    PairRequest {
        worker_id: WorkerId,
        pairing_code_hash: Option<String>,
        /// The telemetry link id the desktop shares with the installations it
        /// pairs. Random and pseudonymous; it travels here because this is the
        /// authenticated handshake, and it is absent on a desktop that has not
        /// started telemetry.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        link_id: Option<String>,
    },
    RunAgent {
        trace_id: String,
        agent_id: AgentId,
        spec: AgentSpec,
        mcp_servers: Vec<McpServer>,
    },
    ScheduleAgent {
        agent_id: AgentId,
        trigger: crate::agent::Trigger,
        mcp_servers: Vec<McpServer>,
    },
    PushSecrets {
        secrets: Vec<crate::secret::Secret>,
    },
    SetVaultSecret {
        name: String,
        value: Vec<u8>,
    },
    GetVaultSecret {
        name: String,
    },
    ListScheduledTasks,
    CancelScheduledTask {
        task_id: String,
    },
    PushMcpServers {
        servers: Vec<crate::agent::McpServer>,
    },
    UpdateAgent {
        agent_id: AgentId,
        spec: AgentSpec,
    },
    RemoveAgent {
        agent_id: AgentId,
    },
    RunTeam {
        trace_id: String,
        team_id: String,
    },
    RunAgentForThreadReply {
        trace_id: String,
        thread_id: String,
        agent_id: AgentId,
        prompt: String,
        spec: AgentSpec,
        mcp_servers: Vec<McpServer>,
    },
    GetTrace {
        trace_id: String,
    },
    /// Stop a run the client started and no longer wants.
    ///
    /// The worker kills the harness behind `trace_id` (a remote DSH run is
    /// stopped with a process-group kill, so its tools and children go with
    /// it) and reports the end on that trace as `agent_finished` with
    /// `Cancelled`. An unknown or already-finished trace is a no-op: cancelling
    /// a run that settled in the meantime is not an error.
    CancelRun {
        trace_id: String,
    },
    /// Answer a run that paused on `AskUser`, so the same turn continues from
    /// where it stopped. `trace_id` is the run's session id.
    AnswerUser {
        trace_id: String,
        answer: String,
    },
    /// Decide the command a run paused on (`CommandProposed`): run the proposed
    /// line, run an edited one, or refuse it, so the same turn continues.
    AnswerCommand {
        trace_id: String,
        decision: crate::harness::CommandDecision,
    },
    /// A file the client attached to a message. `data` is base64 of the raw
    /// bytes; the worker writes them under its workspace and answers with
    /// `AttachmentStored`.
    UploadAttachment {
        upload_id: String,
        name: String,
        data: String,
    },
    /// Speech the phone recorded for the composer, which the worker hands to
    /// the provider the client names. `data` is base64 of the audio bytes and
    /// `mime` says what container they are in (`audio/webm`, `audio/m4a`);
    /// the answer is `TranscriptionResult` under the same `request_id`.
    TranscribeAudio {
        request_id: String,
        provider: String,
        model: String,
        #[serde(default)]
        mime: String,
        data: String,
    },
    QueryEntities {
        entity_type: String,
        query: Option<String>,
    },
    TriggerSnapshot,
    // --- daemon reversibility (rewind/fork/replay/checkpoints/settle) ---
    // These drive the embedded daemon's harness-agnostic transcript
    // reversibility and are surfaced on the worker's WebSocket so the headless
    // worker is reversible. `session_id` names the daemon session/transcript;
    // `new_session_id` names the fork target.
    Rewind {
        session_id: String,
        at: usize,
    },
    Fork {
        session_id: String,
        at: usize,
        new_session_id: String,
    },
    Replay {
        session_id: String,
        at: usize,
    },
    Checkpoints {
        session_id: String,
    },
    Select {
        session_id: String,
        at: usize,
    },
    Apply {
        session_id: String,
        at: usize,
    },
    Release {
        session_id: String,
        at: usize,
    },
    Discard {
        session_id: String,
        at: usize,
    },
    // --- daemon workflow execution ---
    // `request` is a `goble_workflow::WorkflowHostRequest` serialized as JSON, so
    // this protocol crate stays decoupled from the workflow engine crate.
    RunWorkflow {
        request: serde_json::Value,
    },
    // --- pty transport ---
    // A browser client opens a terminal in the sandbox. Panes are owned by the
    // WebSocket connection that opened them and are closed when it goes away.
    // `PtyInput::data` is base64 of the raw keystrokes; see `encode_pty_bytes`.
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerMessage {
    Paired,
    AgentStarted {
        trace_id: String,
        agent_id: AgentId,
    },
    AgentLog {
        trace_id: String,
        step_id: String,
        level: crate::execution::LogLevel,
        message: String,
    },
    AgentFinished {
        trace_id: String,
        status: crate::execution::ExecutionStatus,
    },
    StatusReport {
        worker_id: WorkerId,
        status: crate::worker::WorkerStatus,
        load: u8,
    },
    Pong,
    VaultSecret {
        name: String,
        value: Option<Vec<u8>>,
    },
    VaultError {
        message: String,
    },
    ScheduledTasks {
        tasks: Vec<ScheduledTaskSummary>,
    },
    // Live snapshot of scheduled routines pushed from the worker scheduler.
    RoutinesUpdated {
        routines: Vec<RoutineInfo>,
    },
    TaskCancelled {
        task_id: String,
    },
    ThreadAgentReply {
        trace_id: String,
        thread_id: String,
        content: String,
    },
    AgentStateUpdate {
        trace_id: String,
        state: crate::agent_runtime::RuntimeState,
    },
    AgentToolResult {
        trace_id: String,
        step_id: String,
        name: String,
        result: String,
    },
    // Streaming harness events from worker to desktop.
    AssistantDelta {
        trace_id: String,
        delta: String,
    },
    ToolCallStarted {
        trace_id: String,
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ToolCallFinished {
        trace_id: String,
        id: String,
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
        quick_replies: Vec<String>,
    },
    /// The harness suspended a command tool before running it: `id` is the
    /// suspended call, `candidates` the proposed command lines and `cwd` where
    /// they would run. The client answers with `AnswerCommand`.
    CommandProposed {
        trace_id: String,
        id: String,
        candidates: Vec<String>,
        cwd: String,
    },
    MissionUpdated {
        trace_id: String,
        mission_id: String,
        status: String,
    },
    Done {
        trace_id: String,
    },
    Trace {
        trace_id: String,
        trace: Option<crate::execution::ExecutionTrace>,
    },
    EntityList {
        entity_type: String,
        items: Vec<serde_json::Value>,
    },
    /// Where an uploaded file landed, or why it did not. `path` is inside the
    /// worker's workspace, so the agent can read it with the tools it has.
    AttachmentStored {
        upload_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// What the provider heard in the audio a `TranscribeAudio` carried, or why
    /// it could not ask: no key for the provider, an unreachable endpoint, a
    /// refusal. `text` is the transcript, empty only when nothing was heard.
    TranscriptionResult {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    // --- daemon reversibility / workflow results ---
    RewindResult {
        session_id: String,
        removed: usize,
    },
    ForkResult {
        session_id: String,
        new_session_id: String,
    },
    // Replay forwards the session's transcript as the mirror `AssistantDelta` /
    // `ToolCall*` / `Done` variants on the live stream, then acks with the index
    // the replay started from.
    ReplayResult {
        session_id: String,
        at: usize,
    },
    CheckpointsResult {
        session_id: String,
        checkpoints: Vec<serde_json::Value>,
    },
    SelectResult {
        session_id: String,
        at: usize,
    },
    ApplyResult {
        session_id: String,
        removed: usize,
    },
    ReleaseResult {
        session_id: String,
        released: bool,
    },
    DiscardResult {
        session_id: String,
        removed: usize,
    },
    // `run` is a `goble_workflow::WorkflowRun` serialized as JSON.
    WorkflowRun {
        run: serde_json::Value,
    },
    // --- pty transport ---
    PtyOpened {
        pane_id: String,
    },
    // `data` is base64 of the raw pty bytes; see `encode_pty_bytes`.
    PtyOutput {
        pane_id: String,
        data: String,
    },
    // `code` is `None` when the child ended without a status (for example when
    // the worker could not reap it).
    PtyExit {
        pane_id: String,
        code: Option<i32>,
    },
    PtyError {
        pane_id: String,
        message: String,
    },
}

/// Base64 the raw bytes of a pty stream for `PtyInput::data` / `PtyOutput::data`.
///
/// A pty carries bytes that are not necessarily UTF-8, so the JSON field is
/// base64 rather than a string of the bytes.
pub fn encode_pty_bytes(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Decode `PtyInput::data` / `PtyOutput::data` back to raw pty bytes.
pub fn decode_pty_bytes(data: &str) -> Result<Vec<u8>, base64::DecodeError> {
    base64::engine::general_purpose::STANDARD.decode(data)
}

/// Encode a file's bytes for `UploadAttachment::data`.
pub fn encode_attachment_bytes(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Decode `UploadAttachment::data` back to the file's bytes.
pub fn decode_attachment_bytes(data: &str) -> Result<Vec<u8>, base64::DecodeError> {
    base64::engine::general_purpose::STANDARD.decode(data)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledTaskSummary {
    pub id: String,
    pub agent_id: crate::agent::AgentId,
    pub trigger: crate::agent::Trigger,
    pub enabled: bool,
}

/// A scheduled routine plus its last execution state, shown in the UI panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineInfo {
    pub id: String,
    pub agent_id: crate::agent::AgentId,
    pub trigger: crate::agent::Trigger,
    pub enabled: bool,
    pub last_run_at: Option<String>,
    pub last_status: Option<String>,
}

/// A single line from the remote host stream, shown in the routines panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteLogLine {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u32,
    pub payload: Vec<u8>,
    pub signature: Option<Vec<u8>>,
}

impl Envelope {
    pub fn new(payload: Vec<u8>) -> Self {
        Self {
            version: 1,
            payload,
            signature: None,
        }
    }

    pub fn serialize(&self) -> anyhow::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(anyhow::Error::from)
    }

    pub fn deserialize(bytes: &[u8]) -> anyhow::Result<Self> {
        serde_json::from_slice(bytes).map_err(anyhow::Error::from)
    }
}

/// Whether a [`DesktopMessage`] that failed to deserialize was rejected because
/// its `kind` tag is unknown to this build, rather than because a known frame
/// carried a bad payload.
///
/// A worker shares one relay with clients that may be newer than it, so an
/// unknown frame is ignored instead of failing the connection; a known frame
/// with a broken payload is still an error. serde reports the two cases
/// differently, and only the first says `unknown variant`.
pub fn is_unknown_kind(error: &serde_json::Error) -> bool {
    error.to_string().starts_with("unknown variant")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentId, AgentSpec};

    #[test]
    fn test_roundtrip_desktop_message() {
        let msg = DesktopMessage::RunAgent {
            trace_id: "t1".to_string(),
            agent_id: AgentId::generate(),
            spec: AgentSpec::new("demo", "do nothing"),
            mcp_servers: vec![],
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: DesktopMessage = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }

    /// Cancelling a run names only the trace it stops, so a client that does
    /// not know the frame yet keeps working and one that does can end a remote
    /// harness run without waiting for it.
    #[test]
    fn a_cancel_names_only_the_trace() {
        let cancel = DesktopMessage::CancelRun {
            trace_id: "t1".to_string(),
        };
        let json = serde_json::to_value(&cancel).unwrap();
        assert_eq!(json["kind"], "cancel_run");
        assert_eq!(json["trace_id"], "t1");
        assert_eq!(
            serde_json::from_value::<DesktopMessage>(json).unwrap(),
            cancel
        );
    }

    /// A frame with a newer `kind` is told apart from a broken known frame, so a
    /// worker can ignore the former and still report the latter.
    #[test]
    fn an_unknown_kind_is_recognized_as_such() {
        let unknown =
            serde_json::from_str::<DesktopMessage>(r#"{"kind":"run_on_mars","trace_id":"t1"}"#)
                .expect_err("an unknown kind does not deserialize");
        assert!(is_unknown_kind(&unknown), "{unknown}");

        let bad_payload =
            serde_json::from_str::<DesktopMessage>(r#"{"kind":"cancel_run","trace_id":42}"#)
                .expect_err("a broken known frame does not deserialize");
        assert!(!is_unknown_kind(&bad_payload), "{bad_payload}");
    }

    /// The command gate travels as its own pair of frames: the worker proposes
    /// a line and the client answers with a decision, so both sides name the
    /// same shape.
    #[test]
    fn a_command_proposal_and_its_decision_roundtrip() {
        let proposed = WorkerMessage::CommandProposed {
            trace_id: "t1".to_string(),
            id: "call-1".to_string(),
            candidates: vec!["git status".to_string(), "git diff".to_string()],
            cwd: "/workspace".to_string(),
        };
        let json = serde_json::to_value(&proposed).unwrap();
        assert_eq!(json["kind"], "command_proposed");
        assert_eq!(json["candidates"][0], "git status");
        assert_eq!(
            serde_json::from_value::<WorkerMessage>(json).unwrap(),
            proposed
        );

        let decision = DesktopMessage::AnswerCommand {
            trace_id: "t1".to_string(),
            decision: crate::harness::CommandDecision::Approve("git status".to_string()),
        };
        let json = serde_json::to_value(&decision).unwrap();
        assert_eq!(json["kind"], "answer_command");
        assert_eq!(json["decision"]["type"], "approve");
        assert_eq!(json["decision"]["payload"], "git status");
        assert_eq!(
            serde_json::from_value::<DesktopMessage>(json).unwrap(),
            decision
        );
    }

    /// An upload names the file it carries and the worker answers with where
    /// it landed, so the client can put that path in the turn.
    #[test]
    fn an_upload_and_its_stored_path_roundtrip() {
        let bytes = b"hello";
        let upload = DesktopMessage::UploadAttachment {
            upload_id: "u1".to_string(),
            name: "shot.png".to_string(),
            data: encode_attachment_bytes(bytes),
        };
        let json = serde_json::to_value(&upload).unwrap();
        assert_eq!(json["kind"], "upload_attachment");
        assert_eq!(
            decode_attachment_bytes(json["data"].as_str().unwrap()).unwrap(),
            bytes
        );

        let stored = WorkerMessage::AttachmentStored {
            upload_id: "u1".to_string(),
            path: Some("/workspace/attachments/u1-shot.png".to_string()),
            error: None,
        };
        let json = serde_json::to_value(&stored).unwrap();
        assert_eq!(json["kind"], "attachment_stored");
        assert!(json.get("error").is_none());
        assert_eq!(
            serde_json::from_value::<WorkerMessage>(json).unwrap(),
            stored
        );
    }

    /// A dictation segment names the provider and model to ask, carries the
    /// audio, and comes back as words under the same request id.
    #[test]
    fn a_transcription_request_and_its_answer_roundtrip() {
        let bytes = b"audio bytes";
        let request = DesktopMessage::TranscribeAudio {
            request_id: "r1".to_string(),
            provider: "openai".to_string(),
            model: "gpt-4o-mini-transcribe".to_string(),
            mime: "audio/webm".to_string(),
            data: encode_attachment_bytes(bytes),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["kind"], "transcribe_audio");
        assert_eq!(json["provider"], "openai");
        assert_eq!(
            decode_attachment_bytes(json["data"].as_str().unwrap()).unwrap(),
            bytes
        );

        let answer = WorkerMessage::TranscriptionResult {
            request_id: "r1".to_string(),
            text: Some("walk me through the pairing flow".to_string()),
            error: None,
        };
        let json = serde_json::to_value(&answer).unwrap();
        assert_eq!(json["kind"], "transcription_result");
        assert!(json.get("error").is_none());
        assert_eq!(
            serde_json::from_value::<WorkerMessage>(json).unwrap(),
            answer
        );
    }

    /// A failed transcription carries only the reason, and a client that does
    /// not know the frames yet is not sent fields it would have to understand.
    #[test]
    fn a_failed_transcription_carries_only_its_reason() {
        let answer = WorkerMessage::TranscriptionResult {
            request_id: "r2".to_string(),
            text: None,
            error: Some("no openai key on this worker".to_string()),
        };
        let json = serde_json::to_value(&answer).unwrap();
        assert!(json.get("text").is_none());
        assert_eq!(json["error"], "no openai key on this worker");
    }

    /// The telemetry link rides the pairing handshake, and a build that does
    /// not know the field yet must still read the request.
    #[test]
    fn a_pair_request_carries_the_telemetry_link_and_stays_readable_without_it() {
        let worker_id = WorkerId::generate();
        let msg = DesktopMessage::PairRequest {
            worker_id: worker_id.clone(),
            pairing_code_hash: Some("hash".to_string()),
            link_id: Some("link-1".to_string()),
        };
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["link_id"], "link-1");
        assert_eq!(serde_json::from_value::<DesktopMessage>(json).unwrap(), msg);

        let old: DesktopMessage = serde_json::from_value(serde_json::json!({
            "kind": "pair_request",
            "worker_id": worker_id,
            "pairing_code_hash": "hash"
        }))
        .unwrap();
        assert_eq!(
            old,
            DesktopMessage::PairRequest {
                worker_id,
                pairing_code_hash: Some("hash".to_string()),
                link_id: None,
            }
        );

        // No link means the field is not on the wire at all.
        let without = serde_json::to_value(DesktopMessage::PairRequest {
            worker_id: WorkerId::generate(),
            pairing_code_hash: None,
            link_id: None,
        })
        .unwrap();
        assert!(without.get("link_id").is_none(), "{without}");
    }

    #[test]
    fn test_roundtrip_worker_message_log() {
        let msg = WorkerMessage::AgentLog {
            trace_id: "t1".to_string(),
            step_id: "s1".to_string(),
            level: crate::execution::LogLevel::Info,
            message: "hello".to_string(),
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: WorkerMessage = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_roundtrip_worker_message_status() {
        let msg = WorkerMessage::StatusReport {
            worker_id: WorkerId::generate(),
            status: crate::worker::WorkerStatus::Idle,
            load: 42,
        };
        let bytes = serde_json::to_vec(&msg).unwrap();
        let decoded: WorkerMessage = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn test_envelope_roundtrip() {
        let env = Envelope::new(vec![1, 2, 3]);
        let bytes = env.serialize().unwrap();
        let decoded = Envelope::deserialize(&bytes).unwrap();
        assert_eq!(decoded.payload, vec![1, 2, 3]);
    }

    #[test]
    fn test_roundtrip_desktop_pty_messages() {
        let messages = vec![
            DesktopMessage::OpenPty {
                pane_id: "pane-1".to_string(),
                cwd: "/workspace".to_string(),
                cols: 120,
                rows: 40,
            },
            DesktopMessage::PtyInput {
                pane_id: "pane-1".to_string(),
                data: encode_pty_bytes(b"echo hi\n"),
            },
            DesktopMessage::PtyResize {
                pane_id: "pane-1".to_string(),
                cols: 80,
                rows: 24,
            },
            DesktopMessage::ClosePty {
                pane_id: "pane-1".to_string(),
            },
        ];
        for msg in messages {
            let json = serde_json::to_string(&msg).unwrap();
            let decoded: DesktopMessage = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, msg, "wire form: {json}");
        }
    }

    #[test]
    fn test_roundtrip_worker_pty_messages() {
        let messages = vec![
            WorkerMessage::PtyOpened {
                pane_id: "pane-1".to_string(),
            },
            WorkerMessage::PtyOutput {
                pane_id: "pane-1".to_string(),
                data: encode_pty_bytes(b"hello\n"),
            },
            WorkerMessage::PtyExit {
                pane_id: "pane-1".to_string(),
                code: Some(0),
            },
            WorkerMessage::PtyExit {
                pane_id: "pane-1".to_string(),
                code: None,
            },
            WorkerMessage::PtyError {
                pane_id: "pane-1".to_string(),
                message: "no such pane".to_string(),
            },
        ];
        for msg in messages {
            let json = serde_json::to_string(&msg).unwrap();
            let decoded: WorkerMessage = serde_json::from_str(&json).unwrap();
            assert_eq!(decoded, msg, "wire form: {json}");
        }
    }

    #[test]
    fn test_pty_kind_tags_are_snake_case() {
        let json = serde_json::to_value(DesktopMessage::OpenPty {
            pane_id: "p".to_string(),
            cwd: "/w".to_string(),
            cols: 1,
            rows: 1,
        })
        .unwrap();
        assert_eq!(json["kind"], "open_pty");
        let json = serde_json::to_value(WorkerMessage::PtyExit {
            pane_id: "p".to_string(),
            code: None,
        })
        .unwrap();
        assert_eq!(json["kind"], "pty_exit");
    }

    #[test]
    fn test_pty_bytes_codec_keeps_non_utf8() {
        let bytes = vec![0u8, 159, 146, 150, b'\n'];
        let encoded = encode_pty_bytes(&bytes);
        assert_eq!(decode_pty_bytes(&encoded).unwrap(), bytes);
        assert!(decode_pty_bytes("not base64!").is_err());
    }
}
