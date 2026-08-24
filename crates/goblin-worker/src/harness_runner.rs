use std::sync::Arc;

use futures::StreamExt;
use goble_core::agent::{AgentId, AgentSpec, McpServer};
use goble_core::agent_memory::{merge_compaction, CompactionResult, COMPACTION_PROMPT};
use goble_core::execution::{ExecutionStatus, ExecutionTrace};
use goble_core::harness::{Harness, HarnessEvent, SandboxedCommandRunner};
use goble_core::llm::LlmProvider;
use goble_core::mcp_manager::McpManager;
use goble_core::protocol::WorkerMessage;
use goble_core::secret::Secret;
use goble_core::store::Store;

use crate::agent_memory::{injector, loader};
use crate::state::AppState;

/// Chats longer than this many messages trigger a structured compaction pass
/// that folds the overflow into the agent's persistent memory.
const COMPACT_THRESHOLD: usize = 40;

/// How many of the most recent chat messages are injected as the transcript tail.
const TRANSCRIPT_TAIL_MESSAGES: usize = 20;

/// Run an agent using the full `Harness` runtime instead of the minimal `AgentRuntime`.
pub async fn run_agent_with_harness(
    state: Arc<AppState>,
    trace_id: String,
    agent_id: AgentId,
    spec: AgentSpec,
    mcp_servers: Vec<McpServer>,
    secrets: Vec<Secret>,
    provider: Arc<dyn LlmProvider>,
    model: &str,
) -> anyhow::Result<()> {
    let store = state.store()?;
    let chat_id = trace_id.clone();
    let provider_name = provider.name().to_string();

    // Persist secrets in the store so harness tools (e.g. MCP auth) can find them.
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
            goble_core::agent::McpSource::Github { repo, rev } => Some(format!("{repo}#{rev}")),
            goble_core::agent::McpSource::Npm { package, version } => {
                Some(format!("{package}@{version}"))
            }
            goble_core::agent::McpSource::Local { path } => Some(path.clone()),
            goble_core::agent::McpSource::Url { url } => Some(url.clone()),
        };
        let _ = mcp_manager
            .install_mcp_server(
                &store,
                &server.id,
                &server.name,
                source_str,
                source_value.as_deref(),
                &[],
                Some(server.manifest.clone()),
            )
            .await;
    }

    let workspace_dir = state
        .config
        .lock()
        .workspace_root
        .join("harness")
        .join(&trace_id);
    std::fs::create_dir_all(&workspace_dir)?;

    // Load (or seed) the agent's persistent memory and build the context as
    // identity + memory + transcript tail. The full memory block is always
    // injected so requirements survive transcript summarization.
    let memory = loader::load_or_create(&store, &agent_id, &spec.prompt)?;
    let tail = injector::transcript_tail(&store, &chat_id, TRANSCRIPT_TAIL_MESSAGES)?;
    let prompt = injector::build_context(&spec.prompt, &memory, &tail);

    let runner = Arc::new(SandboxedCommandRunner::default_tools());
    let harness = Harness::new(store.clone())
        .with_reasoning(false)
        .with_llm(provider.clone())
        .with_runner(runner)
        .with_workspace_dir(&workspace_dir)
        .with_mcp_manager(mcp_manager);

    let mut trace = ExecutionTrace::new(agent_id.clone());
    trace.id = trace_id.clone();
    trace.worker_id = Some(state.worker_id.clone());
    trace.status = ExecutionStatus::Running;
    state.store_trace(trace.clone());
    state.emit(WorkerMessage::AgentStarted {
        trace_id: trace_id.clone(),
        agent_id: agent_id.clone(),
    });

    let mut stream = harness.run_turn(&chat_id, &prompt, &provider_name, model);
    let mut finished = false;
    while let Some(event) = stream.next().await {
        match event {
            HarnessEvent::AssistantDelta(delta) => {
                state.emit(WorkerMessage::AssistantDelta {
                    trace_id: trace_id.clone(),
                    delta,
                });
            }
            HarnessEvent::ToolCallStarted {
                id,
                name,
                arguments,
            } => {
                state.emit(WorkerMessage::ToolCallStarted {
                    trace_id: trace_id.clone(),
                    id,
                    name,
                    arguments,
                });
            }
            HarnessEvent::ToolCallFinished { id, result } => {
                state.emit(WorkerMessage::ToolCallFinished {
                    trace_id: trace_id.clone(),
                    id,
                    result,
                });
            }
            HarnessEvent::ToolCallError { id, message } => {
                state.emit(WorkerMessage::ToolCallError {
                    trace_id: trace_id.clone(),
                    id,
                    message,
                });
            }
            HarnessEvent::AskUser {
                question,
                quick_replies,
            } => {
                state.emit(WorkerMessage::AskUser {
                    trace_id: trace_id.clone(),
                    question,
                    quick_replies,
                });
                // Pause the agent until the desktop replies.
                finished = true;
                break;
            }
            HarnessEvent::MissionUpdated { mission_id, status } => {
                state.emit(WorkerMessage::MissionUpdated {
                    trace_id: trace_id.clone(),
                    mission_id,
                    status,
                });
            }
            HarnessEvent::Done => {
                finished = true;
            }
            HarnessEvent::Error(message) => {
                state.emit(WorkerMessage::AgentLog {
                    trace_id: trace_id.clone(),
                    step_id: "harness".to_string(),
                    level: goble_core::execution::LogLevel::Error,
                    message,
                });
            }
            _ => {}
        }
    }

    let status = if finished {
        ExecutionStatus::Success
    } else {
        ExecutionStatus::Failure("harness did not finish".to_string())
    };
    state.update_trace(&trace_id, |t| {
        t.status = status.clone();
    });
    state.emit(WorkerMessage::AgentFinished {
        trace_id: trace_id.clone(),
        status,
    });

    // Compact the overflowing transcript into the agent's canonical memory.
    if injector::should_compact(&store, &chat_id, COMPACT_THRESHOLD).unwrap_or(false) {
        if let Some(result) = run_compaction_turn(&state, &provider, model, &store, &chat_id).await {
            if let Some(mut memory) = store.get_agent_memory(&agent_id.0)? {
                merge_compaction(&mut memory, result);
                store.put_agent_memory(&memory)?;
            }
        }
    }

    Ok(())
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

    let request = goble_core::llm::CompletionRequest::new(
        provider.name().to_string(),
        model.to_string(),
    )
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

/// Run a short agent reply for a thread mention using the harness.
pub async fn run_agent_for_thread_reply_with_harness(
    state: Arc<AppState>,
    trace_id: String,
    agent_id: AgentId,
    spec: AgentSpec,
    _prompt: String,
    mcp_servers: Vec<McpServer>,
    secrets: Vec<Secret>,
    provider: Arc<dyn LlmProvider>,
    model: &str,
) -> anyhow::Result<String> {
    run_agent_with_harness(
        state,
        trace_id,
        agent_id,
        spec,
        mcp_servers,
        secrets,
        provider,
        model,
    )
    .await?;
    Ok("reply submitted".to_string())
}
