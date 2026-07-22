use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::agent::AgentSpec;
use crate::worker::WorkerId;

/// A unit of work dispatched to a worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub worker_id: WorkerId,
    pub payload: TaskPayload,
    pub priority: TaskPriority,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskPayload {
    RunAgent {
        trace_id: String,
        spec: AgentSpec,
    },
    InstallMcp {
        server_id: String,
        manifest: crate::agent::McpManifest,
    },
    ConfigureMcp {
        instance_id: String,
        config: serde_json::Value,
    },
    RunCommand {
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TaskPriority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    Queued,
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

impl Task {
    pub fn new(worker_id: WorkerId, payload: TaskPayload) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            worker_id,
            payload,
            priority: TaskPriority::Normal,
            created_at: chrono::Utc::now(),
        }
    }

    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_serialization() {
        let task = Task::new(
            WorkerId::generate(),
            TaskPayload::RunCommand {
                command: "echo".to_string(),
                args: vec!["hello".to_string()],
                env: HashMap::new(),
            },
        );
        let json = serde_json::to_string(&task).unwrap();
        assert!(json.contains("RunCommand"));
    }
}
