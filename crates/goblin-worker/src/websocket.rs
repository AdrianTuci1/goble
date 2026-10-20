use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use goble_core::protocol::{DesktopMessage, ScheduledTaskSummary, WorkerMessage};
use goble_core::worker::WorkerStatus;
use goble_daemon::DaemonPort;
use goble_harness_types::SessionId;
use goble_workflow::{WorkflowHostRequest, WorkflowRun};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::runner::Runner;
use crate::state::{map_daemon_event, AppState};

/// Frames buffered in each direction of a connection bridge.
pub(crate) const FRAME_QUEUE: usize = 64;

/// One WebSocket frame in transport-independent form.
///
/// [`run_connection`] speaks this type, so the inbound axum endpoint
/// ([`ws_handler`]) and the outbound gateway client share one implementation of
/// the desktop protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkerFrame {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

/// A bridge task that is aborted when the connection future is dropped, so a
/// cancelled connection (a shutdown, an aborted task) leaves no socket reader
/// or writer behind.
pub(crate) struct ConnectionTask(JoinHandle<()>);

impl ConnectionTask {
    pub(crate) fn spawn<F>(future: F) -> Self
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        Self(tokio::spawn(future))
    }

    /// Stop the task immediately.
    pub(crate) async fn abort(mut self) {
        self.0.abort();
        let _ = (&mut self.0).await;
    }

    /// Give the task `grace` to finish, then abort it.
    pub(crate) async fn settle(mut self, grace: Duration) {
        if tokio::time::timeout(grace, &mut self.0).await.is_err() {
            self.0.abort();
            let _ = (&mut self.0).await;
        }
    }
}

impl Drop for ConnectionTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

/// axum adapter: translate socket frames to [`WorkerFrame`]s and back, then run
/// the shared connection loop.
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let (incoming_tx, incoming) = mpsc::channel(FRAME_QUEUE);
    let (outgoing, mut outgoing_rx) = mpsc::channel(FRAME_QUEUE);

    let reader = ConnectionTask::spawn(async move {
        while let Some(message) = receiver.next().await {
            let frame = match message {
                Ok(Message::Text(text)) => WorkerFrame::Text(text.to_string()),
                Ok(Message::Binary(bytes)) => WorkerFrame::Binary(bytes.to_vec()),
                Ok(Message::Close(_)) => WorkerFrame::Close,
                // Control frames are answered by axum; they carry no protocol.
                Ok(Message::Ping(_) | Message::Pong(_)) => continue,
                Err(_) => WorkerFrame::Close,
            };
            let closed = matches!(frame, WorkerFrame::Close);
            if incoming_tx.send(frame).await.is_err() || closed {
                break;
            }
        }
    });

    let writer = ConnectionTask::spawn(async move {
        while let Some(frame) = outgoing_rx.recv().await {
            let message = match frame {
                WorkerFrame::Text(text) => Message::Text(text.into()),
                WorkerFrame::Binary(bytes) => Message::Binary(bytes.into()),
                WorkerFrame::Close => {
                    let _ = sender.send(Message::Close(None)).await;
                    break;
                }
            };
            if sender.send(message).await.is_err() {
                break;
            }
        }
    });

    run_connection(state, incoming, outgoing).await;

    reader.abort().await;
    // `run_connection` drops the outgoing sender when it returns, so the writer
    // ends on its own; give it a moment to flush its close frame.
    writer.settle(Duration::from_millis(500)).await;
}

/// Run one worker connection over transport-independent frames.
///
/// Events from the worker's broadcast channel are forwarded as text frames;
/// incoming desktop messages are dispatched exactly as the axum endpoint did.
/// The connection owns the panes it opens, and closes them when it ends.
pub(crate) async fn run_connection(
    state: Arc<AppState>,
    mut incoming: mpsc::Receiver<WorkerFrame>,
    outgoing: mpsc::Sender<WorkerFrame>,
) {
    let mut rx = state.event_tx.subscribe();

    let runner = Runner::new(state.clone());
    // Panes are owned by this connection: they are opened by it and closed
    // when it goes away, so another client cannot attach to or keep them alive.
    let pane_owner = Uuid::new_v4().to_string();
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(msg) => {
                        let json = match serde_json::to_string(&msg) {
                            Ok(j) => j,
                            Err(_) => continue,
                        };
                        if outgoing.send(WorkerFrame::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            frame = incoming.recv() => {
                match frame {
                    Some(WorkerFrame::Text(text)) => {
                        match serde_json::from_str::<DesktopMessage>(&text) {
                            Ok(desktop_msg) => {
                                // PTY traffic is intercepted before the connection-independent
                                // handlers so the manager knows who owns the pane.
                                if let Some(events) =
                                    handle_pty_desktop_message(&state, &pane_owner, &desktop_msg)
                                {
                                    for event in events {
                                        state.emit(event);
                                    }
                                    continue;
                                }
                                if let Err(e) =
                                    handle_desktop_message(&state, &runner, desktop_msg).await
                                {
                                    state.emit(WorkerMessage::AgentFinished {
                                        trace_id: "unknown".to_string(),
                                        status: goble_core::execution::ExecutionStatus::Failure(
                                            e.to_string(),
                                        ),
                                    });
                                }
                            }
                            Err(_) => {
                                state.emit(WorkerMessage::AgentFinished {
                                    trace_id: "unknown".to_string(),
                                    status: goble_core::execution::ExecutionStatus::Failure(
                                        "invalid message".to_string(),
                                    ),
                                });
                            }
                        }
                    }
                    // Binary frames carry no desktop protocol; they have never
                    // been dispatched.
                    Some(WorkerFrame::Binary(_)) => {}
                    Some(WorkerFrame::Close) | None => break,
                }
            }
        }
    }

    // The socket is gone: so are its terminals. The children are killed here;
    // their `PtyExit` frames have no socket left to reach.
    for event in state.pty.close_owned(&pane_owner) {
        state.emit(event);
    }
}

/// Handle the PTY [`DesktopMessage`]s for the connection named `owner`.
///
/// Returns `Some(events)` when `msg` is one of the four PTY messages, `None`
/// when it belongs to another handler. The caller emits the events; output and
/// exits arrive on the same channel from the manager's reader and waiter tasks.
pub fn handle_pty_desktop_message(
    state: &Arc<AppState>,
    owner: &str,
    msg: &DesktopMessage,
) -> Option<Vec<WorkerMessage>> {
    use DesktopMessage as DM;
    let events = match msg {
        DM::OpenPty {
            pane_id,
            cwd,
            cols,
            rows,
        } => state.pty.spawn(owner, pane_id, cwd, *cols, *rows),
        DM::PtyInput { pane_id, data } => state.pty.input(owner, pane_id, data),
        DM::PtyResize {
            pane_id,
            cols,
            rows,
        } => state.pty.resize(owner, pane_id, *cols, *rows),
        DM::ClosePty { pane_id } => state.pty.close(owner, pane_id),
        _ => return None,
    };
    Some(events)
}

fn pty_pane_id(msg: &DesktopMessage) -> Option<String> {
    match msg {
        DesktopMessage::OpenPty { pane_id, .. }
        | DesktopMessage::PtyInput { pane_id, .. }
        | DesktopMessage::PtyResize { pane_id, .. }
        | DesktopMessage::ClosePty { pane_id } => Some(pane_id.clone()),
        _ => None,
    }
}

async fn handle_desktop_message(
    state: &Arc<AppState>,
    runner: &Runner,
    msg: DesktopMessage,
) -> anyhow::Result<()> {
    match msg {
        // Daemon transcript reversibility + workflow host requests are handled
        // by the shared (and independently testable) `handle_daemon_desktop_message`.
        msg @ (DesktopMessage::Rewind { .. }
        | DesktopMessage::Fork { .. }
        | DesktopMessage::Replay { .. }
        | DesktopMessage::Checkpoints { .. }
        | DesktopMessage::Select { .. }
        | DesktopMessage::Apply { .. }
        | DesktopMessage::Release { .. }
        | DesktopMessage::Discard { .. }
        | DesktopMessage::RunWorkflow { .. }) => {
            handle_daemon_desktop_message(state, &msg)?;
        }
        // PTY messages are connection-scoped and are intercepted in
        // `handle_socket`, which knows the owner. This arm only keeps the match
        // exhaustive; it answers a caller that bypassed that interception.
        msg @ (DesktopMessage::OpenPty { .. }
        | DesktopMessage::PtyInput { .. }
        | DesktopMessage::PtyResize { .. }
        | DesktopMessage::ClosePty { .. }) => {
            if let Some(pane_id) = pty_pane_id(&msg) {
                state.emit(WorkerMessage::PtyError {
                    pane_id,
                    message: "PTY messages are handled per connection".to_string(),
                });
            }
        }
        DesktopMessage::PairRequest {
            worker_id,
            pairing_code_hash,
        } => {
            if worker_id == state.worker_id {
                if state.is_mtls_active()
                    || state.is_paired(pairing_code_hash.as_deref().unwrap_or_default())
                {
                    state.emit(WorkerMessage::Paired);
                }
            }
        }
        DesktopMessage::RunAgent {
            trace_id,
            agent_id,
            spec,
            mcp_servers,
        } => {
            state.store_agent(spec.clone());
            for server in mcp_servers.clone() {
                state.store_mcp(server);
            }
            let secrets = state.secrets.lock().values().cloned().collect();
            runner
                .run_agent(trace_id, agent_id, spec, mcp_servers, secrets)
                .await?;
        }
        DesktopMessage::ScheduleAgent {
            agent_id,
            trigger,
            mcp_servers,
        } => {
            for server in mcp_servers.clone() {
                state.store_mcp(server);
            }
            let scheduler = state
                .scheduler()
                .ok_or_else(|| anyhow::anyhow!("scheduler not available"))?;
            scheduler.schedule(agent_id.clone(), trigger)?;
            state.emit(WorkerMessage::AgentLog {
                trace_id: format!("schedule-{}", agent_id),
                step_id: "scheduler".to_string(),
                level: goble_core::execution::LogLevel::Info,
                message: "scheduled".to_string(),
            });
        }
        DesktopMessage::PushSecrets { secrets } => {
            // A push is a seed, not the mechanism: only the names this
            // workspace does not already hold are taken. The worker's vault is
            // authoritative for the workspace, so a client whose local key
            // differs must not change what the remote turns authenticate with
            // — that is the explicit `SetVaultSecret` act.
            let fresh: Vec<goble_core::secret::Secret> = secrets
                .into_iter()
                .filter(|secret| !state.holds_secret(&secret.name))
                .collect();
            if fresh.is_empty() {
                return Ok(());
            }
            {
                let mut vault = state.file_vault.lock();
                for secret in &fresh {
                    vault.set(&secret.name, &secret.encrypted_value, b"").ok();
                }
            }
            for secret in fresh {
                state.store_secret(secret);
            }
            // `save_vault` takes the same lock, so it is released first.
            state.save_vault(b"").ok();
        }
        DesktopMessage::PushMcpServers { servers } => {
            for server in servers {
                state.store_mcp(server);
            }
        }
        DesktopMessage::UpdateAgent { agent_id, spec } => {
            if spec.id == agent_id {
                state.store_agent(spec);
            }
        }
        DesktopMessage::RemoveAgent { agent_id } => {
            state.agents.lock().remove(&agent_id);
        }
        DesktopMessage::RunTeam { trace_id, team_id } => {
            runner.run_team(trace_id, team_id).await?;
        }
        DesktopMessage::RunAgentForThreadReply {
            trace_id,
            thread_id,
            agent_id,
            prompt,
            spec,
            mcp_servers,
        } => {
            state.store_agent(spec.clone());
            for server in mcp_servers.clone() {
                state.store_mcp(server);
            }
            let secrets = state.secrets.lock().values().cloned().collect();
            let content = runner
                .run_agent_for_thread_reply(
                    trace_id.clone(),
                    agent_id,
                    spec,
                    prompt,
                    mcp_servers,
                    secrets,
                )
                .await?;
            state.emit(WorkerMessage::ThreadAgentReply {
                trace_id,
                thread_id,
                content,
            });
        }
        DesktopMessage::Ping => {
            state.emit(WorkerMessage::Pong);
            state.emit(WorkerMessage::StatusReport {
                worker_id: state.worker_id.clone(),
                status: WorkerStatus::Online,
                load: 0,
            });
        }
        DesktopMessage::SetVaultSecret { name, value } => {
            if state.file_vault.lock().set(&name, &value, b"").is_ok() {
                state.save_vault(b"")?;
            }
            state.emit(WorkerMessage::VaultSecret {
                name,
                value: Some(value),
            });
        }
        DesktopMessage::GetVaultSecret { name } => {
            let value = state.file_vault.lock().get(&name, b"").ok().flatten();
            state.emit(WorkerMessage::VaultSecret { name, value });
        }
        DesktopMessage::ListScheduledTasks => {
            let tasks: Vec<ScheduledTaskSummary> = state
                .scheduler()
                .map(|s| {
                    s.list_tasks()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|t| ScheduledTaskSummary {
                            id: t.id,
                            agent_id: t.agent_id,
                            trigger: t.trigger,
                            enabled: t.enabled,
                        })
                        .collect()
                })
                .unwrap_or_default();
            state.emit(WorkerMessage::ScheduledTasks { tasks });
        }
        DesktopMessage::CancelScheduledTask { task_id } => {
            if let Some(scheduler) = state.scheduler() {
                scheduler.cancel_task(&task_id)?;
            }
            state.emit(WorkerMessage::TaskCancelled { task_id });
        }
        DesktopMessage::GetTrace { trace_id } => {
            let trace = state.get_trace(&trace_id);
            state.emit(WorkerMessage::Trace { trace_id, trace });
        }
        DesktopMessage::QueryEntities {
            entity_type,
            query: _,
        } => {
            let items = query_store_entities(state, &entity_type)?;
            state.emit(WorkerMessage::EntityList { entity_type, items });
        }
        DesktopMessage::TriggerSnapshot => {
            if let Some(provider) = state.snapshot_provider() {
                if let Some(cluster_key) = state.cluster_key() {
                    let store = state.store()?;
                    let worker_id = state.worker_id.clone();
                    let snapshot = goble_core::snapshot::Snapshot::from_store(
                        &store,
                        &worker_id,
                        &cluster_key,
                    )?;
                    provider.upload_snapshot(&worker_id, &snapshot)?;
                    state.emit(WorkerMessage::AgentLog {
                        trace_id: "snapshot".to_string(),
                        step_id: "snapshot".to_string(),
                        level: goble_core::execution::LogLevel::Info,
                        message: "snapshot uploaded".to_string(),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Handle the daemon reversibility and workflow [`DesktopMessage`]s.
///
/// These drive the embedded daemon's harness-agnostic transcript reversibility
/// (rewind/fork/replay/checkpoints/settle) and its workflow host, mapping each to
/// the corresponding [`DaemonPort`] method and forwarding the resulting
/// [`DaemonEvent`]s / results onto the worker's [`WorkerMessage`] event channel.
///
/// Returns `Ok(true)` when `msg` was one of these daemon messages (and was
/// handled), `Ok(false)` when it is not. Kept independent of the [`Runner`] so it
/// can be unit/integration tested without driving a full agent turn.
pub fn handle_daemon_desktop_message(
    state: &Arc<AppState>,
    msg: &DesktopMessage,
) -> anyhow::Result<bool> {
    use DesktopMessage as DM;
    match msg {
        DM::Rewind { session_id, at } => {
            let sid = SessionId::new(session_id.clone());
            let removed = state.daemon_state().rewind(&sid, *at)?;
            state.emit(WorkerMessage::RewindResult {
                session_id: sid.0,
                removed,
            });
            Ok(true)
        }
        DM::Fork {
            session_id,
            at,
            new_session_id,
        } => {
            let sid = SessionId::new(session_id.clone());
            let new_sid =
                state
                    .daemon_state()
                    .fork(&sid, *at, SessionId::new(new_session_id.clone()))?;
            state.emit(WorkerMessage::ForkResult {
                session_id: sid.0,
                new_session_id: new_sid.0,
            });
            Ok(true)
        }
        DM::Replay { session_id, at } => {
            let sid = SessionId::new(session_id.clone());
            let events = state.daemon_state().replay(&sid, *at)?;
            for event in events {
                if let Some(msg) = map_daemon_event(event) {
                    state.emit(msg);
                }
            }
            state.emit(WorkerMessage::ReplayResult {
                session_id: sid.0,
                at: *at,
            });
            Ok(true)
        }
        DM::Checkpoints { session_id } => {
            let sid = SessionId::new(session_id.clone());
            let checkpoints = state
                .daemon_state()
                .checkpoints(&sid)?
                .into_iter()
                .map(|c| serde_json::to_value(c).unwrap_or_default())
                .collect();
            state.emit(WorkerMessage::CheckpointsResult {
                session_id: sid.0,
                checkpoints,
            });
            Ok(true)
        }
        DM::Select { session_id, at } => {
            let sid = SessionId::new(session_id.clone());
            state.daemon_state().select(&sid, *at)?;
            state.emit(WorkerMessage::SelectResult {
                session_id: sid.0,
                at: *at,
            });
            Ok(true)
        }
        DM::Apply { session_id, at } => {
            let sid = SessionId::new(session_id.clone());
            let removed = state.daemon_state().apply(&sid, *at)?;
            state.emit(WorkerMessage::ApplyResult {
                session_id: sid.0,
                removed,
            });
            Ok(true)
        }
        DM::Release { session_id, at } => {
            let sid = SessionId::new(session_id.clone());
            let released = state.daemon_state().release(&sid, *at)?;
            state.emit(WorkerMessage::ReleaseResult {
                session_id: sid.0,
                released,
            });
            Ok(true)
        }
        DM::Discard { session_id, at } => {
            let sid = SessionId::new(session_id.clone());
            let removed = state.daemon_state().discard(&sid, *at)?;
            state.emit(WorkerMessage::DiscardResult {
                session_id: sid.0,
                removed,
            });
            Ok(true)
        }
        DM::RunWorkflow { request } => {
            let req: WorkflowHostRequest = serde_json::from_value(request.clone())?;
            let run: WorkflowRun = state.daemon_state().run_workflow(req)?;
            state.emit(WorkerMessage::WorkflowRun {
                run: serde_json::to_value(&run)?,
            });
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn query_store_entities(
    state: &Arc<AppState>,
    entity_type: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let store = state.store()?;
    match entity_type {
        "agents" => {
            let rows = store.list_agents()?;
            let mut items = Vec::new();
            for (id, name, spec, created_at, updated_at) in rows {
                let spec: serde_json::Value = serde_json::from_str(&spec).unwrap_or_default();
                items.push(serde_json::json!({
                    "id": id,
                    "name": name,
                    "spec": spec,
                    "created_at": created_at,
                    "updated_at": updated_at,
                }));
            }
            Ok(items)
        }
        "teams" => {
            let rows = store.list_teams()?;
            let mut items = Vec::new();
            for (id, name, metadata, created_at) in rows {
                let members: Vec<String> = store
                    .list_team_members(&id)?
                    .into_iter()
                    .map(|(_, agent_id)| agent_id)
                    .collect();
                let metadata: serde_json::Value =
                    serde_json::from_str(&metadata).unwrap_or_default();
                items.push(serde_json::json!({
                    "id": id,
                    "name": name,
                    "metadata": metadata,
                    "members": members,
                    "created_at": created_at,
                }));
            }
            Ok(items)
        }
        "workflows" => {
            let rows = store.list_workflows()?;
            let mut items = Vec::new();
            for (id, name, description, spec, trigger, enabled, created_at, updated_at) in rows {
                let spec: serde_json::Value = serde_json::from_str(&spec).unwrap_or_default();
                items.push(serde_json::json!({
                    "id": id,
                    "name": name,
                    "description": description,
                    "spec": spec,
                    "trigger": trigger,
                    "enabled": enabled,
                    "created_at": created_at,
                    "updated_at": updated_at,
                }));
            }
            Ok(items)
        }
        "executions" => {
            let rows = store.list_executions()?;
            let items = rows
                .into_iter()
                .map(
                    |(id, agent_id, worker_id, status, trace, started_at, finished_at)| {
                        let trace: serde_json::Value =
                            serde_json::from_str(&trace).unwrap_or_default();
                        serde_json::json!({
                            "id": id,
                            "agent_id": agent_id,
                            "worker_id": worker_id,
                            "status": status,
                            "trace": trace,
                            "started_at": started_at,
                            "finished_at": finished_at,
                        })
                    },
                )
                .collect();
            Ok(items)
        }
        "mcp_servers" => {
            let rows = store.list_mcp_servers()?;
            let items = rows
                .into_iter()
                .map(
                    |(
                        id,
                        name,
                        source,
                        source_value,
                        manifest,
                        credentials_key,
                        secret_ids,
                        enabled_tools,
                        installed_at,
                        updated_at,
                    )| {
                        let manifest: serde_json::Value =
                            serde_json::from_str(&manifest).unwrap_or_default();
                        let secret_ids: serde_json::Value =
                            serde_json::from_str(&secret_ids).unwrap_or_default();
                        let enabled_tools: serde_json::Value =
                            serde_json::from_str(&enabled_tools).unwrap_or_default();
                        serde_json::json!({
                            "id": id,
                            "name": name,
                            "source": source,
                            "source_value": source_value,
                            "manifest": manifest,
                            "credentials_key": credentials_key,
                            "secret_ids": secret_ids,
                            "enabled_tools": enabled_tools,
                            "installed_at": installed_at,
                            "updated_at": updated_at,
                        })
                    },
                )
                .collect();
            Ok(items)
        }
        _ => anyhow::bail!("unknown entity type: {entity_type}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::secret::Secret;
    use goble_core::worker::WorkerId;

    /// A worker whose vault lives in a temp dir, so nothing touches
    /// `/var/goblin/vault.json`.
    fn test_state() -> (tempfile::TempDir, Arc<AppState>) {
        let tmp = tempfile::tempdir().unwrap();
        let state = AppState::new(WorkerId::generate());
        state.set_vault_path(tmp.path().join("vault.json"));
        (tmp, state)
    }

    fn pushed(value: &[u8]) -> DesktopMessage {
        DesktopMessage::PushSecrets {
            secrets: vec![Secret::new("llm_api_key", "openai", value.to_vec())],
        }
    }

    #[tokio::test]
    async fn test_pty_messages_are_intercepted_per_connection() {
        let (_tmp, state) = test_state();

        assert!(
            handle_pty_desktop_message(&state, "conn", &DesktopMessage::Ping).is_none(),
            "non-PTY messages fall through to the regular handler"
        );

        let missing = handle_pty_desktop_message(
            &state,
            "conn",
            &DesktopMessage::PtyInput {
                pane_id: "missing".to_string(),
                data: goble_core::protocol::encode_pty_bytes(b"x"),
            },
        )
        .expect("PtyInput is a PTY message");
        assert!(matches!(
            &missing[..],
            [WorkerMessage::PtyError { pane_id, .. }] if pane_id == "missing"
        ));

        let closed = handle_pty_desktop_message(
            &state,
            "conn",
            &DesktopMessage::ClosePty {
                pane_id: "missing".to_string(),
            },
        )
        .expect("ClosePty is a PTY message");
        assert!(closed.is_empty(), "closing an absent pane is a no-op");
    }

    /// The transport-agnostic loop answers the desktop `Ping` with `Pong` and a
    /// status report, without an axum socket in the way.
    #[tokio::test]
    async fn test_run_connection_dispatches_ping_through_channels() {
        let (_tmp, state) = test_state();
        let (incoming_tx, incoming) = tokio::sync::mpsc::channel(8);
        let (outgoing, mut outgoing_rx) = tokio::sync::mpsc::channel(8);

        let connection = tokio::spawn(run_connection(state.clone(), incoming, outgoing));
        incoming_tx
            .send(WorkerFrame::Text(
                serde_json::to_string(&DesktopMessage::Ping).unwrap(),
            ))
            .await
            .unwrap();

        let mut pong = false;
        let mut status = false;
        let answered = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(frame) = outgoing_rx.recv().await {
                let WorkerFrame::Text(text) = frame else {
                    continue;
                };
                match serde_json::from_str::<WorkerMessage>(&text).unwrap() {
                    WorkerMessage::Pong => pong = true,
                    WorkerMessage::StatusReport { .. } => status = true,
                    _ => {}
                }
                if pong && status {
                    return;
                }
            }
        })
        .await;
        assert!(answered.is_ok(), "the connection did not answer the ping");
        assert!(pong && status, "pong={pong} status={status}");

        incoming_tx.send(WorkerFrame::Close).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), connection)
            .await
            .expect("the connection did not end on close")
            .unwrap();
    }

    /// A freshly provisioned worker holds nothing; the push is what fills it.
    #[tokio::test]
    async fn test_push_secrets_fills_a_worker_that_holds_nothing() {
        let (_tmp, state) = test_state();
        let runner = Runner::new(state.clone());

        handle_desktop_message(&state, &runner, pushed(b"sk-client"))
            .await
            .unwrap();

        assert!(
            state
                .secrets
                .lock()
                .values()
                .any(|s| s.name == "llm_api_key" && s.encrypted_value == b"sk-client"),
            "the worker holds the pushed key under the name it resolves"
        );
    }

    /// What `goble secret set --worker …` leaves behind: the vault holds the
    /// value this workspace authenticates with. A client that connects with a
    /// different local key must not rewrite it.
    ///
    /// `CredentialVault::set` refuses an empty passphrase and the message
    /// handlers pass `b""`, so this vault is written under one — the state a
    /// vault written by an explicit act is in.
    #[tokio::test]
    async fn test_push_secrets_does_not_overwrite_a_name_the_vault_holds() {
        let (_tmp, state) = test_state();
        state
            .file_vault
            .lock()
            .set("llm_api_key", b"sk-worker", b"passphrase")
            .unwrap();
        let runner = Runner::new(state.clone());

        handle_desktop_message(&state, &runner, pushed(b"sk-client"))
            .await
            .unwrap();

        assert_eq!(
            state
                .file_vault
                .lock()
                .get("llm_api_key", b"passphrase")
                .unwrap(),
            Some(b"sk-worker".to_vec()),
            "the worker's vault is authoritative"
        );
        assert!(
            state
                .secrets
                .lock()
                .values()
                .all(|s| s.name != "llm_api_key"),
            "nor is the pushed value taken into memory"
        );
    }

    /// The same rule for a name the worker already holds in memory: the second
    /// push is a gap that is not there.
    #[tokio::test]
    async fn test_push_secrets_does_not_overwrite_a_name_already_held() {
        let (_tmp, state) = test_state();
        state.store_secret(Secret::new("llm_api_key", "openai", b"sk-first".to_vec()));
        let runner = Runner::new(state.clone());

        handle_desktop_message(&state, &runner, pushed(b"sk-second"))
            .await
            .unwrap();

        let held: Vec<Vec<u8>> = state
            .secrets
            .lock()
            .values()
            .map(|s| s.encrypted_value.clone())
            .collect();
        assert_eq!(
            held,
            vec![b"sk-first".to_vec()],
            "the value the worker holds stays the one it resolves"
        );
    }
}
