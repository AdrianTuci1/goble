//! A minimal fake worker for `scripts/gateway-smoke.sh`.
//!
//! It connects to the gateway's `/ws/worker` endpoint, answers every
//! `run_agent_for_thread_reply` with a short scripted turn, leaves a run whose
//! prompt contains "slow" running so the caller can observe `running`, 409 and
//! cancel, and logs `cancel_run` frames.
//!
//! ```console
//! cargo run -p goble-gateway --example fake_worker -- \
//!   "ws://127.0.0.1:8788/ws/worker?id=smoke" "$WORKER_TOKEN"
//! ```

use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let endpoint = args
        .next()
        .expect("usage: fake_worker <ws-url> <worker-token>");
    let token = args
        .next()
        .expect("usage: fake_worker <ws-url> <worker-token>");

    let mut request = endpoint.into_client_request()?;
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {token}"))?,
    );
    let (socket, _) = tokio_tungstenite::connect_async(request).await?;
    let (mut sink, mut stream) = socket.split();
    eprintln!("fake-worker: connected");

    while let Some(message) = stream.next().await {
        let Message::Text(text) = message? else {
            continue;
        };
        let Ok(frame) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let kind = frame["kind"].as_str().unwrap_or_default();

        match kind {
            "run_agent_for_thread_reply" => {
                let trace = frame["trace_id"].as_str().unwrap_or_default().to_string();
                let thread_id = frame["thread_id"].as_str().unwrap_or_default().to_string();
                let agent_id = frame["agent_id"].clone();
                let prompt = frame["prompt"].as_str().unwrap_or_default();
                if prompt.contains("slow") {
                    // A turn the smoke test can watch while it is still running.
                    eprintln!("fake-worker: holding run {trace}");
                    continue;
                }
                let frames = [
                    json!({"kind":"agent_started","trace_id":trace,"agent_id":agent_id}),
                    json!({"kind":"assistant_delta","trace_id":trace,"delta":"smoke "}),
                    json!({"kind":"assistant_delta","trace_id":trace,"delta":"ok"}),
                    json!({
                        "kind": "tool_call_started",
                        "trace_id": trace,
                        "id": "call-1",
                        "name": "bash",
                        "arguments": {"cmd": "echo smoke"},
                    }),
                    json!({
                        "kind": "tool_call_finished",
                        "trace_id": trace,
                        "id": "call-1",
                        "result": "smoke",
                    }),
                    json!({"kind":"agent_finished","trace_id":trace,"status":"Success"}),
                    json!({
                        "kind": "thread_agent_reply",
                        "trace_id": trace,
                        "thread_id": thread_id,
                        "content": "smoke ok",
                    }),
                ];
                for frame in frames {
                    sink.send(Message::Text(frame.to_string().into())).await?;
                }
            }
            "cancel_run" => {
                eprintln!(
                    "fake-worker: cancel_run {}",
                    frame["trace_id"].as_str().unwrap_or_default()
                );
            }
            _ => {}
        }
    }
    Ok(())
}
