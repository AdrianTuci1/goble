use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::agent::AgentId;
use crate::worker::WorkerId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub id: String,
    pub agent_id: AgentId,
    pub worker_id: Option<WorkerId>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: ExecutionStatus,
    pub steps: Vec<Step>,
    pub metrics: Vec<Metric>,
    pub root_step_id: Option<String>,
    pub events: Vec<TraceEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceEvent {
    Log { timestamp: DateTime<Utc>, level: LogLevel, message: String },
    AssistantDelta { timestamp: DateTime<Utc>, delta: String },
    ToolCallStarted { timestamp: DateTime<Utc>, id: String, name: String, arguments: serde_json::Value },
    ToolCallFinished { timestamp: DateTime<Utc>, id: String, result: String },
    ToolCallError { timestamp: DateTime<Utc>, id: String, message: String },
    AskUser { timestamp: DateTime<Utc>, question: String, quick_replies: Vec<String> },
    Done { timestamp: DateTime<Utc>, status: ExecutionStatus },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Pending,
    Running,
    Success,
    Failure(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: ExecutionStatus,
    pub logs: Vec<LogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    pub name: String,
    pub value: f64,
    pub recorded_at: DateTime<Utc>,
}

impl ExecutionTrace {
    pub fn new(agent_id: AgentId) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id,
            worker_id: None,
            started_at: Utc::now(),
            finished_at: None,
            status: ExecutionStatus::Pending,
            steps: Vec::new(),
            metrics: Vec::new(),
            root_step_id: None,
            events: Vec::new(),
        }
    }

    pub fn add_root_step(&mut self, name: impl Into<String>) -> &mut Step {
        let step = Step::new(name, None);
        self.root_step_id = Some(step.id.clone());
        self.steps.push(step);
        self.steps.last_mut().unwrap()
    }

    pub fn add_child_step(
        &mut self,
        parent_id: impl Into<String>,
        name: impl Into<String>,
    ) -> Option<&mut Step> {
        let parent_id = parent_id.into();
        if self.steps.iter().any(|s| s.id == parent_id) {
            let step = Step::new(name, Some(parent_id));
            self.steps.push(step);
            return self.steps.last_mut();
        }
        None
    }

    pub fn find_step_mut(&mut self, id: &str) -> Option<&mut Step> {
        self.steps.iter_mut().find(|s| s.id == id)
    }

    pub fn root_step(&self) -> Option<&Step> {
        self.root_step_id
            .as_ref()
            .and_then(|id| self.steps.iter().find(|s| s.id == *id))
    }

    pub fn child_steps(&self, parent_id: &str) -> Vec<&Step> {
        self.steps
            .iter()
            .filter(|s| s.parent_id.as_deref() == Some(parent_id))
            .collect()
    }

    pub fn sequential_view(&self) -> Vec<(usize, &Step)> {
        let mut result = Vec::new();
        if let Some(root) = self.root_step() {
            Self::walk(&self.steps, root, 0, &mut result);
        }
        result
    }

    fn walk<'a>(all: &'a [Step], step: &'a Step, depth: usize, out: &mut Vec<(usize, &'a Step)>) {
        out.push((depth, step));
        for child in all
            .iter()
            .filter(|s| s.parent_id.as_deref() == Some(&step.id))
        {
            Self::walk(all, child, depth + 1, out);
        }
    }

    pub fn finish(&mut self, status: ExecutionStatus) {
        self.finished_at = Some(Utc::now());
        self.status = status;
    }

    /// Add a metric at the trace level.
    pub fn add_metric(&mut self, name: impl Into<String>, value: f64) {
        self.metrics.push(Metric {
            name: name.into(),
            value,
            recorded_at: Utc::now(),
        });
    }

    pub fn add_event(&mut self, event: TraceEvent) {
        self.events.push(event);
    }

    /// Find a step by ID, immutable.
    pub fn find_step(&self, id: &str) -> Option<&Step> {
        self.steps.iter().find(|s| s.id == id)
    }

    /// Return the parent step of a given step, if any.
    pub fn parent_step(&self, step_id: &str) -> Option<&Step> {
        let parent_id = self.find_step(step_id)?.parent_id.as_ref()?;
        self.find_step(parent_id)
    }
}

impl Step {
    pub fn new(name: impl Into<String>, parent_id: Option<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            parent_id,
            started_at: Utc::now(),
            finished_at: None,
            status: ExecutionStatus::Pending,
            logs: Vec::new(),
        }
    }

    pub fn log(&mut self, level: LogLevel, message: impl Into<String>) {
        self.logs.push(LogEntry {
            timestamp: Utc::now(),
            level,
            message: message.into(),
        });
    }

    pub fn finish(&mut self, status: ExecutionStatus) {
        self.finished_at = Some(Utc::now());
        self.status = status;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_step() {
        let mut trace = ExecutionTrace::new(AgentId::generate());
        trace.add_root_step("root");
        assert!(trace.root_step().is_some());
    }

    #[test]
    fn test_child_step_order() {
        let mut trace = ExecutionTrace::new(AgentId::generate());
        let root = trace.add_root_step("root");
        let root_id = root.id.clone();
        root.finish(ExecutionStatus::Success);

        let child1 = trace.add_child_step(&root_id, "child1").unwrap();
        let child1_id = child1.id.clone();
        child1.finish(ExecutionStatus::Success);

        let grandchild = trace.add_child_step(&child1_id, "grandchild").unwrap();
        grandchild.finish(ExecutionStatus::Success);

        let child2 = trace.add_child_step(&root_id, "child2").unwrap();
        child2.finish(ExecutionStatus::Success);

        let view = trace.sequential_view();
        assert_eq!(view.len(), 4);
        assert_eq!(view[0].0, 0);
        assert_eq!(view[1].0, 1);
        assert_eq!(view[2].0, 2);
        assert_eq!(view[3].0, 1);
    }

    #[test]
    fn test_find_step_mut() {
        let mut trace = ExecutionTrace::new(AgentId::generate());
        let root = trace.add_root_step("root");
        let id = root.id.clone();
        trace
            .find_step_mut(&id)
            .unwrap()
            .log(LogLevel::Info, "hello");
        assert_eq!(trace.root_step().unwrap().logs.len(), 1);
    }

    #[test]
    fn test_parent_step_lookup() {
        let mut trace = ExecutionTrace::new(AgentId::generate());
        let root = trace.add_root_step("root");
        let root_id = root.id.clone();
        let child = trace.add_child_step(&root_id, "child").unwrap();
        let child_id = child.id.clone();

        assert_eq!(trace.parent_step(&child_id).unwrap().name, "root");
        assert!(trace.parent_step(&root_id).is_none());
    }

    #[test]
    fn test_metric_collection() {
        let mut trace = ExecutionTrace::new(AgentId::generate());
        trace.add_metric("tokens", 123.0);
        assert_eq!(trace.metrics.len(), 1);
        assert!((trace.metrics[0].value - 123.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_trace_roundtrip_serialization() {
        let mut trace = ExecutionTrace::new(AgentId::generate());
        let root = trace.add_root_step("root");
        let root_id = root.id.clone();
        trace.add_child_step(&root_id, "child").unwrap();
        trace.finish(ExecutionStatus::Success);

        let json = serde_json::to_string(&trace).unwrap();
        let decoded: ExecutionTrace = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.status, ExecutionStatus::Success);
        assert_eq!(decoded.steps.len(), 2);
        assert_eq!(decoded.metrics.len(), 0);
    }

    #[test]
    fn test_add_child_step_rejects_unknown_parent() {
        let mut trace = ExecutionTrace::new(AgentId::generate());
        assert!(trace.add_child_step("unknown", "child").is_none());
    }
}
