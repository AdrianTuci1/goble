use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use goble_daemon_protocol::DaemonEvent;
use goble_harness_types::{CommandDecision, HarnessId, HarnessTurn, SessionId};
use goble_replay::Checkpoint;
use goble_workflow::{WorkflowHostRequest, WorkflowRun};

use crate::port::DaemonPort;

use super::events::map_event;
use super::{DaemonState, ExecutionRecord, SessionState};

impl DaemonPort for DaemonState {
    fn run(&self, turn: HarnessTurn) -> anyhow::Result<()> {
        let harness_id = turn.harness_id.clone();
        let harness = self
            .registry
            .resolve(&harness_id)
            .ok_or_else(|| anyhow::anyhow!("no harness resolved for {harness_id}"))?;

        let session_id = turn.session_id.clone();
        let project_id = turn.project_id.clone();
        let medium_id = turn.medium_id.clone();
        let trace_id = format!("trace-{}", session_id.0);
        // If a per-session agent workspace root is configured, materialize this
        // session's workspace and hand it to the resolved harness before it runs.
        // A concrete harness that supports a per-run workspace (via the
        // `set_workspace_dir` seam) redirects its agent workspace here; a harness
        // that does not simply ignores it and keeps its own workspace.
        if let Some(root) = &self.workspace_root {
            let dir = root.join("harness").join(&trace_id);
            std::fs::create_dir_all(&dir)?;
            harness.set_workspace_dir(&dir);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let ledger = self.ledger(&session_id);
        let turn_index = ledger.record_turn(turn.clone());
        let state = Arc::new(SessionState {
            session_id: session_id.clone(),
            project_id: project_id.clone(),
            medium_id: medium_id.clone(),
            harness: Arc::clone(&harness),
            cancel: Arc::clone(&cancel),
            trace_id: trace_id.clone(),
            started_at: chrono::Utc::now().to_rfc3339(),
            status: Arc::new(Mutex::new("running".to_string())),
            finished_at: Arc::new(Mutex::new(None)),
            events: Arc::new(Mutex::new(Vec::new())),
            ledger,
            turn_index,
        });
        self.sessions
            .lock()
            .unwrap()
            .insert(session_id.clone(), Arc::clone(&state));

        self.sink.emit(DaemonEvent::TraceStarted {
            session_id: session_id.clone(),
            trace_id: trace_id.clone(),
            project_id: project_id.clone(),
            medium_id: medium_id.clone(),
        });

        let run = harness.run(turn, cancel);
        self.spawn_stream(session_id, trace_id, run.events, state);
        Ok(())
    }

    fn resume(
        &self,
        session_id: &SessionId,
        response: &str,
        credential: Option<(String, String)>,
    ) -> anyhow::Result<()> {
        let state = self
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no live session for {}", session_id.0))?;
        let run = state
            .harness
            .resume(session_id, response, credential)
            .ok_or_else(|| anyhow::anyhow!("harness does not support resume"))?;
        self.spawn_stream(
            session_id.clone(),
            state.trace_id.clone(),
            run.events,
            state,
        );
        Ok(())
    }

    fn resume_command(
        &self,
        session_id: &SessionId,
        decision: CommandDecision,
    ) -> anyhow::Result<()> {
        let state = self
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no live session for {}", session_id.0))?;
        let run = state
            .harness
            .resume_command(session_id, decision)
            .ok_or_else(|| anyhow::anyhow!("harness does not support command approval"))?;
        self.spawn_stream(
            session_id.clone(),
            state.trace_id.clone(),
            run.events,
            state,
        );
        Ok(())
    }

    fn cancel(&self, session_id: &SessionId) -> anyhow::Result<()> {
        let state = self
            .sessions
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no live session for {}", session_id.0))?;
        state.cancel.store(true, Ordering::Relaxed);
        state.harness.cancel();
        Ok(())
    }

    fn list_harnesses(&self) -> Vec<HarnessId> {
        let mut ids = self.registry.list();
        ids.sort_by(|a, b| a.0.cmp(&b.0));
        ids
    }

    fn snapshot(&self) -> Vec<ExecutionRecord> {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .map(|s| s.record())
            .collect()
    }

    fn run_workflow(&self, request: WorkflowHostRequest) -> anyhow::Result<WorkflowRun> {
        let host = self
            .workflow_host
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| {
                anyhow::anyhow!("no workflow host installed; install one before running workflows")
            })?;
        host.run(request)
    }

    fn rewind(&self, session_id: &SessionId, at: usize) -> anyhow::Result<usize> {
        if self.sessions.lock().unwrap().contains_key(session_id) {
            anyhow::bail!(
                "session {} is live; cancel or let it settle before rewinding",
                session_id.0
            );
        }
        let ledger = self.ledger(session_id);
        let len = ledger.len();
        let at = at.min(len);

        // When rewind actually drops the tail, restore the harness environment to
        // the snapshot captured when the last *kept* turn (index `at - 1`) settled,
        // so a subsequent turn diverges from that restored state rather than the
        // pre-rewind one. We only do this when the snapshot log has a snapshot for
        // that turn *and* the harness that produced it is reversible and answers
        // `restore`; otherwise rewinding stays transcript-only, which works for
        // every harness.
        if at > 0 && at < len {
            let snapshot = self
                .snapshot_log(session_id)
                .get(at - 1)
                .and_then(|s| s.as_ref())
                .cloned();
            let harness_id = ledger.turn(at - 1).map(|t| t.turn.harness_id);
            if let (Some(snapshot), Some(harness_id)) = (snapshot, harness_id) {
                if let Some(harness) = self.registry.resolve(&harness_id) {
                    if harness.capabilities().reversible {
                        if let Some(run) = harness.restore(session_id, &snapshot) {
                            self.spawn_restore_drain(run.events);
                        }
                    }
                }
            }
        }

        Ok(ledger.rewind_to(at))
    }

    fn fork(
        &self,
        session_id: &SessionId,
        at: usize,
        new_session_id: SessionId,
    ) -> anyhow::Result<SessionId> {
        let ledger = self.ledger(session_id);
        let branch = ledger.fork(at, new_session_id.clone());
        self.ledgers
            .lock()
            .unwrap()
            .insert(new_session_id.clone(), branch);
        Ok(new_session_id)
    }

    fn replay(&self, session_id: &SessionId, at: usize) -> anyhow::Result<Vec<DaemonEvent>> {
        let ledger = self.ledger(session_id);
        Ok(ledger
            .replay(at)
            .into_iter()
            .filter_map(map_event)
            .collect())
    }

    fn checkpoints(&self, session_id: &SessionId) -> anyhow::Result<Vec<Checkpoint>> {
        let ledger = self.ledger(session_id);
        let n = ledger.len();
        Ok((0..=n).map(|i| ledger.checkpoint_at(i)).collect())
    }

    fn select(&self, session_id: &SessionId, at: usize) -> anyhow::Result<()> {
        let ledger = self.ledger(session_id);
        Ok(ledger.select(at)?)
    }

    fn apply(&self, session_id: &SessionId, at: usize) -> anyhow::Result<usize> {
        let ledger = self.ledger(session_id);
        Ok(ledger.apply(at)?)
    }

    fn release(&self, session_id: &SessionId, at: usize) -> anyhow::Result<bool> {
        let ledger = self.ledger(session_id);
        Ok(ledger.release(at)?)
    }

    fn discard(&self, session_id: &SessionId, at: usize) -> anyhow::Result<usize> {
        let ledger = self.ledger(session_id);
        Ok(ledger.discard(at)?)
    }
}
