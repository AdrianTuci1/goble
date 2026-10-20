use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::close_code;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::task::JoinHandle;

use crate::state::GatewayState;

#[derive(Deserialize)]
pub(crate) struct WorkerQuery {
    id: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ClientQuery {
    worker: Option<String>,
}

/// Worker endpoint. Frames are opaque: text and binary messages are copied to
/// the attached clients without being parsed as `DesktopMessage`/`WorkerMessage`.
pub(crate) async fn worker_ws(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    Query(query): Query<WorkerQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if !state.worker_authorized(&headers) {
        return (StatusCode::UNAUTHORIZED, "invalid worker token\n").into_response();
    }
    let Some(worker_id) = query.id.filter(|id| !id.trim().is_empty()) else {
        return (StatusCode::BAD_REQUEST, "missing worker id\n").into_response();
    };
    ws.on_upgrade(move |socket| handle_worker(socket, state, worker_id))
}

async fn handle_worker(socket: WebSocket, state: Arc<GatewayState>, worker_id: String) {
    let (connection_id, mut outbound, mut shutdown) = state.register_worker(worker_id.clone());
    let (mut sink, mut stream) = socket.split();

    let mut sink_shutdown = shutdown.clone();
    let sink_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                message = outbound.recv() => match message {
                    Some(message) => {
                        if sink.send(message).await.is_err() {
                            return;
                        }
                    }
                    None => break,
                },
                _ = sink_shutdown.changed() => break,
            }
        }
        let _ = sink
            .send(Message::Close(Some(CloseFrame {
                code: close_code::NORMAL,
                reason: "gateway closing".into(),
            })))
            .await;
    });

    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if !state.relay_from_worker(
                        &worker_id,
                        &connection_id,
                        Message::Text(text),
                    ) {
                        break;
                    }
                }
                Some(Ok(Message::Binary(bytes))) => {
                    if !state.relay_from_worker(
                        &worker_id,
                        &connection_id,
                        Message::Binary(bytes),
                    ) {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {
                    if !state.worker_current(&worker_id, &connection_id) {
                        break;
                    }
                }
                Some(Err(_)) => break,
            },
            _ = shutdown.changed() => break,
        }
    }

    state.disconnect_worker(&worker_id, &connection_id);
    settle(sink_task).await;
}

/// Client endpoint. Requires the browser session cookie; the requested worker
/// must already be connected.
pub(crate) async fn client_ws(
    State(state): State<Arc<GatewayState>>,
    headers: HeaderMap,
    Query(query): Query<ClientQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if !state.client_authorized(&headers) {
        return (StatusCode::UNAUTHORIZED, "login required\n").into_response();
    }
    let Some(worker_id) = query.worker.filter(|id| !id.trim().is_empty()) else {
        return (StatusCode::BAD_REQUEST, "missing worker id\n").into_response();
    };
    if !state.has_worker(&worker_id) {
        return (StatusCode::NOT_FOUND, "worker not connected\n").into_response();
    }
    ws.on_upgrade(move |socket| handle_client(socket, state, worker_id))
}

async fn handle_client(socket: WebSocket, state: Arc<GatewayState>, worker_id: String) {
    let Some((client_id, mut outbound, mut shutdown)) = state.register_client(&worker_id) else {
        let (mut sink, _) = socket.split();
        let _ = sink
            .send(Message::Close(Some(CloseFrame {
                code: close_code::ERROR,
                reason: "worker not connected".into(),
            })))
            .await;
        return;
    };

    let (mut sink, mut stream) = socket.split();
    let mut sink_shutdown = shutdown.clone();
    let sink_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                message = outbound.recv() => match message {
                    Some(message) => {
                        if sink.send(message).await.is_err() {
                            return;
                        }
                    }
                    None => break,
                },
                _ = sink_shutdown.changed() => break,
            }
        }
        let _ = sink
            .send(Message::Close(Some(CloseFrame {
                code: close_code::NORMAL,
                reason: "gateway closing".into(),
            })))
            .await;
    });

    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if !state
                        .send_to_worker(&worker_id, Message::Text(text))
                        .await
                    {
                        break;
                    }
                }
                Some(Ok(Message::Binary(bytes))) => {
                    if !state
                        .send_to_worker(&worker_id, Message::Binary(bytes))
                        .await
                    {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {}
                Some(Err(_)) => break,
            },
            _ = shutdown.changed() => break,
        }
    }

    state.unregister_client(&worker_id, client_id);
    settle(sink_task).await;
}

/// Give the sink a moment to flush its close frame before abandoning it.
async fn settle(mut task: JoinHandle<()>) {
    if tokio::time::timeout(Duration::from_millis(500), &mut task)
        .await
        .is_err()
    {
        task.abort();
    }
}
