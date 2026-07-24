// Tauri + React application entry
// This crate is not included in the Cargo workspace because it is a Tauri project.
use goble_core::agent::{AgentId, Trigger};
use goble_core::harness::{Harness, HarnessEvent};
use goble_core::protocol::DesktopMessage;
use goble_core::store::Store;
use goble_core::worker::WorkerId;
use goble_core::workflow::{WorkflowId, WorkflowStep};
use serde::Deserialize;
use std::sync::Arc;

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
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<String, String> {
    state.create_chat(&title).map_err(|e| e.to_string())
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
struct RunHarnessRequest {
    chat_id: String,
    prompt: String,
}

#[tauri::command]
fn list_harness_tools() -> Result<Vec<goble_core::harness::ToolSchema>, String> {
    let harness = Harness::new(Store::open_in_memory().map_err(|e| e.to_string())?);
    Ok(harness.list_tools())
}

#[tauri::command]
fn run_harness(
    req: RunHarnessRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    use goble_core::harness::SandboxedCommandRunner;
    let deploy_state = Arc::clone(&state);
    let harness = Harness::new(state.store_clone())
        .with_runner(Arc::new(SandboxedCommandRunner::default_tools()))
        .with_deploy_sender(move |worker_id, msg| deploy_state.send_to_worker(worker_id, msg));
    let mut stream = harness.run_turn(&req.chat_id, &req.prompt);
    let state_clone = Arc::clone(&state);
    let chat_id = req.chat_id.clone();
    tokio::spawn(async move {
        use futures::StreamExt;
        while let Some(event) = stream.next().await {
            match event {
                HarnessEvent::AssistantDelta(delta) => {
                    state_clone.add_chat_message(&chat_id, "assistant", &delta).ok();
                }
                HarnessEvent::ToolCallStarted { id, name, arguments } => {
                    let payload = serde_json::json!({
                        "id": id,
                        "name": name,
                        "arguments": arguments
                    });
                    state_clone.add_chat_message(&chat_id, "tool", &payload.to_string()).ok();
                }
                HarnessEvent::ToolCallFinished { id, result } => {
                    let payload = serde_json::json!({
                        "id": id,
                        "status": "finished",
                        "result": result
                    });
                    state_clone.add_chat_message(&chat_id, "tool", &payload.to_string()).ok();
                }
                HarnessEvent::ToolCallError { id, message } => {
                    let payload = serde_json::json!({
                        "id": id,
                        "status": "error",
                        "message": message
                    });
                    state_clone.add_chat_message(&chat_id, "tool", &payload.to_string()).ok();
                }
                HarnessEvent::Done => {}
                HarnessEvent::Error(e) => {
                    state_clone.add_log(format!("harness error: {e}"));
                }
            }
        }
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
            run_agent,
            schedule_agent,
            run_harness,
            list_harness_tools
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
