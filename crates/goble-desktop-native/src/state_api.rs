use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use goble_core::agent::{AgentId, Trigger};
use goble_core::execution::ExecutionTrace;
use goble_core::harness::Harness;
use goble_core::mcp_client::McpTool;
use goble_core::mcp_manager::McpServerSummary;
use goble_core::mcp_registry::McpSearchResult;
use goble_core::protocol::DesktopMessage;
use goble_core::thread::{MessageId, Participant, ParticipantId, ThreadId, ThreadKind, UserId};
use goble_core::worker::WorkerId;
use goble_core::workflow::{WorkflowId, WorkflowStep};
use goble_desktop_service::{
    AgentInfo, Chat, ChatMessage, ClusterIdentityInfo, DesktopState, ExecutionInfo, Intent,
    LogEntry, LlmSetting, TeamInfo, ThreadMessageSummary, ThreadSummary, VaultSecretInfo,
    WorkerConnection, WorkerInvite, WorkerInstallResult, WorkflowInfo,
};

// ---------------------------------------------------------------------------
// Workers
// ---------------------------------------------------------------------------

pub fn list_workers(state: &Arc<DesktopState>) -> Vec<WorkerConnection> {
    state.list_workers()
}

pub struct AddWorkerRequest {
    pub name: String,
    pub url: String,
}

pub fn add_worker(state: &Arc<DesktopState>, req: AddWorkerRequest) -> anyhow::Result<WorkerConnection> {
    let worker_id = WorkerId::generate();
    state.add_worker(worker_id.clone(), req.name.clone(), req.url.clone())?;
    Ok(state
        .list_workers()
        .into_iter()
        .find(|w| w.id == worker_id.to_string())
        .unwrap_or_else(|| WorkerConnection {
            id: worker_id.to_string(),
            name: req.name,
            url: req.url,
            paired: false,
            tags: Vec::new(),
        }))
}

pub fn tag_worker(state: &Arc<DesktopState>, worker_id: &str, tag: &str) -> anyhow::Result<()> {
    state.tag_worker(&WorkerId(worker_id.to_string()), tag.to_string())
}

pub fn remove_worker(state: &Arc<DesktopState>, worker_id: &str) {
    state.remove_worker(&WorkerId(worker_id.to_string()));
}

pub struct PairWorkerRequest {
    pub worker_id: String,
    pub pairing_code: String,
}

pub fn pair_worker(state: Arc<DesktopState>, req: PairWorkerRequest) -> anyhow::Result<bool> {
    state.pair_worker(&WorkerId(req.worker_id), req.pairing_code)
}

pub fn ping_worker(state: &Arc<DesktopState>, worker_id: &str) -> anyhow::Result<()> {
    state.send_to_worker(&WorkerId(worker_id.to_string()), DesktopMessage::Ping)
}

pub fn worker_logs(state: &Arc<DesktopState>) -> Vec<LogEntry> {
    state.get_logs()
}

pub struct InstallWorkerRequest {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub private_key: String,
    pub release_tag: String,
    pub repo: Option<String>,
    pub pairing_code: String,
}

pub fn install_worker(
    state: &Arc<DesktopState>,
    req: InstallWorkerRequest,
) -> anyhow::Result<WorkerInstallResult> {
    let creds = goble_desktop_service::SshCredentials {
        host: req.host,
        user: req.user,
        port: req.port,
        private_key: req.private_key,
    };
    let repo = req.repo.as_deref().unwrap_or("AdrianTuci1/goble");
    state
        .install_worker_ssh(creds, &req.release_tag, repo, &req.pairing_code)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

pub fn generate_worker_invite(state: &Arc<DesktopState>, worker_id: &str) -> anyhow::Result<WorkerInvite> {
    state.generate_worker_invite(worker_id)
}

// ---------------------------------------------------------------------------
// Cluster identity
// ---------------------------------------------------------------------------

pub fn get_cluster_identity(state: &Arc<DesktopState>) -> Option<ClusterIdentityInfo> {
    state.get_cluster_identity().map(|i| ClusterIdentityInfo {
        cluster_name: i.cluster_name,
        ca_cert_pem: i.ca.identity.cert_pem,
        device_serial: i.device.serial().to_string(),
    })
}

pub struct CreateClusterRequest {
    pub name: String,
    pub passphrase: String,
}

pub fn create_cluster(
    state: &Arc<DesktopState>,
    req: CreateClusterRequest,
) -> anyhow::Result<ClusterIdentityInfo> {
    let identity = state.create_cluster(&req.name, &req.passphrase)?;
    Ok(ClusterIdentityInfo {
        cluster_name: identity.cluster_name,
        ca_cert_pem: identity.ca.identity.cert_pem,
        device_serial: identity.device.serial().to_string(),
    })
}

pub struct ImportClusterKeyRequest {
    pub key: String,
    pub name: String,
    pub passphrase: String,
}

pub fn import_cluster_key(
    state: &Arc<DesktopState>,
    req: ImportClusterKeyRequest,
) -> anyhow::Result<ClusterIdentityInfo> {
    let identity = state.import_cluster_key(&req.key, &req.name, &req.passphrase)?;
    Ok(ClusterIdentityInfo {
        cluster_name: identity.cluster_name,
        ca_cert_pem: identity.ca.identity.cert_pem,
        device_serial: identity.device.serial().to_string(),
    })
}

pub fn export_cluster_key(state: &Arc<DesktopState>) -> anyhow::Result<String> {
    state.export_cluster_key()
}

pub fn export_cluster_backup(state: &Arc<DesktopState>) -> anyhow::Result<serde_json::Value> {
    let backup = state.export_cluster_backup()?;
    serde_json::to_value(&backup).map_err(|e| anyhow::anyhow!("{e}"))
}

pub struct ExportIdentityRequest {
    pub passphrase: String,
}

pub fn export_identity_wallet(
    state: &Arc<DesktopState>,
    req: ExportIdentityRequest,
) -> anyhow::Result<String> {
    state.export_identity_wallet(&req.passphrase)
}

pub struct ImportIdentityRequest {
    pub wallet: String,
    pub passphrase: String,
}

pub fn import_identity_wallet(
    state: &Arc<DesktopState>,
    req: ImportIdentityRequest,
) -> anyhow::Result<ClusterIdentityInfo> {
    let identity = state.import_identity_wallet(&req.wallet, &req.passphrase)?;
    Ok(ClusterIdentityInfo {
        cluster_name: identity.cluster_name,
        ca_cert_pem: identity.ca.identity.cert_pem,
        device_serial: identity.device.serial().to_string(),
    })
}

pub fn unlock_cluster_identity(state: &Arc<DesktopState>, passphrase: &str) -> anyhow::Result<bool> {
    state.unlock_cluster_identity(passphrase)
}

pub fn has_cluster_identity(state: &Arc<DesktopState>) -> bool {
    state.has_stored_cluster_identity()
}

pub struct ClusterHelmInstallRequest {
    pub name: String,
    pub namespace: String,
    pub replicas: u32,
    pub storage_class: Option<String>,
    pub persistence_size: String,
    pub provider: String,
    pub endpoint: Option<String>,
    pub bucket: Option<String>,
    pub access_key_id: Option<String>,
    pub secret_access_key: Option<String>,
    pub region: Option<String>,
    pub interval_seconds: u64,
    pub local_chart: Option<String>,
}

pub fn cluster_helm_install(
    state: &Arc<DesktopState>,
    req: ClusterHelmInstallRequest,
) -> anyhow::Result<String> {
    state.cluster_helm_install(
        req.name,
        req.namespace,
        req.replicas,
        req.storage_class,
        req.persistence_size,
        req.provider,
        req.endpoint,
        req.bucket,
        req.access_key_id,
        req.secret_access_key,
        req.region,
        req.interval_seconds,
        req.local_chart,
    )
}

// ---------------------------------------------------------------------------
// Chat
// ---------------------------------------------------------------------------

pub fn create_chat(
    state: &Arc<DesktopState>,
    title: &str,
    provider: Option<&str>,
    model: Option<&str>,
) -> anyhow::Result<String> {
    state.create_chat(title, provider, model)
}

pub fn list_chats(state: &Arc<DesktopState>) -> Vec<Chat> {
    state.list_chats()
}

pub fn chat_messages(state: &Arc<DesktopState>, chat_id: &str) -> anyhow::Result<Vec<ChatMessage>> {
    state.list_chat_messages(chat_id)
}

pub fn add_chat_message(
    state: &Arc<DesktopState>,
    chat_id: &str,
    role: &str,
    content: &str,
) -> anyhow::Result<()> {
    state.add_chat_message(chat_id, role, content)
}

pub struct SetChatModelRequest {
    pub chat_id: String,
    pub provider: String,
    pub model: String,
}

pub fn set_chat_model(state: &Arc<DesktopState>, req: SetChatModelRequest) -> anyhow::Result<()> {
    state.set_chat_model(&req.chat_id, &req.provider, &req.model)
}

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

pub fn list_agents(state: &Arc<DesktopState>) -> Vec<AgentInfo> {
    state.list_agents()
}

pub struct CreateAgentRequest {
    pub name: String,
    pub prompt: String,
    pub description: Option<String>,
    pub tools: Vec<String>,
}

pub fn create_agent(
    state: &Arc<DesktopState>,
    req: CreateAgentRequest,
) -> anyhow::Result<AgentInfo> {
    state.create_agent(
        &req.name,
        &req.prompt,
        req.description.as_deref(),
        req.tools,
    )
}

pub struct UpdateAgentRequest {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub description: Option<String>,
    pub tools: Vec<String>,
}

pub fn update_agent(
    state: &Arc<DesktopState>,
    req: UpdateAgentRequest,
) -> anyhow::Result<AgentInfo> {
    state.update_agent(
        &AgentId(req.id),
        &req.name,
        &req.prompt,
        req.description.as_deref(),
        req.tools,
    )
}

pub fn delete_agent(state: &Arc<DesktopState>, agent_id: &str) -> anyhow::Result<()> {
    state.delete_agent(&AgentId(agent_id.to_string()))
}

pub struct RuntimeTarget {
    pub kind: String,
    pub tag: Option<String>,
    pub worker_id: Option<String>,
}

pub struct RunAgentRequest {
    pub target: RuntimeTarget,
    pub agent_id: String,
    pub prompt: String,
}

pub fn run_agent(state: &Arc<DesktopState>, req: RunAgentRequest) -> anyhow::Result<()> {
    let worker_id = state.resolve_worker_for_target(
        &req.target.kind,
        req.target.tag.as_deref(),
        req.target.worker_id.as_deref(),
    )?;
    state.run_agent(&worker_id, &AgentId(req.agent_id), &req.prompt)
}

pub struct ScheduleAgentRequest {
    pub worker_id: String,
    pub agent_id: String,
    pub trigger: String,
}

pub fn schedule_agent(state: &Arc<DesktopState>, req: ScheduleAgentRequest) -> anyhow::Result<()> {
    let trigger = Trigger::Cron {
        expression: req.trigger,
    };
    state.schedule_agent(&WorkerId(req.worker_id), &AgentId(req.agent_id), trigger)
}

pub struct RunAgentForThreadReplyRequest {
    pub target: RuntimeTarget,
    pub thread_id: String,
    pub agent_id: String,
    pub prompt: String,
}

pub fn run_agent_for_thread_reply(
    state: &Arc<DesktopState>,
    req: RunAgentForThreadReplyRequest,
) -> anyhow::Result<()> {
    let worker_id = state.resolve_worker_for_target(
        &req.target.kind,
        req.target.tag.as_deref(),
        req.target.worker_id.as_deref(),
    )?;
    state.run_agent_for_thread_reply(
        &worker_id,
        &ThreadId(req.thread_id),
        &AgentId(req.agent_id),
        &req.prompt,
    )
}

pub fn classify_intent(
    state: &Arc<DesktopState>,
    provider: &str,
    model: &str,
    text: &str,
) -> anyhow::Result<Intent> {
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|e| anyhow::anyhow!("no tokio runtime: {e}"))?;
    handle.block_on(state.classify_intent(provider, model, text))
}

// ---------------------------------------------------------------------------
// Workflows
// ---------------------------------------------------------------------------

pub fn list_workflows(state: &Arc<DesktopState>) -> Vec<WorkflowInfo> {
    state.list_workflows()
}

pub struct CreateWorkflowRequest {
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
    pub trigger: String,
}

pub fn create_workflow(
    state: &Arc<DesktopState>,
    req: CreateWorkflowRequest,
) -> anyhow::Result<WorkflowInfo> {
    let trigger = Trigger::Cron {
        expression: req.trigger,
    };
    state.create_workflow(&req.name, &req.description, req.steps, trigger)
}

pub fn delete_workflow(state: &Arc<DesktopState>, workflow_id: &str) -> anyhow::Result<()> {
    state.delete_workflow(&WorkflowId(workflow_id.to_string()))
}

// ---------------------------------------------------------------------------
// Teams
// ---------------------------------------------------------------------------

pub fn list_teams(state: &Arc<DesktopState>) -> Vec<TeamInfo> {
    state.list_teams()
}

pub struct CreateTeamRequest {
    pub id: String,
    pub name: String,
    pub metadata: String,
    pub agent_ids: Vec<String>,
}

pub fn create_team(state: &Arc<DesktopState>, req: CreateTeamRequest) -> anyhow::Result<TeamInfo> {
    state.create_team(&req.id, &req.name, &req.metadata, req.agent_ids)
}

// ---------------------------------------------------------------------------
// Executions
// ---------------------------------------------------------------------------

pub fn list_executions(state: &Arc<DesktopState>) -> Vec<ExecutionInfo> {
    state.list_executions()
}

pub fn get_execution_trace(
    state: &Arc<DesktopState>,
    trace_id: &str,
) -> anyhow::Result<ExecutionTrace> {
    state
        .get_execution_trace(trace_id)
        .ok_or_else(|| anyhow::anyhow!("trace not found"))
}

// ---------------------------------------------------------------------------
// Vault
// ---------------------------------------------------------------------------

pub fn list_vault_secrets(state: &Arc<DesktopState>) -> Vec<VaultSecretInfo> {
    state.list_vault_secrets()
}

pub struct VaultSecretRequest {
    pub name: String,
    pub value: String,
}

pub fn set_vault_secret(state: &Arc<DesktopState>, req: VaultSecretRequest) -> anyhow::Result<()> {
    state.set_vault_secret(&req.name, &req.value)
}

pub struct UnlockVaultRequest {
    pub passphrase: String,
}

pub fn unlock_vault(
    state: Arc<DesktopState>,
    req: UnlockVaultRequest,
) -> anyhow::Result<Vec<String>> {
    let res = state.unlock_vault(req.passphrase)?;
    Arc::clone(&state).restore_clients();
    Ok(res)
}

// ---------------------------------------------------------------------------
// LLM settings
// ---------------------------------------------------------------------------

pub struct LlmSettingRequest {
    pub provider: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
    pub temperature: Option<f32>,
}

pub fn set_llm_setting(
    state: &Arc<DesktopState>,
    req: LlmSettingRequest,
) -> anyhow::Result<()> {
    state.set_llm_setting(
        &req.provider,
        &req.api_key,
        req.base_url.as_deref(),
        &req.model,
        req.temperature,
    )
}

pub fn get_llm_setting(state: &Arc<DesktopState>, provider: &str) -> Option<LlmSetting> {
    state.get_llm_setting(provider)
}

// ---------------------------------------------------------------------------
// MCP
// ---------------------------------------------------------------------------

pub fn search_mcp_servers(state: &Arc<DesktopState>, query: &str) -> Vec<McpSearchResult> {
    state.search_mcp_servers(query)
}

pub fn list_mcp_servers(state: &Arc<DesktopState>) -> anyhow::Result<Vec<McpServerSummary>> {
    state.list_mcp_servers()
}

pub struct InstallMcpRequest {
    pub id: String,
    pub name: String,
    pub source: String,
    pub source_value: Option<String>,
    pub secret_ids: Vec<String>,
}

pub fn install_mcp_server(
    state: &Arc<DesktopState>,
    req: InstallMcpRequest,
) -> anyhow::Result<String> {
    state.install_mcp_server(
        &req.id,
        &req.name,
        &req.source,
        req.source_value.as_deref(),
        req.secret_ids,
        None,
    )
}

pub struct UpdateMcpRequest {
    pub id: String,
    pub name: Option<String>,
    pub source_value: Option<String>,
    pub secret_ids: Vec<String>,
}

pub fn update_mcp_server(
    state: &Arc<DesktopState>,
    req: UpdateMcpRequest,
) -> anyhow::Result<String> {
    state.update_mcp_server(
        &req.id,
        req.name.as_deref(),
        req.source_value.as_deref(),
        Some(req.secret_ids),
        None,
    )
}

pub struct UpdateMcpMetaRequest {
    pub id: String,
    pub secret_ids: Vec<String>,
    pub enabled_tools: Vec<String>,
}

pub fn update_mcp_server_meta(
    state: &Arc<DesktopState>,
    req: UpdateMcpMetaRequest,
) -> anyhow::Result<String> {
    state.update_mcp_server_meta(&req.id, req.secret_ids, req.enabled_tools)
}

pub fn delete_mcp_server(state: &Arc<DesktopState>, id: &str) -> anyhow::Result<String> {
    state.delete_mcp_server(id)
}

pub fn discover_mcp_tools(state: &Arc<DesktopState>, id: &str) -> anyhow::Result<Vec<McpTool>> {
    state.discover_mcp_tools(id)
}

pub struct TestCallMcpRequest {
    pub id: String,
    pub tool_name: String,
    pub arguments: Option<serde_json::Value>,
}

pub fn test_call_mcp_tool(
    state: &Arc<DesktopState>,
    req: TestCallMcpRequest,
) -> anyhow::Result<serde_json::Value> {
    state.test_call_mcp_tool(
        &req.id,
        &req.tool_name,
        req.arguments.unwrap_or(serde_json::json!({})),
    )
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

static HARNESS_CANCEL: once_cell::sync::Lazy<
    std::sync::Mutex<HashMap<String, Arc<AtomicBool>>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

pub fn list_harness_tools() -> anyhow::Result<Vec<goble_core::harness::ToolSchema>> {
    let harness = Harness::new(goble_core::store::Store::open_in_memory()?);
    Ok(harness.list_tools())
}

pub struct RunHarnessRequest {
    pub chat_id: String,
    pub prompt: String,
    pub provider: String,
    pub model: String,
}

pub fn run_harness(state: &Arc<DesktopState>, req: RunHarnessRequest) -> anyhow::Result<()> {
    use futures::StreamExt;
    use goble_core::harness::HarnessEvent;
    use goble_core::harness::SandboxedCommandRunner;

    let (llm, model_name) = state.resolve_llm_provider(&req.provider, &req.model);
    let provider_name = if req.provider.is_empty() {
        "openai"
    } else {
        &req.provider
    };
    let deploy_state = Arc::clone(state);
    let cancel = Arc::new(AtomicBool::new(false));
    HARNESS_CANCEL
        .lock()
        .unwrap()
        .insert(req.chat_id.clone(), cancel.clone());
    let harness = Harness::new(state.store_clone())
        .with_llm(llm)
        .with_runner(Arc::new(SandboxedCommandRunner::default_tools()))
        .with_deploy_sender(move |worker_id, msg| deploy_state.send_to_worker(worker_id, msg))
        .with_cancel(cancel.clone());
    let chat_id_for_cleanup = req.chat_id.clone();
    let mut stream = harness.run_turn(&req.chat_id, &req.prompt, provider_name, &model_name);
    let state_clone = Arc::clone(state);
    let chat_id = req.chat_id.clone();
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|e| anyhow::anyhow!("no tokio runtime: {e}"))?;
    handle.spawn(async move {
        while let Some(event) = stream.next().await {
            let payload = serde_json::json!({
                "chat_id": &chat_id,
                "event": event,
            });
            state_clone.emit("harness:event", payload.clone());
            if let HarnessEvent::Error(e) = &event {
                state_clone.add_log(format!("harness error: {e}"));
            }
        }
        HARNESS_CANCEL.lock().unwrap().remove(&chat_id_for_cleanup);
    });
    Ok(())
}

pub fn cancel_harness(chat_id: &str) {
    if let Some(cancel) = HARNESS_CANCEL.lock().unwrap().get(chat_id) {
        cancel.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Threads
// ---------------------------------------------------------------------------

pub fn list_threads(state: &Arc<DesktopState>) -> Vec<ThreadSummary> {
    state
        .thread_store()
        .list_threads_with_read_status()
        .into_iter()
        .map(|(t, read_at)| ThreadSummary {
            last_read_at: read_at.map(|dt| dt.to_rfc3339()),
            ..ThreadSummary::from(t)
        })
        .collect()
}

pub struct CreateThreadRequest {
    pub kind: ThreadKind,
    pub title: String,
    pub is_private: bool,
    pub participants: Vec<Participant>,
    pub tags: Vec<String>,
}

pub fn create_thread(
    state: &Arc<DesktopState>,
    req: CreateThreadRequest,
) -> anyhow::Result<ThreadSummary> {
    let owner_id = state
        .thread_store()
        .get_profile()
        .map(|p| UserId(p.id.to_string()))
        .unwrap_or_else(UserId::generate);
    let thread = state
        .thread_store()
        .create_thread(
            req.kind,
            req.title,
            owner_id,
            req.is_private,
            req.participants,
            req.tags,
        )
        .map(ThreadSummary::from)?;
    state.emit("threads:updated", ());
    Ok(thread)
}

pub fn delete_thread(state: &Arc<DesktopState>, thread_id: &str) -> bool {
    state
        .thread_store()
        .delete_thread(&ThreadId(thread_id.to_string()))
}

pub struct AddThreadParticipantRequest {
    pub thread_id: String,
    pub participant: Participant,
}

pub fn add_thread_participant(
    state: &Arc<DesktopState>,
    req: AddThreadParticipantRequest,
) -> anyhow::Result<()> {
    state
        .thread_store()
        .add_participant(&ThreadId(req.thread_id), req.participant)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit("threads:updated", ());
    Ok(())
}

pub struct RemoveThreadParticipantRequest {
    pub thread_id: String,
    pub participant_id: String,
}

pub fn remove_thread_participant(
    state: &Arc<DesktopState>,
    req: RemoveThreadParticipantRequest,
) -> anyhow::Result<()> {
    state
        .thread_store()
        .remove_participant(
            &ThreadId(req.thread_id),
            &ParticipantId(req.participant_id),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit("threads:updated", ());
    Ok(())
}

pub fn get_thread_participants(
    state: &Arc<DesktopState>,
    thread_id: &str,
) -> anyhow::Result<Vec<Participant>> {
    state
        .thread_store()
        .list_participants(&ThreadId(thread_id.to_string()))
        .map_err(|e| anyhow::anyhow!("{e}"))
}

pub fn get_thread_messages(
    state: &Arc<DesktopState>,
    thread_id: &str,
) -> anyhow::Result<Vec<ThreadMessageSummary>> {
    state
        .thread_store()
        .list_messages(&ThreadId(thread_id.to_string()))
        .map(|messages| messages.into_iter().map(ThreadMessageSummary::from).collect())
        .map_err(|e| anyhow::anyhow!("{e}"))
}

pub struct PostMessageRequest {
    pub thread_id: String,
    pub content: String,
    pub reply_to: Option<String>,
    pub tags: Vec<String>,
    pub mentions: Vec<String>,
    pub trace_id: Option<String>,
}

pub fn post_thread_message(
    state: &Arc<DesktopState>,
    req: PostMessageRequest,
) -> anyhow::Result<ThreadMessageSummary> {
    let author = state
        .thread_store()
        .get_profile()
        .map(|p| Participant::User(UserId(p.id.to_string())))
        .unwrap_or_else(|| Participant::User(UserId::generate()));
    let reply_to = req.reply_to.map(MessageId);
    let mentions: Vec<ParticipantId> = req.mentions.into_iter().map(ParticipantId).collect();
    let thread_id = ThreadId(req.thread_id.clone());
    let message = state
        .thread_store()
        .post_message(
            &thread_id,
            author,
            req.content,
            reply_to,
            req.tags,
            mentions,
            req.trace_id,
        )
        .map(ThreadMessageSummary::from)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit(
        "thread:message:created",
        serde_json::json!({
            "thread_id": req.thread_id.clone(),
            "message": message.clone(),
        }),
    );
    state.emit(
        "thread:messages:updated",
        serde_json::json!({ "thread_id": req.thread_id }),
    );
    Ok(message)
}

pub fn mark_thread_read(state: &Arc<DesktopState>, thread_id: &str) -> anyhow::Result<()> {
    state
        .thread_store()
        .mark_thread_read(&ThreadId(thread_id.to_string()))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit(
        "thread:updated",
        serde_json::json!({ "thread_id": thread_id }),
    );
    Ok(())
}

pub struct UpdateThreadMessageRequest {
    pub thread_id: String,
    pub message_id: String,
    pub content: String,
}

pub fn update_thread_message(
    state: &Arc<DesktopState>,
    req: UpdateThreadMessageRequest,
) -> anyhow::Result<ThreadMessageSummary> {
    let me = state
        .thread_store()
        .get_profile()
        .ok_or_else(|| anyhow::anyhow!("profile not set"))?;
    let msg = state
        .thread_store()
        .update_message(
            &ThreadId(req.thread_id.clone()),
            &MessageId(req.message_id.clone()),
            &ParticipantId::user(me.id.0.clone()),
            req.content,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit(
        "thread:messages:updated",
        serde_json::json!({ "thread_id": req.thread_id }),
    );
    Ok(ThreadMessageSummary::from(msg))
}

pub struct DeleteThreadMessageRequest {
    pub thread_id: String,
    pub message_id: String,
}

pub fn delete_thread_message(
    state: &Arc<DesktopState>,
    req: DeleteThreadMessageRequest,
) -> anyhow::Result<()> {
    let me = state
        .thread_store()
        .get_profile()
        .ok_or_else(|| anyhow::anyhow!("profile not set"))?;
    state
        .thread_store()
        .delete_message(
            &ThreadId(req.thread_id.clone()),
            &MessageId(req.message_id.clone()),
            &ParticipantId::user(me.id.0.clone()),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit(
        "thread:messages:updated",
        serde_json::json!({ "thread_id": req.thread_id }),
    );
    Ok(())
}

pub struct ReactionRequest {
    pub thread_id: String,
    pub message_id: String,
    pub emoji: String,
}

fn thread_participant_id(state: &Arc<DesktopState>) -> ParticipantId {
    state
        .thread_store()
        .get_profile()
        .map(|p| ParticipantId::user(p.id.to_string()))
        .unwrap_or_else(|| ParticipantId::user(UserId::generate().to_string()))
}

pub fn add_thread_reaction(
    state: &Arc<DesktopState>,
    req: ReactionRequest,
) -> anyhow::Result<()> {
    let participant_id = thread_participant_id(state);
    state
        .thread_store()
        .add_reaction(
            &ThreadId(req.thread_id.clone()),
            &MessageId(req.message_id),
            participant_id,
            req.emoji,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit(
        "thread:messages:updated",
        serde_json::json!({ "thread_id": req.thread_id }),
    );
    Ok(())
}

pub fn remove_thread_reaction(
    state: &Arc<DesktopState>,
    req: ReactionRequest,
) -> anyhow::Result<()> {
    let participant_id = thread_participant_id(state);
    state
        .thread_store()
        .remove_reaction(
            &ThreadId(req.thread_id.clone()),
            &MessageId(req.message_id),
            &participant_id,
            &req.emoji,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit(
        "thread:messages:updated",
        serde_json::json!({ "thread_id": req.thread_id }),
    );
    Ok(())
}

pub struct InviteUserByPublicKeyRequest {
    pub thread_id: String,
    pub public_key_pem: String,
    pub name: String,
}

pub fn invite_user_by_public_key(
    state: &Arc<DesktopState>,
    req: InviteUserByPublicKeyRequest,
) -> anyhow::Result<Participant> {
    let participant = state
        .thread_store()
        .invite_user_by_public_key(
            &ThreadId(req.thread_id.clone()),
            req.public_key_pem,
            req.name,
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    state.emit("threads:updated", ());
    Ok(participant)
}

pub fn migrate_legacy_chats_to_threads(
    state: &Arc<DesktopState>,
) -> anyhow::Result<Vec<ThreadSummary>> {
    state
        .migrate_legacy_chats_to_threads()
        .map(|summaries| {
            state.emit("threads:updated", ());
            summaries
        })
        .map_err(|e| anyhow::anyhow!("{e}"))
}

// ---------------------------------------------------------------------------
// Profile / authorized keys
// ---------------------------------------------------------------------------

pub fn get_user_profile(state: &Arc<DesktopState>) -> anyhow::Result<goble_core::user::UserProfile> {
    state
        .thread_store()
        .get_profile()
        .ok_or_else(|| anyhow::anyhow!("profile not found"))
}

pub struct UserProfileRequest {
    pub name: String,
    pub email: String,
    pub avatar_url: Option<String>,
    pub public_key_pem: Option<String>,
}

pub fn set_user_profile(
    state: &Arc<DesktopState>,
    req: UserProfileRequest,
) -> anyhow::Result<()> {
    let id = state
        .thread_store()
        .get_profile()
        .map(|p| p.id)
        .unwrap_or_else(goble_core::principal::PrincipalId::generate);
    let mut profile = goble_core::user::UserProfile::new(id, req.name, req.email);
    if let Some(url) = req.avatar_url {
        profile = profile.with_avatar_url(url);
    }
    if let Some(pem) = req.public_key_pem {
        profile = profile.with_public_key(pem);
    }
    state.thread_store().set_profile(profile)
}

pub fn list_authorized_keys(
    state: &Arc<DesktopState>,
) -> anyhow::Result<Vec<goble_core::user::AuthorizedKey>> {
    Ok(state.thread_store().list_authorized_keys())
}

pub struct AuthorizedKeyRequest {
    pub id: String,
    pub name: String,
    pub public_key_pem: String,
    pub fingerprint: String,
    pub thread_ids: Vec<String>,
}

pub fn add_authorized_key(
    state: &Arc<DesktopState>,
    req: AuthorizedKeyRequest,
) -> anyhow::Result<()> {
    let mut key =
        goble_core::user::AuthorizedKey::new(req.id, req.name, req.public_key_pem, req.fingerprint);
    key.thread_ids = req.thread_ids;
    state
        .thread_store()
        .add_authorized_key(key)
        .map_err(|e| anyhow::anyhow!("{e}"))
}

pub fn remove_authorized_key(state: &Arc<DesktopState>, id: &str) -> bool {
    state.thread_store().remove_authorized_key(id)
}
