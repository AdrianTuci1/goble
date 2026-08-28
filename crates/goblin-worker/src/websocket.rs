use std::sync::Arc;

use axum::extract::connect_info::{Connected, ConnectInfo};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::serve::IncomingStream;
use futures::{SinkExt, StreamExt};
use goble_core::identity::{extract_role_from_der, extract_serial_from_der};
use goble_core::protocol::{DesktopMessage, ScheduledTaskSummary, WorkerMessage};
use goble_core::worker::WorkerStatus;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::server::TlsStream;

use crate::runner::Runner;
use crate::state::{AppState, ClientSession};

/// Connection metadata extracted from an mTLS WebSocket connection.
#[derive(Clone, Debug)]
pub struct TlsPeer {
    pub certs: Option<Vec<rustls::pki_types::CertificateDer<'static>>>,
    pub peer_addr: Option<std::net::SocketAddr>,
}

impl Connected<TlsStream<TcpStream>> for TlsPeer {
    fn connect_info(stream: TlsStream<TcpStream>) -> Self {
        let (tcp, conn) = stream.get_ref();
        let certs = conn.peer_certificates().map(|chain| {
            chain
                .iter()
                .map(|c| rustls::pki_types::CertificateDer::from(c.as_ref().to_vec()))
                .collect()
        });
        Self {
            certs,
            peer_addr: tcp.peer_addr().ok(),
        }
    }
}

impl Connected<TcpStream> for TlsPeer {
    fn connect_info(stream: TcpStream) -> Self {
        Self {
            certs: None,
            peer_addr: stream.peer_addr().ok(),
        }
    }
}

impl Connected<IncomingStream<'_, TcpListener>> for TlsPeer {
    fn connect_info(stream: IncomingStream<'_, TcpListener>) -> Self {
        Self {
            certs: None,
            peer_addr: Some(*stream.remote_addr()),
        }
    }
}

impl Connected<IncomingStream<'_, crate::listener::TlsListener>> for TlsPeer {
    fn connect_info(stream: IncomingStream<'_, crate::listener::TlsListener>) -> Self {
        let peer_addr = Some(*stream.remote_addr());
        let certs = stream
            .io()
            .get_ref()
            .1
            .peer_certificates()
            .map(|chain| {
                chain
                    .iter()
                    .map(|c| rustls::pki_types::CertificateDer::from(c.as_ref().to_vec()))
                    .collect()
            });
        Self { certs, peer_addr }
    }
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<TlsPeer>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state, peer))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>, peer: TlsPeer) {
    let session = peer
        .certs
        .as_ref()
        .and_then(|certs| certs.first())
        .and_then(|cert| {
            let user_id = extract_serial_from_der(cert.as_ref()).ok()?;
            let role = extract_role_from_der(cert.as_ref()).ok()?;
            let session = ClientSession { user_id, role };
            state.insert_session(session.clone());
            Some(session)
        });

    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.event_tx.subscribe();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Ok(msg) => {
                            let json = match serde_json::to_string(&msg) {
                                Ok(j) => j,
                                Err(_) => continue,
                            };
                            if sender.send(Message::Text(json.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });

    let runner = Runner::new(state.clone());
    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(text) = msg {
            match serde_json::from_str::<DesktopMessage>(&text) {
                Ok(desktop_msg) => {
                    if let Err(e) = handle_desktop_message(
                        &state,
                        &runner,
                        session.clone(),
                        desktop_msg,
                    )
                    .await
                    {
                        state.emit(WorkerMessage::AgentFinished {
                            trace_id: "unknown".to_string(),
                            status: goble_core::execution::ExecutionStatus::Failure(e.to_string()),
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
    }

    if let Some(s) = session {
        state.sessions.lock().remove(&s.user_id);
    }
}

pub async fn handle_desktop_message(
    state: &Arc<AppState>,
    runner: &Runner,
    session: Option<ClientSession>,
    msg: DesktopMessage,
) -> anyhow::Result<()> {
    match msg {
        DesktopMessage::PairRequest {
            worker_id,
            pairing_code_hash,
        } => {
            if worker_id == state.worker_id {
                // In mTLS mode the client certificate already authenticates the desktop.
                // If we have a session, mark it active and accept the pairing immediately.
                if state.is_mtls_active() && session.is_some() {
                    state.emit(WorkerMessage::Paired);
                } else if state.is_ssh_proxy_mode() && state.pairing_hash.lock().is_none() {
                    if let Some(hash) = pairing_code_hash.clone() {
                        state.set_pairing_hash(hash);
                    }
                    state.emit(WorkerMessage::Paired);
                } else if state.is_mtls_active()
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
            for secret in secrets.clone() {
                state.store_secret(secret);
            }
            let mut vault = state.file_vault.lock();
            for secret in secrets {
                vault.set(&secret.name, &secret.encrypted_value, b"").ok();
            }
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
