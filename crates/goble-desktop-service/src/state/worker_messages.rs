use chrono::Utc;
use goble_core::execution::{ExecutionTrace, LogLevel, TraceEvent};
use goble_core::protocol::WorkerMessage;
use goble_core::worker::WorkerId;
use serde::Serialize;

use super::{DesktopState, ExecutionInfo};

#[derive(Debug, Clone, Serialize)]
struct ThreadMessagesUpdatedPayload {
    thread_id: String,
}

impl DesktopState {
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
                // The run's real start time is stamped once and carried both on
                // the ledger record and on the event, so the app's live chrome
                // reads the service's own start instead of guessing one from
                // when the event happened to arrive.
                let started_at = Utc::now().to_rfc3339();
                let mut executions = self.executions.lock();
                executions.insert(
                    trace_id.clone(),
                    ExecutionInfo {
                        id: trace_id.clone(),
                        agent_id: Some(agent_id.to_string()),
                        worker_id: Some(worker_id.to_string()),
                        status: "running".to_string(),
                        trace: ExecutionTrace::new(agent_id.clone()),
                        started_at: started_at.clone(),
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
                        "started_at": started_at,
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
            WorkerMessage::RoutinesUpdated { routines } => {
                self.emit(
                    "worker:routines",
                    serde_json::json!({ "worker_id": worker_id.to_string(), "routines": routines }),
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
}
