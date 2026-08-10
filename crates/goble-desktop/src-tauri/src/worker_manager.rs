use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use goble_core::worker::WorkerId;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

use crate::state::DesktopState;

pub struct WorkerClient {
    pub worker_id: WorkerId,
    pub url: String,
    tx: mpsc::UnboundedSender<DesktopMessage>,
}

impl WorkerClient {
    pub async fn connect(
        state: Arc<DesktopState>,
        worker_id: WorkerId,
        url: String,
        pairing_code: String,
    ) -> anyhow::Result<Self> {
        let (ws_stream, _): (WebSocketStream<MaybeTlsStream<TcpStream>>, _) =
            connect_async(&url).await?;
        let (mut write, mut read) = ws_stream.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<DesktopMessage>();

        let hash = goble_core::crypto::hash_pairing_code(&pairing_code, &[0u8; 16])?;

        let pair_msg = DesktopMessage::PairRequest {
            worker_id: worker_id.clone(),
            pairing_code_hash: hash,
        };
        let json = serde_json::to_string(&pair_msg)?;
        write
            .send(Message::Text(json.into()))
            .await
            .map_err(|e| anyhow::anyhow!("send pair: {}", e))?;

        let worker_id_clone = worker_id.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let json = match serde_json::to_string(&msg) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                if write
                    .send(Message::Text(json.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            let _ = write.close().await;
        });

        let state_clone = state.clone();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = read.next().await {
                if let Message::Text(text) = msg {
                    if let Ok(worker_msg) = serde_json::from_str::<WorkerMessage>(&text) {
                        state_clone.handle_worker_message(&worker_id_clone, worker_msg);
                    }
                }
            }
            state_clone.remove_worker(&worker_id_clone);
        });

        Ok(Self { worker_id, url, tx })
    }

    pub fn send(&self, msg: DesktopMessage) -> anyhow::Result<()> {
        self.tx
            .send(msg)
            .map_err(|e| anyhow::anyhow!("channel closed: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::store::Store;
    use std::time::Duration;

    #[tokio::test]
    async fn test_worker_client_connect_mock() {
        use tokio_tungstenite::accept_async;

        let state = DesktopState::new(Store::open_in_memory().unwrap(), crate::thread_store::ThreadStore::new(std::path::PathBuf::new()).unwrap());
        let worker_id = WorkerId::generate();
        state
            .add_worker(worker_id.clone(), "mock".to_string(), "ws://127.0.0.1:0".to_string())
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
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
                            DesktopMessage::PairRequest {
                                worker_id,
                                pairing_code_hash: _,
                            } => {
                                let resp = WorkerMessage::Paired;
                                let _ = ws
                                    .send(Message::Text(
                                        serde_json::to_string(&resp).unwrap().into(),
                                    ))
                                    .await;
                                assert_eq!(worker_id, WorkerId(worker_id.0.clone()));
                            }
                            DesktopMessage::Ping => {
                                let resp = WorkerMessage::Pong;
                                let _ = ws
                                    .send(Message::Text(
                                        serde_json::to_string(&resp).unwrap().into(),
                                    ))
                                    .await;
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        let client = WorkerClient::connect(
            state.clone(),
            worker_id.clone(),
            url,
            "0000".to_string(),
        )
        .await;
        assert!(client.is_ok(), "{:?}", client.err());
        let client = client.unwrap();
        client.send(DesktopMessage::Ping).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(state.list_workers()[0].paired);
    }
}
