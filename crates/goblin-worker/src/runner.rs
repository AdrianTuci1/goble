use std::sync::Arc;

use goble_core::agent::{AgentId, AgentSpec, McpServer};
use goble_core::execution::{ExecutionStatus, ExecutionTrace};
use goble_core::llm::LlmProvider;
use goble_core::protocol::WorkerMessage;
use goble_core::secret::Secret;

use crate::harness_runner;
use crate::llm_factory::default_provider_factory;
use crate::state::AppState;

pub type ProviderFactory = Box<dyn Fn() -> anyhow::Result<Arc<dyn LlmProvider>> + Send + Sync>;

pub struct Runner {
    state: Arc<AppState>,
    provider_factory: ProviderFactory,
}

impl Runner {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state: state.clone(),
            provider_factory: Box::new(move || {
                let secrets = state.secrets.lock().clone();
                default_provider_factory(secrets)
            }),
        }
    }

    pub fn new_with_mock_provider(state: Arc<AppState>) -> Self {
        Self {
            state,
            provider_factory: Box::new(|| {
                Ok(std::sync::Arc::new(goble_core::llm::MockProvider::new(
                    "mock",
                    goble_core::llm::CompletionResponse {
                        content: "ok".to_string(),
                        tool_calls: vec![],
                    },
                )))
            }),
        }
    }

    pub fn new_with_provider_factory(state: Arc<AppState>, factory: ProviderFactory) -> Self {
        Self {
            state,
            provider_factory: factory,
        }
    }

    pub async fn run_agent(
        &self,
        trace_id: String,
        agent_id: AgentId,
        spec: AgentSpec,
        mcp_servers: Vec<McpServer>,
        secrets: Vec<Secret>,
    ) -> anyhow::Result<()> {
        let provider = (self.provider_factory)()?;
        let model_name = self
            .state
            .config
            .lock()
            .llm_model
            .clone()
            .unwrap_or_else(|| "gpt-4o-mini".into());
        harness_runner::run_agent_with_harness(
            self.state.clone(),
            trace_id,
            agent_id,
            spec,
            mcp_servers,
            secrets,
            provider,
            &model_name,
        )
        .await
    }

    pub async fn run_agent_for_thread_reply(
        &self,
        trace_id: String,
        agent_id: AgentId,
        spec: AgentSpec,
        prompt: String,
        mcp_servers: Vec<McpServer>,
        secrets: Vec<Secret>,
    ) -> anyhow::Result<String> {
        let provider = (self.provider_factory)()?;
        let model_name = self
            .state
            .config
            .lock()
            .llm_model
            .clone()
            .unwrap_or_else(|| "gpt-4o-mini".into());
        let content = harness_runner::run_agent_for_thread_reply_with_harness(
            self.state.clone(),
            trace_id,
            agent_id,
            spec,
            prompt,
            mcp_servers,
            secrets,
            provider,
            &model_name,
        )
        .await?;
        Ok(content)
    }

    pub async fn run_team(&self, trace_id: String, team_id: String) -> anyhow::Result<()> {
        let store = self.state.store()?;
        let team_members = store.list_team_members(&team_id)?;
        if team_members.is_empty() {
            anyhow::bail!("team {} has no members", team_id);
        }

        let mut trace = ExecutionTrace::new(AgentId(team_id.clone()));
        trace.id = trace_id.clone();
        trace.worker_id = Some(self.state.worker_id.clone());
        trace.status = ExecutionStatus::Running;
        self.state.store_trace(trace.clone());
        self.state.emit(WorkerMessage::AgentStarted {
            trace_id: trace_id.clone(),
            agent_id: AgentId(team_id.clone()),
        });

        let root_id = trace
            .add_root_step(format!("run team {}", team_id))
            .id
            .clone();
        trace.find_step_mut(&root_id).unwrap().log(
            goble_core::execution::LogLevel::Info,
            format!(
                "running team {} with {} members",
                team_id,
                team_members.len()
            ),
        );
        self.state.store_trace(trace.clone());

        let mcp_servers: Vec<McpServer> = self.state.mcp_servers.lock().values().cloned().collect();
        let secrets: Vec<Secret> = self.state.secrets.lock().values().cloned().collect();
        let model_name = self
            .state
            .config
            .lock()
            .llm_model
            .clone()
            .unwrap_or_else(|| "gpt-4o-mini".into());

        for (_, agent_id_str) in team_members {
            let agent_row = store.get_agent(&agent_id_str)?;
            let spec = match agent_row {
                Some((_, _, spec_json, _, _)) => serde_json::from_str::<AgentSpec>(&spec_json)
                    .map_err(|e| anyhow::anyhow!("failed to parse agent spec: {e}"))?,
                None => {
                    self.state.update_trace(&trace_id, |t| {
                        t.find_step_mut(&root_id).unwrap().log(
                            goble_core::execution::LogLevel::Error,
                            format!("agent {} not found in store", agent_id_str),
                        );
                    });
                    continue;
                }
            };
            let agent_id = AgentId(agent_id_str);
            let sub_trace_id = uuid::Uuid::new_v4().to_string();
            let provider = (self.provider_factory)()?;
            if let Err(e) = harness_runner::run_agent_with_harness(
                self.state.clone(),
                sub_trace_id,
                agent_id,
                spec,
                mcp_servers.clone(),
                secrets.clone(),
                provider,
                &model_name,
            )
            .await
            {
                self.state.update_trace(&trace_id, |t| {
                    t.find_step_mut(&root_id).unwrap().log(
                        goble_core::execution::LogLevel::Error,
                        format!("member run failed: {e}"),
                    );
                });
            }
        }

        trace
            .find_step_mut(&root_id)
            .unwrap()
            .finish(ExecutionStatus::Success);
        trace.finish(ExecutionStatus::Success);
        self.state.store_trace(trace.clone());
        self.state.emit(WorkerMessage::AgentFinished {
            trace_id,
            status: ExecutionStatus::Success,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use goble_core::worker::WorkerId;
    use tempfile::TempDir;

    fn test_state() -> (TempDir, Arc<AppState>) {
        let state = AppState::new(WorkerId::generate());
        let tmp_state = tempfile::tempdir().unwrap();
        state
            .set_store_path(tmp_state.path().join("worker.db"))
            .unwrap();
        (tmp_state, state)
    }

    #[tokio::test]
    async fn test_run_agent_success() {
        let (_tmp, state) = test_state();
        let runner = Runner::new_with_mock_provider(state.clone());
        let spec = AgentSpec::new("demo", "do nothing");
        let id = spec.id.clone();
        runner
            .run_agent("trace-1".to_string(), id, spec, vec![], vec![])
            .await
            .unwrap();
        let trace = state.get_trace("trace-1").unwrap();
        assert_eq!(trace.status, ExecutionStatus::Success);
    }

    #[tokio::test]
    async fn test_run_team_success() {
        let (_tmp, state) = test_state();
        let store = state.store().unwrap();
        let spec = AgentSpec::new("member-1", "Member agent.");
        let agent_id = spec.id.clone();
        store
            .insert_agent(
                &agent_id.0,
                &spec.name,
                &serde_json::to_string(&spec).unwrap(),
                &spec.created_at,
                &spec.updated_at,
            )
            .unwrap();
        store
            .insert_team("team-1", "team", "{}", &Utc::now().to_rfc3339())
            .unwrap();
        store.insert_team_member("team-1", &agent_id.0).unwrap();

        let runner = Runner::new_with_mock_provider(state.clone());
        runner
            .run_team("team-trace".to_string(), "team-1".to_string())
            .await
            .unwrap();
        let trace = state.get_trace("team-trace").unwrap();
        assert_eq!(trace.agent_id.0, "team-1");
        assert!(trace.root_step().is_some());
    }
}
