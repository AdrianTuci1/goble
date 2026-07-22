use std::sync::Arc;

use goble_core::agent::{AgentId, Trigger};
use goble_core::protocol::WorkerMessage;

use crate::runner::Runner;
use crate::state::AppState;

pub struct Scheduler {
    state: Arc<AppState>,
    runner: Runner,
}

impl Scheduler {
    pub fn new(state: Arc<AppState>) -> Self {
        let runner = Runner::new(state.clone());
        Self { state, runner }
    }

    pub async fn trigger_agent(&self, agent_id: AgentId) -> anyhow::Result<String> {
        let spec = self
            .state
            .agents
            .lock()
            .get(&agent_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("agent not found"))?;
        let trace_id = uuid::Uuid::new_v4().to_string();
        self.runner
            .run_agent(trace_id.clone(), agent_id, spec)
            .await?;
        Ok(trace_id)
    }

    pub async fn handle_trigger(
        &self,
        agent_id: AgentId,
        trigger: Trigger,
    ) -> anyhow::Result<String> {
        match trigger {
            Trigger::Manual => self.trigger_agent(agent_id).await,
            Trigger::Http { path } => {
                let trace_id = uuid::Uuid::new_v4().to_string();
                let spec = self
                    .state
                    .agents
                    .lock()
                    .get(&agent_id)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("agent not found"))?;
                self.runner
                    .run_agent(trace_id.clone(), agent_id, spec)
                    .await?;
                self.state.emit(WorkerMessage::AgentLog {
                    trace_id: trace_id.clone(),
                    step_id: "http".to_string(),
                    level: goble_core::execution::LogLevel::Info,
                    message: format!("triggered via HTTP {}", path),
                });
                Ok(trace_id)
            }
            Trigger::Cron { expression } => {
                let trace_id = uuid::Uuid::new_v4().to_string();
                self.state.emit(WorkerMessage::AgentLog {
                    trace_id: trace_id.clone(),
                    step_id: "cron".to_string(),
                    level: goble_core::execution::LogLevel::Info,
                    message: format!("scheduled with cron: {}", expression),
                });
                Ok(trace_id)
            }
            Trigger::Heartbeat { interval_seconds } => {
                let trace_id = uuid::Uuid::new_v4().to_string();
                self.state.emit(WorkerMessage::AgentLog {
                    trace_id: trace_id.clone(),
                    step_id: "heartbeat".to_string(),
                    level: goble_core::execution::LogLevel::Info,
                    message: format!("heartbeat trigger every {}s", interval_seconds),
                });
                Ok(trace_id)
            }
        }
    }

    pub fn start_heartbeat_loop(self: Arc<Self>, interval: std::time::Duration) {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                self.state.emit(WorkerMessage::StatusReport {
                    worker_id: self.state.worker_id.clone(),
                    status: goble_core::worker::WorkerStatus::Online,
                    load: 0,
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::agent::AgentSpec;
    use goble_core::worker::WorkerId;

    #[tokio::test]
    async fn test_manual_trigger_finds_agent() {
        let state = AppState::new(WorkerId::generate());
        let scheduler = Scheduler::new(state.clone());
        let spec = AgentSpec::new("demo", "do nothing");
        let id = spec.id.clone();
        state.store_agent(spec);
        let trace_id = scheduler.trigger_agent(id).await.unwrap();
        assert!(!trace_id.is_empty());
    }

    #[tokio::test]
    async fn test_manual_trigger_missing_agent_fails() {
        let state = AppState::new(WorkerId::generate());
        let scheduler = Scheduler::new(state);
        let result = scheduler.trigger_agent(AgentId::generate()).await;
        assert!(result.is_err());
    }
}
