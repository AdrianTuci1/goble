use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use goble_core::agent::AgentSpec;
use goble_core::crypto::hash_pairing_code;
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use goble_core::worker::WorkerId;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn test_multi_worker_round_robin_dispatch() {
    let worker_a_id = WorkerId::generate();
    let worker_b_id = WorkerId::generate();
    let state_a = goblin_worker::state::AppState::new(worker_a_id.clone());
    let state_b = goblin_worker::state::AppState::new(worker_b_id.clone());

    let addr_a = spawn_worker(state_a.clone()).await;
    let addr_b = spawn_worker(state_b.clone()).await;

    let url_a = format!("ws://{}/ws", addr_a);
    let url_b = format!("ws://{}/ws", addr_b);

    let (mut ws_a, _) = tokio_tungstenite::connect_async(&url_a).await.unwrap();
    let (mut ws_b, _) = tokio_tungstenite::connect_async(&url_b).await.unwrap();

    let code = "12345678";
    state_a.set_pairing_hash(hash_pairing_code(code, &[0u8; 16]).unwrap());
    state_b.set_pairing_hash(hash_pairing_code(code, &[0u8; 16]).unwrap());

    let pair = DesktopMessage::PairRequest {
        worker_id: worker_a_id.clone(),
        pairing_code_hash: hash_pairing_code(code, &[0u8; 16]).unwrap(),
    };
    ws_a.send(Message::Text(serde_json::to_string(&pair).unwrap().into()))
        .await
        .unwrap();

    let pair_b = DesktopMessage::PairRequest {
        worker_id: worker_b_id.clone(),
        pairing_code_hash: hash_pairing_code(code, &[0u8; 16]).unwrap(),
    };
    ws_b.send(Message::Text(
        serde_json::to_string(&pair_b).unwrap().into(),
    ))
    .await
    .unwrap();

    let spec = AgentSpec::new("test", "do nothing");
    let trace_a = uuid::Uuid::new_v4().to_string();
    let run_a = DesktopMessage::RunAgent {
        trace_id: trace_a.clone(),
        agent_id: spec.id.clone(),
        spec: spec.clone(),
    };
    ws_a.send(Message::Text(serde_json::to_string(&run_a).unwrap().into()))
        .await
        .unwrap();

    let trace_b = uuid::Uuid::new_v4().to_string();
    let run_b = DesktopMessage::RunAgent {
        trace_id: trace_b.clone(),
        agent_id: spec.id.clone(),
        spec,
    };
    ws_b.send(Message::Text(serde_json::to_string(&run_b).unwrap().into()))
        .await
        .unwrap();

    let mut finished_a = false;
    let mut finished_b = false;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);

    while tokio::time::Instant::now() < deadline && !(finished_a && finished_b) {
        tokio::select! {
            msg = ws_a.next() => {
                if let Some(Ok(Message::Text(text))) = msg {
                    if let Ok(WorkerMessage::AgentFinished { trace_id, .. }) = serde_json::from_str::<WorkerMessage>(&text) {
                        if trace_id == trace_a { finished_a = true; }
                    }
                }
            }
            msg = ws_b.next() => {
                if let Some(Ok(Message::Text(text))) = msg {
                    if let Ok(WorkerMessage::AgentFinished { trace_id, .. }) = serde_json::from_str::<WorkerMessage>(&text) {
                        if trace_id == trace_b { finished_b = true; }
                    }
                }
            }
        }
    }

    assert!(finished_a, "worker a should finish");
    assert!(finished_b, "worker b should finish");
}

async fn spawn_worker(state: Arc<goblin_worker::state::AppState>) -> std::net::SocketAddr {
    let app = axum::Router::new()
        .route(
            "/ws",
            axum::routing::get(goblin_worker::websocket::ws_handler),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    addr
}
