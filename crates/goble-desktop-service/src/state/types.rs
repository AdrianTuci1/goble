use goble_core::agent::{AgentSpec, Trigger};
use goble_core::execution::ExecutionTrace;
use goble_core::harness::ToolCallStatus;
use goble_core::workflow::WorkflowStep;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConnection {
    pub id: String,
    pub name: String,
    pub url: String,
    pub paired: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: String,
    pub timestamp: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub agent_id: Option<String>,
    pub worker_id: Option<String>,
    /// Where the agent for this conversation should run: `"local"` or `"remote"`.
    /// `None` when the user has not chosen yet.
    pub workspace_routing: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    /// JSON array of tool-call metadata (`[{id,name,arguments}]`) attached to an
    /// assistant message that invoked tools. `None` for user/tool-result rows.
    pub tool_calls: Option<String>,
    pub created_at: String,
}

/// One live tool-call transition, emitted as `chat:tool` so the app can render
/// a running call before the turn ends. Mirrors the daemon's
/// `ToolCallStarted`/`ToolCallFinished`/`ToolCallError` events in the shape the
/// renderer consumes, and is what the per-chat in-flight map holds while the
/// call runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallEvent {
    pub chat_id: String,
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub status: ToolCallStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

/// One live reasoning (thinking) transition, emitted as `chat:reasoning` so the
/// app can build the model's thinking rows as they stream. Mirrors the daemon's
/// `ReasoningStarted`/`ReasoningDelta`/`ReasoningDone` events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReasoningEvent {
    pub chat_id: String,
    /// The step this transition belongs to. `started`/`done` carry the step's
    /// own index; a delta does not (it applies to the step already open).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<usize>,
    /// The thinking mode, known on `started`/`done`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mode: String,
    /// New text in this transition (empty for `started`/`done`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub delta: String,
    /// The step's full text, carried on `done`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// The tool-call decision the step settled on, carried on `done`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    pub phase: ReasoningPhase,
}

/// Which reasoning transition a [`ReasoningEvent`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningPhase {
    Started,
    Delta,
    Done,
}

/// A command the harness suspended before running, emitted as
/// `chat:command_proposed` so the app can show the composer's proposal card
/// (A5) and answer it with a `CommandDecision`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandProposedEvent {
    pub chat_id: String,
    /// The suspended tool call the proposal answers.
    pub id: String,
    /// The candidate command lines, the first being what was proposed.
    pub candidates: Vec<String>,
    /// The directory the command would run in.
    pub cwd: String,
}

/// A sub-agent child was spawned on a conversation, emitted as
/// `chat:subagent_spawned` so the app can open the transcript row (S5) and the
/// work-overlay entry. Mirrors the daemon's `SubAgentSpawned`; `chat_id` is the
/// parent conversation, `subagent_id` the child's own conversation id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubAgentSpawnedEvent {
    pub chat_id: String,
    pub subagent_id: String,
    pub subagent_type: String,
    pub description: String,
    pub parent_call_id: String,
    pub run_in_background: bool,
}

/// A live sub-agent's record moved forward, emitted as `chat:subagent_progress`
/// with the child's status kind, activity line, budget counters and elapsed
/// time. Mirrors the daemon's `SubAgentProgress`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubAgentProgressEvent {
    pub chat_id: String,
    pub subagent_id: String,
    pub status: String,
    pub activity: String,
    pub turns: u32,
    pub tool_calls: u32,
    pub tokens: u64,
    pub duration_ms: u64,
}

/// A sub-agent reached a terminal status, emitted as `chat:subagent_finished`:
/// `output` for `completed`, `error` for `failed`/`cancelled`, with the final
/// counters. Mirrors the daemon's `SubAgentFinished`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubAgentFinishedEvent {
    pub chat_id: String,
    pub subagent_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
    pub turns: u32,
    pub tool_calls: u32,
    pub tokens: u64,
}

/// One model call's token accounting, emitted as `chat:usage`. Mirrors the
/// daemon's `TokenUsage`: the counts the provider itself reported, with no
/// price on them. `cached` is the part of `input` served from the provider's
/// prompt cache, absent when the provider reports no cache accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsageEvent {
    pub input: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached: Option<u64>,
    pub output: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {    pub id: String,
    pub name: String,
    pub spec: AgentSpec,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
    pub trigger: Trigger,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamInfo {
    pub id: String,
    pub name: String,
    pub metadata: String,
    pub created_at: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultSecretInfo {
    pub key: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IntentParams {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub expression: Option<String>,
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub intent: String,
    #[serde(default)]
    pub params: IntentParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSetting {
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionInfo {
    pub id: String,
    pub agent_id: Option<String>,
    pub worker_id: Option<String>,
    pub status: String,
    pub trace: ExecutionTrace,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterIdentityInfo {
    pub cluster_name: String,
    pub ca_cert_pem: String,
    pub device_serial: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerInvite {
    pub worker_id: String,
    pub cluster_key: String,
    pub bundle: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub owner_id: String,
    pub participants: Vec<goble_core::thread::Participant>,
    pub tags: Vec<String>,
    pub last_read_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<goble_core::thread::Thread> for ThreadSummary {
    fn from(t: goble_core::thread::Thread) -> Self {
        Self {
            id: t.id.0,
            kind: format!("{:?}", t.kind).to_lowercase(),
            title: t.title,
            owner_id: t.owner_id.0,
            participants: t.participants,
            tags: t.tags,
            last_read_at: None,
            created_at: t.created_at.to_rfc3339(),
            updated_at: t.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadMessageSummary {
    pub id: String,
    pub thread_id: String,
    pub author: goble_core::thread::Participant,
    pub content: String,
    pub reply_to: Option<String>,
    pub tags: Vec<String>,
    pub participant_mentions: Vec<String>,
    pub reactions: Vec<ThreadReactionSummary>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThreadReactionSummary {
    pub emoji: String,
    pub participant_id: String,
}

impl From<goble_core::thread::ThreadMessage> for ThreadMessageSummary {
    fn from(m: goble_core::thread::ThreadMessage) -> Self {
        Self {
            id: m.id.0,
            thread_id: m.thread_id.0,
            author: m.author,
            content: m.content,
            reply_to: m.reply_to.map(|r| r.0),
            tags: m.tags,
            participant_mentions: m
                .participant_mentions
                .iter()
                .map(|p| p.to_string())
                .collect(),
            reactions: m
                .reactions
                .into_iter()
                .map(|r| ThreadReactionSummary {
                    emoji: r.emoji,
                    participant_id: r.participant_id.to_string(),
                })
                .collect(),
            created_at: m.created_at.to_rfc3339(),
            updated_at: m.updated_at.to_rfc3339(),
        }
    }
}
