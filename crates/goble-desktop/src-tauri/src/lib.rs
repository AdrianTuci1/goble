// Tauri + React application entry
// This crate is not included in the Cargo workspace because it is a Tauri project.
use goble_core::protocol::DesktopMessage;
use goble_core::store::Store;
use goble_core::worker::WorkerId;
use serde::Serialize;
use std::sync::Arc;

pub mod state;

#[derive(Serialize)]
struct WorkerInfo {
    id: String,
    name: String,
    url: String,
    paired: bool,
}

#[tauri::command]
fn list_workers(state: tauri::State<'_, Arc<state::DesktopState>>) -> Result<Vec<WorkerInfo>, String> {
    let workers = state.list_workers();
    Ok(workers
        .into_iter()
        .map(|w| WorkerInfo {
            id: w.worker_id.to_string(),
            name: w.name,
            url: w.url,
            paired: w.paired,
        })
        .collect())
}

#[tauri::command]
fn worker_logs(state: tauri::State<'_, Arc<state::DesktopState>>) -> Result<Vec<String>, String> {
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

pub fn run() {
    let store = Store::open_in_memory().expect("open store");
    let state = state::DesktopState::new(store);

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            list_workers,
            worker_logs,
            ping_worker,
            add_log
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
