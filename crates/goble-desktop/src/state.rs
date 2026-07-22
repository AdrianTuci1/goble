use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use goble_core::agent::{AgentSpec, Chat, ChatMessage, McpServer, Team};
use goble_core::execution::ExecutionTrace;
use goble_core::protocol::WorkerMessage;
use goble_core::store::Store;
use goble_core::worker::WorkerId;

/// Global desktop state.
pub struct DesktopState {
    pub store: Arc<Mutex<Store>>,
    pub workers: Arc<Mutex<HashMap<WorkerId, WorkerConnection>>>,
    pub chats: Arc<Mutex<Vec<Chat>>>,
    pub messages: Arc<Mutex<HashMap<String, Vec<ChatMessage>>>>,
    pub agents: Arc<Mutex<Vec<AgentSpec>>>,
    pub teams: Arc<Mutex<Vec<Team>>>,
    pub mcps: Arc<Mutex<Vec<McpServer>>>,
    pub executions: Arc<Mutex<HashMap<String, ExecutionTrace>>>,
    pub logs: Arc<Mutex<Vec<String>>>,
}

#[derive(Debug, Clone)]
pub struct WorkerConnection {
    pub worker_id: WorkerId,
    pub name: String,
    pub url: String,
    pub paired: bool,
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
        })
    }

    pub fn add_worker(&self, worker_id: WorkerId, name: String, url: String, paired: bool) {
        self.workers.lock().insert(
            worker_id.clone(),
            WorkerConnection {
                worker_id,
                name,
                url,
                paired,
            },
        );
    }

    pub fn remove_worker(&self, worker_id: &WorkerId) {
        self.workers.lock().remove(worker_id);
    }

    pub fn add_chat(&self, chat: Chat) {
        self.chats.lock().push(chat);
    }

    pub fn add_message(&self, chat_id: &str, message: ChatMessage) {
        self.messages
            .lock()
            .entry(chat_id.to_string())
            .or_default()
            .push(message);
    }

    pub fn add_agent(&self, agent: AgentSpec) {
        self.agents.lock().push(agent);
    }

    pub fn add_team(&self, team: Team) {
        self.teams.lock().push(team);
    }

    pub fn add_mcp(&self, server: McpServer) {
        self.mcps.lock().push(server);
    }

    pub fn handle_worker_message(&self, worker_id: &WorkerId, msg: WorkerMessage) {
        let mut logs = self.logs.lock();
        match &msg {
            WorkerMessage::AgentStarted { trace_id, agent_id } => {
                logs.push(format!("[{}] agent started: {}", trace_id, agent_id));
            }
            WorkerMessage::AgentLog {
                trace_id,
                step_id,
                level,
                message,
            } => {
                logs.push(format!(
                    "[{}][{}][{:?}] {}",
                    trace_id, step_id, level, message
                ));
            }
            WorkerMessage::AgentFinished { trace_id, status } => {
                logs.push(format!("[{}] finished: {:?}", trace_id, status));
            }
            WorkerMessage::Pong => {
                logs.push(format!("[{}] pong", worker_id));
            }
            WorkerMessage::Paired => {
                logs.push(format!("[{}] paired", worker_id));
                if let Some(conn) = self.workers.lock().get_mut(worker_id) {
                    conn.paired = true;
                }
            }
            WorkerMessage::StatusReport { load, .. } => {
                logs.push(format!("[{}] status load={}", worker_id, load));
            }
        }
    }

    pub fn get_logs(&self) -> Vec<String> {
        self.logs.lock().clone()
    }

    pub fn add_chat_log(&self, message: impl Into<String>) {
        self.logs.lock().push(message.into());
    }
}
