use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::broadcast;

use crate::file_vault::FileVault;
use crate::leader::LeaderState;
use crate::scheduler::Scheduler;
use goble_core::agent::{AgentId, AgentSpec, McpServer};
use goble_core::app_home::GobleHome;
use goble_core::cluster_key::ClusterKey;
use goble_core::execution::ExecutionTrace;
use goble_core::identity::Identity;
use goble_core::protocol::WorkerMessage;
use goble_core::provision::WorkerBundle;
use goble_core::secret::Secret;
use goble_core::snapshot::SnapshotProvider;
use goble_core::store::Store;
use goble_core::worker::WorkerId;

/// Shared application state for the Goblin worker.
pub struct AppState {
    pub worker_id: WorkerId,
    pub pairing_hash: Mutex<Option<String>>,
    pub worker_bundle: Mutex<Option<WorkerBundle>>,
    pub desktop_identity: Mutex<Option<Identity>>,
    pub agents: Mutex<std::collections::HashMap<AgentId, AgentSpec>>,
    pub mcp_servers: Mutex<std::collections::HashMap<String, McpServer>>,
    pub secrets: Mutex<std::collections::HashMap<String, Secret>>,
    pub file_vault: Mutex<FileVault>,
    pub traces: Mutex<std::collections::HashMap<String, ExecutionTrace>>,
    pub event_tx: broadcast::Sender<WorkerMessage>,
    pub scheduler: Mutex<Option<Arc<Scheduler>>>,
    pub config: Mutex<WorkerConfig>,
    pub store: Mutex<Option<Store>>,
    pub store_path: Mutex<Option<PathBuf>>,
    pub cluster_key: Mutex<Option<ClusterKey>>,
    pub snapshot_provider: Mutex<Option<Arc<dyn SnapshotProvider>>>,
    pub cluster_mode: Mutex<bool>,
    pub leader_state: Mutex<Option<LeaderState>>,
}

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub workspace_root: std::path::PathBuf,
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
    pub llm_base_url: Option<String>,
}

fn default_workspace_root() -> PathBuf {
    GobleHome::locate()
        .and_then(|h| {
            h.ensure_workspace().ok();
            Ok(h.workspaces_dir())
        })
        .unwrap_or_else(|_| PathBuf::from("/tmp/goblin/workspaces"))
}

fn default_vault_path() -> PathBuf {
    GobleHome::locate()
        .and_then(|h| {
            h.ensure_base().ok();
            Ok(h.root().join("vault.json"))
        })
        .unwrap_or_else(|_| PathBuf::from("/tmp/goblin/vault.json"))
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            workspace_root: default_workspace_root(),
            llm_provider: std::env::var("LLM_PROVIDER").ok(),
            llm_model: std::env::var("LLM_MODEL").ok(),
            llm_base_url: std::env::var("LLM_BASE_URL").ok(),
        }
    }
}

impl AppState {
    pub fn new(worker_id: WorkerId) -> Arc<Self> {
        let (event_tx, _rx) = broadcast::channel(1024);
        Arc::new(Self {
            worker_id,
            pairing_hash: Mutex::new(None),
            worker_bundle: Mutex::new(None),
            desktop_identity: Mutex::new(None),
            agents: Mutex::new(std::collections::HashMap::new()),
            mcp_servers: Mutex::new(std::collections::HashMap::new()),
            secrets: Mutex::new(std::collections::HashMap::new()),
            file_vault: Mutex::new(FileVault::new(default_vault_path())),
            traces: Mutex::new(std::collections::HashMap::new()),
            event_tx,
            scheduler: Mutex::new(None),
            config: Mutex::new(WorkerConfig::default()),
            store: Mutex::new(None),
            store_path: Mutex::new(None),
            cluster_key: Mutex::new(None),
            snapshot_provider: Mutex::new(None),
            cluster_mode: Mutex::new(false),
            leader_state: Mutex::new(None),
        })
    }

    pub fn set_cluster_mode(&self, cluster_mode: bool) {
        *self.cluster_mode.lock() = cluster_mode;
    }

    pub fn cluster_mode(&self) -> bool {
        *self.cluster_mode.lock()
    }

    pub fn set_leader_state(&self, leader_state: LeaderState) {
        *self.leader_state.lock() = Some(leader_state);
    }

    pub fn leader_state(&self) -> Option<LeaderState> {
        self.leader_state.lock().clone()
    }

    pub fn is_scheduler_leader(&self) -> bool {
        self.leader_state().map(|l| l.is_leader()).unwrap_or(true)
    }

    pub fn set_scheduler(&self, scheduler: Arc<Scheduler>) {
        *self.scheduler.lock() = Some(scheduler);
    }

    pub fn scheduler(&self) -> Option<Arc<Scheduler>> {
        self.scheduler.lock().clone()
    }

    pub fn set_pairing_hash(&self, hash: String) {
        *self.pairing_hash.lock() = Some(hash);
    }

    pub fn set_worker_bundle(&self, bundle: WorkerBundle) {
        *self.worker_bundle.lock() = Some(bundle);
    }

    pub fn worker_bundle(&self) -> Option<WorkerBundle> {
        self.worker_bundle.lock().clone()
    }

    pub fn set_desktop_identity(&self, identity: Identity) {
        *self.desktop_identity.lock() = Some(identity);
    }

    pub fn requires_pairing_hash(&self) -> bool {
        self.worker_bundle.lock().is_none()
    }

    pub fn is_paired(&self, hash: &str) -> bool {
        if self.worker_bundle.lock().is_some() {
            return true;
        }
        self.pairing_hash.lock().as_ref() == Some(&hash.to_string())
    }

    pub fn is_mtls_active(&self) -> bool {
        self.worker_bundle.lock().is_some()
    }

    pub fn store_agent(&self, spec: AgentSpec) {
        self.agents.lock().insert(spec.id.clone(), spec);
    }

    pub fn store_mcp(&self, server: McpServer) {
        self.mcp_servers.lock().insert(server.id.clone(), server);
    }

    pub fn store_secret(&self, secret: Secret) {
        self.secrets.lock().insert(secret.id.clone(), secret);
    }

    pub fn set_vault_path(&self, path: std::path::PathBuf) {
        let mut vault = self.file_vault.lock();
        vault.set_path(path);
    }

    pub fn load_vault(&self, passphrase: &[u8]) -> anyhow::Result<()> {
        self.file_vault.lock().load(passphrase)
    }

    pub fn save_vault(&self, passphrase: &[u8]) -> anyhow::Result<()> {
        self.file_vault.lock().save(passphrase)
    }

    pub fn set_store_path(&self, path: std::path::PathBuf) -> anyhow::Result<()> {
        *self.store_path.lock() = Some(path);
        Ok(())
    }

    pub fn store_path(&self) -> Option<PathBuf> {
        self.store_path.lock().clone()
    }

    pub fn store(&self) -> anyhow::Result<Store> {
        let mut store = self.store.lock();
        if store.is_none() {
            if let Some(path) = self.store_path() {
                *store = Some(Store::open(path)?);
            }
        }
        store
            .clone()
            .ok_or_else(|| anyhow::anyhow!("worker store not initialized"))
    }

    pub fn set_cluster_key(&self, key: ClusterKey) {
        *self.cluster_key.lock() = Some(key);
    }

    pub fn set_snapshot_provider(&self, provider: Arc<dyn SnapshotProvider>) {
        *self.snapshot_provider.lock() = Some(provider);
    }

    pub fn snapshot_provider(&self) -> Option<Arc<dyn SnapshotProvider>> {
        self.snapshot_provider.lock().clone()
    }

    pub fn cluster_key(&self) -> Option<ClusterKey> {
        self.cluster_key.lock().clone()
    }

    pub fn store_trace(&self, trace: ExecutionTrace) {
        self.traces.lock().insert(trace.id.clone(), trace);
    }

    #[allow(dead_code)]
    pub fn get_trace(&self, id: &str) -> Option<ExecutionTrace> {
        self.traces.lock().get(id).cloned()
    }

    pub fn update_trace<F>(&self, id: &str, f: F)
    where
        F: FnOnce(&mut ExecutionTrace),
    {
        let mut traces = self.traces.lock();
        if let Some(trace) = traces.get_mut(id) {
            f(trace);
        }
    }

    pub fn emit(&self, msg: WorkerMessage) {
        let _ = self.event_tx.send(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_pairing() {
        let state = AppState::new(WorkerId::generate());
        assert!(!state.is_paired("abc"));
        state.set_pairing_hash("abc".to_string());
        assert!(state.is_paired("abc"));
    }

    #[test]
    fn test_state_agent_storage() {
        let state = AppState::new(WorkerId::generate());
        let spec = AgentSpec::new("demo", "do nothing");
        let id = spec.id.clone();
        state.store_agent(spec.clone());
        let stored = state.agents.lock().get(&id).cloned();
        assert_eq!(stored, Some(spec));
    }
}
