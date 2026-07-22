use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use goble_core::worker::WorkerId;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

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
        let (ws_stream, _) = tokio_tungstenite::connect_async(&url).await?;
        let (mut write, mut read) = ws_stream.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<DesktopMessage>();

        let hash = goble_core::crypto::hash_pairing_code(&pairing_code, &[0u8; 16])?;

        let pair_msg = DesktopMessage::PairRequest {
            worker_id: worker_id.clone(),
            pairing_code_hash: hash,
        };
        let json = serde_json::to_string(&pair_msg)?;
        write.send(Message::Text(json.into())).await?;

        let worker_id_clone = worker_id.clone();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let json = match serde_json::to_string(&msg) {
                    Ok(j) => j,
                    Err(_) => continue,
                };
                if write.send(Message::Text(json.into())).await.is_err() {
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
        self.tx.send(msg)?;
        Ok(())
    }
}
