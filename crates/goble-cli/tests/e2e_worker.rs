use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use goble_core::agent::AgentSpec;
use goble_core::crypto::hash_pairing_code;
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use goble_core::worker::WorkerId;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn test_worker_run_agent_flow() {
    let worker_id = WorkerId::generate();
    let state = goblin_worker::state::AppState::new(worker_id.clone());
    let addr = spawn_worker(state.clone()).await;

    let url = format!("ws://{}/ws", addr);
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();

    let code = "12345678";
    state.set_pairing_hash(hash_pairing_code(code, &[0u8; 16]).unwrap());

    let pair = DesktopMessage::PairRequest {
        worker_id: worker_id.clone(),
        pairing_code_hash: hash_pairing_code(code, &[0u8; 16]).unwrap(),
    };
    ws.send(Message::Text(serde_json::to_string(&pair).unwrap().into()))
        .await
        .unwrap();

    let spec = AgentSpec::new("test", "do nothing");
    let trace_id = uuid::Uuid::new_v4().to_string();
    let agent_id = spec.id.clone();
    let run = DesktopMessage::RunAgent {
        trace_id: trace_id.clone(),
        agent_id,
        spec,
    };
    ws.send(Message::Text(serde_json::to_string(&run).unwrap().into()))
        .await
        .unwrap();

    let mut saw_started = false;
    let mut saw_finished = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        let timeout = tokio::time::timeout_at(deadline, ws.next()).await;
        if let Ok(Some(Ok(Message::Text(text)))) = timeout {
            if let Ok(msg) = serde_json::from_str::<WorkerMessage>(&text) {
                match &msg {
                    WorkerMessage::AgentStarted { trace_id: t, .. } if t == &trace_id => {
                        saw_started = true;
                    }
                    WorkerMessage::AgentFinished { trace_id: t, .. } if t == &trace_id => {
                        saw_finished = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    assert!(saw_started, "expected AgentStarted");
    assert!(saw_finished, "expected AgentFinished");
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
