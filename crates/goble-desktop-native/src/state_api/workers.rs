use std::sync::Arc;

use goble_core::protocol::DesktopMessage;
use goble_core::worker::WorkerId;
use goble_desktop_service::{
    DesktopState, LogEntry, WorkerConnection, WorkerInstallResult, WorkerInvite,
};

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
