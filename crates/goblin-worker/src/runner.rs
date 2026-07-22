use std::sync::Arc;

use goble_core::agent::{AgentId, AgentSpec};
use goble_core::execution::{ExecutionStatus, ExecutionTrace, LogLevel};
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
        let mut trace = ExecutionTrace::new(agent_id.clone());
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

        let root_id = trace.add_root_step("execute agent").id.clone();
        self.state.store_trace(trace.clone());

        let setup_id = trace
            .add_child_step(&root_id, "prepare workspace")
            .unwrap()
            .id
            .clone();
        trace.find_step_mut(&setup_id).unwrap().log(
            LogLevel::Info,
            format!("workspace ready at {}", workspace.path.display()),
        );
        trace
            .find_step_mut(&setup_id)
            .unwrap()
            .log(LogLevel::Info, format!("agent prompt: {}", spec.prompt));
        trace
            .find_step_mut(&setup_id)
            .unwrap()
            .finish(ExecutionStatus::Success);
        self.state.store_trace(trace.clone());

        let mcp_id = trace
            .add_child_step(&root_id, "attach mcp servers")
            .unwrap()
            .id
            .clone();
        let mcp_ids = spec.mcp_ids.clone();
        let mcp_servers = self.state.mcp_servers.lock().clone();
        for id in &mcp_ids {
            let msg = if let Some(server) = mcp_servers.get(id) {
                format!("attached {}", server.name)
            } else {
                format!("mcp server {} not found", id)
            };
            let level = if mcp_servers.contains_key(id) {
                LogLevel::Info
            } else {
                LogLevel::Warn
            };
            trace.find_step_mut(&mcp_id).unwrap().log(level, msg);
        }
        trace
            .find_step_mut(&mcp_id)
            .unwrap()
            .finish(ExecutionStatus::Success);
        self.state.store_trace(trace.clone());

        let run_id = trace
            .add_child_step(&root_id, "execute agent logic")
            .unwrap()
            .id
            .clone();
        trace
            .find_step_mut(&run_id)
            .unwrap()
            .log(LogLevel::Info, "starting agent runtime");
        trace
            .find_step_mut(&run_id)
            .unwrap()
            .log(LogLevel::Info, format!("tools: {:?}", spec.tools));
        trace
            .find_step_mut(&run_id)
            .unwrap()
            .finish(ExecutionStatus::Success);
        self.state.store_trace(trace.clone());

        trace
            .find_step_mut(&root_id)
            .unwrap()
            .finish(ExecutionStatus::Success);
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
        let mut trace = ExecutionTrace::new(AgentId(team_id.clone()));
        trace.id = trace_id.clone();
        trace.worker_id = Some(self.state.worker_id.clone());
        trace.status = ExecutionStatus::Running;
        self.state.store_trace(trace.clone());

        self.state
            .emit(goble_core::protocol::WorkerMessage::AgentStarted {
                trace_id: trace_id.clone(),
                agent_id: AgentId(team_id.clone()),
            });

        let root_id = trace
            .add_root_step(format!("run team {}", team_id))
            .id
            .clone();
        trace
            .find_step_mut(&root_id)
            .unwrap()
            .log(LogLevel::Info, format!("running team {}", team_id));
        trace
            .find_step_mut(&root_id)
            .unwrap()
            .finish(ExecutionStatus::Success);
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
        let trace = state.get_trace("trace-1").unwrap();
        assert!(trace.root_step().is_some());
        assert!(!trace.sequential_view().is_empty());
    }

    #[tokio::test]
    async fn test_run_team_success() {
        let state = AppState::new(WorkerId::generate());
        let runner = Runner::new(state.clone());
        runner
            .run_team("team-trace".to_string(), "team-1".to_string())
            .await
            .unwrap();
        let trace = state.get_trace("team-trace").unwrap();
        assert_eq!(trace.agent_id.0, "team-1");
        assert!(trace.root_step().is_some());
    }
}
