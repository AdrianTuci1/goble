use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use goble_core::worker::WorkerStatus;

use crate::runner::Runner;
use crate::state::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
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
                    if let Err(e) = handle_desktop_message(&state, &runner, desktop_msg).await {
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
}

async fn handle_desktop_message(
    state: &Arc<AppState>,
    runner: &Runner,
    msg: DesktopMessage,
) -> anyhow::Result<()> {
    match msg {
        DesktopMessage::PairRequest {
            worker_id,
            pairing_code_hash,
        } => {
            if worker_id == state.worker_id && state.is_paired(&pairing_code_hash) {
                state.emit(WorkerMessage::Paired);
            }
        }
        DesktopMessage::RunAgent {
            trace_id,
            agent_id,
            spec,
        } => {
            state.store_agent(spec.clone());
            runner.run_agent(trace_id, agent_id, spec).await?;
        }
        DesktopMessage::PushSecrets { secrets } => {
            for secret in secrets {
                state.store_secret(secret);
            }
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
        DesktopMessage::ScheduleAgent { agent_id, trigger } => {
            state.emit(WorkerMessage::AgentLog {
                trace_id: format!("schedule-{}", agent_id),
                step_id: "scheduler".to_string(),
                level: goble_core::execution::LogLevel::Info,
                message: format!("scheduled {:?}", trigger),
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
    }
    Ok(())
}
