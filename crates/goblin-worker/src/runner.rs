use std::sync::Arc;

use goble_core::agent::{AgentId, AgentSpec};
use goble_core::execution::{ExecutionStatus, LogLevel};
use goble_core::workspace::Workspace;

use crate::state::AppState;

pub struct Runner {
    state: Arc<AppState>,
}

impl Runner {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    pub async fn run_agent(
        &self,
        trace_id: String,
        agent_id: AgentId,
        spec: AgentSpec,
    ) -> anyhow::Result<()> {
        let mut trace = goble_core::execution::ExecutionTrace::new(agent_id.clone());
        trace.id = trace_id.clone();
        trace.worker_id = Some(self.state.worker_id.clone());
        trace.status = ExecutionStatus::Running;
        self.state.store_trace(trace.clone());

        self.state
            .emit(goble_core::protocol::WorkerMessage::AgentStarted {
                trace_id: trace_id.clone(),
                agent_id: agent_id.clone(),
            });

        let workspace = Workspace::new(agent_id.clone(), &self.state.config.lock().workspace_root);
        let _ = workspace.ensure_exists();

        let setup_step = trace.add_step("prepare workspace");
        setup_step.log(
            LogLevel::Info,
            format!("workspace ready at {}", workspace.path.display()),
        );
        setup_step.log(LogLevel::Info, format!("agent prompt: {}", spec.prompt));
        setup_step.finish(ExecutionStatus::Success);
        self.state.store_trace(trace.clone());

        let mcp_step = trace.add_step("attach mcp servers");
        let mcp_ids = spec.mcp_ids.clone();
        let mcp_servers = self.state.mcp_servers.lock().clone();
        for id in &mcp_ids {
            if let Some(server) = mcp_servers.get(id) {
                mcp_step.log(LogLevel::Info, format!("attached {}", server.name));
            } else {
                mcp_step.log(LogLevel::Warn, format!("mcp server {} not found", id));
            }
        }
        mcp_step.finish(ExecutionStatus::Success);
        self.state.store_trace(trace.clone());

        let run_step = trace.add_step("execute agent logic");
        run_step.log(LogLevel::Info, "starting agent runtime");
        run_step.log(LogLevel::Info, format!("tools: {:?}", spec.tools));
        run_step.finish(ExecutionStatus::Success);
        self.state.store_trace(trace.clone());

        trace.finish(ExecutionStatus::Success);
        self.state.store_trace(trace.clone());
        self.state
            .emit(goble_core::protocol::WorkerMessage::AgentFinished {
                trace_id,
                status: ExecutionStatus::Success,
            });
        Ok(())
    }

    pub async fn run_team(&self, trace_id: String, team_id: String) -> anyhow::Result<()> {
        let mut trace = goble_core::execution::ExecutionTrace::new(AgentId(team_id.clone()));
        trace.id = trace_id.clone();
        trace.worker_id = Some(self.state.worker_id.clone());
        trace.status = ExecutionStatus::Running;
        self.state.store_trace(trace.clone());

        self.state
            .emit(goble_core::protocol::WorkerMessage::AgentStarted {
                trace_id: trace_id.clone(),
                agent_id: AgentId(team_id.clone()),
            });

        let step = trace.add_step("run team");
        step.log(LogLevel::Info, format!("running team {}", team_id));
        step.finish(ExecutionStatus::Success);
        trace.finish(ExecutionStatus::Success);
        self.state.store_trace(trace.clone());
        self.state
            .emit(goble_core::protocol::WorkerMessage::AgentFinished {
                trace_id,
                status: ExecutionStatus::Success,
            });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::worker::WorkerId;

    #[tokio::test]
    async fn test_run_agent_success() {
        let state = AppState::new(WorkerId::generate());
        let runner = Runner::new(state.clone());
        let spec = AgentSpec::new("demo", "do nothing");
        let id = spec.id.clone();
        runner
            .run_agent("trace-1".to_string(), id, spec)
            .await
            .unwrap();
        assert!(state.get_trace("trace-1").is_some());
    }
}
