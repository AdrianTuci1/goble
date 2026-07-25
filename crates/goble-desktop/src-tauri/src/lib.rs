// Tauri + React application entry
// This crate is not included in the Cargo workspace because it is a Tauri project.
use goble_core::agent::{AgentId, Trigger};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use goble_core::harness::Harness;
use goble_core::protocol::DesktopMessage;
use goble_core::store::Store;
use goble_core::worker::WorkerId;
use goble_core::workflow::{WorkflowId, WorkflowStep};
use serde::Deserialize;

pub mod state;
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
struct RunAgentRequest {
    worker_id: String,
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


static HARNESS_CANCEL: once_cell::sync::Lazy<std::sync::Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
#[tauri::command]
fn list_workers(
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<Vec<state::WorkerConnection>, String> {
    Ok(state.list_workers())
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
        }))
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

    let provider_name = if req.provider.is_empty() { "openai" } else { &req.provider };
    let (llm, model_name): (Arc<dyn goble_core::llm::LlmProvider>, String) = match provider_name.to_lowercase().as_str() {
        "openai" | "openrouter" => {
            let setting = state.get_llm_setting(provider_name);
            if let Some(s) = setting {
                if !s.api_key.is_empty() {
                    let base = s.base_url.unwrap_or_else(|| {
                        if provider_name == "openai" {
                            "https://api.openai.com/v1".to_string()
                        } else {
                            "https://openrouter.ai/api/v1".to_string()
                        }
                    });
                    let provider: Arc<dyn goble_core::llm::LlmProvider> = if provider_name == "openai" {
                        Arc::new(goble_core::llm::OpenAiProvider::new("openai", s.api_key, base))
                    } else {
                        Arc::new(goble_core::llm::OpenRouterProvider::new(s.api_key))
                    };
                    let model = if req.model.is_empty() { s.model } else { req.model.clone() };
                    (provider, model)
                } else {
                    (Arc::new(goble_core::llm::MockProvider::new("mock", goble_core::llm::CompletionResponse {
                        content: "No API key configured for this provider. Add one in Settings.".to_string(),
                        tool_calls: Vec::new(),
                    })), req.model.clone())
                }
            } else {
                (Arc::new(goble_core::llm::MockProvider::new("mock", goble_core::llm::CompletionResponse {
                    content: "No LLM provider configured. Add one in Settings.".to_string(),
                    tool_calls: Vec::new(),
                })), req.model.clone())
            }
        }
        "anthropic" => {
            let setting = state.get_llm_setting("anthropic");
            if let Some(s) = setting {
                if !s.api_key.is_empty() {
                    (Arc::new(goble_core::llm::AnthropicProvider::new(s.api_key)), if req.model.is_empty() { s.model } else { req.model.clone() })
                } else {
                    (Arc::new(goble_core::llm::MockProvider::new("mock", goble_core::llm::CompletionResponse {
                        content: "No Anthropic API key configured. Add one in Settings.".to_string(),
                        tool_calls: Vec::new(),
                    })), req.model.clone())
                }
            } else {
                (Arc::new(goble_core::llm::MockProvider::new("mock", goble_core::llm::CompletionResponse {
                    content: "No Anthropic provider configured. Add one in Settings.".to_string(),
                    tool_calls: Vec::new(),
                })), req.model.clone())
            }
        }
        "ollama" => {
            let setting = state.get_llm_setting("ollama");
            let base = setting.as_ref().and_then(|s| s.base_url.clone()).unwrap_or_else(|| "http://localhost:11434".to_string());
            (Arc::new(goble_core::llm::OllamaProvider::new(base)), if req.model.is_empty() { setting.map(|s| s.model).unwrap_or_default() } else { req.model.clone() })
        }
        "deepseek" => {
            let setting = state.get_llm_setting("deepseek");
            if let Some(s) = setting {
                if !s.api_key.is_empty() {
                    let base = s.base_url.unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
                    let provider: Arc<dyn goble_core::llm::LlmProvider> = Arc::new(goble_core::llm::OpenAiProvider::new("deepseek", s.api_key, base));
                    let model = if req.model.is_empty() { s.model } else { req.model.clone() };
                    (provider, model)
                } else {
                    (Arc::new(goble_core::llm::MockProvider::new("mock", goble_core::llm::CompletionResponse {
                        content: "No DeepSeek API key configured. Add one in Settings.".to_string(),
                        tool_calls: Vec::new(),
                    })), req.model.clone())
                }
            } else {
                (Arc::new(goble_core::llm::MockProvider::new("mock", goble_core::llm::CompletionResponse {
                    content: "No DeepSeek provider configured. Add one in Settings.".to_string(),
                    tool_calls: Vec::new(),
                })), req.model.clone())
            }
        }
        _ => {
            (Arc::new(goble_core::llm::MockProvider::new("mock", goble_core::llm::CompletionResponse {
                content: format!("Unknown provider {provider_name}."),
                tool_calls: Vec::new(),
            })), req.model.clone())
        }
    };

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
    state
        .unlock_vault(req.passphrase)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn run_agent(
    req: RunAgentRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    let worker_id = WorkerId(req.worker_id);
    let agent_id = AgentId(req.agent_id);
    state
        .run_agent(&worker_id, &agent_id, &req.prompt)
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

pub fn run() {
    let state = state::DesktopState::open_default().expect("open store");
    let _ = state.load_from_store();

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            list_workers,
            add_worker,
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
            run_agent,
            schedule_agent,
            set_chat_model,
            run_harness,
            cancel_harness,
            list_harness_tools
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
