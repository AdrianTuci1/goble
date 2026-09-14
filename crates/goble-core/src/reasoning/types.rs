use serde::{Deserialize, Serialize};

use crate::harness::ThinkingMode;
use crate::llm::LlmToolCall;

pub(super) const MAX_REASONING_STEPS: usize = 6;
pub(super) const MAX_EXECUTION_STEPS: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningStep {
    pub step: usize,
    pub mode: ThinkingMode,
    pub content: String,
    pub decision: ReasoningDecision,
    pub tool_calls: Vec<LlmToolCall>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningDecision {
    Continue,
    Execute,
    AskUser,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionState {
    pub id: String,
    pub chat_id: String,
    pub goal: String,
    pub status: String,
    pub plan: Option<String>,
    pub workflow_id: Option<String>,
    pub reasoning_steps: Vec<ReasoningStep>,
    pub pending_ask: Option<PendingAsk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAsk {
    pub id: String,
    pub question: String,
    pub quick_replies: Vec<String>,
}
