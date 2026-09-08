use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use goble_daemon::DaemonState;
use goble_daemon_client::{BroadcastSink, DaemonClient, InProcessClient};
use goble_daemon_protocol::DaemonEvent;
use goble_harness_runtime::HarnessRegistry;
use parking_lot::Mutex;
use tokio::sync::broadcast;

use crate::file_vault::FileVault;
use crate::leader::LeaderState;
use crate::scheduler::Scheduler;
use goble_core::agent::{AgentId, AgentSpec, McpServer};
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
    /// The embedded daemon (composition root) that agent runs are routed through.
    /// Lazily built on first use so `config.workspace_root` is known.
    pub daemon_state: Mutex<Option<Arc<DaemonState>>>,
    /// GUI/worker-side facade for the daemon, used to drive `run`/`resume`.
    pub daemon_client: Mutex<Option<Arc<dyn DaemonClient>>>,
    /// Guards the single daemon-event -> `WorkerMessage` translator task so it
    /// is spawned exactly once per `AppState`.
    daemon_translator_spawned: std::sync::Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub workspace_root: std::path::PathBuf,
    pub llm_provider: Option<String>,
    pub llm_model: Option<String>,
    pub llm_base_url: Option<String>,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            workspace_root: std::path::PathBuf::from("/var/goblin/workspaces"),
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
            file_vault: Mutex::new(FileVault::new(PathBuf::from("/var/goblin/vault.json"))),
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
            daemon_state: Mutex::new(None),
            daemon_client: Mutex::new(None),
            daemon_translator_spawned: Arc::new(AtomicBool::new(false)),
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

    /// The GUI/worker-side daemon facade. Builds the embedded daemon on first
    /// use and returns the client used to drive run/resume/cancel.
    pub fn daemon(self: &Arc<Self>) -> Arc<dyn DaemonClient> {
        self.ensure_daemon();
        self.spawn_daemon_translator();
        self.daemon_client
            .lock()
            .clone()
            .expect("daemon client initialized with daemon_state")
    }

    /// The [`DaemonState`] the composition root seeds with harnesses.
    pub fn daemon_state(self: &Arc<Self>) -> Arc<DaemonState> {
        self.ensure_daemon()
    }

    /// Lazily build the embedded daemon: a [`DaemonState`] over the worker's
    /// store address, a [`BroadcastSink`] fanning events to a broadcast channel,
    /// and an [`InProcessClient`] offering the same [`DaemonClient`] surface the
    /// GUI uses. `config.workspace_root` is read at build time so each session's
    /// harness run gets a per-session agent workspace under `<root>/harness/`.
    fn ensure_daemon(self: &Arc<Self>) -> Arc<DaemonState> {
        let mut daemon_state = self.daemon_state.lock();
        if let Some(daemon) = daemon_state.as_ref() {
            return Arc::clone(daemon);
        }
        let (tx, _) = broadcast::channel(256);
        let sink: Arc<dyn goble_daemon::DaemonEventSink> =
            Arc::new(BroadcastSink::new(tx.clone()));
        let workspace_root = self.config.lock().workspace_root.clone();
        let daemon = DaemonState::new(HarnessRegistry::new(), sink)
            .with_workspace_root(workspace_root);
        let client: Arc<dyn DaemonClient> = Arc::new(InProcessClient::new(daemon.clone(), tx));
        *daemon_state = Some(daemon.clone());
        *self.daemon_client.lock() = Some(client);
        daemon
    }

    /// Spawn the single daemon-event -> [`WorkerMessage`] translator task, once.
    ///
    /// Subscribes to the daemon's broadcast (via the client) and forwards each
    /// streamed event as a [`WorkerMessage`] on the worker's own event channel,
    /// so the WebSocket / desktop observer sees the same live events the daemon
    /// emits. Relay/lifecycle frames (`Done`, `TraceStarted`, `TraceFinished`)
    /// are dropped: they do not map to worker messages and the runner emits its
    /// own `AgentStarted`/`AgentFinished`.
    fn spawn_daemon_translator(self: &Arc<Self>) {
        if self.daemon_translator_spawned.swap(true, Ordering::SeqCst) {
            return;
        }
        let client = self
            .daemon_client
            .lock()
            .clone()
            .expect("daemon client initialized with daemon_state");
        let mut rx = client.subscribe();
        let this = Arc::clone(self);
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                while let Ok(event) = rx.recv().await {
                    if let Some(msg) = map_daemon_event(event) {
                        this.emit(msg);
                    }
                }
            });
        } else {
            // No tokio runtime is current; daemon runs always happen inside a
            // runtime, so defer the spawn until the next access and reset the
            // flag so it is retried.
            self.daemon_translator_spawned.store(false, Ordering::SeqCst);
        }
    }
}

/// Map one daemon [`DaemonEvent`] into the `WorkerMessage` shape the worker's
/// event channel carries. The relay/lifecycle frames (`Done`, `TraceStarted`,
/// `TraceFinished`) do not correspond to worker messages — the runner emits its
/// own `AgentStarted`/`AgentFinished` — so they are dropped.
pub(crate) fn map_daemon_event(event: DaemonEvent) -> Option<WorkerMessage> {
    use goble_daemon_protocol::DaemonEvent as DE;
    match event {
        DE::AssistantDelta { session_id, delta } => Some(WorkerMessage::AssistantDelta {
            trace_id: session_id.0,
            delta,
        }),
        DE::ToolCallStarted {
            session_id,
            id,
            name,
            arguments,
        } => Some(WorkerMessage::ToolCallStarted {
            trace_id: session_id.0,
            id,
            name,
            arguments,
        }),
        DE::ToolCallFinished {
            session_id,
            id,
            result,
        } => Some(WorkerMessage::ToolCallFinished {
            trace_id: session_id.0,
            id,
            result,
        }),
        DE::ToolCallError {
            session_id,
            id,
            message,
        } => Some(WorkerMessage::ToolCallError {
            trace_id: session_id.0,
            id,
            message,
        }),
        DE::AskUser {
            session_id,
            question,
            quick_replies,
        } => Some(WorkerMessage::AskUser {
            trace_id: session_id.0,
            question,
            quick_replies,
        }),
        DE::MissionUpdated {
            session_id,
            mission_id,
            status,
        } => Some(WorkerMessage::MissionUpdated {
            trace_id: session_id.0,
            mission_id,
            status,
        }),
        DE::Error { session_id, message } => Some(WorkerMessage::AgentLog {
            trace_id: session_id.0,
            step_id: "harness".to_string(),
            level: goble_core::execution::LogLevel::Error,
            message,
        }),
        DE::Done { .. }
        | DE::TraceStarted { .. }
        | DE::TraceFinished { .. }
        | DE::ScreenHandoff { .. } => None,
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

    #[test]
    fn test_map_daemon_event_assistant_delta() {
        let ev = DaemonEvent::AssistantDelta {
            session_id: goble_harness_types::SessionId::new("trace-1"),
            delta: "hello".to_string(),
        };
        let msg = map_daemon_event(ev).unwrap();
        assert!(matches!(
            msg,
            WorkerMessage::AssistantDelta { trace_id, delta } if trace_id == "trace-1" && delta == "hello"
        ));
    }

    #[test]
    fn test_map_daemon_event_drops_lifecycle_frames() {
        let sid = goble_harness_types::SessionId::new("trace-1");
        assert!(map_daemon_event(DaemonEvent::Done { session_id: sid.clone() }).is_none());
        assert!(map_daemon_event(DaemonEvent::TraceStarted {
            session_id: sid.clone(),
            trace_id: "t".to_string(),
            project_id: goble_harness_types::ProjectId::new("p"),
            medium_id: goble_harness_types::MediumId::new("m"),
        }).is_none());
        assert!(map_daemon_event(DaemonEvent::TraceFinished {
            session_id: sid,
            trace_id: "t".to_string(),
            status: "success".to_string(),
            project_id: goble_harness_types::ProjectId::new("p"),
            medium_id: goble_harness_types::MediumId::new("m"),
        }).is_none());
    }

    #[test]
    fn test_daemon_composition_builds_without_runtime() {
        let state = AppState::new(WorkerId::generate());
        let daemon_state = state.daemon_state();
        let _client = state.daemon();
        // Both accessors expose the same daemon instance.
        assert!(Arc::ptr_eq(&daemon_state, &state.daemon_state()));
    }
}
