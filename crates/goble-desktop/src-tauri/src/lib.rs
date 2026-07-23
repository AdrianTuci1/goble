// Tauri + React application entry
// This crate is not included in the Cargo workspace because it is a Tauri project.
use goble_core::agent::AgentId;
use goble_core::protocol::DesktopMessage;
use goble_core::worker::WorkerId;
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
fn run_agent(
    req: RunAgentRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    let worker_id = WorkerId(req.worker_id);
    let agent_id = AgentId(req.agent_id);
    let spec = goble_core::agent::AgentSpec::new(&agent_id.0, &req.prompt);
    state
        .send_to_worker(
            &worker_id,
            DesktopMessage::RunAgent {
                trace_id: format!("desktop-{}", uuid::Uuid::new_v4()),
                agent_id,
                spec,
            },
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
    let trigger = goble_core::agent::Trigger::Cron {
        expression: req.trigger,
    };
    state
        .send_to_worker(
            &worker_id,
            DesktopMessage::ScheduleAgent { agent_id, trigger },
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_vault_secret(
    req: VaultSecretRequest,
    state: tauri::State<'_, Arc<state::DesktopState>>,
) -> Result<(), String> {
    let workers = state.list_workers();
    let worker_id = workers
        .into_iter()
        .find(|w| w.paired)
        .map(|w| WorkerId(w.id))
        .ok_or("no paired worker")?;
    state
        .send_to_worker(
            &worker_id,
            DesktopMessage::SetVaultSecret {
                name: req.name,
                value: req.value.into_bytes(),
            },
        )
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
            chat_messages,
            add_chat_message,
            run_agent,
            schedule_agent,
            set_vault_secret
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
