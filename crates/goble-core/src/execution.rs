use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::agent::AgentId;
use crate::worker::WorkerId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub id: String,
    pub agent_id: AgentId,
    pub worker_id: Option<WorkerId>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub status: ExecutionStatus,
    pub steps: Vec<Step>,
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
        }
    }

    pub fn add_step(&mut self, name: impl Into<String>) -> &mut Step {
        let step = Step {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            started_at: Utc::now(),
            finished_at: None,
            status: ExecutionStatus::Pending,
            logs: Vec::new(),
        };
        self.steps.push(step);
        self.steps.last_mut().unwrap()
    }

    pub fn finish(&mut self, status: ExecutionStatus) {
        self.finished_at = Some(Utc::now());
        self.status = status;
    }
}

impl Step {
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
