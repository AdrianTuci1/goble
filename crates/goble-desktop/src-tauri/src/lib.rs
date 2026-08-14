// Tauri + React application entry
// This crate is not included in the Cargo workspace because it is a Tauri project.
use goble_core::agent::{AgentId, Trigger};
use goble_core::mcp_manager::McpServerSummary;
use goble_core::mcp_registry::McpSearchResult;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use goble_core::harness::Harness;
use goble_core::protocol::DesktopMessage;
use goble_core::store::Store;
use goble_core::worker::WorkerId;
use goble_core::workflow::{WorkflowId, WorkflowStep};
use serde::{Deserialize, Serialize};

pub mod state;
pub mod ssh_installer;
pub mod thread_store;
pub mod worker_manager;


#[derive(Deserialize)]
struct AddWorkerRequest {
    name: String,
    url: String,
}

#[derive(Deserialize)]
struct PairWorkerRequest {
    worker_id: String,
    pairing_code: String,
}

#[derive(Deserialize)]
struct ClassifyIntentRequest {
    provider: String,
    model: String,
    text: String,
}

#[derive(Serialize)]
struct ClassifyIntentResponse {
    intent: String,
    params: state::IntentParams,
}

#[tauri::command]
async fn classify_intent(
    req: ClassifyIntentRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<ClassifyIntentResponse, String> {
    state
        .classify_intent(&req.provider, &req.model, &req.text)
        .await
        .map(|intent| ClassifyIntentResponse {
            intent: intent.intent,
            params: intent.params,
        })
        .map_err(|e| e.to_string())
}

#[derive(Deserialize)]
struct RunAgentRequest {
    target: RuntimeTarget,
    chat_id: Option<String>,
    agent_id: String,
    prompt: String,
}

#[derive(Deserialize)]
struct RuntimeTarget {
    kind: String,
    tag: Option<String>,
    worker_id: Option<String>,
}

#[derive(Deserialize)]
struct RunAgentForThreadReplyRequest {
    target: RuntimeTarget,
    thread_id: String,
    agent_id: String,
    prompt: String,
}

#[derive(Deserialize)]
struct ScheduleRequest {
    worker_id: String,
    agent_id: String,
    trigger: String,
}

#[derive(Deserialize)]
struct VaultSecretRequest {
    name: String,
    value: String,
}

#[derive(Deserialize)]
struct CreateAgentRequest {
    name: String,
    prompt: String,
    description: Option<String>,
    tools: Vec<String>,
}

#[derive(Deserialize)]
struct CreateWorkflowRequest {
    name: String,
    description: String,
    steps: Vec<WorkflowStep>,
    trigger: String,
}

#[derive(Deserialize)]
struct CreateTeamRequest {
    id: String,
    name: String,
    metadata: String,
    agent_ids: Vec<String>,
}

#[derive(Deserialize)]
struct UnlockVaultRequest {
    passphrase: String,
}

#[derive(Deserialize)]
struct LlmSettingRequest {
    provider: String,
    api_key: String,
    base_url: Option<String>,
    model: String,
    temperature: Option<f32>,
}

#[derive(Deserialize)]
struct RunHarnessRequest {
    chat_id: String,
    prompt: String,
    provider: String,
    model: String,
}

#[derive(Deserialize)]
struct SetChatModelRequest {
    chat_id: String,
    provider: String,
    model: String,
}

#[derive(Deserialize)]
struct SearchMcpRequest {
    query: String,
}

#[derive(Deserialize)]
struct InstallMcpRequest {
    id: String,
    name: String,
    source: String,
    source_value: Option<String>,
    secret_ids: Vec<String>,
}

#[derive(Deserialize)]
struct UpdateMcpRequest {
    id: String,
    name: Option<String>,
    source_value: Option<String>,
    secret_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMcpMetaRequest {
    pub id: String,
    pub secret_ids: Vec<String>,
    pub enabled_tools: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InstallWorkerRequest {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub private_key: String,
    pub release_tag: String,
    pub repo: Option<String>,
    pub pairing_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpIdRequest {
    id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterIdentityInfo {
    pub cluster_name: String,
    pub ca_cert_pem: String,
    pub device_serial: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateClusterRequest {
    pub name: String,
    pub passphrase: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportClusterKeyRequest {
    pub key: String,
    pub name: String,
    pub passphrase: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UnlockClusterIdentityRequest {
    pub passphrase: String,
}

#[derive(Debug, Clone, Deserialize)]
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

static HARNESS_CANCEL: once_cell::sync::Lazy<std::sync::Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[tauri::command]
fn list_workers(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<state::WorkerConnection>, String> {
    Ok(state.list_workers())
}

#[tauri::command]
fn install_worker(
    req: InstallWorkerRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<ssh_installer::WorkerInstallResult, String> {
    let cluster = state.get_cluster_identity().ok_or("no cluster identity configured")?;
    let creds = ssh_installer::SshCredentials {
        host: req.host,
        user: req.user,
        port: req.port,
        private_key: req.private_key,
    };
    let repo = req.repo.as_deref().unwrap_or("AdrianTuci1/goble");
    ssh_installer::install_worker(&cluster, &creds, &req.release_tag, repo, &req.pairing_code)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_cluster_identity(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Option<ClusterIdentityInfo>, String> {
    Ok(state.get_cluster_identity().map(|i| ClusterIdentityInfo {
        cluster_name: i.cluster_name,
        ca_cert_pem: i.ca.identity.cert_pem,
        device_serial: i.device.serial().to_string(),
    }))
}

#[tauri::command]
fn create_cluster(
    req: CreateClusterRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<ClusterIdentityInfo, String> {
    let identity = state
        .create_cluster(&req.name, &req.passphrase)
        .map_err(|e| e.to_string())?;
    Ok(ClusterIdentityInfo {
        cluster_name: identity.cluster_name,
        ca_cert_pem: identity.ca.identity.cert_pem,
        device_serial: identity.device.serial().to_string(),
    })
}

#[tauri::command]
fn import_cluster_key(
    req: ImportClusterKeyRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<ClusterIdentityInfo, String> {
    let identity = state
        .import_cluster_key(&req.key, &req.name, &req.passphrase)
        .map_err(|e| e.to_string())?;
    Ok(ClusterIdentityInfo {
        cluster_name: identity.cluster_name,
        ca_cert_pem: identity.ca.identity.cert_pem,
        device_serial: identity.device.serial().to_string(),
    })
}

#[tauri::command]
fn unlock_cluster_identity(
    req: UnlockClusterIdentityRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<bool, String> {
    state
        .unlock_cluster_identity(&req.passphrase)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn has_cluster_identity(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<bool, String> {
    Ok(state.has_stored_cluster_identity())
}

#[tauri::command]
fn export_cluster_key(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<String, String> {
    state.export_cluster_key().map_err(|e| e.to_string())
}

#[tauri::command]
fn export_cluster_backup(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<String, String> {
    let backup = state.export_cluster_backup().map_err(|e| e.to_string())?;
    serde_json::to_string(&backup).map_err(|e| e.to_string())
}

#[tauri::command]
fn cluster_helm_install(
    req: ClusterHelmInstallRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<String, String> {
    state
        .cluster_helm_install(
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
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn add_worker(
    req: AddWorkerRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<state::WorkerConnection, String> {
    let worker_id = WorkerId::generate();
    state
        .add_worker(worker_id.clone(), req.name.clone(), req.url.clone())
        .map_err(|e| e.to_string())?;
    Ok(state
        .list_workers()
        .into_iter()
        .find(|w| w.id == worker_id.to_string())
        .unwrap_or(state::WorkerConnection {
            id: worker_id.to_string(),
            name: req.name,
            url: req.url,
            paired: false,
            tags: Vec::new(),
        }))
}

#[tauri::command]
fn tag_worker(
    worker_id: String,
    tag: String,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    state
        .tag_worker(&WorkerId(worker_id), tag)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn pair_worker(
    req: PairWorkerRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<bool, String> {
    let id = WorkerId(req.worker_id);
    Arc::clone(&state)
        .pair_worker(&id, req.pairing_code)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn worker_logs(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<state::LogEntry>, String> {
    Ok(state.get_logs())
}

#[tauri::command]
fn ping_worker(
    worker_id: String,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    let id = WorkerId(worker_id);
    state
        .send_to_worker(&id, DesktopMessage::Ping)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn add_log(
    message: String,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    state.add_chat_log(message);
    Ok(())
}

#[tauri::command]
fn create_chat(
    title: String,
    provider: Option<String>,
    model: Option<String>,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<String, String> {
    state.create_chat(&title, provider.as_deref(), model.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_chats(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<state::Chat>, String> {
    Ok(state.list_chats())
}

#[tauri::command]
fn chat_messages(
    chat_id: String,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<state::ChatMessage>, String> {
    state.list_chat_messages(&chat_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_chat_message(
    chat_id: String,
    role: String,
    content: String,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    state
        .add_chat_message(&chat_id, &role, &content)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_agents(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<state::AgentInfo>, String> {
    Ok(state.list_agents())
}

#[tauri::command]
fn create_agent(
    req: CreateAgentRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<state::AgentInfo, String> {
    state
        .create_agent(
            &req.name,
            &req.prompt,
            req.description.as_deref(),
            req.tools,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_agent(
    agent_id: String,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    state.delete_agent(&AgentId(agent_id)).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_workflows(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<state::WorkflowInfo>, String> {
    Ok(state.list_workflows())
}

#[tauri::command]
fn create_workflow(
    req: CreateWorkflowRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<state::WorkflowInfo, String> {
    let trigger = Trigger::Cron {
        expression: req.trigger,
    };
    state
        .create_workflow(&req.name, &req.description, req.steps, trigger)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_workflow(
    workflow_id: String,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    state
        .delete_workflow(&WorkflowId(workflow_id))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_teams(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<state::TeamInfo>, String> {
    Ok(state.list_teams())
}

#[tauri::command]
fn create_team(
    req: CreateTeamRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<state::TeamInfo, String> {
    state
        .create_team(&req.id, &req.name, &req.metadata, req.agent_ids)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_executions(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<state::ExecutionInfo>, String> {
    Ok(state.list_executions())
}

#[tauri::command]
fn list_vault_secrets(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<state::VaultSecretInfo>, String> {
    Ok(state.list_vault_secrets())
}

#[tauri::command]
fn set_vault_secret(
    req: VaultSecretRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    state
        .set_vault_secret(&req.name, &req.value)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn unlock_vault(
    req: UnlockVaultRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<String>, String> {
    let res = state.unlock_vault(req.passphrase).map_err(|e| e.to_string())?;
    Arc::clone(&state).restore_clients();
    Ok(res)
}

#[tauri::command]
fn set_chat_model(
    req: SetChatModelRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    state.set_chat_model(&req.chat_id, &req.provider, &req.model)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_llm_setting(
    req: LlmSettingRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    state
        .set_llm_setting(&req.provider, &req.api_key, req.base_url.as_deref(), &req.model, req.temperature)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_llm_setting(
    provider: String,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Option<state::LlmSetting>, String> {
    Ok(state.get_llm_setting(&provider))
}

#[tauri::command]
fn list_harness_tools() -> Result<Vec<goble_core::harness::ToolSchema>, String> {
    let harness = Harness::new(Store::open_in_memory().map_err(|e| e.to_string())?);
    Ok(harness.list_tools())
}


#[tauri::command]
fn run_agent(
    req: RunAgentRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    let worker_id = state
        .resolve_worker_for_target(&req.target.kind, req.target.tag.as_deref(), req.target.worker_id.as_deref())
        .map_err(|e| e.to_string())?;
    let agent_id = AgentId(req.agent_id);
    state
        .run_agent(&worker_id, &agent_id, &req.prompt)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn run_agent_for_thread_reply(
    req: RunAgentForThreadReplyRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    let worker_id = state
        .resolve_worker_for_target(&req.target.kind, req.target.tag.as_deref(), req.target.worker_id.as_deref())
        .map_err(|e| e.to_string())?;
    let agent_id = AgentId(req.agent_id);
    state
        .run_agent_for_thread_reply(
            &worker_id,
            &goble_core::thread::ThreadId(req.thread_id),
            &agent_id,
            &req.prompt,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn schedule_agent(
    req: ScheduleRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    let worker_id = WorkerId(req.worker_id);
    let agent_id = AgentId(req.agent_id);
    let trigger = Trigger::Cron {
        expression: req.trigger,
    };
    state
        .schedule_agent(&worker_id, &agent_id, trigger)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn search_mcp_servers(
    req: SearchMcpRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<McpSearchResult>, String> {
    Ok(state.search_mcp_servers(&req.query))
}

#[tauri::command]
fn list_mcp_servers(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<McpServerSummary>, String> {
    state.list_mcp_servers().map_err(|e| e.to_string())
}

#[tauri::command]
fn install_mcp_server(
    req: InstallMcpRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<String, String> {
    state
        .install_mcp_server(&req.id, &req.name, &req.source, req.source_value.as_deref(), req.secret_ids, None)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn update_mcp_server(
    req: UpdateMcpRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<String, String> {
    state
        .update_mcp_server(&req.id, req.name.as_deref(), req.source_value.as_deref(), Some(req.secret_ids), None)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn update_mcp_server_meta(
    req: UpdateMcpMetaRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<String, String> {
    state
        .update_mcp_server_meta(&req.id,
            req.secret_ids,
            req.enabled_tools,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_mcp_server(
    req: McpIdRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<String, String> {
    state.delete_mcp_server(&req.id).map_err(|e| e.to_string())
}

#[tauri::command]
fn discover_mcp_tools(
    req: McpIdRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<goble_core::mcp_client::McpTool>, String> {
    state.discover_mcp_tools(&req.id).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
struct TestCallMcpRequest {
    id: String,
    tool_name: String,
    arguments: Option<serde_json::Value>,
}

#[tauri::command]
fn test_call_mcp_tool(
    req: TestCallMcpRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<serde_json::Value, String> {
    state
        .test_call_mcp_tool(&req.id, &req.tool_name, req.arguments.unwrap_or(serde_json::json!({})))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn cancel_harness(chat_id: String) {
    if let Some(cancel) = HARNESS_CANCEL.lock().unwrap().get(&chat_id) {
        cancel.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
fn run_harness(
    req: RunHarnessRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    use goble_core::harness::{HarnessEvent, SandboxedCommandRunner};
    use futures::StreamExt;

    let (llm, model_name) = state.resolve_llm_provider(&req.provider, &req.model);

    let provider_name = if req.provider.is_empty() { "openai" } else { &req.provider };
    let deploy_state = Arc::clone(&state);
    let cancel = Arc::new(AtomicBool::new(false));
    HARNESS_CANCEL.lock().unwrap().insert(req.chat_id.clone(), cancel.clone());
    let harness = Harness::new(state.store_clone())
        .with_llm(llm)
        .with_runner(Arc::new(SandboxedCommandRunner::default_tools()))
        .with_deploy_sender(move |worker_id, msg| deploy_state.send_to_worker(worker_id, msg))
        .with_cancel(cancel.clone());
    let chat_id_for_cleanup = req.chat_id.clone();
    let mut stream = harness.run_turn(&req.chat_id, &req.prompt, provider_name, &model_name);
    let state_clone = Arc::clone(&state);
    let chat_id = req.chat_id.clone();
    tokio::spawn(async move {
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


#[derive(Deserialize)]
pub struct CreateThreadRequest {
    kind: goble_core::thread::ThreadKind,
    title: String,
    is_private: bool,
    participants: Vec<goble_core::thread::Participant>,
    tags: Vec<String>,
}

#[derive(Serialize)]
pub struct ThreadSummary {
    id: String,
    kind: String,
    title: String,
    owner_id: String,
    participants: Vec<goble_core::thread::Participant>,
    tags: Vec<String>,
    created_at: String,
    updated_at: String,
}

impl From<goble_core::thread::Thread> for ThreadSummary {
    fn from(t: goble_core::thread::Thread) -> Self {
        Self {
            id: t.id.0,
            kind: format!("{:?}", t.kind).to_lowercase(),
            title: t.title,
            owner_id: t.owner_id.0,
            participants: t.participants,
            tags: t.tags,
            created_at: t.created_at.to_rfc3339(),
            updated_at: t.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Serialize)]
pub struct ThreadMessageSummary {
    id: String,
    thread_id: String,
    author: goble_core::thread::Participant,
    content: String,
    reply_to: Option<String>,
    tags: Vec<String>,
    participant_mentions: Vec<String>,
    reactions: Vec<ThreadReactionSummary>,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
pub struct ThreadReactionSummary {
    emoji: String,
    participant_id: String,
}

impl From<goble_core::thread::ThreadMessage> for ThreadMessageSummary {
    fn from(m: goble_core::thread::ThreadMessage) -> Self {
        Self {
            id: m.id.0,
            thread_id: m.thread_id.0,
            author: m.author,
            content: m.content,
            reply_to: m.reply_to.map(|r| r.0),
            tags: m.tags,
            participant_mentions: m.participant_mentions.iter().map(|p| p.to_string()).collect(),
            reactions: m
                .reactions
                .into_iter()
                .map(|r| ThreadReactionSummary {
                    emoji: r.emoji,
                    participant_id: r.participant_id.to_string(),
                })
                .collect(),
            created_at: m.created_at.to_rfc3339(),
            updated_at: m.updated_at.to_rfc3339(),
        }
    }
}


#[tauri::command]
fn migrate_legacy_chats_to_threads(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<ThreadSummary>, String> {
    state.migrate_legacy_chats_to_threads()
}
#[tauri::command]
fn list_threads(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Vec<ThreadSummary> {
    state
        .thread_store()
        .list_threads()
        .into_iter()
        .map(ThreadSummary::from)
        .collect()
}

#[tauri::command]
fn create_thread(
    req: CreateThreadRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<ThreadSummary, String> {
    let owner_id = state
        .thread_store()
        .get_profile()
        .map(|p| goble_core::thread::UserId(p.id.to_string()))
        .unwrap_or_else(|| goble_core::thread::UserId::generate());
    let thread = state
        .thread_store()
        .create_thread(req.kind, req.title, owner_id, req.is_private, req.participants, req.tags)
        .map(ThreadSummary::from)
        .map_err(|e| e.to_string())?;
    state.emit("threads:updated", ());
    Ok(thread)
}

#[derive(Deserialize)]
pub struct ThreadIdRequest {
    thread_id: String,
}

#[tauri::command]
fn delete_thread(
    req: ThreadIdRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> bool {
    state.thread_store().delete_thread(&goble_core::thread::ThreadId(req.thread_id))
}

#[derive(Deserialize)]
pub struct AddThreadParticipantRequest {
    thread_id: String,
    participant: goble_core::thread::Participant,
}

#[tauri::command]
fn add_thread_participant(
    req: AddThreadParticipantRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    state
        .thread_store()
        .add_participant(&goble_core::thread::ThreadId(req.thread_id), req.participant)
        .map_err(|e| e.to_string())?;
    state.emit("threads:updated", ());
    Ok(())
}

#[derive(Deserialize)]
pub struct RemoveThreadParticipantRequest {
    thread_id: String,
    participant_id: String,
}

#[tauri::command]
fn remove_thread_participant(
    req: RemoveThreadParticipantRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    state
        .thread_store()
        .remove_participant(
            &goble_core::thread::ThreadId(req.thread_id),
            &goble_core::thread::ParticipantId(req.participant_id),
        )
        .map_err(|e| e.to_string())?;
    state.emit("threads:updated", ());
    Ok(())
}

#[tauri::command]
fn get_thread_participants(
    req: ThreadIdRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<goble_core::thread::Participant>, String> {
    state
        .thread_store()
        .list_participants(&goble_core::thread::ThreadId(req.thread_id))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_thread_messages(
    req: ThreadIdRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<ThreadMessageSummary>, String> {
    state
        .thread_store()
        .list_messages(&goble_core::thread::ThreadId(req.thread_id))
        .map(|messages| messages.into_iter().map(ThreadMessageSummary::from).collect())
        .map_err(|e| e.to_string())
}


#[derive(Deserialize)]
pub struct InviteUserByPublicKeyRequest {
    thread_id: String,
    public_key_pem: String,
    name: String,
}

#[tauri::command]
fn invite_user_by_public_key(
    req: InviteUserByPublicKeyRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<goble_core::thread::Participant, String> {
    let participant = state
        .thread_store()
        .invite_user_by_public_key(
            &goble_core::thread::ThreadId(req.thread_id),
            req.public_key_pem,
            req.name,
        )
        .map_err(|e| e.to_string())?;
    state.emit("threads:updated", ());
    Ok(participant)
}

#[tauri::command]
fn get_authorized_keys(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<goble_core::user::AuthorizedKey>, String> {
    Ok(state.thread_store().list_authorized_keys())
}

#[derive(Deserialize)]
pub struct PostMessageRequest {
    thread_id: String,
    content: String,
    reply_to: Option<String>,
    tags: Vec<String>,
    mentions: Vec<String>,
}

#[tauri::command]
fn post_thread_message(
    req: PostMessageRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<ThreadMessageSummary, String> {
    let author = state
        .thread_store()
        .get_profile()
        .map(|p| goble_core::thread::Participant::User(goble_core::thread::UserId(p.id.to_string())))
        .unwrap_or_else(|| goble_core::thread::Participant::User(goble_core::thread::UserId::generate()));
    let reply_to = req.reply_to.map(goble_core::thread::MessageId);
    let mentions = req
        .mentions
        .into_iter()
        .map(goble_core::thread::ParticipantId)
        .collect();
    let thread_id = goble_core::thread::ThreadId(req.thread_id.clone());
    let message = state
        .thread_store()
        .post_message(
            &thread_id,
            author,
            req.content,
            reply_to,
            req.tags,
            mentions,
        )
        .map(ThreadMessageSummary::from)
        .map_err(|e| e.to_string())?;
    state.emit("thread:messages:updated", ThreadMessagesUpdatedPayload { thread_id: req.thread_id });
    Ok(message)
}

#[derive(Serialize, Clone)]
struct ThreadMessagesUpdatedPayload {
    thread_id: String,
}

#[derive(Deserialize)]
pub struct ReactionRequest {
    thread_id: String,
    message_id: String,
    emoji: String,
}

#[tauri::command]
fn add_thread_reaction(
    req: ReactionRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    let participant_id = state
        .thread_store()
        .get_profile()
        .map(|p| goble_core::thread::ParticipantId::user(p.id.to_string()))
        .unwrap_or_else(|| goble_core::thread::ParticipantId::user(goble_core::thread::UserId::generate().to_string()));
    state
        .thread_store()
        .add_reaction(
            &goble_core::thread::ThreadId(req.thread_id.clone()),
            &goble_core::thread::MessageId(req.message_id),
            participant_id,
            req.emoji,
        )
        .map_err(|e| e.to_string())?;
    state.emit("thread:messages:updated", ThreadMessagesUpdatedPayload { thread_id: req.thread_id });
    Ok(())
}

#[tauri::command]
fn remove_thread_reaction(
    req: ReactionRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    let participant_id = state
        .thread_store()
        .get_profile()
        .map(|p| goble_core::thread::ParticipantId::user(p.id.to_string()))
        .unwrap_or_else(|| goble_core::thread::ParticipantId::user(goble_core::thread::UserId::generate().to_string()));
    state
        .thread_store()
        .remove_reaction(
            &goble_core::thread::ThreadId(req.thread_id.clone()),
            &goble_core::thread::MessageId(req.message_id),
            &participant_id,
            &req.emoji,
        )
        .map_err(|e| e.to_string())?;
    state.emit("thread:messages:updated", ThreadMessagesUpdatedPayload { thread_id: req.thread_id });
    Ok(())
}

#[derive(Deserialize)]
pub struct UserProfileRequest {
    name: String,
    email: String,
    avatar_url: Option<String>,
    public_key_pem: Option<String>,
}

#[tauri::command]
fn get_user_profile(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<goble_core::user::UserProfile, String> {
    state
        .thread_store()
        .get_profile()
        .ok_or_else(|| "profile not found".to_string())
}

#[tauri::command]
fn set_user_profile(
    req: UserProfileRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
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
    state.thread_store().set_profile(profile).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
pub struct AuthorizedKeyRequest {
    id: String,
    name: String,
    public_key_pem: String,
    fingerprint: String,
    thread_ids: Vec<String>,
}

#[tauri::command]
fn list_authorized_keys(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Vec<goble_core::user::AuthorizedKey> {
    state.thread_store().list_authorized_keys()
}

#[tauri::command]
fn add_authorized_key(
    req: AuthorizedKeyRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    let mut key = goble_core::user::AuthorizedKey::new(
        req.id,
        req.name,
        req.public_key_pem,
        req.fingerprint,
    );
    key.thread_ids = req.thread_ids;
    state.thread_store().add_authorized_key(key).map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_authorized_key(
    id: String,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> bool {
    state.thread_store().remove_authorized_key(&id)
}


pub fn run() {
    let state = state::DesktopState::open_default().expect("open store");
    let state_for_setup: Arc<state::DesktopState> = Arc::clone(&state);
    tauri::Builder::default()
        .manage(state)
        .setup(move |app| {
            state_for_setup.set_app_handle(app.handle().clone());
            Arc::clone(&state_for_setup).restore_clients();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_workers,
            install_worker,
            add_worker,
            tag_worker,
            pair_worker,
            worker_logs,
            ping_worker,
            add_log,
            create_chat,
            list_chats,
            chat_messages,
            add_chat_message,
            list_agents,
            create_agent,
            delete_agent,
            list_workflows,
            create_workflow,
            delete_workflow,
            list_teams,
            create_team,
            list_executions,
            list_vault_secrets,
            set_vault_secret,
            unlock_vault,
            set_llm_setting,
            get_llm_setting,
            run_agent_for_thread_reply,
            run_agent,
            schedule_agent,
            set_chat_model,
            run_harness,
            cancel_harness,
            classify_intent,
            list_harness_tools,
            search_mcp_servers,
            install_mcp_server,
            list_mcp_servers,
            delete_mcp_server,
            test_call_mcp_tool,
            update_mcp_server,
            update_mcp_server_meta,
            discover_mcp_tools,
            get_cluster_identity,
            create_cluster,
            import_cluster_key,
            export_cluster_key,
            export_cluster_backup,
            cluster_helm_install,
            unlock_cluster_identity,
            has_cluster_identity,
            list_threads,
            create_thread,
            delete_thread,
            add_thread_participant,
            remove_thread_participant,
            get_thread_participants,
            invite_user_by_public_key,
            get_thread_messages,
            post_thread_message,
            add_thread_reaction,
            remove_thread_reaction,
            get_user_profile,
            set_user_profile,
            list_authorized_keys,
            add_authorized_key,
            remove_authorized_key
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
