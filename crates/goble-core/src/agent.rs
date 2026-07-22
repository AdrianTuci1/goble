use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub String);

impl AgentId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Trigger {
    Manual,
    Cron { expression: String },
    Http { path: String },
    Heartbeat { interval_seconds: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpec {
    pub id: AgentId,
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub tools: Vec<String>,
    pub triggers: Vec<Trigger>,
    pub mcp_ids: Vec<String>,
}

impl AgentSpec {
    pub fn new(name: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            id: AgentId::generate(),
            name: name.into(),
            description: String::new(),
            prompt: prompt.into(),
            tools: Vec::new(),
            triggers: vec![Trigger::Manual],
            mcp_ids: Vec::new(),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_trigger(mut self, trigger: Trigger) -> Self {
        self.triggers.push(trigger);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentState {
    Draft,
    Deployed,
    Running,
    Paused,
    Error(String),
}
