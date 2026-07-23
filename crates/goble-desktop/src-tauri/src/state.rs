use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use goble_core::agent::{AgentId, AgentSpec, Trigger};
use goble_core::execution::ExecutionTrace;
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use goble_core::store::Store;
use goble_core::vault::CredentialVault;
use goble_core::worker::WorkerId;
use goble_core::workflow::{Workflow, WorkflowId, WorkflowStep};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::worker_manager::WorkerClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerConnection {
    pub id: String,
    pub name: String,
    pub url: String,
    pub paired: bool,
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
    app_handle: Mutex<Option<AppHandle>>,
}

impl DesktopState {
    pub fn new(store: Store) -> Arc<Self> {
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
            app_handle: Mutex::new(None),
        })
    }

    pub fn set_app_handle(&self, handle: AppHandle) {
        *self.app_handle.lock() = Some(handle);
    }

    pub fn open_default() -> anyhow::Result<Arc<Self>> {
        let path = dirs::config_dir()
            .map(|p| p.join("goble").join("store.db"))
            .unwrap_or_else(|| Path::new(".goble").join("store.db"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = Store::open(path)?;
        Ok(Self::new(store))
    }

    pub fn set_vault_passphrase(&self, passphrase: String) {
        *self.vault_passphrase.lock() = passphrase.into_bytes();
    }

    pub fn add_worker(
        &self,
        worker_id: WorkerId,
        name: String,
        url: String,
    ) -> anyhow::Result<()> {
        let conn = WorkerConnection {
            id: worker_id.to_string(),
            name: name.clone(),
            url: url.clone(),
            paired: false,
        };
        self.store.lock().insert_worker(
            &worker_id.to_string(),
            &conn.name,
            Some(&url),
            "unpaired",
            None,
            "{}",
            &Utc::now().to_rfc3339(),
            &Utc::now().to_rfc3339(),
        )?;
        self.workers.lock().insert(worker_id, conn);
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

    pub fn pair_worker(
        self: Arc<Self>,
        worker_id: &WorkerId,
        pairing_code: String,
    ) -> anyhow::Result<bool> {
        let conn = self.workers.lock().get(worker_id).cloned();
        if let Some(conn) = conn {
            let state = self.clone();
            let wid = worker_id.clone();
            let url = conn.url.clone();
            tokio::spawn(async move {
                let conn_name = conn.name.clone();
                match WorkerClient::connect(state.clone(), wid.clone(), url.clone(), pairing_code).await {
                    Ok(client) => {
                        state.clients.lock().insert(wid.clone(), client);
                        if let Some(c) = state.workers.lock().get_mut(&wid) {
                            c.paired = true;
                        }
                        let _ = state.store.lock().insert_worker(
                            &wid.to_string(),
                            &conn_name,
                            Some(&url),
                            "paired",
                            None,
                            "{}",
                            &Utc::now().to_rfc3339(),
                            &Utc::now().to_rfc3339(),
                        );
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

    pub fn send_to_worker(
        &self,
        worker_id: &WorkerId,
        msg: DesktopMessage,
    ) -> anyhow::Result<()> {
        if let Some(client) = self.clients.lock().get(worker_id) {
            let _ = client.send(msg);
            Ok(())
        } else {
            anyhow::bail!("worker not connected: {}", worker_id)
        }
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
                step_id,
                level,
                message,
            } => {
                let entry = format!(
                    "[{}] [{}] {} - {:?}: {}",
                    worker_id, trace_id, step_id, level, message
                );
                self.add_log(entry);
                self.emit(
                    "agent:log",
                    serde_json::json!({
                        "worker_id": worker_id.to_string(),
                        "trace_id": trace_id,
                        "step_id": step_id,
                        "level": format!("{:?}", level),
                        "message": message,
                    }),
                );
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
            WorkerMessage::StatusReport { worker_id, status, load } => {
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

    pub fn add_chat_message(
        &self,
        chat_id: &str,
        role: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();
        self.store
            .lock()
            .insert_chat_message(&id, chat_id, role, content, None, &created_at)?;
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

    pub fn list_chat_messages(&self, chat_id: &str,
    ) -> anyhow::Result<Vec<ChatMessage>> {
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

    pub fn create_chat(&self, title: &str) -> anyhow::Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.store.lock().insert_chat(&id, title, &now, &now)?;
        let chat = Chat {
            id: id.clone(),
            title: title.to_string(),
            agent_id: None,
            worker_id: None,
            updated_at: now,
        };
        self.chats.lock().push(chat);
        self.emit("chats:updated", ());
        Ok(id)
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
        self.store.lock().insert_agent(
            &id.to_string(),
            name,
            &spec_json,
            &now,
            &now,
        )?;
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

    pub fn list_agents(&self) -> Vec<AgentInfo> {
        self.agents.lock().values().cloned().collect()
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
        self.store
            .lock()
            .insert_team(id, name, metadata, &now)?;
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

    pub fn set_vault_secret(
        &self,
        key: &str,
        value: &str,
    ) -> anyhow::Result<()> {
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
                if self.vault.lock().get(key, &passphrase.as_bytes()).is_err() {
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
        self.send_to_worker(
            worker_id,
            DesktopMessage::RunAgent {
                trace_id: format!("desktop-{}", uuid::Uuid::new_v4()),
                agent_id: agent_id.clone(),
                spec,
            },
        )
    }

    pub fn schedule_agent(
        &self,
        worker_id: &WorkerId,
        agent_id: &AgentId,
        trigger: Trigger,
    ) -> anyhow::Result<()> {
        self.send_to_worker(
            worker_id,
            DesktopMessage::ScheduleAgent {
                agent_id: agent_id.clone(),
                trigger,
            },
        )
    }

    pub fn list_executions(&self) -> Vec<ExecutionInfo> {
        self.executions.lock().values().cloned().collect()
    }

    pub fn load_from_store(&self) -> anyhow::Result<()> {
        let workers = self.store.lock().list_workers()?;
        let mut map = self.workers.lock();
        for (id, name, host, status, _pk, _config, _created, _updated) in workers {
            map.insert(
                WorkerId(id.clone()),
                WorkerConnection {
                    id,
                    name,
                    url: host.unwrap_or_default(),
                    paired: status == "paired",
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
        for (id, name, description, spec_json, trigger_json, enabled, created_at, updated_at) in workflows {
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

        if let Some(blob) = self.store.lock().get_setting("vault_blob")? {
            if let Ok(vault) = CredentialVault::from_bytes(blob.as_bytes()) {
                *self.vault.lock() = vault;
            }
        }

        Ok(())
    }

    fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) {
        if let Some(handle) = self.app_handle.lock().as_ref() {
            let _ = handle.emit(event, payload);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_add_worker() {
        let state = DesktopState::new(Store::open_in_memory().unwrap());
        let wid = WorkerId::generate();
        state
            .add_worker(wid.clone(), "vps".to_string(), "wss://localhost:8787/ws".to_string())
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
        let state = DesktopState::new(Store::open_in_memory().unwrap());
        state.add_log("hello");
        assert_eq!(state.get_logs().len(), 1);
        let chat_id = state.create_chat("Test chat").unwrap();
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
        let state = DesktopState::new(Store::open_in_memory().unwrap());
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
        let state = DesktopState::new(Store::open_in_memory().unwrap());
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
        let state = DesktopState::new(Store::open_in_memory().unwrap());
        let wid = WorkerId::generate();
        state.add_worker(wid.clone(), "vps".to_string(), "ws://localhost:8787/ws".to_string()).unwrap();
        state.handle_worker_message(
            &wid,
            WorkerMessage::Paired,
        );
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
        let state = DesktopState::new(Store::open_in_memory().unwrap());
        state.set_vault_passphrase("secret".to_string());
        let agent = state.create_agent("greeter", "say hello", Some("test agent"), vec![]).unwrap();
        let step = WorkflowStep {
            id: uuid::Uuid::new_v4().to_string(),
            name: "greet".to_string(),
            agent_id: AgentId(agent.id.clone()),
            input_template: "Greet the user".to_string(),
            depends_on: vec![],
        };
        let wf = state.create_workflow("hello", "Hello workflow", vec![step], goble_core::agent::Trigger::Manual).unwrap();
        assert_eq!(state.list_agents().len(), 1);
        assert_eq!(state.list_workflows().len(), 1);

        state.create_team("team1", "Platform", "{}", vec![agent.id.clone()]).unwrap();
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
        let state = DesktopState::new(store);
        state.set_vault_passphrase("p".to_string());
        let agent = state.create_agent("a", "prompt", None, vec![]).unwrap();
        let step = WorkflowStep {
            id: uuid::Uuid::new_v4().to_string(),
            name: "s".to_string(),
            agent_id: AgentId(agent.id.clone()),
            input_template: "in".to_string(),
            depends_on: vec![],
        };
        state.create_workflow("wf", "desc", vec![step], goble_core::agent::Trigger::Manual).unwrap();
        state.create_team("t", "Team", "{}", vec![agent.id]).unwrap();
        state.set_vault_secret("k", "v").unwrap();

        let state2 = DesktopState::new(Store::open(tmp.path().join("store.db")).unwrap());
        state2.load_from_store().unwrap();
        assert_eq!(state2.list_agents().len(), 1);
        assert_eq!(state2.list_workflows().len(), 1);
        assert_eq!(state2.list_teams().len(), 1);
        assert_eq!(state2.list_vault_secrets().len(), 1);
    }
}
