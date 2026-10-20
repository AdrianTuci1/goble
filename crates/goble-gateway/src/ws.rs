use std::collections::HashSet;
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

    // Panes are opened by the worker and live until explicitly closed. The
    // worker cannot tell which browser opened one, so the gateway remembers the
    // `open_pty` frames this client sent and closes the leftovers when its
    // socket goes away. Without this, a closed browser tab would leave shells
    // running in the sandbox for the worker's lifetime.
    let mut panes: HashSet<String> = HashSet::new();

    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    track_pane(&mut panes, &text);
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
    for pane_id in panes {
        let close = serde_json::json!({"kind": "close_pty", "pane_id": pane_id}).to_string();
        let _ = state
            .send_to_worker(&worker_id, Message::Text(close.into()))
            .await;
    }
    settle(sink_task).await;
}

/// Note the panes a browser client opened or closed. Frames stay opaque to the
/// relay; only the two lifecycle kinds are inspected.
fn track_pane(panes: &mut HashSet<String>, text: &str) {
    let Ok(frame) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let Some(pane_id) = frame.get("pane_id").and_then(|value| value.as_str()) else {
        return;
    };
    if pane_id.trim().is_empty() {
        return;
    }
    match frame.get("kind").and_then(|value| value.as_str()) {
        Some("open_pty") => {
            panes.insert(pane_id.to_string());
        }
        Some("close_pty") => {
            panes.remove(pane_id);
        }
        _ => {}
    }
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

#[cfg(test)]
mod tests {
    use super::track_pane;
    use std::collections::HashSet;

    #[test]
    fn open_and_close_track_the_panes_a_client_owns() {
        let mut panes = HashSet::new();

        track_pane(
            &mut panes,
            r#"{"kind":"open_pty","pane_id":"p1","cwd":"/workspace","cols":80,"rows":24}"#,
        );
        track_pane(&mut panes, r#"{"kind":"open_pty","pane_id":"p2"}"#);
        assert_eq!(panes.len(), 2);

        track_pane(&mut panes, r#"{"kind":"close_pty","pane_id":"p1"}"#);
        assert_eq!(panes, HashSet::from(["p2".to_string()]));
    }

    #[test]
    fn unrelated_and_malformed_frames_do_not_touch_the_set() {
        let mut panes = HashSet::from(["p1".to_string()]);

        track_pane(&mut panes, "not json");
        track_pane(&mut panes, r#"{"kind":"ping"}"#);
        track_pane(&mut panes, r#"{"kind":"pty_input","pane_id":"p1"}"#);
        track_pane(&mut panes, r#"{"kind":"open_pty","pane_id":""}"#);
        track_pane(&mut panes, r#"{"kind":"open_pty","pane_id":7}"#);

        assert_eq!(panes, HashSet::from(["p1".to_string()]));
    }
}
