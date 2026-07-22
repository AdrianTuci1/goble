use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for a single V8 Isolate runtime instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IsolateConfig {
    pub id: String,
    pub max_memory_mb: usize,
    pub max_cpu_time_ms: u64,
    pub allow_network: bool,
    pub allowed_hosts: Vec<String>,
    pub allow_filesystem: bool,
    pub allowed_paths: Vec<String>,
    pub env: HashMap<String, String>,
}

impl IsolateConfig {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            max_memory_mb: 128,
            max_cpu_time_ms: 30_000,
            allow_network: false,
            allowed_hosts: Vec::new(),
            allow_filesystem: false,
            allowed_paths: Vec::new(),
            env: HashMap::new(),
        }
    }

    pub fn with_network(mut self, allow: bool) -> Self {
        self.allow_network = allow;
        self
    }

    pub fn with_allowed_host(mut self, host: impl Into<String>) -> Self {
        self.allowed_hosts.push(host.into());
        self
    }

    pub fn with_filesystem(mut self, allow: bool) -> Self {
        self.allow_filesystem = allow;
        self
    }

    pub fn with_allowed_path(mut self, path: impl Into<String>) -> Self {
        self.allowed_paths.push(path.into());
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }
}

/// Shared MCP instance that can be attached to multiple agent isolates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpInstance {
    pub id: String,
    pub server_id: String,
    pub runtime: crate::agent::McpRuntime,
    pub config: serde_json::Value,
    pub credentials_key: Option<String>,
}

/// Descriptor for an agent runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRuntime {
    pub agent_id: crate::agent::AgentId,
    pub isolate_config: IsolateConfig,
    pub mcp_instances: Vec<String>,
    pub source: AgentSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentSource {
    Spec { prompt: String, tools: Vec<String> },
    Bundle { path: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_isolate_config_builder() {
        let cfg = IsolateConfig::new("agent-1")
            .with_network(true)
            .with_allowed_host("api.openai.com")
            .with_filesystem(true)
            .with_allowed_path("/tmp/agent-1")
            .with_env("LOG_LEVEL", "debug");
        assert!(cfg.allow_network);
        assert_eq!(cfg.allowed_hosts, vec!["api.openai.com"]);
        assert!(cfg.allow_filesystem);
        assert_eq!(cfg.env.get("LOG_LEVEL"), Some(&"debug".to_string()));
    }

    #[test]
    fn test_agent_runtime_serialization() {
        let rt = AgentRuntime {
            agent_id: crate::agent::AgentId::generate(),
            isolate_config: IsolateConfig::new("runtime-1"),
            mcp_instances: vec!["mcp-1".to_string()],
            source: AgentSource::Spec {
                prompt: "demo".to_string(),
                tools: vec!["fs".to_string()],
            },
        };
        let json = serde_json::to_string(&rt).unwrap();
        assert!(json.contains("runtime-1"));
    }
}
