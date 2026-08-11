use std::collections::HashMap;
use std::sync::Arc;

use goble_core::agent::{AgentId, AgentSpec};
use goble_core::execution::{ExecutionStatus, ExecutionTrace, LogLevel};
use goble_core::llm::LlmProvider;
use goble_core::mcp_installer::McpInstaller;
use goble_core::workspace::Workspace;

use crate::agent_runtime::AgentRuntime;
use crate::state::AppState;

pub type ProviderFactory = Box<dyn Fn() -> anyhow::Result<Box<dyn LlmProvider>> + Send + Sync>;

pub struct Runner {
    state: Arc<AppState>,
    installer: McpInstaller,
    provider_factory: ProviderFactory,
}

impl Runner {
    pub fn new(state: Arc<AppState>) -> Self {
        let installer = McpInstaller::new(state.config.lock().workspace_root.join("cache"));
        let factory = default_provider_factory(state.clone());
        Self {
            state,
            installer,
            provider_factory: factory,
        }
    }

    pub fn new_with_provider_factory(
        state: Arc<AppState>,
        factory: ProviderFactory,
    ) -> Self {
        let installer = McpInstaller::new(state.config.lock().workspace_root.join("cache"));
        Self {
            state,
            installer,
            provider_factory: factory,
        }
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
        self.state.emit(goble_core::protocol::WorkerMessage::AgentStarted {
            trace_id: trace_id.clone(),
            agent_id: agent_id.clone(),
        });

        let workspace = Workspace::new(agent_id.clone(), &self.state.config.lock().workspace_root);
        let _ = workspace.ensure_exists();

        let root_id = trace.add_root_step("execute agent").id.clone();
        self.state.store_trace(trace.clone());

        let install_id = trace
            .add_child_step(&root_id, "install mcp servers")
            .unwrap()
            .id
            .clone();
        let mcp_servers = self.state.mcp_servers.lock().clone();
        let mut installed = HashMap::new();
        for id in &spec.mcp_ids {
            if let Some(server) = mcp_servers.get(id) {
                match self.installer.install(server).await {
                    Ok(mcp) => {
                        trace.find_step_mut(&install_id).unwrap().log(
                            LogLevel::Info,
                            format!("installed {} at {}", mcp.id, mcp.path.display()),
                        );
                        installed.insert(mcp.id.clone(), (mcp, server.clone()));
                    }
                    Err(e) => {
                        trace
                            .find_step_mut(&install_id)
                            .unwrap()
                            .log(LogLevel::Error, format!("failed to install {}: {}", id, e));
                    }
                }
            } else {
                trace
                    .find_step_mut(&install_id)
                    .unwrap()
                    .log(LogLevel::Warn, format!("mcp server {} not registered", id));
            }
        }
        trace
            .find_step_mut(&install_id)
            .unwrap()
            .finish(ExecutionStatus::Success);
        self.state.store_trace(trace.clone());

        let mcp_id = trace
            .add_child_step(&root_id, "connect mcp servers")
            .unwrap()
            .id
            .clone();
        let secrets = self.state.secrets.lock().clone();
        for (id, (mcp, server)) in &installed {
            let mut env = server
                .credentials_key
                .as_ref()
                .and_then(|s| {
                    serde_json::from_str::<std::collections::HashMap<String, String>>(s).ok()
                })
                .unwrap_or_default()
                .iter()
                .filter_map(|(env_name, secret_id)| {
                    secrets.get(secret_id).map(|secret| {
                        let value = String::from_utf8_lossy(&secret.encrypted_value).to_string();
                        (env_name.clone(), value)
                    })
                })
                .collect::<std::collections::HashMap<String, String>>();
            env.insert(
                "GOBLIN_AGENT_WORKSPACE".to_string(),
                workspace.path.to_string_lossy().to_string(),
            );
            match mcp.start_client(env) {
                Ok(client) => match client.initialize() {
                    Ok(_) => {
                        trace
                            .find_step_mut(&mcp_id)
                            .unwrap()
                            .log(LogLevel::Info, format!("mcp {} initialized", id));
                        match client.list_tools() {
                            Ok(tools) => {
                                trace
                                    .find_step_mut(&mcp_id)
                                    .unwrap()
                                    .log(LogLevel::Info, format!("mcp {} tools: {:?}", id, tools));
                            }
                            Err(e) => {
                                trace.find_step_mut(&mcp_id).unwrap().log(
                                    LogLevel::Warn,
                                    format!("mcp {} list_tools failed: {}", id, e),
                                );
                            }
                        }
                    }
                    Err(e) => {
                        trace.find_step_mut(&mcp_id).unwrap().log(
                            LogLevel::Error,
                            format!("mcp {} initialize failed: {}", id, e),
                        );
                    }
                },
                Err(e) => {
                    trace
                        .find_step_mut(&mcp_id)
                        .unwrap()
                        .log(LogLevel::Error, format!("mcp {} start failed: {}", id, e));
                }
            }
        }
        trace
            .find_step_mut(&mcp_id)
            .unwrap()
            .finish(ExecutionStatus::Success);
        self.state.store_trace(trace.clone());

        let provider = (self.provider_factory)()?;
        let runtime = AgentRuntime::new(self.state.clone());
        let (_trace, _summary) = runtime
            .run(trace_id, agent_id, spec, None, provider)
            .await?;
        Ok(())
    }

    /// Run agent logic against the configured LLM provider and return a reply string.
    pub async fn run_agent_for_thread_reply(
        &self,
        trace_id: String,
        agent_id: AgentId,
        spec: AgentSpec,
        prompt: String,
    ) -> anyhow::Result<String> {
        let provider = (self.provider_factory)()?;
        let runtime = AgentRuntime::new(self.state.clone());
        let (_trace, summary): (ExecutionTrace, Option<String>) = runtime
            .run(trace_id, agent_id, spec, Some(prompt), provider)
            .await?;
        Ok(summary.unwrap_or_else(|| "no reply".into()))
    }

    pub async fn run_team(&self, trace_id: String, team_id: String) -> anyhow::Result<()> {
        let mut trace = ExecutionTrace::new(AgentId(team_id.clone()));
        trace.id = trace_id.clone();
        trace.worker_id = Some(self.state.worker_id.clone());
        trace.status = ExecutionStatus::Running;
        self.state.store_trace(trace.clone());
        self.state.emit(goble_core::protocol::WorkerMessage::AgentStarted {
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

fn default_provider_factory(
    state: Arc<AppState>,
) -> ProviderFactory {
    Box::new(move || {
        let secrets = state.secrets.lock().clone();
        let key = secrets
            .get("llm_api_key")
            .and_then(|s| String::from_utf8(s.encrypted_value.clone()).ok())
            .ok_or_else(|| anyhow::anyhow!("no llm_api_key secret available"))?;
        let provider = std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "openai".into());
        Ok(goble_core::llm::create_provider(&provider, &key, None))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::worker::WorkerId;

    fn mock_factory() -> ProviderFactory {
        Box::new(|| {
            Ok(goble_core::llm::create_provider(
                "mock",
                "test-key",
                None,
            ))
        })
    }

    #[tokio::test]
    async fn test_run_agent_success() {
        let state = AppState::new(WorkerId::generate());
        let runner = Runner::new_with_provider_factory(state.clone(), mock_factory());
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

    #[tokio::test]
    async fn test_agent_workspaces_are_isolated() {
        use std::path::Path;
        let state = AppState::new(WorkerId::generate());
        let spec_a = AgentSpec::new("agent-a", "agent a");
        let spec_b = AgentSpec::new("agent-b", "agent b");
        let ws_a = Workspace::new(spec_a.id.clone(), &state.config.lock().workspace_root);
        let ws_b = Workspace::new(spec_b.id.clone(), &state.config.lock().workspace_root);
        ws_a.ensure_exists().unwrap();
        ws_b.ensure_exists().unwrap();
        std::fs::write(ws_a.path.join("secret.txt"), "agent-a-only").unwrap();
        assert!(Path::new(&ws_a.path.join("secret.txt")).exists());
        assert!(!Path::new(&ws_b.path.join("secret.txt")).exists());
        assert_ne!(ws_a.path, ws_b.path);
    }
}
