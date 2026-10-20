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
