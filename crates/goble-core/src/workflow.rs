use serde::{Deserialize, Serialize};

use crate::agent::{AgentId, Trigger};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkflowId(pub String);

impl WorkflowId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl std::fmt::Display for WorkflowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workflow {
    pub id: WorkflowId,
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
    pub trigger: Trigger,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub name: String,
    pub agent_id: AgentId,
    pub input_template: String,
    pub depends_on: Vec<String>,
}

impl Workflow {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: WorkflowId::generate(),
            name: name.into(),
            description: description.into(),
            steps: Vec::new(),
            trigger: Trigger::Manual,
            enabled: true,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn with_steps(mut self, steps: Vec<WorkflowStep>) -> Self {
        self.steps = steps;
        self
    }

    pub fn with_step(mut self, step: WorkflowStep) -> Self {
        self.steps.push(step);
        self
    }

    pub fn with_trigger(mut self, trigger: Trigger) -> Self {
        self.trigger = trigger;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    pub workflow_id: Option<WorkflowId>,
    pub agent_id: AgentId,
    pub trigger: Trigger,
    pub enabled: bool,
    pub next_run: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentSpec;

    #[test]
    fn test_workflow_builder() {
        let step = WorkflowStep {
            id: uuid::Uuid::new_v4().to_string(),
            name: "greet".to_string(),
            agent_id: AgentId::generate(),
            input_template: "Say hello".to_string(),
            depends_on: vec![],
        };
        let wf = Workflow::new("hello", "Greeting workflow").with_step(step);
        assert_eq!(wf.steps.len(), 1);
        assert!(wf.enabled);
    }

    #[test]
    fn test_workflow_serialization() {
        let wf = Workflow::new("x", "y");
        let json = serde_json::to_string(&wf).unwrap();
        let decoded: Workflow = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.name, "x");
    }
}
