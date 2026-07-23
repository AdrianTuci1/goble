use chrono::{DateTime, Utc};
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
    pub created_at: String,
    pub updated_at: String,
}

impl AgentSpec {
    pub fn new(name: impl Into<String>, prompt: impl Into<String>) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            id: AgentId::generate(),
            name: name.into(),
            description: String::new(),
            prompt: prompt.into(),
            tools: Vec::new(),
            triggers: vec![Trigger::Manual],
            mcp_ids: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
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

/// MCP server manifest + install metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub source: McpSource,
    pub manifest: McpManifest,
    pub credentials_key: Option<String>,
    pub installed_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpSource {
    Github { repo: String, rev: String },
    Npm { package: String, version: String },
    Local { path: String },
    Url { url: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpManifest {
    pub schema_version: String,
    pub entrypoint: String,
    pub runtime: McpRuntime,
    pub auth_schema: Vec<AuthField>,
    pub capabilities: Vec<String>,
    pub config_schema: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum McpRuntime {
    V8Isolate,
    Binary { command: String, args: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthField {
    pub name: String,
    pub label: String,
    pub field_type: AuthFieldType,
    pub required: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthFieldType {
    Token,
    Password,
    Text,
    Url,
    File,
}

/// Team is a manually managed group of agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub agent_ids: Vec<AgentId>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

/// Model for the desktop chat view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub agent_id: Option<AgentId>,
    pub worker_id: Option<crate::worker::WorkerId>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: ChatRole,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_manifest_roundtrip() {
        let manifest = McpManifest {
            schema_version: "1".to_string(),
            entrypoint: "dist/index.js".to_string(),
            runtime: McpRuntime::V8Isolate,
            auth_schema: vec![AuthField {
                name: "api_key".to_string(),
                label: "API Key".to_string(),
                field_type: AuthFieldType::Token,
                required: true,
                description: None,
            }],
            capabilities: vec!["filesystem".to_string()],
            config_schema: serde_json::Value::Object(Default::default()),
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let back: McpManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, back);
    }

    #[test]
    fn test_team_serialization() {
        let team = Team {
            id: "t1".to_string(),
            name: "Core".to_string(),
            agent_ids: vec![AgentId::generate()],
            metadata: serde_json::Value::Object(Default::default()),
            created_at: Utc::now(),
        };
        let json = serde_json::to_string(&team).unwrap();
        assert!(json.contains("Core"));
    }
}
