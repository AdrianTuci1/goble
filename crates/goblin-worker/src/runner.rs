use std::sync::Arc;

use goble_core::agent::{AgentId, AgentSpec, McpServer};
use goble_core::agent_memory::{merge_compaction, CompactionResult, COMPACTION_PROMPT};
use goble_core::execution::{ExecutionStatus, ExecutionTrace};
use goble_core::harness::{harness_sandbox, SandboxedCommandRunner};
use goble_core::llm::{CompletionRequest, LlmProvider};
use goble_core::mcp_manager::McpManager;
use goble_core::protocol::WorkerMessage;
use goble_core::secret::Secret;
use goble_core::store::Store;
use goble_daemon_protocol::DaemonEvent;
use goble_harness_internal::InternalHarness;
use goble_harness_types::{HarnessId, HarnessTurn, MediumId, ProjectId, SessionId};
use tokio::sync::broadcast;

use crate::agent_memory::{injector, loader};
use crate::llm_factory::default_provider_factory;
use crate::state::AppState;

pub type ProviderFactory = Box<dyn Fn() -> anyhow::Result<Arc<dyn LlmProvider>> + Send + Sync>;

/// Chats longer than this many messages trigger a structured compaction pass
/// that folds the overflow into the agent's persistent memory.
const COMPACT_THRESHOLD: usize = 40;

/// How many of the most recent chat messages are injected as the transcript tail.
const TRANSCRIPT_TAIL_MESSAGES: usize = 20;

/// Drives a single agent (or a team of agents) through the embedded daemon.
///
/// The runner owns the worker-side trace bookkeeping (`ExecutionTrace`,
/// `AgentStarted`/`AgentFinished`) and the provider resolution; the actual
/// harness execution runs inside `goble-daemon`, which owns the harness
/// registry, the per-session execution ledger and the event stream.
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

    /// The configured model name, defaulting to a small chat model when unset.
    fn model_name(&self) -> String {
        self.state
            .config
            .lock()
            .llm_model
            .clone()
            .unwrap_or_else(|| "gpt-4o-mini".into())
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
        let model_name = self.model_name();
        let store = self.state.store()?;
        let chat_id = trace_id.clone();
        let (_harness_id, turn) = self
            .prepare_run(
                &store,
                &trace_id,
                &agent_id,
                &spec,
                &mcp_servers,
                &secrets,
                provider.clone(),
                &model_name,
            )
            .await?;
        let session_id = turn.session_id.clone();

        self.begin_trace(&trace_id, &agent_id);
        let client = self.state.daemon();
        let mut rx = client.subscribe();
        client.run(turn)?;
        let status = await_turn(&mut rx, &session_id).await;
        self.finish_trace(&trace_id, status.clone());

        self.maybe_compact(&provider, &model_name, &store, &chat_id, &agent_id)
            .await;
        Ok(())
    }

    pub async fn run_agent_for_thread_reply(
        &self,
        trace_id: String,
        agent_id: AgentId,
        spec: AgentSpec,
        _prompt: String,
        mcp_servers: Vec<McpServer>,
        secrets: Vec<Secret>,
    ) -> anyhow::Result<String> {
        self.run_agent(trace_id, agent_id, spec, mcp_servers, secrets)
            .await?;
        Ok("reply submitted".to_string())
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
            if let Err(e) = self
                .run_agent(
                    sub_trace_id,
                    agent_id,
                    spec,
                    mcp_servers.clone(),
                    secrets.clone(),
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

    /// Persist the run's secrets and MCP servers into the store, build the agent
    /// prompt (identity + memory + transcript tail), materialize the per-run
    /// agent workspace, and register a fresh [`InternalHarness`] in the daemon.
    /// Returns the registered harness id and the [`HarnessTurn`] to drive.
    async fn prepare_run(
        &self,
        store: &Store,
        trace_id: &str,
        agent_id: &AgentId,
        spec: &AgentSpec,
        mcp_servers: &[McpServer],
        secrets: &[Secret],
        provider: Arc<dyn LlmProvider>,
        model_name: &str,
    ) -> anyhow::Result<(HarnessId, HarnessTurn)> {
        // Persist secrets so harness tools (e.g. MCP auth) resolve them.
        for secret in secrets {
            store.insert_vault_secret(&secret.name, &secret.encrypted_value, "", "")?;
        }

        // Install/register MCP servers in the store.
        let mcp_manager = McpManager::new();
        for server in mcp_servers {
            let source_str = match &server.source {
                goble_core::agent::McpSource::Github { .. } => "github",
                goble_core::agent::McpSource::Npm { .. } => "npm",
                goble_core::agent::McpSource::Local { .. } => "local",
                goble_core::agent::McpSource::Url { .. } => "url",
            };
            let source_value = match &server.source {
                goble_core::agent::McpSource::Github { repo, rev } => {
                    Some(format!("{repo}#{rev}"))
                }
                goble_core::agent::McpSource::Npm { package, version } => {
                    Some(format!("{package}@{version}"))
                }
                goble_core::agent::McpSource::Local { path } => Some(path.clone()),
                goble_core::agent::McpSource::Url { url } => Some(url.clone()),
            };
            let _ = mcp_manager
                .install_mcp_server(
                    store,
                    &server.id,
                    &server.name,
                    source_str,
                    source_value.as_deref(),
                    &[],
                    Some(server.manifest.clone()),
                )
                .await;
        }

        // Build the prompt as identity + memory + transcript tail.
        let memory = loader::load_or_create(store, agent_id, &spec.prompt)?;
        let tail = injector::transcript_tail(store, trace_id, TRANSCRIPT_TAIL_MESSAGES)?;
        let prompt = injector::build_context(&spec.prompt, &memory, &tail);

        let workspace_dir = self
            .state
            .config
            .lock()
            .workspace_root
            .join("harness")
            .join(trace_id);
        std::fs::create_dir_all(&workspace_dir)?;

        let provider_name = provider.name().to_string();
        let harness_id = HarnessId::new(format!("internal-{trace_id}"));
        let harness = InternalHarness::new(store.clone())
            .with_id(harness_id.clone())
            .with_llm(provider)
            .with_provider(provider_name)
            .with_model(model_name.to_string())
            .with_reasoning(false)
            .with_runner(Arc::new(
                SandboxedCommandRunner::default_tools()
                    .with_sandbox(harness_sandbox()),
            ))
            .with_workspace_dir(&workspace_dir);
        self.state.daemon_state().register(Arc::new(harness));

        let mut turn = HarnessTurn::new(harness_id.clone(), SessionId::new(trace_id), prompt);
        turn.project_id = ProjectId::new("default");
        turn.medium_id = MediumId::new("local");
        Ok((harness_id, turn))
    }

    /// Record the running trace and emit `AgentStarted` (mirrors the old
    /// harness-runner preamble).
    fn begin_trace(&self, trace_id: &str, agent_id: &AgentId) {
        let mut trace = ExecutionTrace::new(agent_id.clone());
        trace.id = trace_id.to_string();
        trace.worker_id = Some(self.state.worker_id.clone());
        trace.status = ExecutionStatus::Running;
        self.state.store_trace(trace);
        self.state.emit(WorkerMessage::AgentStarted {
            trace_id: trace_id.to_string(),
            agent_id: agent_id.clone(),
        });
    }

    /// Update the trace's terminal status and emit `AgentFinished`.
    fn finish_trace(&self, trace_id: &str, status: ExecutionStatus) {
        self.state.update_trace(trace_id, |t| t.status = status.clone());
        self.state.emit(WorkerMessage::AgentFinished {
            trace_id: trace_id.to_string(),
            status,
        });
    }

    /// Compact the overflowing transcript into the agent's canonical memory after
    /// a long run (mirrors the harness-runner post-pass). A no-op unless the
    /// transcript exceeds [`COMPACT_THRESHOLD`].
    async fn maybe_compact(
        &self,
        provider: &Arc<dyn LlmProvider>,
        model_name: &str,
        store: &Store,
        chat_id: &str,
        agent_id: &AgentId,
    ) {
        if !injector::should_compact(store, chat_id, COMPACT_THRESHOLD).unwrap_or(false) {
            return;
        }
        if let Some(result) =
            run_compaction_turn(&self.state, provider, model_name, store, chat_id).await
        {
            if let Ok(Some(mut memory)) = store.get_agent_memory(&agent_id.0) {
                merge_compaction(&mut memory, result);
                let _ = store.put_agent_memory(&memory);
            }
        }
    }
}

/// Wait for a daemon turn to settle, returning the worker-side `ExecutionStatus`.
///
/// A turn settles on `TraceFinished`; a turn that paused on an `AskUser` reports
/// success (matching the harness-runner, which paused and finished on an ask).
async fn await_turn(
    rx: &mut broadcast::Receiver<DaemonEvent>,
    sid: &SessionId,
) -> ExecutionStatus {
    while let Ok(event) = rx.recv().await {
        match &event {
            DaemonEvent::TraceFinished {
                session_id,
                status,
                ..
            } if session_id == sid => {
                return if status == "success" {
                    ExecutionStatus::Success
                } else {
                    ExecutionStatus::Failure("harness did not finish".to_string())
                };
            }
            DaemonEvent::AskUser { session_id, .. } if session_id == sid => {
                return ExecutionStatus::Success;
            }
            _ => {}
        }
    }
    ExecutionStatus::Failure("event stream closed".to_string())
}

/// Run a single non-streaming compaction turn against the full transcript and
/// parse the structured [`CompactionResult`] the model returns.
async fn run_compaction_turn(
    state: &Arc<AppState>,
    provider: &Arc<dyn LlmProvider>,
    model: &str,
    store: &Store,
    chat_id: &str,
) -> Option<CompactionResult> {
    let transcript = match injector::transcript_tail(store, chat_id, 200) {
        Ok(t) => t,
        Err(_) => return None,
    };
    if transcript.trim().is_empty() {
        return None;
    }

    let request = CompletionRequest::new(provider.name().to_string(), model.to_string())
        .with_system(COMPACTION_PROMPT)
        .with_user(transcript);

    let response = match provider.complete(request).await {
        Ok(r) => r,
        Err(e) => {
            state.emit(WorkerMessage::AgentLog {
                trace_id: chat_id.to_string(),
                step_id: "compaction".to_string(),
                level: goble_core::execution::LogLevel::Error,
                message: format!("compaction failed: {e}"),
            });
            return None;
        }
    };

    let text = response.content.trim();
    let json_text = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .map(|s| s.trim().trim_end_matches("```").trim())
        .unwrap_or(text);

    match serde_json::from_str::<CompactionResult>(json_text) {
        Ok(result) => Some(result),
        Err(e) => {
            state.emit(WorkerMessage::AgentLog {
                trace_id: chat_id.to_string(),
                step_id: "compaction".to_string(),
                level: goble_core::execution::LogLevel::Error,
                message: format!("compaction parse failed: {e}"),
            });
            None
        }
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
        state.config.lock().workspace_root = tmp_state.path().join("workspaces");
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
