//! SSH proxy mode for the goblin worker.
//!
//! Instead of listening on a TCP port and accepting WebSocket connections, this
//! mode reads [`DesktopMessage`] JSON lines from stdin and writes
//! [`WorkerMessage`] JSON lines to stdout. A desktop client connects by
//! spawning the worker binary on a remote host over an SSH session, so the
//! remote machine only needs SSH (port 22) exposed, matching warp-new's model.

use std::sync::Arc;
use std::time::Duration;

use goble_core::execution::ExecutionStatus;
use goble_core::protocol::{DesktopMessage, WorkerMessage};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::runner::Runner;
use crate::state::AppState;
use crate::websocket;

/// Run the worker in SSH-proxy mode until stdin closes.
///
/// A single task reads from stdin and subscribes to the worker's event
/// broadcast, writing every outgoing [`WorkerMessage`] as one newline-terminated
/// JSON object to stdout. This keeps stdout serialized without any lock.
pub async fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    let runner = Runner::new(state.clone());
    let mut event_rx = state.event_tx.subscribe();
    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();
    let mut stdout = tokio::io::stdout();
    let mut stdin_closed = false;

    loop {
        if stdin_closed {
            // Drain any worker events that were emitted while processing the
            // last request, then exit after a short grace period.
            match tokio::time::timeout(Duration::from_millis(100), event_rx.recv()).await {
                Ok(Ok(msg)) => {
                    if !write_message(&mut stdout, &msg).await {
                        break;
                    }
                    continue;
                }
                _ => break,
            }
        }

        tokio::select! {
            Ok(msg) = event_rx.recv() => {
                if !write_message(&mut stdout, &msg).await {
                    break;
                }
            }
            line = lines.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<DesktopMessage>(&line) {
                            Ok(msg) => {
                                if let Err(e) = websocket::handle_desktop_message(&state, &runner, msg).await {
                                    state.emit(WorkerMessage::AgentFinished {
                                        trace_id: "unknown".to_string(),
                                        status: ExecutionStatus::Failure(e.to_string()),
                                    });
                                }
                            }
                            Err(_) => {
                                state.emit(WorkerMessage::AgentFinished {
                                    trace_id: "unknown".to_string(),
                                    status: ExecutionStatus::Failure(
                                        "invalid message".to_string(),
                                    ),
                                });
                            }
                        }
                    }
                    Ok(None) => {
                        stdin_closed = true;
                    }
                    Err(_) => break,
                }
            }
        }
    }

    Ok(())
}

async fn write_message(stdout: &mut tokio::io::Stdout, msg: &WorkerMessage) -> bool {
    let json = match serde_json::to_string(msg) {
        Ok(j) => j,
        Err(_) => return true,
    };
    if stdout.write_all(json.as_bytes()).await.is_err() {
        return false;
    }
    if stdout.write_all(b"\n").await.is_err() {
        return false;
    }
    if stdout.flush().await.is_err() {
        return false;
    }
    true
}
