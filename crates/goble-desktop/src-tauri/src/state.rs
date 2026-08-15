use std::collections::HashMap;
use std::sync::Arc;

use crate::ThreadSummary;
use anyhow::Context;
use base64::Engine;
use chrono::Utc;
use goble_core::agent::{AgentId, AgentSpec, Trigger};
use goble_core::cluster_key::{ClusterBackup, ClusterIdentity, ClusterKey};
use goble_core::encrypted_wallet::IdentityWallet;
use goble_core::execution::{ExecutionTrace, LogLevel, TraceEvent};
use goble_core::identity::ClusterRole;
use goble_core::llm::{self, CompletionRequest, LlmProvider};
use goble_core::mcp_client::McpTool;
use goble_core::mcp_manager::{McpManager, McpServerSummary};
use goble_core::mcp_registry::McpSearchResult;
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use goble_core::store::Store;
use goble_core::thread::{Participant, ThreadId, ThreadKind, UserId};
use goble_core::vault::CredentialVault;
use goble_core::worker::{WorkerConfig, WorkerId};
use goble_core::worker_pool::{WorkerPool, WorkerPoolStrategy, WorkerSnapshot};
use goble_core::workflow::{Workflow, WorkflowId, WorkflowStep};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::worker_manager::WorkerClient;

const WORKER_PAIRING_CODE_VAULT_PREFIX: &str = "worker:";

fn parse_mcp_source(
    source: &str,
    source_value: Option<&str>,
) -> anyhow::Result<goble_core::agent::McpSource> {
    match source {
        "npm" => Ok(goble_core::agent::McpSource::Npm {
            package: source_value.unwrap_or("").to_string(),
            version: "latest".to_string(),
        }),
        "github" => {
            let parts: Vec<&str> = source_value.unwrap_or("").split('#').collect();
            Ok(goble_core::agent::McpSource::Github {
                repo: parts.first().unwrap_or(&"").to_string(),
                rev: parts.get(1).unwrap_or(&"main").to_string(),
            })
        }
        "local" => Ok(goble_core::agent::McpSource::Local {
            path: source_value.unwrap_or("").to_string(),
        }),
        "url" | "sse" => Ok(goble_core::agent::McpSource::Url {
            url: source_value.unwrap_or("").to_string(),
        }),
        _ => anyhow::bail!("unknown mcp source: {source}"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConnection {
    pub id: String,
    pub name: String,
    pub url: String,
    pub paired: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: String,
    pub timestamp: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub agent_id: Option<String>,
    pub worker_id: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub spec: AgentSpec,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
    pub trigger: Trigger,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamInfo {
    pub id: String,
    pub name: String,
    pub metadata: String,
    pub created_at: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultSecretInfo {
    pub key: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IntentParams {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub expression: Option<String>,
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub intent: String,
    #[serde(default)]
    pub params: IntentParams,
}

fn fallback_mock() -> (Arc<dyn LlmProvider>, String) {
    (
        Arc::new(llm::MockProvider::new(
            "mock",
            llm::CompletionResponse {
                content: "No LLM provider configured or API key missing. Add one in Settings."
                    .to_string(),
                tool_calls: Vec::new(),
            },
        )),
        "mock".to_string(),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSetting {
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionInfo {
    pub id: String,
    pub agent_id: Option<String>,
    pub worker_id: Option<String>,
    pub status: String,
    pub trace: ExecutionTrace,
    pub started_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ThreadMessagesUpdatedPayload {
    thread_id: String,
}

pub struct DesktopState {
    store: Arc<Mutex<Store>>,
    workers: Arc<Mutex<HashMap<WorkerId, WorkerConnection>>>,
    chats: Arc<Mutex<Vec<Chat>>>,
    messages: Arc<Mutex<HashMap<String, Vec<ChatMessage>>>>,
    agents: Arc<Mutex<HashMap<AgentId, AgentInfo>>>,
    workflows: Arc<Mutex<HashMap<WorkflowId, WorkflowInfo>>>,
    teams: Arc<Mutex<HashMap<String, TeamInfo>>>,
    executions: Arc<Mutex<HashMap<String, ExecutionInfo>>>,
    vault: Arc<Mutex<CredentialVault>>,
    vault_passphrase: Mutex<Vec<u8>>,
    logs: Arc<Mutex<Vec<LogEntry>>>,
    clients: Arc<Mutex<HashMap<WorkerId, WorkerClient>>>,
    mcp_manager: McpManager,
    app_handle: Mutex<Option<AppHandle>>,
    cluster_identity: Mutex<Option<ClusterIdentity>>,
    thread_store: Arc<crate::thread_store::ThreadStore>,
}
impl DesktopState {
    pub fn open_default() -> anyhow::Result<Arc<Self>> {
        let store = Store::open("goble_store.sqlite")?;
        let thread_store_path = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("no data dir"))?
            .join("com.goble.desktop")
            .join("threads");
        let thread_store = crate::thread_store::ThreadStore::new(thread_store_path)?;
        let state = Self::new(store, thread_store);
        let _ = state.load_from_store();
        Ok(state)
    }

    pub fn new(store: Store, thread_store: crate::thread_store::ThreadStore) -> Arc<Self> {
        Arc::new(Self {
            store: Arc::new(Mutex::new(store)),
            workers: Arc::new(Mutex::new(HashMap::new())),
            chats: Arc::new(Mutex::new(Vec::new())),
            messages: Arc::new(Mutex::new(HashMap::new())),
            agents: Arc::new(Mutex::new(HashMap::new())),
            workflows: Arc::new(Mutex::new(HashMap::new())),
            teams: Arc::new(Mutex::new(HashMap::new())),
            executions: Arc::new(Mutex::new(HashMap::new())),
            vault: Arc::new(Mutex::new(CredentialVault::new())),
            vault_passphrase: Mutex::new(Vec::new()),
            logs: Arc::new(Mutex::new(Vec::new())),
            clients: Arc::new(Mutex::new(HashMap::new())),
            mcp_manager: McpManager::new(),
            app_handle: Mutex::new(None),
            cluster_identity: Mutex::new(None),
            thread_store: Arc::new(thread_store),
        })
    }
    pub fn thread_store(&self) -> Arc<crate::thread_store::ThreadStore> {
        Arc::clone(&self.thread_store)
    }

    pub fn set_app_handle(&self, handle: AppHandle) {
        *self.app_handle.lock() = Some(handle);
    }

    /// Reconnect workers that were previously paired and have a stored pairing code in the vault.
    pub fn restore_clients(self: Arc<Self>) {
        let paired: Vec<(WorkerId, String)> = self
            .workers
            .lock()
            .values()
            .filter(|c| c.paired)
            .map(|c| (WorkerId(c.id.clone()), c.url.clone()))
            .collect();
        if paired.is_empty() {
            return;
        }
        let passphrase = self.vault_passphrase.lock().clone();
        if passphrase.is_empty() {
            self.add_log("vault locked; skip auto-reconnect for paired workers");
            return;
        }
        for (wid, _) in paired {
            let config = match self.get_worker_config(&wid) {
                Ok(c) => c,
                Err(e) => {
                    self.add_log(format!("no config for worker {wid}; skip reconnect: {e}"));
                    continue;
                }
            };
            let vault_key = format!("{WORKER_PAIRING_CODE_VAULT_PREFIX}{}:pairing_code", wid);
            let code = match self.vault.lock().get(&vault_key, &passphrase) {
                Ok(Some(v)) => String::from_utf8_lossy(&v).to_string(),
                Ok(None) => {
                    self.add_log(format!(
                        "no stored pairing code for worker {wid}; skip reconnect"
                    ));
                    continue;
                }
                Err(e) => {
                    self.add_log(format!(
                        "failed to decrypt pairing code for worker {wid}: {e}"
                    ));
                    continue;
                }
            };
            let state = self.clone();
            let worker_id = wid.clone();
            tokio::spawn(async move {
                match WorkerClient::connect(state.clone(), worker_id.clone(), &config, code).await {
                    Ok(client) => {
                        state.clients.lock().insert(worker_id.clone(), client);
                        state.add_log(format!("worker {worker_id} reconnected"));
                    }
                    Err(e) => {
                        state.add_log(format!("failed to reconnect worker {worker_id}: {e}"));
                    }
                }
            });
        }
    }

    fn store_pairing_code(&self, worker_id: &WorkerId, pairing_code: &str) {
        let passphrase = self.vault_passphrase.lock().clone();
        if passphrase.is_empty() {
            return;
        }
        let vault_key = format!(
            "{WORKER_PAIRING_CODE_VAULT_PREFIX}{}:pairing_code",
            worker_id
        );
        let _ = self
            .vault
            .lock()
            .set(&vault_key, pairing_code.as_bytes(), &passphrase);
        if let Ok(bytes) = self.vault.lock().to_bytes() {
            let _ = self
                .store
                .lock()
                .set_setting("vault_blob", &String::from_utf8_lossy(&bytes));
        }
    }

    fn device_id() -> String {
        format!(
            "desktop-{}",
            uuid::Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap_or("unknown")
        )
    }

    pub fn export_cluster_key(&self) -> anyhow::Result<String> {
        match self.get_cluster_identity() {
            Some(identity) => Ok(identity.export_key()),
            None => anyhow::bail!("no cluster identity configured"),
        }
    }

    pub fn export_cluster_backup(&self) -> anyhow::Result<ClusterBackup> {
        match self.get_cluster_identity() {
            Some(identity) => identity.export_backup(),
            None => anyhow::bail!("no cluster identity configured"),
        }
    }

    pub fn get_cluster_identity(&self) -> Option<ClusterIdentity> {
        self.cluster_identity.lock().clone()
    }

    pub fn create_cluster(&self, name: &str, passphrase: &str) -> anyhow::Result<ClusterIdentity> {
        let identity = ClusterIdentity::generate(name, &Self::device_id(), ClusterRole::Owner)?;
        self.set_cluster_identity(identity.clone(), passphrase)
    }

    pub fn import_cluster_key(
        &self,
        key_b64: &str,
        name: &str,
        passphrase: &str,
    ) -> anyhow::Result<ClusterIdentity> {
        let key = ClusterKey::from_base64(key_b64)?;
        let identity =
            ClusterIdentity::from_key(key, name, &Self::device_id(), ClusterRole::Admin)?;
        self.set_cluster_identity(identity.clone(), passphrase)
    }

    pub fn export_identity_wallet(&self, passphrase: &str) -> anyhow::Result<String> {
        let identity = self
            .get_cluster_identity()
            .context("no cluster identity unlocked")?;
        let wallet = IdentityWallet::from(&identity);
        let sealed = wallet.seal(passphrase.as_bytes())?;
        Ok(serde_json::to_string(&sealed)?)
    }

    pub fn import_identity_wallet(
        &self,
        wallet_json: &str,
        passphrase: &str,
    ) -> anyhow::Result<ClusterIdentity> {
        let sealed: goble_core::encrypted_wallet::EncryptedWallet =
            serde_json::from_str(wallet_json).context("invalid wallet JSON")?;
        let wallet = IdentityWallet::open(&sealed, passphrase.as_bytes())?;
        let device_id = Self::device_id();
        let identity = wallet.to_cluster_identity(&device_id, ClusterRole::Admin)?;
        *self.cluster_identity.lock() = Some(identity.clone());
        Ok(identity)
    }

    pub fn unlock_cluster_identity(&self, passphrase: &str) -> anyhow::Result<bool> {
        let wallet = self.store.lock().get_cluster_wallet()?;
        match wallet {
            Some(wallet) => {
                let bytes = wallet.open(passphrase.as_bytes())?;
                let identity_wallet: IdentityWallet = serde_json::from_slice(&bytes)?;
                let device_id = Self::device_id();
                let identity =
                    identity_wallet.to_cluster_identity(&device_id, ClusterRole::Admin)?;
                *self.cluster_identity.lock() = Some(identity);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    pub fn has_stored_cluster_identity(&self) -> bool {
        self.store
            .lock()
            .get_cluster_wallet()
            .ok()
            .flatten()
            .is_some()
    }

    /// Generate a `helm install` command for a Goblin worker cluster. Requires an
    /// unlocked cluster identity so the worker mTLS bundle and snapshot key can be
    /// embedded in the generated command.
    #[allow(clippy::too_many_arguments)]
    pub fn cluster_helm_install(
        &self,
        name: String,
        namespace: String,
        replicas: u32,
        storage_class: Option<String>,
        persistence_size: String,
        provider: String,
        endpoint: Option<String>,
        bucket: Option<String>,
        access_key_id: Option<String>,
        secret_access_key: Option<String>,
        region: Option<String>,
        interval_seconds: u64,
        local_chart: Option<String>,
    ) -> anyhow::Result<String> {
        let identity = self
            .get_cluster_identity()
            .context("cluster identity is not unlocked")?;
        let worker_id = format!("{}-0", name);
        let bundle = identity
            .ca
            .sign_worker_bundle(&worker_id, &identity.cluster_name, 365)?;
        let bundle_json = serde_json::to_string(&bundle)?;
        let bundle_b64 = base64::engine::general_purpose::STANDARD.encode(bundle_json);
        let cluster_key_b64 = identity.export_key();

        let chart_ref = local_chart
            .map(|p| format!("{} ", p))
            .unwrap_or_else(|| "goble/goblin-cluster ".to_string());
        let mut parts = vec![
            format!("helm install {} ", name),
            chart_ref,
            format!("--namespace {} --create-namespace ", namespace),
            format!("--set replicas={} ", replicas),
            format!("--set workerBundle={} ", bundle_b64),
            format!("--set clusterKey={} ", cluster_key_b64),
            "--set snapshot.enabled=true ".to_string(),
            format!("--set snapshot.provider={} ", provider),
            format!("--set snapshot.intervalSeconds={} ", interval_seconds),
        ];
        if let Some(region) = region {
            parts.push(format!("--set snapshot.region={} ", region));
        }
        if let Some(endpoint) = endpoint {
            parts.push(format!("--set snapshot.endpoint={} ", endpoint));
        }
        if let Some(bucket) = bucket {
            parts.push(format!("--set snapshot.bucket={} ", bucket));
        }
        if let Some(access_key_id) = access_key_id {
            parts.push(format!("--set snapshot.accessKeyId={} ", access_key_id));
        }
        if let Some(secret_access_key) = secret_access_key {
            parts.push(format!(
                "--set snapshot.secretAccessKey={} ",
                secret_access_key
            ));
        }
        if let Some(storage_class) = storage_class {
            parts.push(format!("--set persistence.storageClass={} ", storage_class));
        }
        parts.push(format!("--set persistence.size={}", persistence_size));
        Ok(parts.join(""))
    }

    fn set_cluster_identity(
        &self,
        identity: ClusterIdentity,
        passphrase: &str,
    ) -> anyhow::Result<ClusterIdentity> {
        let wallet = IdentityWallet::from(&identity);
        let sealed = wallet.seal(passphrase.as_bytes())?;
        self.store.lock().set_cluster_wallet(&sealed)?;
        *self.cluster_identity.lock() = Some(identity.clone());
        self.emit("cluster:updated", ());
        Ok(identity)
    }

    pub fn set_vault_passphrase(&self, passphrase: String) {
        *self.vault_passphrase.lock() = passphrase.into_bytes();
    }

    pub fn add_worker(&self, worker_id: WorkerId, name: String, url: String) -> anyhow::Result<()> {
        let conn = WorkerConnection {
            id: worker_id.to_string(),
            name: name.clone(),
            url: url.clone(),
            paired: false,
            tags: Vec::new(),
        };
        let config = WorkerConfig::new(&name, &url, "");
        self.store.lock().insert_worker(
            &worker_id.to_string(),
            &conn.name,
            Some(&url),
            "unpaired",
            None,
            &serde_json::to_string(&config)?,
            &Utc::now().to_rfc3339(),
            &Utc::now().to_rfc3339(),
        )?;
        self.workers.lock().insert(worker_id, conn);
        self.emit("workers:updated", ());
        Ok(())
    }

    pub fn tag_worker(&self, worker_id: &WorkerId, tag: String) -> anyhow::Result<()> {
        let mut workers = self.workers.lock();
        let conn = workers
            .get_mut(worker_id)
            .ok_or_else(|| anyhow::anyhow!("worker not found"))?;
        if !conn.tags.contains(&tag) {
            conn.tags.push(tag.clone());
        }
        let mut config: WorkerConfig = serde_json::from_str(
            &self
                .store
                .lock()
                .get_worker(&worker_id.to_string())?
                .map(|(_, _, _, cfg)| cfg)
                .unwrap_or_default(),
        )
        .unwrap_or_else(|_| WorkerConfig::new(&conn.name, &conn.url, ""));
        config.tags = conn.tags.clone();
        self.store.lock().insert_worker(
            &worker_id.to_string(),
            &conn.name,
            Some(&conn.url),
            if conn.paired { "paired" } else { "unpaired" },
            None,
            &serde_json::to_string(&config)?,
            &Utc::now().to_rfc3339(),
            &Utc::now().to_rfc3339(),
        )?;
        self.emit("workers:updated", ());
        Ok(())
    }

    pub fn remove_worker(&self, worker_id: &WorkerId) {
        self.workers.lock().remove(worker_id);
        let _ = self.store.lock().delete_worker(&worker_id.to_string());
        self.clients.lock().remove(worker_id);
        self.emit("workers:updated", ());
    }

    pub fn list_workers(&self) -> Vec<WorkerConnection> {
        self.workers.lock().values().cloned().collect()
    }

    pub fn get_worker_config(&self, worker_id: &WorkerId) -> anyhow::Result<WorkerConfig> {
        match self.store.lock().get_worker(&worker_id.to_string())? {
            Some((_, _, _, config_json)) => {
                serde_json::from_str(&config_json).context("invalid worker config")
            }
            None => anyhow::bail!("worker not found"),
        }
    }

    pub fn pair_worker(
        self: Arc<Self>,
        worker_id: &WorkerId,
        pairing_code: String,
    ) -> anyhow::Result<bool> {
        let conn = self.workers.lock().get(worker_id).cloned();
        let cluster = self.get_cluster_identity();
        if let Some(conn) = conn {
            let state = self.clone();
            let wid = worker_id.clone();
            let code = pairing_code.clone();
            tokio::spawn(async move {
                let conn_name = conn.name.clone();
                let url = conn.url.clone();
                let config = match cluster {
                    Some(identity) => {
                        let bundle = match identity.ca.sign_worker_bundle(
                            &wid.to_string(),
                            &identity.cluster_name,
                            365,
                        ) {
                            Ok(b) => b,
                            Err(e) => {
                                state.add_log(format!(
                                    "failed to sign worker bundle for {}: {}",
                                    wid, e
                                ));
                                return;
                            }
                        };
                        let mut cfg = WorkerConfig::new(&conn_name, &url, "");
                        cfg.id = wid.clone();
                        cfg.port = url
                            .split(':')
                            .next_back()
                            .and_then(|p| p.parse().ok())
                            .unwrap_or(7878);
                        cfg.worker_bundle = Some(bundle);
                        cfg.desktop_identity = Some(identity.device);
                        cfg
                    }
                    None => {
                        let mut cfg = WorkerConfig::new(&conn_name, &url, "");
                        cfg.id = wid.clone();
                        cfg.port = url
                            .split(':')
                            .next_back()
                            .and_then(|p| p.parse().ok())
                            .unwrap_or(7878);
                        cfg
                    }
                };
                match WorkerClient::connect(state.clone(), wid.clone(), &config, code.clone()).await
                {
                    Ok(client) => {
                        state.clients.lock().insert(wid.clone(), client);
                        if let Some(c) = state.workers.lock().get_mut(&wid) {
                            c.paired = true;
                        }
                        let config_json =
                            serde_json::to_string(&config).unwrap_or_else(|_| "{}".to_string());
                        let _ = state.store.lock().insert_worker(
                            &wid.to_string(),
                            &conn_name,
                            Some(&config.websocket_url()),
                            "paired",
                            None,
                            &config_json,
                            &Utc::now().to_rfc3339(),
                            &Utc::now().to_rfc3339(),
                        );
                        state.store_pairing_code(&wid, &code);
                        state.add_log(format!("worker {} paired", wid));
                        state.emit("workers:updated", ());
                    }
                    Err(e) => {
                        state.add_log(format!("failed to connect worker {}: {}", wid, e));
                    }
                }
            });
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn send_to_worker(&self, worker_id: &WorkerId, msg: DesktopMessage) -> anyhow::Result<()> {
        if let Some(client) = self.clients.lock().get(worker_id) {
            let _ = client.send(msg);
            Ok(())
        } else {
            anyhow::bail!("worker not connected: {}", worker_id)
        }
    }

    /// Resolve an abstract runtime target to a concrete paired worker id.
    pub fn resolve_worker_for_target(
        &self,
        target_kind: &str,
        tag: Option<&str>,
        worker_id: Option<&str>,
    ) -> anyhow::Result<WorkerId> {
        let all_workers = self.list_workers();
        let paired_workers: Vec<WorkerConnection> =
            all_workers.into_iter().filter(|w| w.paired).collect();

        if target_kind == "worker" {
            let id = worker_id.ok_or_else(|| anyhow::anyhow!("worker target missing worker_id"))?;
            if paired_workers.iter().any(|w| w.id == id) {
                return Ok(WorkerId(id.to_string()));
            }
            anyhow::bail!("worker {} is not paired", id);
        }

        if target_kind == "local" {
            anyhow::bail!("local runtime target is not supported yet");
        }

        let strategy = match tag {
            Some(t) => WorkerPoolStrategy::TaggedFirst { tag: t.to_string() },
            None => WorkerPoolStrategy::RoundRobin,
        };
        let mut pool = WorkerPool::new(strategy);
        let snapshots: Vec<WorkerSnapshot> = paired_workers
            .iter()
            .map(|w| WorkerSnapshot {
                worker_id: WorkerId(w.id.clone()),
                name: w.name.clone(),
                url: w.url.clone(),
                status: goble_core::worker::WorkerStatus::Online,
                load: 0,
                tags: w.tags.clone(),
            })
            .collect();
        pool.select(&snapshots)
            .map(|s| s.worker_id.clone())
            .ok_or_else(|| anyhow::anyhow!("no paired worker available for target {}", target_kind))
    }

    pub fn handle_worker_message(&self, worker_id: &WorkerId, msg: WorkerMessage) {
        match msg {
            WorkerMessage::Paired => {
                if let Some(c) = self.workers.lock().get_mut(worker_id) {
                    c.paired = true;
                }
                self.emit("workers:updated", ());
                self.add_log(format!("worker {} paired confirmed", worker_id));
            }
            WorkerMessage::AgentLog {
                trace_id,
                step_id: _,
                level,
                message,
            } => {
                let entry = format!("[{}] [{}] {:?}: {}", worker_id, trace_id, level, message);
                self.add_log(entry);
                if let Some(exec) = self.executions.lock().get_mut(&trace_id) {
                    let level = match level {
                        goble_core::execution::LogLevel::Debug => LogLevel::Debug,
                        goble_core::execution::LogLevel::Info => LogLevel::Info,
                        goble_core::execution::LogLevel::Warn => LogLevel::Warn,
                        goble_core::execution::LogLevel::Error => LogLevel::Error,
                    };
                    exec.trace.add_event(TraceEvent::Log {
                        timestamp: Utc::now(),
                        level,
                        message: message.clone(),
                    });
                }
                self.emit(
                    "agent:log",
                    serde_json::json!({
                        "worker_id": worker_id.to_string(),
                        "trace_id": trace_id,
                        "level": format!("{:?}", level),
                        "message": message,
                    }),
                );
                self.emit("executions:updated", ());
            }
            WorkerMessage::AgentStarted { trace_id, agent_id } => {
                self.add_log(format!(
                    "worker {} started agent {} trace {}",
                    worker_id, agent_id, trace_id
                ));
                let mut executions = self.executions.lock();
                executions.insert(
                    trace_id.clone(),
                    ExecutionInfo {
                        id: trace_id.clone(),
                        agent_id: Some(agent_id.to_string()),
                        worker_id: Some(worker_id.to_string()),
                        status: "running".to_string(),
                        trace: ExecutionTrace::new(agent_id.clone()),
                        started_at: Utc::now().to_rfc3339(),
                        finished_at: None,
                    },
                );
                drop(executions);
                self.emit("executions:updated", ());
                self.emit(
                    "agent:started",
                    serde_json::json!({
                        "worker_id": worker_id.to_string(),
                        "trace_id": trace_id,
                        "agent_id": agent_id.to_string(),
                    }),
                );
            }
            WorkerMessage::AgentFinished { trace_id, status } => {
                self.add_log(format!(
                    "worker {} finished trace {} status {:?}",
                    worker_id, trace_id, status
                ));
                if let Some(exec) = self.executions.lock().get_mut(&trace_id) {
                    exec.status = format!("{:?}", status);
                    exec.finished_at = Some(Utc::now().to_rfc3339());
                    exec.trace.finish(status.clone());
                    let _ = self.store.lock().insert_execution(
                        &exec.id,
                        exec.agent_id.as_deref(),
                        exec.worker_id.as_deref(),
                        &exec.status,
                        &serde_json::to_string(&exec.trace).unwrap_or_default(),
                        &exec.started_at,
                        exec.finished_at.as_deref(),
                    );
                }
                self.emit("executions:updated", ());
                self.emit(
                    "agent:finished",
                    serde_json::json!({
                        "worker_id": worker_id.to_string(),
                        "trace_id": trace_id,
                        "status": format!("{:?}", status),
                    }),
                );
            }
            WorkerMessage::AgentStateUpdate { trace_id, state } => {
                self.emit(
                    "agent:state_update",
                    serde_json::json!({
                        "worker_id": worker_id.to_string(),
                        "trace_id": trace_id,
                        "state": state,
                    }),
                );
            }
            WorkerMessage::AssistantDelta { trace_id, delta } => {
                if let Some(exec) = self.executions.lock().get_mut(&trace_id) {
                    exec.trace.add_event(TraceEvent::AssistantDelta {
                        timestamp: Utc::now(),
                        delta,
                    });
                }
                self.emit("executions:updated", ());
            }
            WorkerMessage::ToolCallStarted {
                trace_id,
                id,
                name,
                arguments,
            } => {
                if let Some(exec) = self.executions.lock().get_mut(&trace_id) {
                    exec.trace.add_event(TraceEvent::ToolCallStarted {
                        timestamp: Utc::now(),
                        id,
                        name,
                        arguments,
                    });
                }
                self.emit("executions:updated", ());
            }
            WorkerMessage::ToolCallFinished {
                trace_id,
                id,
                result,
            } => {
                if let Some(exec) = self.executions.lock().get_mut(&trace_id) {
                    exec.trace.add_event(TraceEvent::ToolCallFinished {
                        timestamp: Utc::now(),
                        id,
                        result,
                    });
                }
                self.emit("executions:updated", ());
            }
            WorkerMessage::ToolCallError {
                trace_id,
                id,
                message,
            } => {
                if let Some(exec) = self.executions.lock().get_mut(&trace_id) {
                    exec.trace.add_event(TraceEvent::ToolCallError {
                        timestamp: Utc::now(),
                        id,
                        message,
                    });
                }
                self.emit("executions:updated", ());
            }
            WorkerMessage::AskUser {
                trace_id,
                question,
                quick_replies,
            } => {
                if let Some(exec) = self.executions.lock().get_mut(&trace_id) {
                    exec.trace.add_event(TraceEvent::AskUser {
                        timestamp: Utc::now(),
                        question,
                        quick_replies,
                    });
                }
                self.emit("executions:updated", ());
            }
            WorkerMessage::AgentToolResult {
                trace_id,
                step_id,
                name,
                result,
            } => {
                self.emit(
                    "agent:tool_result",
                    serde_json::json!({
                        "worker_id": worker_id.to_string(),
                        "trace_id": trace_id,
                        "step_id": step_id,
                        "name": name,
                        "result": result,
                    }),
                );
            }
            WorkerMessage::StatusReport {
                worker_id,
                status,
                load,
            } => {
                self.emit(
                    "worker:status",
                    serde_json::json!({
                        "worker_id": worker_id.to_string(),
                        "status": format!("{:?}", status),
                        "load": load,
                    }),
                );
            }
            WorkerMessage::Pong => {
                self.add_log(format!("worker {} pong", worker_id));
            }
            WorkerMessage::VaultSecret { name, value } => {
                if let Some(v) = value {
                    let passphrase = self.vault_passphrase.lock().clone();
                    let _ = self.vault.lock().set(&name, &v, &passphrase);
                    let _ = self.store.lock().insert_vault_secret(
                        &name,
                        &v,
                        "{}",
                        &Utc::now().to_rfc3339(),
                    );
                }
                self.emit(
                    "vault:secret",
                    serde_json::json!({
                        "name": name,
                    }),
                );
            }
            WorkerMessage::VaultError { message } => {
                self.add_log(format!("vault error: {}", message));
            }
            WorkerMessage::ScheduledTasks { tasks } => {
                self.emit(
                    "worker:scheduled_tasks",
                    serde_json::json!({ "tasks": tasks }),
                );
            }
            WorkerMessage::TaskCancelled { task_id } => {
                self.add_log(format!("task {} cancelled", task_id));
                self.emit(
                    "worker:task_cancelled",
                    serde_json::json!({ "task_id": task_id }),
                );
            }
            WorkerMessage::ThreadAgentReply {
                trace_id,
                thread_id,
                content,
            } => {
                self.add_log(format!(
                    "worker {} posted thread reply {}: {}",
                    worker_id, thread_id, content
                ));
                let agent_id = self
                    .executions
                    .lock()
                    .get(&trace_id)
                    .and_then(|e| e.agent_id.clone())
                    .unwrap_or_default();
                let _ = self.thread_store().post_message(
                    &goble_core::thread::ThreadId(thread_id.clone()),
                    goble_core::thread::Participant::Agent(goble_core::agent::AgentId(agent_id)),
                    content,
                    None,
                    vec![],
                    vec![],
                    Some(trace_id.clone()),
                );
                self.emit(
                    "thread:messages:updated",
                    ThreadMessagesUpdatedPayload { thread_id },
                );
            }
            _ => {
                // Ignore unhandled agent runtime and worker message variants for now.
            }
        }
    }

    pub fn add_log(&self, message: impl Into<String>) {
        let entry = LogEntry {
            id: format!("{}", uuid::Uuid::new_v4()),
            timestamp: Utc::now().to_rfc3339(),
            message: message.into(),
        };
        self.logs.lock().push(entry);
        self.emit("logs:updated", ());
    }

    pub fn get_logs(&self) -> Vec<LogEntry> {
        self.logs.lock().clone()
    }

    pub fn add_chat_log(&self, message: impl Into<String>) {
        self.add_log(message);
    }

    pub fn add_chat_message(&self, chat_id: &str, role: &str, content: &str) -> anyhow::Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();
        let tool_calls = if role == "tool" {
            serde_json::from_str::<Vec<serde_json::Value>>(content)
                .ok()
                .and_then(|v| serde_json::to_string(&v).ok())
        } else {
            None
        };
        self.store.lock().insert_chat_message(
            &id,
            chat_id,
            role,
            content,
            tool_calls.as_deref(),
            &created_at,
        )?;
        self.messages
            .lock()
            .entry(chat_id.to_string())
            .or_default()
            .push(ChatMessage {
                id,
                role: role.to_string(),
                content: content.to_string(),
                created_at,
            });
        self.emit("chat:updated", serde_json::json!({ "chat_id": chat_id }));
        Ok(())
    }

    pub fn list_chat_messages(&self, chat_id: &str) -> anyhow::Result<Vec<ChatMessage>> {
        let rows = self.store.lock().list_chat_messages(chat_id)?;
        Ok(rows
            .into_iter()
            .map(|(id, role, content, _, created_at)| ChatMessage {
                id,
                role,
                content,
                created_at,
            })
            .collect())
    }

    pub fn store_clone(&self) -> Store {
        self.store.lock().clone()
    }

    pub fn create_chat(
        &self,
        title: &str,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> anyhow::Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.store
            .lock()
            .insert_chat(&id, title, provider, model, &now, &now)?;
        let chat = Chat {
            id: id.clone(),
            title: title.to_string(),
            provider: provider.map(|s| s.to_string()),
            model: model.map(|s| s.to_string()),
            agent_id: None,
            worker_id: None,
            updated_at: now,
        };
        self.chats.lock().push(chat);
        self.emit("chats:updated", ());
        Ok(id)
    }

    pub fn set_chat_model(&self, id: &str, provider: &str, model: &str) -> anyhow::Result<()> {
        self.store.lock().set_chat_model(id, provider, model)?;
        if let Some(chat) = self.chats.lock().iter_mut().find(|c| c.id == id) {
            chat.provider = Some(provider.to_string());
            chat.model = Some(model.to_string());
        }
        self.emit("chats:updated", ());
        Ok(())
    }

    pub fn list_chats(&self) -> Vec<Chat> {
        self.chats.lock().clone()
    }

    pub fn create_agent(
        &self,
        name: &str,
        prompt: &str,
        description: Option<&str>,
        tools: Vec<String>,
    ) -> anyhow::Result<AgentInfo> {
        let mut spec = AgentSpec::new(name, prompt);
        if let Some(d) = description {
            spec = spec.with_description(d);
        }
        spec = spec.with_tools(tools);
        let id = spec.id.clone();
        let now = Utc::now().to_rfc3339();
        let spec_json = serde_json::to_string(&spec)?;
        self.store
            .lock()
            .insert_agent(&id.to_string(), name, &spec_json, &now, &now)?;
        let info = AgentInfo {
            id: id.to_string(),
            name: name.to_string(),
            spec,
            created_at: now.clone(),
            updated_at: now,
        };
        self.agents.lock().insert(id, info.clone());
        self.emit("agents:updated", ());
        Ok(info)
    }

    pub fn delete_agent(&self, id: &AgentId) -> anyhow::Result<()> {
        self.store.lock().delete_agent(&id.to_string())?;
        self.agents.lock().remove(id);
        self.emit("agents:updated", ());
        Ok(())
    }

    pub fn update_agent(
        &self,
        id: &AgentId,
        name: &str,
        prompt: &str,
        description: Option<&str>,
        tools: Vec<String>,
    ) -> anyhow::Result<AgentInfo> {
        let mut spec = AgentSpec::new(name, prompt);
        if let Some(d) = description {
            spec = spec.with_description(d);
        }
        spec = spec.with_tools(tools);
        spec.id = id.clone();
        let now = Utc::now().to_rfc3339();
        let spec_json = serde_json::to_string(&spec)?;
        self.store
            .lock()
            .update_agent(&id.to_string(), name, &spec_json, &now)?;
        let info = AgentInfo {
            id: id.to_string(),
            name: name.to_string(),
            spec,
            created_at: self
                .agents
                .lock()
                .get(id)
                .map(|a| a.created_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
        };
        self.agents.lock().insert(id.clone(), info.clone());
        self.emit("agents:updated", ());
        Ok(info)
    }

    pub fn list_agents(&self) -> Vec<AgentInfo> {
        self.agents.lock().values().cloned().collect()
    }

    pub fn list_executions(&self) -> Vec<ExecutionInfo> {
        self.executions.lock().values().cloned().collect()
    }

    pub fn get_execution_trace(&self, trace_id: &str) -> Option<ExecutionTrace> {
        self.executions
            .lock()
            .get(trace_id)
            .map(|e| e.trace.clone())
    }

    pub fn create_workflow(
        &self,
        name: &str,
        description: &str,
        steps: Vec<WorkflowStep>,
        trigger: Trigger,
    ) -> anyhow::Result<WorkflowInfo> {
        let mut wf = Workflow::new(name, description).with_trigger(trigger);
        for step in steps {
            wf = wf.with_step(step);
        }
        let id = wf.id.clone();
        let now = Utc::now().to_rfc3339();
        let spec_json = serde_json::to_string(&wf)?;
        let trigger_json = serde_json::to_string(&wf.trigger)?;
        self.store.lock().insert_workflow(
            &id.to_string(),
            name,
            description,
            &spec_json,
            &trigger_json,
            wf.enabled,
            &now,
            &now,
        )?;
        let info = WorkflowInfo {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            steps: wf.steps,
            trigger: wf.trigger,
            enabled: wf.enabled,
            created_at: now.clone(),
            updated_at: now,
        };
        self.workflows.lock().insert(id, info.clone());
        self.emit("workflows:updated", ());
        Ok(info)
    }

    pub fn delete_workflow(&self, id: &WorkflowId) -> anyhow::Result<()> {
        self.store.lock().delete_workflow(&id.to_string())?;
        self.workflows.lock().remove(id);
        self.emit("workflows:updated", ());
        Ok(())
    }

    pub fn list_workflows(&self) -> Vec<WorkflowInfo> {
        self.workflows.lock().values().cloned().collect()
    }

    pub fn create_team(
        &self,
        id: &str,
        name: &str,
        metadata: &str,
        agent_ids: Vec<String>,
    ) -> anyhow::Result<TeamInfo> {
        let now = Utc::now().to_rfc3339();
        self.store.lock().insert_team(id, name, metadata, &now)?;
        for agent_id in &agent_ids {
            self.store.lock().insert_team_member(id, agent_id)?;
        }
        let info = TeamInfo {
            id: id.to_string(),
            name: name.to_string(),
            metadata: metadata.to_string(),
            created_at: now,
            members: agent_ids,
        };
        self.teams.lock().insert(id.to_string(), info.clone());
        self.emit("teams:updated", ());
        Ok(info)
    }

    pub fn list_teams(&self) -> Vec<TeamInfo> {
        self.teams.lock().values().cloned().collect()
    }

    pub fn set_vault_secret(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let passphrase = self.vault_passphrase.lock().clone();
        if passphrase.is_empty() {
            anyhow::bail!("vault passphrase not set");
        }
        let value_bytes = value.as_bytes();
        self.vault.lock().set(key, value_bytes, &passphrase)?;
        let encrypted = self.vault.lock().to_bytes()?;
        // Persist encrypted vault as a single JSON blob under vault_blob setting
        self.store
            .lock()
            .set_setting("vault_blob", &String::from_utf8_lossy(&encrypted))?;
        self.emit("vault:updated", ());
        Ok(())
    }

    pub fn unlock_vault(&self, passphrase: String) -> anyhow::Result<Vec<String>> {
        let bytes = self
            .store
            .lock()
            .get_setting("vault_blob")?
            .unwrap_or_default();
        if !bytes.is_empty() {
            let vault = CredentialVault::from_bytes(bytes.as_bytes())?;
            // Verify by trying to get a random key? Actually we can just set and unlock.
            let keys = vault.keys();
            self.vault_passphrase.lock().clear();
            self.vault_passphrase.lock().extend(passphrase.as_bytes());
            // Try to decrypt all entries to verify passphrase
            for key in &keys {
                if self.vault.lock().get(key, passphrase.as_bytes()).is_err() {
                    anyhow::bail!("wrong passphrase");
                }
            }
            *self.vault.lock() = vault;
        } else {
            self.vault_passphrase.lock().clear();
            self.vault_passphrase.lock().extend(passphrase.as_bytes());
        }
        self.emit("vault:updated", ());
        Ok(self.vault.lock().keys())
    }

    pub fn list_vault_secrets(&self) -> Vec<VaultSecretInfo> {
        self.vault
            .lock()
            .keys()
            .into_iter()
            .map(|key| VaultSecretInfo {
                key,
                updated_at: Utc::now().to_rfc3339(),
            })
            .collect()
    }

    pub fn set_llm_setting(
        &self,
        provider: &str,
        api_key: &str,
        base_url: Option<&str>,
        model: &str,
        temperature: Option<f32>,
    ) -> anyhow::Result<()> {
        self.store
            .lock()
            .set_llm_setting(provider, api_key, base_url, model, temperature)?;
        Ok(())
    }

    pub fn resolve_llm_provider(
        &self,
        provider_name: &str,
        model_override: &str,
    ) -> (Arc<dyn LlmProvider>, String) {
        let provider_name = if provider_name.is_empty() {
            "openai"
        } else {
            provider_name
        };
        match provider_name.to_lowercase().as_str() {
            "openai" | "openrouter" => {
                let setting = self.get_llm_setting(provider_name);
                if let Some(s) = setting {
                    if !s.api_key.is_empty() {
                        let base = s.base_url.unwrap_or_else(|| {
                            if provider_name == "openai" {
                                "https://api.openai.com/v1".to_string()
                            } else {
                                "https://openrouter.ai/api/v1".to_string()
                            }
                        });
                        let provider: Arc<dyn LlmProvider> = if provider_name == "openai" {
                            Arc::new(llm::OpenAiProvider::new("openai", s.api_key, base))
                        } else {
                            Arc::new(llm::OpenRouterProvider::new(s.api_key))
                        };
                        let model = if model_override.is_empty() {
                            s.model
                        } else {
                            model_override.to_string()
                        };
                        return (provider, model);
                    }
                }
                fallback_mock()
            }
            "anthropic" => {
                let setting = self.get_llm_setting("anthropic");
                if let Some(s) = setting {
                    if !s.api_key.is_empty() {
                        return (
                            Arc::new(llm::AnthropicProvider::new(s.api_key)),
                            if model_override.is_empty() {
                                s.model
                            } else {
                                model_override.to_string()
                            },
                        );
                    }
                }
                fallback_mock()
            }
            "ollama" => {
                let setting = self.get_llm_setting("ollama");
                let base = setting
                    .as_ref()
                    .and_then(|s| s.base_url.clone())
                    .unwrap_or_else(|| "http://localhost:11434".to_string());
                (
                    Arc::new(llm::OllamaProvider::new(base)),
                    if model_override.is_empty() {
                        setting.map(|s| s.model).unwrap_or_default()
                    } else {
                        model_override.to_string()
                    },
                )
            }
            "deepseek" => {
                let setting = self.get_llm_setting("deepseek");
                if let Some(s) = setting {
                    if !s.api_key.is_empty() {
                        let base = s
                            .base_url
                            .unwrap_or_else(|| "https://api.deepseek.com/v1".to_string());
                        return (
                            Arc::new(llm::OpenAiProvider::new("deepseek", s.api_key, base)),
                            if model_override.is_empty() {
                                s.model
                            } else {
                                model_override.to_string()
                            },
                        );
                    }
                }
                fallback_mock()
            }
            _ => fallback_mock(),
        }
    }

    pub async fn classify_intent(
        &self,
        provider: &str,
        model: &str,
        text: &str,
    ) -> anyhow::Result<Intent> {
        let (llm, model_name) = self.resolve_llm_provider(provider, model);
        let system = "You are an intent classifier for a desktop AI agent app. The user can ask you to do the following in natural language. Return ONLY a JSON object with no markdown, no explanation.\n\nAvailable intents:\n- chat: general conversation\n- create_agent: user wants to create an agent (extract name, prompt, optional tools)\n- install_mcp: user wants to install an MCP connector (extract source, value)\n- search_mcp: user wants to find an MCP connector (extract query)\n- schedule_agent: user wants to schedule an agent to run repeatedly (extract agent name/id, cron expression)\n- create_workflow: user wants to create a workflow of agents (extract name, cron expression, list of agents by name or id)\n- run_agent: user wants to run an existing agent with a prompt (extract agent name/id, prompt)\n\nReturn JSON shape: {\"intent\": \"...\", \"params\": {\"name\":\"...\", \"prompt\":\"...\", \"tools\":[], \"source\":\"...\", \"value\":\"...\", \"query\":\"...\", \"agent\":\"...\", \"expression\":\"...\", \"agents\":[], \"message\":\"...\"}}".to_string();
        let req = CompletionRequest::new(provider, model_name)
            .with_system(system)
            .with_user(text);
        let res = llm
            .complete(req)
            .await
            .map_err(|e| anyhow::anyhow!("llm error: {e}"))?;
        let content = res
            .content
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let intent: Intent = serde_json::from_str(content)
            .map_err(|e| anyhow::anyhow!("parse intent error: {e} from content: {content}"))?;
        Ok(intent)
    }

    pub fn get_llm_setting(&self, provider: &str) -> Option<LlmSetting> {
        self.store
            .lock()
            .get_llm_setting(provider)
            .ok()
            .flatten()
            .map(|(api_key, base_url, model, temperature)| LlmSetting {
                api_key,
                base_url,
                model,
                temperature,
            })
    }

    pub fn run_agent(
        &self,
        worker_id: &WorkerId,
        agent_id: &AgentId,
        prompt: &str,
    ) -> anyhow::Result<()> {
        let spec = if let Some(agent) = self.agents.lock().get(agent_id) {
            agent.spec.clone()
        } else {
            AgentSpec::new(&agent_id.0, prompt)
        };
        let mcp_servers = self.resolve_mcp_servers_for_agent(&spec)?;
        self.send_to_worker(
            worker_id,
            DesktopMessage::RunAgent {
                trace_id: format!("desktop-{}", uuid::Uuid::new_v4()),
                agent_id: agent_id.clone(),
                spec,
                mcp_servers,
            },
        )
    }

    pub fn schedule_agent(
        &self,
        worker_id: &WorkerId,
        agent_id: &AgentId,
        trigger: Trigger,
    ) -> anyhow::Result<()> {
        let spec = self.agents.lock().get(agent_id).cloned();
        let mcp_servers = if let Some(ref s) = spec {
            self.resolve_mcp_servers_for_agent(&s.spec)?
        } else {
            vec![]
        };
        self.send_to_worker(
            worker_id,
            DesktopMessage::ScheduleAgent {
                agent_id: agent_id.clone(),
                trigger,
                mcp_servers,
            },
        )
    }

    /// Resolve MCP servers referenced by an agent spec, substituting vault secrets into the server config.
    fn resolve_mcp_servers_for_agent(
        &self,
        spec: &AgentSpec,
    ) -> anyhow::Result<Vec<goble_core::agent::McpServer>> {
        if spec.mcp_ids.is_empty() {
            return Ok(vec![]);
        }
        let summaries = self.list_mcp_servers()?;
        let passphrase = self.vault_passphrase.lock().clone();
        let mut resolved = Vec::new();
        for id in &spec.mcp_ids {
            let summary = match summaries.iter().find(|s| &s.id == id) {
                Some(s) => s,
                None => {
                    anyhow::bail!("mcp server {id} referenced by agent {} not found", spec.id);
                }
            };
            let server = self.resolve_mcp_server_with_secrets(summary, &passphrase)?;
            resolved.push(server);
        }
        Ok(resolved)
    }

    fn resolve_mcp_server_with_secrets(
        &self,
        summary: &McpServerSummary,
        passphrase: &[u8],
    ) -> anyhow::Result<goble_core::agent::McpServer> {
        let rows = self.store.lock().list_mcp_servers()?;
        let row = rows
            .into_iter()
            .find(|(i, _, _, _, _, _, _, _, _, _)| i == &summary.id)
            .context(format!("mcp server {} not found", summary.id))?;
        let manifest: goble_core::agent::McpManifest = serde_json::from_str(&row.4)?;
        let source = parse_mcp_source(&summary.source, summary.source_value.as_deref())?;
        let mut server = goble_core::agent::McpServer {
            id: summary.id.clone(),
            name: summary.name.clone(),
            source,
            manifest,
            credentials_key: None,
            installed_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        if !summary.secret_ids.is_empty() {
            if passphrase.is_empty() {
                anyhow::bail!("vault is locked; cannot resolve secrets for {}", summary.id);
            }
            let mut env = std::collections::HashMap::new();
            for key in &summary.secret_ids {
                let value = self.vault.lock().get(key, passphrase)?.context(format!(
                    "vault secret {key} missing for mcp server {}",
                    summary.id
                ))?;
                env.insert(key.clone(), String::from_utf8_lossy(&value).to_string());
            }
            server.credentials_key = Some(serde_json::to_string(&env)?);
        }
        Ok(server)
    }

    pub fn list_mcp_servers(&self) -> Result<Vec<McpServerSummary>, anyhow::Error> {
        self.mcp_manager.list_mcp_servers(&self.store.lock())
    }

    pub fn search_mcp_servers(&self, query: &str) -> Vec<McpSearchResult> {
        tauri::async_runtime::block_on(self.mcp_manager.search_mcp_servers(query))
    }

    pub fn test_call_mcp_tool(
        &self,
        id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, anyhow::Error> {
        let store = self.store.lock();
        tauri::async_runtime::block_on(
            self.mcp_manager
                .test_call_tool(&store, id, tool_name, arguments),
        )
    }

    pub fn install_mcp_server(
        &self,
        id: &str,
        name: &str,
        source: &str,
        source_value: Option<&str>,
        secret_ids: Vec<String>,
        manifest: Option<goble_core::agent::McpManifest>,
    ) -> Result<String, anyhow::Error> {
        let store = self.store.lock().clone();
        let secrets: Vec<goble_core::secret::Secret> = secret_ids
            .iter()
            .map(|name| goble_core::secret::Secret::new(name, "mcp", vec![]))
            .collect();
        tauri::async_runtime::block_on(self.mcp_manager.install_mcp_server(
            &store,
            id,
            name,
            source,
            source_value,
            &secrets,
            manifest,
        ))
    }

    pub fn update_mcp_server(
        &self,
        id: &str,
        name: Option<&str>,
        source_value: Option<&str>,
        secret_ids: Option<Vec<String>>,
        manifest: Option<goble_core::agent::McpManifest>,
    ) -> Result<String, anyhow::Error> {
        let store = self.store.lock().clone();
        let secrets = secret_ids.map(|ids| {
            ids.iter()
                .map(|name| goble_core::secret::Secret::new(name, "mcp", vec![]))
                .collect::<Vec<_>>()
        });
        tauri::async_runtime::block_on(self.mcp_manager.update_mcp_server(
            &store,
            id,
            name,
            source_value,
            secrets.as_deref(),
            manifest,
        ))
    }

    pub fn delete_mcp_server(&self, id: &str) -> Result<String, anyhow::Error> {
        let store = self.store.lock().clone();
        self.mcp_manager.delete_mcp_server(&store, id)
    }

    pub fn update_mcp_server_meta(
        &self,
        id: &str,
        secret_ids: Vec<String>,
        enabled_tools: Vec<String>,
    ) -> Result<String, anyhow::Error> {
        let store = self.store.lock().clone();
        self.mcp_manager
            .update_mcp_server_meta(&store, id, &secret_ids, &enabled_tools)
    }

    pub fn discover_mcp_tools(&self, id: &str) -> Result<Vec<McpTool>, anyhow::Error> {
        self.mcp_manager
            .discover_and_enable_all(&self.store.lock(), id)?;
        self.mcp_manager.discover_and_register(id)
    }

    pub fn load_from_store(&self) -> anyhow::Result<()> {
        let workers = self.store.lock().list_workers()?;
        let mut map = self.workers.lock();
        for (id, name, host, status, _pk, config, _created, _updated) in workers {
            let tags = serde_json::from_str::<WorkerConfig>(&config)
                .map(|c| c.tags)
                .unwrap_or_default();
            map.insert(
                WorkerId(id.clone()),
                WorkerConnection {
                    id,
                    name,
                    url: host.unwrap_or_default(),
                    paired: status == "paired",
                    tags,
                },
            );
        }
        drop(map);

        let agents = self.store.lock().list_agents()?;
        let mut agents_map = self.agents.lock();
        for (id, name, spec_json, created_at, updated_at) in agents {
            if let Ok(spec) = serde_json::from_str::<AgentSpec>(&spec_json) {
                agents_map.insert(
                    AgentId(id.clone()),
                    AgentInfo {
                        id: id.clone(),
                        name,
                        spec,
                        created_at,
                        updated_at,
                    },
                );
            }
        }
        drop(agents_map);

        let workflows = self.store.lock().list_workflows()?;
        let mut wf_map = self.workflows.lock();
        for (id, name, description, spec_json, trigger_json, enabled, created_at, updated_at) in
            workflows
        {
            if let (Ok(wf), Ok(trigger)) = (
                serde_json::from_str::<Workflow>(&spec_json),
                serde_json::from_str::<Trigger>(&trigger_json),
            ) {
                wf_map.insert(
                    WorkflowId(id.clone()),
                    WorkflowInfo {
                        id: id.clone(),
                        name,
                        description,
                        steps: wf.steps,
                        trigger,
                        enabled,
                        created_at,
                        updated_at,
                    },
                );
            }
        }
        drop(wf_map);

        let teams = self.store.lock().list_teams()?;
        let mut team_map = self.teams.lock();
        for (id, name, metadata, created_at) in teams {
            let members = self.store.lock().list_team_members(&id).unwrap_or_default();
            team_map.insert(
                id.clone(),
                TeamInfo {
                    id: id.clone(),
                    name,
                    metadata,
                    created_at,
                    members: members.into_iter().map(|(_, a)| a).collect(),
                },
            );
        }
        drop(team_map);

        let executions = self.store.lock().list_executions()?;
        let mut exec_map = self.executions.lock();
        for (id, agent_id, worker_id, status, trace_json, started_at, finished_at) in executions {
            if let Ok(trace) = serde_json::from_str::<ExecutionTrace>(&trace_json) {
                exec_map.insert(
                    id.clone(),
                    ExecutionInfo {
                        id,
                        agent_id,
                        worker_id,
                        status,
                        trace,
                        started_at,
                        finished_at,
                    },
                );
            }
        }
        drop(exec_map);

        let chats = self.store.lock().list_chats()?;
        let mut chats_vec = self.chats.lock();
        for (id, title, provider, model, _created_at, updated_at) in chats {
            chats_vec.push(Chat {
                id,
                title,
                provider,
                model,
                agent_id: None,
                worker_id: None,
                updated_at,
            });
        }
        drop(chats_vec);

        if let Some(blob) = self.store.lock().get_setting("vault_blob")? {
            if let Ok(vault) = CredentialVault::from_bytes(blob.as_bytes()) {
                *self.vault.lock() = vault;
            }
        }

        Ok(())
    }

    pub fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) {
        if let Some(handle) = self.app_handle.lock().as_ref() {
            let _ = handle.emit(event, payload);
        }
    }

    /// Convert legacy chat conversations into threads with a single participant (the user).
    pub fn run_agent_for_thread_reply(
        &self,
        worker_id: &WorkerId,
        thread_id: &ThreadId,
        agent_id: &AgentId,
        prompt: &str,
    ) -> anyhow::Result<()> {
        let (id, name, spec, created_at, updated_at) = self
            .store
            .lock()
            .get_agent(&agent_id.0)?
            .ok_or_else(|| anyhow::anyhow!("agent not found: {}", agent_id.0))?;
        let agent_spec = AgentSpec {
            id: AgentId(id),
            name: name.clone(),
            description: name.clone(),
            prompt: spec,
            tools: vec![],
            triggers: vec![],
            mcp_ids: vec![],
            created_at,
            updated_at,
        };
        let mcp_servers = self.resolve_mcp_servers_for_agent(&agent_spec)?;
        let msg = DesktopMessage::RunAgentForThreadReply {
            trace_id: format!("{}-{}", thread_id.0, uuid::Uuid::new_v4()),
            thread_id: thread_id.0.clone(),
            agent_id: agent_id.clone(),
            prompt: prompt.to_string(),
            spec: agent_spec,
            mcp_servers,
        };
        self.send_to_worker(worker_id, msg)
    }
    pub fn migrate_legacy_chats_to_threads(&self) -> Result<Vec<ThreadSummary>, String> {
        let owner = self
            .thread_store()
            .get_profile()
            .map(|p| UserId(p.id.0))
            .unwrap_or_else(UserId::generate);
        let mut summaries = Vec::new();
        for conv in self.list_chats() {
            let participants = vec![Participant::User(owner.clone())];
            let thread = self
                .thread_store()
                .create_thread(
                    ThreadKind::Chat,
                    conv.title,
                    owner.clone(),
                    false,
                    participants,
                    vec![],
                )
                .map_err(|e| e.to_string())?;
            if let Some(messages) = self.messages.lock().get(&conv.id).cloned() {
                for msg in messages {
                    let author = if msg.role == "user" {
                        Participant::User(owner.clone())
                    } else {
                        Participant::Agent(AgentId(
                            conv.agent_id.clone().unwrap_or_else(|| "agent".to_string()),
                        ))
                    };
                    let _ = self.thread_store().post_message(
                        &thread.id,
                        author,
                        msg.content,
                        None,
                        vec![],
                        vec![],
                        None,
                    );
                }
            }
            summaries.push(ThreadSummary::from(thread));
        }
        Ok(summaries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_add_worker() {
        let state = DesktopState::new(
            Store::open_in_memory().unwrap(),
            crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
        );
        let wid = WorkerId::generate();
        state
            .add_worker(
                wid.clone(),
                "vps".to_string(),
                "wss://localhost:8787/ws".to_string(),
            )
            .unwrap();
        let workers = state.list_workers();
        assert_eq!(workers.len(), 1);
        assert_eq!(workers[0].name, "vps");
        assert!(!workers[0].paired);
        state.remove_worker(&wid);
        assert!(state.list_workers().is_empty());
    }

    #[test]
    fn test_state_logs_and_chat() {
        let state = DesktopState::new(
            Store::open_in_memory().unwrap(),
            crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
        );
        state.add_log("hello");
        assert_eq!(state.get_logs().len(), 1);
        let chat_id = state.create_chat("Test chat", None, None).unwrap();
        state
            .add_chat_message(&chat_id, "user", "hello agent")
            .unwrap();
        let msgs = state.list_chat_messages(&chat_id).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].role, "user");
        assert_eq!(msgs[0].content, "hello agent");
    }

    #[test]
    fn test_state_agent_and_workflow() {
        let state = DesktopState::new(
            Store::open_in_memory().unwrap(),
            crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
        );
        let agent = state
            .create_agent("greeter", "say hello", Some("test agent"), vec![])
            .unwrap();
        assert_eq!(state.list_agents().len(), 1);
        let step = WorkflowStep {
            id: uuid::Uuid::new_v4().to_string(),
            name: "greet".to_string(),
            agent_id: AgentId(agent.id.clone()),
            input_template: "Greet the user".to_string(),
            depends_on: vec![],
        };
        let wf = state
            .create_workflow("hello", "Hello workflow", vec![step], Trigger::Manual)
            .unwrap();
        assert_eq!(state.list_workflows().len(), 1);
        assert_eq!(wf.steps.len(), 1);
        state.delete_workflow(&WorkflowId(wf.id)).unwrap();
        assert!(state.list_workflows().is_empty());
        state.delete_agent(&AgentId(agent.id)).unwrap();
        assert!(state.list_agents().is_empty());
    }

    #[test]
    fn test_state_team_and_vault() {
        let state = DesktopState::new(
            Store::open_in_memory().unwrap(),
            crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
        );
        state.set_vault_passphrase("passphrase".to_string());
        state.set_vault_secret("api_key", "sk-123").unwrap();
        assert_eq!(state.list_vault_secrets().len(), 1);
        state
            .create_team("team1", "Platform", "{}", vec!["a1".to_string()])
            .unwrap();
        assert_eq!(state.list_teams().len(), 1);
    }

    #[test]
    fn test_worker_message_handling() {
        let state = DesktopState::new(
            Store::open_in_memory().unwrap(),
            crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
        );
        let wid = WorkerId::generate();
        state
            .add_worker(
                wid.clone(),
                "vps".to_string(),
                "ws://localhost:8787/ws".to_string(),
            )
            .unwrap();
        state.handle_worker_message(&wid, WorkerMessage::Paired);
        assert!(state.list_workers()[0].paired);
        state.handle_worker_message(
            &wid,
            WorkerMessage::AgentLog {
                trace_id: "t1".to_string(),
                step_id: "s1".to_string(),
                level: goble_core::execution::LogLevel::Info,
                message: "hello".to_string(),
            },
        );
        assert_eq!(state.get_logs().len(), 2);
    }

    #[test]
    fn test_agent_workflow_team_vault_roundtrip() {
        let state = DesktopState::new(
            Store::open_in_memory().unwrap(),
            crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
        );
        state.set_vault_passphrase("secret".to_string());
        let agent = state
            .create_agent("greeter", "say hello", Some("test agent"), vec![])
            .unwrap();
        let step = WorkflowStep {
            id: uuid::Uuid::new_v4().to_string(),
            name: "greet".to_string(),
            agent_id: AgentId(agent.id.clone()),
            input_template: "Greet the user".to_string(),
            depends_on: vec![],
        };
        let wf = state
            .create_workflow(
                "hello",
                "Hello workflow",
                vec![step],
                goble_core::agent::Trigger::Manual,
            )
            .unwrap();
        assert_eq!(state.list_agents().len(), 1);
        assert_eq!(state.list_workflows().len(), 1);

        state
            .create_team("team1", "Platform", "{}", vec![agent.id.clone()])
            .unwrap();
        assert_eq!(state.list_teams().len(), 1);
        assert_eq!(state.list_teams()[0].members.len(), 1);

        state.set_vault_secret("api_key", "sk-123").unwrap();
        assert_eq!(state.list_vault_secrets().len(), 1);

        state.delete_workflow(&WorkflowId(wf.id)).unwrap();
        state.delete_agent(&AgentId(agent.id)).unwrap();
        assert!(state.list_workflows().is_empty());
        assert!(state.list_agents().is_empty());
    }

    #[test]
    fn test_persistence_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().join("store.db")).unwrap();
        let state = DesktopState::new(
            store,
            crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
        );
        state.set_vault_passphrase("p".to_string());
        let agent = state.create_agent("a", "prompt", None, vec![]).unwrap();
        let step = WorkflowStep {
            id: uuid::Uuid::new_v4().to_string(),
            name: "s".to_string(),
            agent_id: AgentId(agent.id.clone()),
            input_template: "in".to_string(),
            depends_on: vec![],
        };
        state
            .create_workflow("wf", "desc", vec![step], goble_core::agent::Trigger::Manual)
            .unwrap();
        state
            .create_team("t", "Team", "{}", vec![agent.id])
            .unwrap();
        state.set_vault_secret("k", "v").unwrap();

        let state2 = DesktopState::new(
            Store::open(tmp.path().join("store.db")).unwrap(),
            crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
        );
        state2.load_from_store().unwrap();
        assert_eq!(state2.list_agents().len(), 1);
        assert_eq!(state2.list_workflows().len(), 1);
        assert_eq!(state2.list_teams().len(), 1);
        assert_eq!(state2.list_vault_secrets().len(), 1);
    }

    #[test]
    fn test_cluster_identity_encrypted_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().join("store.db")).unwrap();
        let state = DesktopState::new(
            store,
            crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
        );
        let identity = state.create_cluster("test-cluster", "secret-pass").unwrap();
        assert!(!identity.cluster_name.is_empty());

        let state2 = DesktopState::new(
            Store::open(tmp.path().join("store.db")).unwrap(),
            crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
        );
        assert!(state2.has_stored_cluster_identity());
        assert!(state2.unlock_cluster_identity("wrong").is_err());
        assert!(state2.unlock_cluster_identity("secret-pass").unwrap());
        let loaded = state2.get_cluster_identity().unwrap();
        assert_eq!(loaded.cluster_name, identity.cluster_name);
    }

    #[test]
    fn migrate_legacy_chats_creates_threads() {
        let state = DesktopState::new(
            Store::open_in_memory().unwrap(),
            crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
        );
        // Create a legacy chat with two messages
        state.create_chat("legacy chat", None, None).unwrap();
        let chats = state.list_chats();
        let chat = &chats[0];
        state.add_chat_message(&chat.id, "user", "hello").unwrap();
        state
            .add_chat_message(&chat.id, "user", "hi there")
            .unwrap();

        let threads = state.migrate_legacy_chats_to_threads().unwrap();
        assert_eq!(threads.len(), 1);
        let summary = &threads[0];
        assert_eq!(summary.title, "legacy chat");

        let messages = state
            .thread_store()
            .list_messages(&goble_core::thread::ThreadId(summary.id.clone()))
            .unwrap();
        assert_eq!(messages.len(), 2);
    }
}
