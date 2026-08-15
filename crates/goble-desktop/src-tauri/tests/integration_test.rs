use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use goble_core::store::Store;
use goble_core::worker::WorkerId;
use goble_desktop_tauri_lib::state::DesktopState;
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn test_state_worker_pair_and_message_flow() {
    let state = DesktopState::new(
        Store::open_in_memory().unwrap(),
        goble_desktop_tauri_lib::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
    );
    let worker_id = WorkerId::generate();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("ws://127.0.0.1:{}/ws", port);

    state
        .add_worker(worker_id.clone(), "mock".to_string(), url.clone())
        .unwrap();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        while let Some(Ok(msg)) = ws.next().await {
            if let Message::Text(text) = msg {
                if let Ok(desktop_msg) = serde_json::from_str::<DesktopMessage>(&text) {
                    match desktop_msg {
                        DesktopMessage::PairRequest { .. } => {
                            let resp = WorkerMessage::Paired;
                            let _ = ws
                                .send(Message::Text(serde_json::to_string(&resp).unwrap().into()))
                                .await;
                        }
                        DesktopMessage::Ping => {
                            let resp = WorkerMessage::Pong;
                            let _ = ws
                                .send(Message::Text(serde_json::to_string(&resp).unwrap().into()))
                                .await;
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    Arc::clone(&state)
        .pair_worker(&worker_id, "0000".to_string())
        .unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(state.list_workers()[0].paired);

    state
        .send_to_worker(&worker_id, DesktopMessage::Ping)
        .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let logs: Vec<String> = state.get_logs().iter().map(|l| l.message.clone()).collect();
    assert!(logs.iter().any(|m| m.contains("pong")));
}

#[test]
fn test_desktop_state_persistence_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(tmp.path().join("store.db")).unwrap();
    let state = DesktopState::new(
        store,
        goble_desktop_tauri_lib::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
    );

    let worker_id = WorkerId::generate();
    state
        .add_worker(
            worker_id.clone(),
            "persisted".to_string(),
            "ws://x/ws".to_string(),
        )
        .unwrap();
    let chat_id = state.create_chat("Persistent", None, None).unwrap();
    state.add_chat_message(&chat_id, "user", "hello").unwrap();

    let state2 = DesktopState::new(
        Store::open(tmp.path().join("store.db")).unwrap(),
        goble_desktop_tauri_lib::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap(),
    );
    state2.load_from_store().unwrap();

    assert_eq!(state2.list_workers().len(), 1);
    assert_eq!(state2.list_workers()[0].name, "persisted");
    assert_eq!(state2.list_chat_messages(&chat_id).unwrap().len(), 1);
}
