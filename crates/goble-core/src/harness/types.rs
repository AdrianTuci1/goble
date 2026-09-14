use crate::llm::LlmToolCall;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// The lifecycle state of a [`ChatToolCall`], persisted alongside it so a
/// re-read from the store can render a terminal state without the live wire.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    /// Planned by the model; execution has not started.
    #[default]
    Pending,
    Running,
    Finished,
    Error,
}

/// One tool call recorded on a chat message's `tool_calls` column. `status` and
/// `result` are written on start and updated on finish/error. Rows persisted by
/// older builds carry only `id`/`name`/`arguments`, so the new fields default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    #[serde(default)]
    pub status: ToolCallStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

impl ChatToolCall {
    /// A planned call, before it has started running.
    pub fn planned(call: &LlmToolCall) -> Self {
        Self {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
            status: ToolCallStatus::Pending,
            result: None,
        }
    }
}

/// An event emitted by the harness while processing a user turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum HarnessEvent {
    AssistantDelta(String),
    ToolCallStarted {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    ToolCallFinished {
        id: String,
        result: String,
    },
    ToolCallError {
        id: String,
        message: String,
    },
    ThinkingModeChanged(String),
    ReasoningStarted {
        step: usize,
        mode: String,
    },
    ReasoningDelta(String),
    ReasoningDone {
        step: usize,
        mode: String,
        content: String,
        decision: String,
    },
    AskUser {
        question: String,
        quick_replies: Vec<String>,
    },
    /// A command tool is waiting on the user's approval before it runs. The
    /// candidates are the proposed command lines and `cwd` is where they would
    /// run; resume with a [`CommandDecision`] to run one or refuse it.
    CommandProposed {
        id: String,
        candidates: Vec<String>,
        cwd: String,
    },
    /// A sub-agent child was registered (S4, emitted by S3's registry on
    /// spawn). `chat_id` is the parent conversation the row lives in; the rest
    /// is the child's [`crate::subagent::SubAgentSpec`] identity.
    SubAgentSpawned {
        chat_id: String,
        subagent_id: String,
        subagent_type: String,
        description: String,
        parent_call_id: String,
        run_in_background: bool,
    },
    /// A live child's record moved forward: its status kind, activity line,
    /// budget counters and the elapsed time from its own clock.
    SubAgentProgress {
        chat_id: String,
        subagent_id: String,
        status: String,
        activity: String,
        turns: u32,
        tool_calls: u32,
        tokens: u64,
        duration_ms: u64,
    },
    /// A child reached a terminal status: `output` for `completed`, `error`
    /// for `failed`/`cancelled`, with the record's final counters.
    SubAgentFinished {
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
        mission_id: String,
        status: String,
    },
    /// One model call's token accounting, straight from the provider's own
    /// response (see [`crate::llm::TokenUsage`]). A conversation's totals are
    /// the app's sum over these; there is no price on it.
    TokenUsage {
        chat_id: String,
        input: u64,
        cached: Option<u64>,
        output: u64,
    },
    Done,
    Error(String),
}

/// The user's decision on a command the harness proposed (A6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CommandDecision {
    /// Run the chosen candidate text verbatim.
    Approve(String),
    /// Run the user's edited text instead.
    Edit(String),
    /// Refuse to run the command; the tool call becomes a `ToolCallError`.
    Reject(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingMode {
    Direct,
    Contemplating,
    Ruminating,
    Baking,
    Reflecting,
    Verifying,
    Debugging,
    Synthesizing,
    Planning,
}

impl ThinkingMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThinkingMode::Direct => "direct",
            ThinkingMode::Contemplating => "contemplating",
            ThinkingMode::Ruminating => "ruminating",
            ThinkingMode::Baking => "baking",
            ThinkingMode::Reflecting => "reflecting",
            ThinkingMode::Verifying => "verifying",
            ThinkingMode::Debugging => "debugging",
            ThinkingMode::Synthesizing => "synthesizing",
            ThinkingMode::Planning => "planning",
        }
    }

    pub fn prompt(&self) -> &'static str {
        match self {
            ThinkingMode::Direct => "Think briefly and then act or reply.",
            ThinkingMode::Contemplating => "Explore multiple angles and viewpoints before deciding. Do not commit yet.",
            ThinkingMode::Ruminating => "Let the problem sit. Revisit assumptions, constraints, and edge cases.",
            ThinkingMode::Baking => "Allow the idea to mature. Synthesize partial thoughts without rushing to a conclusion.",
            ThinkingMode::Reflecting => "Check what you already know, what is missing, and whether the goal is clear.",
            ThinkingMode::Verifying => "Validate facts, plan consistency, and tool availability before acting.",
            ThinkingMode::Debugging => "Diagnose why the previous approach might fail or what could go wrong.",
            ThinkingMode::Synthesizing => "Combine gathered information into a coherent plan or answer.",
            ThinkingMode::Planning => "Break the goal into concrete steps, tools, and dependencies.",
        }
    }
}

impl Default for ThinkingMode {
    fn default() -> Self {
        ThinkingMode::Direct
    }
}

impl std::str::FromStr for ThinkingMode {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "direct" => Ok(ThinkingMode::Direct),
            "contemplating" => Ok(ThinkingMode::Contemplating),
            "ruminating" => Ok(ThinkingMode::Ruminating),
            "baking" => Ok(ThinkingMode::Baking),
            "reflecting" => Ok(ThinkingMode::Reflecting),
            "verifying" => Ok(ThinkingMode::Verifying),
            "debugging" => Ok(ThinkingMode::Debugging),
            "synthesizing" => Ok(ThinkingMode::Synthesizing),
            "planning" => Ok(ThinkingMode::Planning),
            _ => Err(format!("unknown thinking mode: {s}")),
        }
    }
}

/// Web-search backend configuration, resolved by the caller (e.g. from the
/// model/LLM settings) and passed into the harness. When `api_key` and
/// `base_url` are both set, `web_search` uses the hosted backend; otherwise it
/// falls back to scraping DuckDuckGo. An empty config means DuckDuckGo always.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}
