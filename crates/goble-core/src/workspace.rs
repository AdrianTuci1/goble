use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::agent::AgentId;

/// A workspace on a worker where an agent performs work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub agent_id: AgentId,
    pub path: PathBuf,
    pub repository_url: Option<String>,
    pub repository_path: Option<PathBuf>,
    pub metadata: serde_json::Value,
}

impl Workspace {
    pub fn new(agent_id: AgentId, base_path: impl AsRef<Path>) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let path = base_path.as_ref().join(&id);
        Self {
            id,
            agent_id,
            path,
            repository_url: None,
            repository_path: None,
            metadata: serde_json::Value::Object(Default::default()),
        }
    }

    pub fn with_repository(
        mut self,
        url: impl Into<String>,
        relative_path: impl AsRef<Path>,
    ) -> Self {
        self.repository_url = Some(url.into());
        self.repository_path = Some(self.path.join(relative_path));
        self
    }

    pub fn ensure_exists(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.path)
    }

    pub fn metadata_dir(&self) -> PathBuf {
        self.path.join(".goblin")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.metadata_dir().join("logs")
    }

    pub fn agent_spec_path(&self) -> PathBuf {
        self.metadata_dir().join("agent.json")
    }

    pub fn mcp_config_path(&self) -> PathBuf {
        self.metadata_dir().join("mcp.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_paths() {
        let ws = Workspace::new(AgentId::generate(), "/tmp/goblin")
            .with_repository("https://github.com/example/repo", "repo");
        assert!(ws.path.to_string_lossy().contains("/tmp/goblin/"));
        assert_eq!(ws.repository_path, Some(ws.path.join("repo")));
        assert_eq!(ws.agent_spec_path(), ws.path.join(".goblin/agent.json"));
    }
}
