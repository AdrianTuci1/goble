use std::pin::Pin;
use std::sync::Arc;

use futures::{Stream, StreamExt};
use goble_daemon_protocol::DaemonEvent;
use goble_harness_protocol::HarnessServerEvent;
use goble_harness_types::SessionId;
use goble_replay::TurnStatus;

use super::events::map_event;
use super::{DaemonState, SessionState};

impl DaemonState {
    pub(super) fn spawn_stream(
        &self,
        session_id: SessionId,
        trace_id: String,
        stream: Pin<Box<dyn Stream<Item = HarnessServerEvent> + Send>>,
        state: Arc<SessionState>,
    ) {
        let sink = Arc::clone(&self.sink);
        let sessions = Arc::clone(&self.sessions);
        let checkpoint_sink = Arc::clone(&self.checkpoint_sink);
        let snapshots = Arc::clone(&self.snapshots);
        let session_id_for_remove = session_id.clone();
        tokio::spawn(async move {
            let mut stream = stream;
            let mut final_status = "running".to_string();
            let mut final_error: Option<String> = None;
            while let Some(ev) = stream.next().await {
                // Record the raw harness event into the transcript ledger first,
                // so the (mapped) wire mirror and the reversible history agree.
                let _ = state.ledger.append_events_to(state.turn_index, [ev.clone()]);
                if let Some(daemon_event) = map_event(ev) {
                    match &daemon_event {
                        DaemonEvent::Done { .. } => final_status = "success".to_string(),
                        DaemonEvent::Error { message, .. } => {
                            final_status = "failed".to_string();
                            final_error = Some(message.clone());
                        }
                        _ => {}
                    }
                    state.events.lock().unwrap().push(daemon_event.clone());
                    sink.emit(daemon_event);
                }
            }
            // A terminal event (`Done` / `Error`) settles the turn: settle the
            // ledger's turn and stream `TraceFinished`. Otherwise the turn is
            // suspended (it paused on an `AskUser` and ended without a terminal
            // event), so the session stays live and the same harness handles a
            // later `Resume`. Cancellation surfaces as `Error("cancelled")`, so a
            // cancelled turn still settles here.
            if final_status == "running" {
                return;
            }
            let turn_status = match final_status.as_str() {
                "success" => TurnStatus::Success,
                _ => TurnStatus::Failed(final_error.unwrap_or_else(|| "failed".to_string())),
            };
            let _ = state.ledger.settle_turn_at(state.turn_index, turn_status);
            // Capture the harness's environment snapshot for this settled turn,
            // if the harness is reversible and produced one; `None` records a
            // turn whose harness is not environment-reversible.
            let snapshot = if state.harness.capabilities().reversible {
                state.harness.snapshot(&session_id)
            } else {
                None
            };
            let mut snapshots = snapshots.lock().unwrap();
            let log = snapshots.entry(session_id.clone()).or_default();
            if log.len() <= state.turn_index {
                log.resize(state.turn_index + 1, None);
            }
            log[state.turn_index] = snapshot;
            drop(snapshots);
            // Persist the durable checkpoint, if a sink is attached.
            if let Some(sink) = checkpoint_sink.lock().unwrap().as_ref() {
                sink.persist(&state.ledger.checkpoint());
            }
            *state.status.lock().unwrap() = final_status.clone();
            *state.finished_at.lock().unwrap() = Some(chrono::Utc::now().to_rfc3339());
            sink.emit(DaemonEvent::TraceFinished {
                session_id: session_id.clone(),
                trace_id,
                status: final_status,
                project_id: state.project_id.clone(),
                medium_id: state.medium_id.clone(),
            });
            sessions.lock().unwrap().remove(&session_id_for_remove);
        });
    }

    /// Drive a harness's restore event stream so its environment is rebuilt.
    ///
    /// Unlike [`Self::spawn_stream`], this does not settle a transcript turn or
    /// remove the session — rewind only runs against a settled session, and the
    /// transcript's history must be preserved for replay. It simply consumes the
    /// restore stream so the harness reaches the restored state.
    pub(super) fn spawn_restore_drain(
        &self,
        stream: Pin<Box<dyn Stream<Item = HarnessServerEvent> + Send>>,
    ) {
        tokio::spawn(async move {
            let mut stream = stream;
            while stream.next().await.is_some() {}
        });
    }
}
