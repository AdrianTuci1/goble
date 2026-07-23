use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use goble_core::store::Store;
use goble_core::worker::WorkerId;
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

pub struct DesktopState {
    store: Arc<Mutex<Store>>,
    workers: Arc<Mutex<HashMap<WorkerId, WorkerConnection>>>,
    chats: Arc<Mutex<Vec<Chat>>>,
    messages: Arc<Mutex<HashMap<String, Vec<ChatMessage>>>>,
    agents: Arc<Mutex<Vec<(String, String, String, String, String)>>>,
    teams: Arc<Mutex<Vec<(String, String, String)>>>,
    mcps: Arc<Mutex<Vec<(String, String, String, String, Option<String>, String, String)>>>,
    #[allow(dead_code)]
    executions: Arc<Mutex<HashMap<String, String>>>,
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
            agents: Arc::new(Mutex::new(Vec::new())),
            teams: Arc::new(Mutex::new(Vec::new())),
            mcps: Arc::new(Mutex::new(Vec::new())),
            executions: Arc::new(Mutex::new(HashMap::new())),
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

    pub fn add_worker(
        &self,
        worker_id: WorkerId,
        name: String,
        url: String,
    ) -> anyhow::Result<()> {
        let conn = WorkerConnection {
            id: worker_id.to_string(),
            name,
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
                self.add_log(format!("worker {} started agent {} trace {}", worker_id, agent_id, trace_id));
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
                self.emit(
                    "vault:secret",
                    serde_json::json!({
                        "name": name,
                        "value": value.as_ref().map(|v| String::from_utf8_lossy(v).to_string()),
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

    pub fn create_chat(&self, title: &str) -> anyhow::Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        self.store.lock().insert_chat(&id,
            title,
            &now,
            &now,
        )?;
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
        self.agents.lock().extend(self.store.lock().list_agents()?);
        self.teams.lock().extend(self.store.lock().list_teams()?.into_iter().map(|(a,b,c,_d)| (a,b,c)));
        self.mcps.lock().extend(self.store.lock().list_mcp_servers()?);
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
}
