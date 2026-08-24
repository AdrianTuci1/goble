use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use cron::Schedule;
use goble_core::agent::{AgentId, Trigger};
use goble_core::protocol::{RoutineInfo, WorkerMessage};

use crate::leader::LeaderState;
use crate::runner::Runner;
use crate::state::AppState;
use crate::task_store::{ScheduledTask, TaskStore};

pub struct Scheduler {
    state: Arc<AppState>,
    runner: Runner,
    store: Arc<std::sync::Mutex<TaskStore>>,
}

impl Scheduler {
    pub fn new(state: Arc<AppState>, store: TaskStore, runner: Runner) -> Self {
        Self {
            state,
            runner,
            store: Arc::new(std::sync::Mutex::new(store)),
        }
    }

    pub fn new_with_default_runner(state: Arc<AppState>, store: TaskStore) -> Self {
        let runner = Runner::new(state.clone());
        Self {
            state,
            runner,
            store: Arc::new(std::sync::Mutex::new(store)),
        }
    }

    pub fn schedule(&self, agent_id: AgentId, trigger: Trigger) -> anyhow::Result<ScheduledTask> {
        let task = ScheduledTask::new(agent_id, trigger);
        self.store.lock().unwrap().insert(&task)?;
        self.state.emit(WorkerMessage::AgentLog {
            trace_id: task.id.clone(),
            step_id: "scheduler".to_string(),
            level: goble_core::execution::LogLevel::Info,
            message: format!("scheduled task {} with {:?}", task.id, task.trigger),
        });
        self.emit_routines();
        Ok(task)
    }

    pub fn list_tasks(&self) -> anyhow::Result<Vec<ScheduledTask>> {
        self.store.lock().unwrap().list()
    }

    pub fn cancel_task(&self, task_id: &str) -> anyhow::Result<bool> {
        let deleted = self.store.lock().unwrap().delete(task_id)?;
        self.emit_routines();
        Ok(deleted)
    }

    pub fn pause_task(&self, task_id: &str, enabled: bool) -> anyhow::Result<bool> {
        let updated = self.store.lock().unwrap().enable(task_id, enabled)?;
        self.emit_routines();
        Ok(updated)
    }

    /// Push the current routine snapshot to the desktop (leader only).
    pub fn emit_routines(&self) {
        if !self.state.is_scheduler_leader() {
            return;
        }
        let routines = self
            .store
            .lock()
            .unwrap()
            .list()
            .map(|tasks| tasks.into_iter().map(routine_info).collect())
            .unwrap_or_default();
        self.state.emit(WorkerMessage::RoutinesUpdated { routines });
    }

    pub async fn trigger_agent(&self, agent_id: AgentId) -> anyhow::Result<String> {
        let spec = self
            .state
            .agents
            .lock()
            .get(&agent_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("agent not found"))?;
        let mcp_servers = self.state.mcp_servers.lock().values().cloned().collect();
        let secrets = self.state.secrets.lock().values().cloned().collect();
        let trace_id = uuid::Uuid::new_v4().to_string();
        self.runner
            .run_agent(trace_id.clone(), agent_id, spec, mcp_servers, secrets)
            .await?;
        Ok(trace_id)
    }

    pub async fn handle_trigger(
        &self,
        agent_id: AgentId,
        trigger: Trigger,
    ) -> anyhow::Result<String> {
        match trigger {
            Trigger::Manual
            | Trigger::Http { .. }
            | Trigger::Cron { .. }
            | Trigger::Heartbeat { .. } => self.trigger_agent(agent_id).await,
        }
    }

    pub fn start_loop(self: Arc<Self>, tick_interval: Duration, leader_state: LeaderState) {
        tokio::spawn(async move {
            let mut last_heartbeat = std::collections::HashMap::<String, Instant>::new();
            loop {
                tokio::time::sleep(tick_interval).await;

                let is_leader = leader_state.is_leader();
                let tasks = match self.store.lock().unwrap().list() {
                    Ok(t) => t,
                    Err(e) => {
                        self.state.emit(WorkerMessage::AgentLog {
                            trace_id: "scheduler".to_string(),
                            step_id: "tick".to_string(),
                            level: goble_core::execution::LogLevel::Error,
                            message: format!("store list error: {e}"),
                        });
                        continue;
                    }
                };

                let now = Utc::now();
                for task in tasks {
                    if !task.enabled || !is_leader {
                        continue;
                    }
                    let due = match &task.trigger {
                        Trigger::Manual | Trigger::Http { .. } => continue,
                        Trigger::Cron { expression } => {
                            let schedule = match expression.parse::<Schedule>() {
                                Ok(s) => s,
                                Err(e) => {
                                    self.state.emit(WorkerMessage::AgentLog {
                                        trace_id: task.id.clone(),
                                        step_id: "cron".to_string(),
                                        level: goble_core::execution::LogLevel::Error,
                                        message: format!("invalid cron expression: {e}"),
                                    });
                                    continue;
                                }
                            };
                            match schedule.upcoming(chrono::Utc).next() {
                                Some(t) => t < now,
                                None => continue,
                            }
                        }
                        Trigger::Heartbeat { interval_seconds } => {
                            let interval = Duration::from_secs(*interval_seconds);
                            let last = last_heartbeat
                                .entry(task.id.clone())
                                .or_insert_with(Instant::now);
                            last.elapsed() >= interval
                        }
                    };

                    if due {
                        if let Trigger::Heartbeat { .. } = &task.trigger {
                            last_heartbeat.insert(task.id.clone(), Instant::now());
                        }
                        let _ = self
                            .store
                            .lock()
                            .unwrap()
                            .mark_run(&task.id, "running");
                        let scheduler = Arc::clone(&self);
                        let task_id = task.id.clone();
                        tokio::spawn(async move {
                            let result = scheduler.trigger_agent(task.agent_id.clone()).await;
                            match &result {
                                Ok(_) => {
                                    let _ = scheduler
                                        .store
                                        .lock()
                                        .unwrap()
                                        .mark_run(&task_id, "success");
                                }
                                Err(e) => {
                                    let _ = scheduler
                                        .store
                                        .lock()
                                        .unwrap()
                                        .mark_run(&task_id, "error");
                                    scheduler.state.emit(WorkerMessage::AgentLog {
                                        trace_id: task_id,
                                        step_id: "scheduler".to_string(),
                                        level: goble_core::execution::LogLevel::Error,
                                        message: format!("trigger failed: {e}"),
                                    });
                                }
                            }
                        });
                    }
                }

                // Keep the desktop's routine panel in sync on every tick.
                self.emit_routines();

                self.state.emit(WorkerMessage::StatusReport {
                    worker_id: self.state.worker_id.clone(),
                    status: goble_core::worker::WorkerStatus::Online,
                    load: 0,
                });
            }
        });
    }

    pub fn start_heartbeat_loop(self: Arc<Self>, interval: Duration) {
        let _ = interval;
    }
}

fn routine_info(task: ScheduledTask) -> RoutineInfo {
    RoutineInfo {
        id: task.id,
        agent_id: task.agent_id,
        trigger: task.trigger,
        enabled: task.enabled,
        last_run_at: task.last_run_at.map(|t| t.to_rfc3339()),
        last_status: task.last_status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::agent::AgentSpec;
    use goble_core::llm::LlmProvider;
    use goble_core::worker::WorkerId;
    use tempfile::TempDir;

    fn mock_factory() -> crate::runner::ProviderFactory {
        Box::new(|| {
            let boxed = goble_core::llm::create_provider("mock", "test-key", None);
            let arc: std::sync::Arc<dyn LlmProvider> = std::sync::Arc::from(boxed);
            Ok(arc)
        })
    }

    #[tokio::test]
    async fn test_manual_trigger_finds_agent() {
        let tmp = TempDir::new().unwrap();
        let store = TaskStore::open(tmp.path().join("tasks.db")).unwrap();
        let state = AppState::new(WorkerId::generate());
        let tmp_state = tempfile::tempdir().unwrap();
        state
            .set_store_path(tmp_state.path().join("worker.db"))
            .unwrap();
        let runner =
            crate::runner::Runner::new_with_provider_factory(state.clone(), mock_factory());
        let scheduler = Scheduler::new(state.clone(), store, runner);
        let spec = AgentSpec::new("demo", "do nothing");
        let id = spec.id.clone();
        state.store_agent(spec);
        let trace_id = scheduler.trigger_agent(id).await.unwrap();
        assert!(!trace_id.is_empty());
    }

    #[tokio::test]
    async fn test_manual_trigger_missing_agent_fails() {
        let tmp = TempDir::new().unwrap();
        let store = TaskStore::open(tmp.path().join("tasks.db")).unwrap();
        let state = AppState::new(WorkerId::generate());
        let runner =
            crate::runner::Runner::new_with_provider_factory(state.clone(), mock_factory());
        let scheduler = Scheduler::new(state, store, runner);
        let result = scheduler.trigger_agent(AgentId::generate()).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_schedule_persists() {
        let tmp = TempDir::new().unwrap();
        let store = TaskStore::open(tmp.path().join("tasks.db")).unwrap();
        let state = AppState::new(WorkerId::generate());
        let runner =
            crate::runner::Runner::new_with_provider_factory(state.clone(), mock_factory());
        let scheduler = Scheduler::new(state, store, runner);
        let agent_id = AgentId::generate();
        let task = scheduler
            .schedule(
                agent_id.clone(),
                Trigger::Heartbeat {
                    interval_seconds: 30,
                },
            )
            .unwrap();

        let loaded_store = TaskStore::open(tmp.path().join("tasks.db")).unwrap();
        let tasks = loaded_store.list().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].agent_id, agent_id);

        assert!(scheduler.cancel_task(&task.id).unwrap());
        assert!(scheduler.list_tasks().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_scheduler_loop_triggers_heartbeat() {
        let tmp = TempDir::new().unwrap();
        let store = TaskStore::open(tmp.path().join("tasks.db")).unwrap();
        let state = AppState::new(WorkerId::generate());
        let tmp_state = tempfile::tempdir().unwrap();
        state
            .set_store_path(tmp_state.path().join("worker.db"))
            .unwrap();
        let spec = AgentSpec::new("demo", "do nothing");
        let agent_id = spec.id.clone();
        state.store_agent(spec);
        let runner =
            crate::runner::Runner::new_with_provider_factory(state.clone(), mock_factory());
        let scheduler = Arc::new(Scheduler::new(state, store, runner));
        scheduler
            .schedule(
                agent_id.clone(),
                Trigger::Heartbeat {
                    interval_seconds: 1,
                },
            )
            .unwrap();
        let scheduler_clone = Arc::clone(&scheduler);
        scheduler_clone.start_loop(Duration::from_millis(100), LeaderState::new(true));
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let tasks = scheduler.list_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
    }

    #[tokio::test]
    async fn test_scheduler_loop_skips_triggers_when_not_leader() {
        let tmp = TempDir::new().unwrap();
        let store = TaskStore::open(tmp.path().join("tasks.db")).unwrap();
        let state = AppState::new(WorkerId::generate());
        let tmp_state = tempfile::tempdir().unwrap();
        state
            .set_store_path(tmp_state.path().join("worker.db"))
            .unwrap();
        let spec = AgentSpec::new("demo", "do nothing");
        let agent_id = spec.id.clone();
        state.store_agent(spec);
        let runner =
            crate::runner::Runner::new_with_provider_factory(state.clone(), mock_factory());
        let scheduler = Arc::new(Scheduler::new(state, store, runner));
        scheduler
            .schedule(
                agent_id.clone(),
                Trigger::Heartbeat {
                    interval_seconds: 1,
                },
            )
            .unwrap();
        let scheduler_clone = Arc::clone(&scheduler);
        scheduler_clone.start_loop(Duration::from_millis(100), LeaderState::new(false));
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let tasks = scheduler.list_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
    }
}
