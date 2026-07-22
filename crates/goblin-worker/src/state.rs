use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::broadcast;

use goble_core::agent::{AgentId, AgentSpec, McpServer};
use goble_core::execution::ExecutionTrace;
use goble_core::protocol::WorkerMessage;
use goble_core::secret::Secret;
use goble_core::worker::WorkerId;

/// Shared application state for the Goblin worker.
pub struct AppState {
    pub worker_id: WorkerId,
    pub pairing_salt: [u8; 16],
    pub pairing_hash: Mutex<Option<String>>,
    pub agents: Mutex<HashMap<AgentId, AgentSpec>>,
    pub mcp_servers: Mutex<HashMap<String, McpServer>>,
    pub secrets: Mutex<HashMap<String, Secret>>,
    pub traces: Mutex<HashMap<String, ExecutionTrace>>,
    pub event_tx: broadcast::Sender<WorkerMessage>,
    pub config: Mutex<WorkerConfig>,
}

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub workspace_root: std::path::PathBuf,
    pub base_url: String,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            workspace_root: std::path::PathBuf::from("/var/goblin/workspaces"),
            base_url: "http://0.0.0.0:8787".to_string(),
        }
    }
}

impl AppState {
    pub fn new(worker_id: WorkerId) -> Arc<Self> {
        let (event_tx, _rx) = broadcast::channel(1024);
        Arc::new(Self {
            worker_id,
            pairing_salt: goble_core::crypto::generate_salt(),
            pairing_hash: Mutex::new(None),
            agents: Mutex::new(HashMap::new()),
            mcp_servers: Mutex::new(HashMap::new()),
            secrets: Mutex::new(HashMap::new()),
            traces: Mutex::new(HashMap::new()),
            event_tx,
            config: Mutex::new(WorkerConfig::default()),
        })
    }

    pub fn set_pairing_hash(&self, hash: String) {
        *self.pairing_hash.lock() = Some(hash);
    }

    pub fn is_paired(&self, hash: &str) -> bool {
        self.pairing_hash.lock().as_ref() == Some(&hash.to_string())
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

    pub fn store_trace(&self, trace: ExecutionTrace) {
        self.traces.lock().insert(trace.id.clone(), trace);
    }

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
