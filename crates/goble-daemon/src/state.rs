//! The daemon state: harness registry + per-session execution ledger + sink.
//!
//! [`DaemonState`] is the framework-agnostic core both composition roots (the
//! embedded daemon inside the GUI binary and the headless `goblin-worker`)
//! share. It owns the [`HarnessRegistry`] (seeded by the composition root with
//! the internal harness and any external ones), the per-session execution
//! ledger that backs [`DaemonPort::snapshot`], and the [`DaemonEventSink`] that
//! streams wire events back to the client. It depends only on the harness seam
//! crates and `goble-daemon-protocol` — no `goble-core`, no UI, no transport.

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use futures::{Stream, StreamExt};
use goble_daemon_protocol::DaemonEvent;
use goble_harness_protocol::HarnessServerEvent;
use goble_harness_runtime::{HarnessRegistry, HarnessRuntime};
use goble_harness_types::{
    HarnessId, HarnessSnapshot, HarnessTurn, MediumId, ProjectId, SessionId,
};
use goble_replay::{Checkpoint, CheckpointSink, ReplayLedger, TurnStatus};
use goble_workflow::{StepOutcome, Workflow, WorkflowHostRequest, WorkflowRun};
use serde::{Deserialize, Serialize};

use crate::port::DaemonPort;
use crate::sink::DaemonEventSink;
use crate::workflow::{HarnessStepExecutor, WorkflowHost};

/// A serializable snapshot of one session's execution, for [`DaemonPort::snapshot`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub session_id: SessionId,
    pub trace_id: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub events: Vec<DaemonEvent>,
    pub project_id: ProjectId,
    pub medium_id: MediumId,
}

/// The per-session state the daemon keeps while a turn runs.
struct SessionState {
    session_id: SessionId,
    project_id: ProjectId,
    medium_id: MediumId,
    harness: Arc<dyn HarnessRuntime>,
    cancel: Arc<AtomicBool>,
    trace_id: String,
    started_at: String,
    status: Arc<Mutex<String>>,
    finished_at: Arc<Mutex<Option<String>>>,
    events: Arc<Mutex<Vec<DaemonEvent>>>,
    /// The transcript ledger this session's turn is being recorded into. Cloned
    /// from the daemon's persistent [`DaemonState`] ledger, so a live run and a
    /// later rewind/fork/replay share the same history.
    ledger: ReplayLedger,
    /// Index, within the ledger, of the turn this run is recording. Index-aware
    /// appends keep each turn's events on its own record even when a session is
    /// re-run before the previous drain has consumed its stream.
    turn_index: usize,
}

impl SessionState {
    fn record(&self) -> ExecutionRecord {
        ExecutionRecord {
            session_id: self.session_id.clone(),
            trace_id: self.trace_id.clone(),
            status: self.status.lock().unwrap().clone(),
            started_at: self.started_at.clone(),
            finished_at: self.finished_at.lock().unwrap().clone(),
            events: self.events.lock().unwrap().clone(),
            project_id: self.project_id.clone(),
            medium_id: self.medium_id.clone(),
        }
    }
}

/// The daemon core. `Arc<DaemonState>` is shared across tasks; clone it to
/// call [`DaemonPort`] methods or to seed the registry.
#[derive(Clone)]
pub struct DaemonState {
    registry: HarnessRegistry,
    sessions: Arc<Mutex<HashMap<SessionId, Arc<SessionState>>>>,
    /// The persistent per-session transcript ledgers. Unlike `sessions`, a
    /// ledger is retained after its turn settles, so a past session can still be
    /// rewound, forked, replayed, and its checkpoints listed.
    ledgers: Arc<Mutex<HashMap<SessionId, ReplayLedger>>>,
    /// Optional durable hook: when a turn settles, the session checkpoint is
    /// pushed here so the reversible history survives a restart. `None` keeps
    /// the daemon in-memory only.
    checkpoint_sink: Arc<Mutex<Option<Arc<dyn CheckpointSink>>>>,
    /// Per-session environment-snapshot log. One entry per settled turn, indexed
    /// the same as the session's transcript ledger's turns. `Some` when the
    /// harness is reversible and produced a snapshot for that turn; `None`
    /// otherwise.
    snapshots: Arc<Mutex<HashMap<SessionId, Vec<Option<HarnessSnapshot>>>>>,
    sink: Arc<dyn DaemonEventSink>,
    /// Optional per-session agent workspace root. When set, each session's
    /// harness run gets a workspace at `<root>/harness/<trace_id>`, created
    /// before the run streams events (see [`DaemonPort::run`]). `None` (the
    /// default) leaves each harness's own workspace untouched.
    workspace_root: Option<PathBuf>,
    /// The workflow host (journal + run ledger), installed by the composition
    /// root once it wires a step executor. `None` keeps `run_workflow` an error
    /// until a host is installed.
    workflow_host: Arc<Mutex<Option<Arc<WorkflowHost>>>>,
}

impl DaemonState {
    /// Build a daemon around a (possibly pre-seeded) registry and an event sink.
    pub fn new(registry: HarnessRegistry, sink: Arc<dyn DaemonEventSink>) -> Arc<Self> {
        Arc::new(Self {
            registry,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            ledgers: Arc::new(Mutex::new(HashMap::new())),
            checkpoint_sink: Arc::new(Mutex::new(None)),
            snapshots: Arc::new(Mutex::new(HashMap::new())),
            sink,
            workspace_root: None,
            workflow_host: Arc::new(Mutex::new(None)),
        })
    }

    /// Set the optional per-session agent workspace root.
    ///
    /// When set, each session's harness run gets a workspace at
    /// `<root>/harness/<trace_id>`, created before the run streams events. The
    /// directory is handed to the resolved harness through the object-safe
    /// [`HarnessRuntime::set_workspace_dir`] seam; a concrete harness that
    /// supports a per-run workspace redirects its agent workspace there, while a
    /// harness that does not leaves its own workspace untouched. The default is
    /// `None` (no per-session workspace).
    pub fn with_workspace_root(mut self: Arc<Self>, workspace_root: impl Into<PathBuf>) -> Arc<Self> {
        Arc::make_mut(&mut self).workspace_root = Some(workspace_root.into());
        self
    }

    /// Attach (or clear) a durable checkpoint sink, called with a session's
    /// checkpoint each time a turn settles. `None` keeps the daemon in-memory.
    pub fn set_checkpoint_sink(&self, sink: Option<Arc<dyn CheckpointSink>>) {
        *self.checkpoint_sink.lock().unwrap() = sink;
    }

    /// Seed a session's transcript ledger from a persisted [`Checkpoint`], so a
    /// ledger rebuilt from storage on startup can be rewound/forked/replayed.
    pub fn restore_checkpoint(&self, cp: &Checkpoint) {
        self.ledgers
            .lock()
            .unwrap()
            .insert(cp.session_id.clone(), ReplayLedger::from_checkpoint(cp));
    }

    /// The recorded environment snapshots for a session, one per settled turn
    /// and indexed the same as the transcript ledger's turns. `None` marks a
    /// turn whose harness was not reversible (or produced no snapshot).
    pub fn snapshot_log(&self, session_id: &SessionId) -> Vec<Option<HarnessSnapshot>> {
        self.snapshots
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    /// The transcript ledger for a session, creating (and remembering) it if the
    /// session has not run yet. The returned ledger shares state with the one
    /// stored on `self`, so recording a live turn and reading the history stay
    /// consistent.
    fn ledger(&self, session_id: &SessionId) -> ReplayLedger {
        let mut ledgers = self.ledgers.lock().unwrap();
        ledgers
            .entry(session_id.clone())
            .or_insert_with(|| ReplayLedger::new(session_id.clone()))
            .clone()
    }

    /// The harness registry. Use to seed the internal/external harnesses.
    pub fn registry(&self) -> &HarnessRegistry {
        &self.registry
    }

    /// Register a harness directly on the backing registry.
    pub fn register(&self, harness: Arc<dyn HarnessRuntime>) {
        self.registry.register(harness);
    }

    /// The event sink events are streamed to.
    pub fn sink(&self) -> Arc<dyn DaemonEventSink> {
        Arc::clone(&self.sink)
    }

    // --- workflow wiring ---

    /// Install a pre-built workflow host (journal + run ledger). Callers use
    /// this when they want to inject a scripted [`MockStep`]-style executor.
    pub fn install_workflow_host(&self, host: Arc<WorkflowHost>) {
        *self.workflow_host.lock().unwrap() = Some(host);
    }

    /// Build and install a workflow host whose executor runs each step through
    /// this daemon's harness run seam, scoped to `harness_id`/`project_id`/
    /// `medium_id`. Session ids for the steps derive from `session_prefix`.
    ///
    /// The run seam captures a weak reference to the daemon, so the host lives
    /// alongside the daemon without forming a strong reference cycle.
    pub fn install_workflow_executor(
        self: &Arc<Self>,
        harness_id: HarnessId,
        project_id: ProjectId,
        medium_id: MediumId,
        session_prefix: impl Into<String>,
    ) {
        let weak = Arc::downgrade(self);
        let run_seam = std::sync::Arc::new(move |turn: HarnessTurn| match weak.upgrade() {
            Some(daemon) => daemon.run_turn_blocking(turn),
            None => StepOutcome::Failed("daemon went away while running step".to_string()),
        });
        let executor =
            HarnessStepExecutor::new(harness_id, project_id, medium_id, session_prefix, run_seam);
        self.install_workflow_host(Arc::new(WorkflowHost::new(executor)));
    }

    /// Register a workflow on the installed host so `run_workflow` can run it.
    pub fn register_workflow(&self, workflow: Workflow) -> anyhow::Result<()> {
        self.workflow_host
            .lock()
            .unwrap()
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no workflow host installed; install one first"))
            .and_then(|host| host.register_workflow(workflow))
    }

    /// Synchronously run a harness turn to a settled [`StepOutcome`].
    ///
    /// This is the daemon run seam the workflow step executor drives: it
    /// resolves the harness, runs the turn, and drains the resulting event
    /// stream, reporting `Succeeded` on `Done` and `Failed` on `Error`. The
    /// stream is drained with the standalone futures executor so it does not
    /// need an ambient tokio runtime.
    pub fn run_turn_blocking(&self, turn: HarnessTurn) -> StepOutcome {
        let harness_id = turn.harness_id.clone();
        let harness = match self.registry.resolve(&harness_id) {
            Some(harness) => harness,
            None => return StepOutcome::Failed(format!("no harness resolved for {harness_id}")),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let run = harness.run(turn, cancel);
        let mut stream = run.events;
        let mut outcome = StepOutcome::Succeeded;
        while let Some(event) = futures::executor::block_on(stream.next()) {
            match event {
                HarnessServerEvent::Done { .. } => outcome = StepOutcome::Succeeded,
                HarnessServerEvent::Error { message, .. } => {
                    outcome = StepOutcome::Failed(message)
                }
                _ => {}
            }
        }
        outcome
    }

    fn spawn_stream(
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
    fn spawn_restore_drain(
        &self,
        stream: Pin<Box<dyn Stream<Item = HarnessServerEvent> + Send>>,
    ) {
        tokio::spawn(async move {
            let mut stream = stream;
            while stream.next().await.is_some() {}
        });
    }
}

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

/// Map a harness [`HarnessServerEvent`] into the daemon [`DaemonEvent`] shape.
///
/// The host-discovery frames (`Ready`, `ToolList`) are not part of the daemon
/// stream and are dropped.
fn map_event(ev: HarnessServerEvent) -> Option<DaemonEvent> {
    match ev {
        HarnessServerEvent::Ready
        | HarnessServerEvent::ToolList { .. }
        | HarnessServerEvent::Checkpoint { .. } => None,
        HarnessServerEvent::AssistantDelta { session_id, delta } => {
            Some(DaemonEvent::AssistantDelta { session_id, delta })
        }
        HarnessServerEvent::ToolCallStarted {
            session_id,
            id,
            name,
            arguments,
        } => Some(DaemonEvent::ToolCallStarted {
            session_id,
            id,
            name,
            arguments,
        }),
        HarnessServerEvent::ToolCallFinished { session_id, id, result } => {
            Some(DaemonEvent::ToolCallFinished {
                session_id,
                id,
                result,
            })
        }
        HarnessServerEvent::ToolCallError {
            session_id,
            id,
            message,
        } => Some(DaemonEvent::ToolCallError {
            session_id,
            id,
            message,
        }),
        HarnessServerEvent::AskUser {
            session_id,
            question,
            quick_replies,
        } => Some(DaemonEvent::AskUser {
            session_id,
            question,
            quick_replies,
        }),
        HarnessServerEvent::MissionUpdated {
            session_id,
            mission_id,
            status,
        } => Some(DaemonEvent::MissionUpdated {
            session_id,
            mission_id,
            status,
        }),
        HarnessServerEvent::Done { session_id } => Some(DaemonEvent::Done { session_id }),
        HarnessServerEvent::Error { session_id, message } => {
            Some(DaemonEvent::Error { session_id, message })
        }
        HarnessServerEvent::ScreenHandoff { session_id, config } => {
            Some(DaemonEvent::ScreenHandoff { session_id, config })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_harness_runtime::{HarnessRun, MockHarness};
    use goble_harness_types::{HarnessCapabilities, HarnessTurn};

    /// A harness whose run never yields, so the daemon session stays live.
    struct BlockingHarness {
        id: HarnessId,
    }

    impl BlockingHarness {
        fn new(id: HarnessId) -> Self {
            Self { id }
        }
    }

    impl HarnessRuntime for BlockingHarness {
        fn id(&self) -> HarnessId {
            self.id.clone()
        }
        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::internal()
        }
        fn run(&self, _turn: HarnessTurn, _cancel: Arc<AtomicBool>) -> HarnessRun {
            HarnessRun {
                events: Box::pin(futures::stream::pending()),
            }
        }
    }

    /// A harness whose run pauses on an `AskUser` then ends (a suspension), and
    /// whose `resume` streams the answer then `Done`.
    struct AskHarness {
        id: HarnessId,
    }

    impl AskHarness {
        fn new(id: HarnessId) -> Self {
            Self { id }
        }
    }

    impl HarnessRuntime for AskHarness {
        fn id(&self) -> HarnessId {
            self.id.clone()
        }
        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::internal()
        }
        fn run(&self, turn: HarnessTurn, _cancel: Arc<AtomicBool>) -> HarnessRun {
            let session_id = turn.session_id;
            HarnessRun {
                events: Box::pin(futures::stream::iter(vec![
                    HarnessServerEvent::AskUser {
                        session_id: session_id.clone(),
                        question: "continue?".to_string(),
                        quick_replies: vec!["yes".to_string()],
                    },
                ])),
            }
        }
        fn resume(
            &self,
            session_id: &SessionId,
            response: &str,
            _credential: Option<(String, String)>,
        ) -> Option<HarnessRun> {
            Some(HarnessRun {
                events: Box::pin(futures::stream::iter(vec![
                    HarnessServerEvent::AssistantDelta {
                        session_id: session_id.clone(),
                        delta: response.to_string(),
                    },
                    HarnessServerEvent::Done {
                        session_id: session_id.clone(),
                    },
                ])),
            })
        }
    }

    #[derive(Default)]
    struct CollectingSink {
        events: Mutex<Vec<DaemonEvent>>,
    }

    impl DaemonEventSink for CollectingSink {
        fn emit(&self, event: DaemonEvent) {
            self.events.lock().unwrap().push(event);
        }
    }

    fn turn(session: &str) -> HarnessTurn {
        HarnessTurn::new(HarnessId::new("mock"), SessionId::new(session), "say hi")
    }

    #[tokio::test]
    async fn run_turn_streams_to_sink() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let sink = Arc::new(CollectingSink::default());
        let daemon = DaemonState::new(registry, sink.clone());

        daemon.run(turn("s1")).unwrap();
        // The mock harness finishes immediately; give the spawned drain time to run.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let events = sink.events.lock().unwrap();
        assert!(events.iter().any(
            |e| matches!(e, DaemonEvent::AssistantDelta { delta, .. } if delta == "hi")
        ));
        assert!(events.iter().any(|e| matches!(e, DaemonEvent::TraceStarted { .. })));
        assert!(events.iter().any(|e| matches!(e, DaemonEvent::TraceFinished { .. })));
    }

    #[tokio::test]
    async fn snapshot_lists_live_session() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(BlockingHarness::new(HarnessId::new("mock"))));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        daemon.run(turn("s1")).unwrap();
        let snap = daemon.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].session_id, SessionId::new("s1"));
        assert_eq!(snap[0].status, "running");
    }

    #[tokio::test]
    async fn cancel_reports_live_session() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(BlockingHarness::new(HarnessId::new("mock"))));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        daemon.run(turn("s1")).unwrap();
        assert!(daemon.cancel(&SessionId::new("s1")).is_ok());
        assert!(daemon.cancel(&SessionId::new("nope")).is_err());
    }

    /// The execution record and the `TraceFinished` lifecycle event carry the
    /// turn's project/medium identity, so a consumer can correlate a recorded
    /// run to the project and medium it ran on.
    #[tokio::test]
    async fn execution_record_and_trace_finished_carry_identity() {
        // A settled turn: the sink's `TraceFinished` carries the turn identity.
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let sink = Arc::new(CollectingSink::default());
        let daemon = DaemonState::new(registry, sink.clone());

        let mut turn = HarnessTurn::new(HarnessId::new("mock"), SessionId::new("s1"), "scenario");
        turn.project_id = ProjectId::new("p1");
        turn.medium_id = MediumId::new("remote");
        daemon.run(turn).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let events = sink.events.lock().unwrap();
        let finished = events
            .iter()
            .find_map(|e| match e {
                DaemonEvent::TraceFinished {
                    project_id,
                    medium_id,
                    ..
                } => Some((project_id.clone(), medium_id.clone())),
                _ => None,
            })
            .expect("a settled turn streams TraceFinished");
        assert_eq!(finished, (ProjectId::new("p1"), MediumId::new("remote")));
        drop(events);

        // A live (suspended) session: the snapshot's `ExecutionRecord` carries
        // the same identity, so a consumer reading the record can tell project
        // and medium apart even while the turn is still running.
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(BlockingHarness::new(HarnessId::new("mock"))));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));
        let mut turn = HarnessTurn::new(HarnessId::new("mock"), SessionId::new("s2"), "scenario");
        turn.project_id = ProjectId::new("p2");
        turn.medium_id = MediumId::new("vm");
        daemon.run(turn).unwrap();

        let snap = daemon.snapshot();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].project_id, ProjectId::new("p2"));
        assert_eq!(snap[0].medium_id, MediumId::new("vm"));
    }

    #[test]
    fn list_harnesses_sorts() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("b"), "x")));
        registry.register(Arc::new(MockHarness::new(HarnessId::new("a"), "y")));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));
        assert_eq!(
            daemon.list_harnesses(),
            vec![HarnessId::new("a"), HarnessId::new("b")]
        );
    }

    #[test]
    fn map_drops_host_discovery_frames() {
        assert!(map_event(HarnessServerEvent::Ready).is_none());
        assert!(matches!(
            map_event(HarnessServerEvent::Done {
                session_id: SessionId::new("s1")
            }),
            Some(DaemonEvent::Done { .. })
        ));
    }

    /// A turn that suspends on an `AskUser` keeps its session live (so a later
    /// `Resume` finds the same harness), and the resumed `Done` settles it.
    #[tokio::test]
    async fn resume_continues_a_suspended_session() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(AskHarness::new(HarnessId::new("ask"))));
        let sink = Arc::new(CollectingSink::default());
        let daemon = DaemonState::new(registry, sink.clone());

        let turn = HarnessTurn::new(HarnessId::new("ask"), SessionId::new("s1"), "scenario");
        daemon.run(turn).unwrap();
        // Let the spawned drain consume the AskUser and settle at suspension.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Suspended: no terminal event, so the session must still be live.
        assert_eq!(
            daemon.snapshot().len(),
            1,
            "a suspended turn keeps its session so Resume can find it"
        );
        assert!(
            daemon.snapshot()[0].events.iter().any(|e| matches!(e, DaemonEvent::AskUser { .. })),
            "the pending ask is part of the execution record"
        );

        // Resume drives the same harness; the answer delta then `Done` settle it.
        daemon.resume(&SessionId::new("s1"), "yes", None).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        assert_eq!(daemon.snapshot().len(), 0, "the resumed turn settles the session");
        let events = sink.events.lock().unwrap();
        assert!(events.iter().any(|e| matches!(e, DaemonEvent::TraceFinished { .. })));
    }

    // --- reversibility: harness-agnostic rewind/fork/replay/checkpoints ---

    #[tokio::test]
    async fn reversal_works_against_an_arbitrary_harness() {
        // MockHarness is a stand-in for *any* harness (internal, CLI, remote):
        // it just emits an event stream. Reversibility must not depend on the
        // harness volunteering snapshot/undo — it is reconstructed from the
        // recorded transcript.
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        daemon.run(turn("s1")).unwrap();
        daemon.run(turn("s2")).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Both turns settled, so no live sessions survive...
        assert_eq!(daemon.snapshot().len(), 0);
        // ...but the transcript ledgers are retained for reversal.
        let cps = daemon.checkpoints(&SessionId::new("s1")).unwrap();
        assert_eq!(cps.len(), 2, "checkpoint at index 0 (empty) and 1 (the turn)");

        // Replay re-emits the recorded transcript in the daemon wire shape.
        let evs = daemon.replay(&SessionId::new("s1"), 0).unwrap();
        assert!(evs.iter().any(
            |e| matches!(e, DaemonEvent::AssistantDelta { delta, .. } if delta == "hi")
        ));
        assert!(evs.iter().any(|e| matches!(e, DaemonEvent::Done { .. })));

        // Fork carries the prefix into an independent, new session.
        let forked =
            daemon.fork(&SessionId::new("s1"), 1, SessionId::new("s1-fork")).unwrap();
        assert_eq!(forked, SessionId::new("s1-fork"));
        assert_eq!(daemon.checkpoints(&SessionId::new("s1-fork")).unwrap().len(), 2);

        // Rewind keeps `at` turns; at the end it is a no-op.
        assert_eq!(daemon.rewind(&SessionId::new("s1"), 1).unwrap(), 0);
        assert_eq!(daemon.rewind(&SessionId::new("s1"), 0).unwrap(), 1);
    }

    #[tokio::test]
    async fn reverse_a_multi_turn_transcript() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        // Two turns on the same session accumulate in one transcript.
        daemon.run(turn("s3")).unwrap();
        daemon.run(turn("s3")).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        assert_eq!(daemon.checkpoints(&SessionId::new("s3")).unwrap().len(), 3);
        assert_eq!(daemon.replay(&SessionId::new("s3"), 0).unwrap().len(), 4); // delta+done x2

        // Rewind to one turn drops the later turn's events from the replay.
        daemon.rewind(&SessionId::new("s3"), 1).unwrap();
        assert_eq!(daemon.replay(&SessionId::new("s3"), 0).unwrap().len(), 2);
    }

    #[tokio::test]
    async fn rewind_rejects_a_live_session() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(BlockingHarness::new(HarnessId::new("mock"))));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        daemon.run(turn("s1")).unwrap();
        assert!(
            daemon.rewind(&SessionId::new("s1"), 0).is_err(),
            "a running session cannot be rewound mid-flight"
        );
    }

    // --- durable persistence hook ---

    #[derive(Default)]
    struct CapturingCheckpointSink {
        stored: Mutex<Option<Checkpoint>>,
    }

    impl CheckpointSink for CapturingCheckpointSink {
        fn persist(&self, cp: &Checkpoint) {
            *self.stored.lock().unwrap() = Some(cp.clone());
        }
    }

    #[tokio::test]
    async fn settled_turn_persists_its_checkpoint() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));
        let sink = Arc::new(CapturingCheckpointSink::default());
        daemon.set_checkpoint_sink(Some(Arc::clone(&sink) as Arc<dyn CheckpointSink>));

        daemon.run(turn("s1")).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let cp = sink.stored.lock().unwrap().clone().expect("a settled turn persists");
        assert_eq!(cp.session_id, SessionId::new("s1"));
        assert_eq!(cp.at, 1);
        assert!(cp.turns[0]
            .events
            .iter()
            .any(|e| matches!(e, HarnessServerEvent::Done { .. })));
    }

    #[tokio::test]
    async fn restored_checkpoint_seeds_a_rewindable_ledger() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        daemon.run(turn("s1")).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Capture the settled checkpoint, then simulate a restart: a fresh
        // daemon seeded from the persisted checkpoint can still be rewound and
        // replayed, even without any harness registered.
        let cp = daemon
            .checkpoints(&SessionId::new("s1"))
            .unwrap()
            .pop()
            .unwrap();
        let restarted = DaemonState::new(HarnessRegistry::new(), Arc::new(crate::sink::NoopSink));
        restarted.restore_checkpoint(&cp);

        assert_eq!(restarted.rewind(&SessionId::new("s1"), 0).unwrap(), 1);
        assert_eq!(restarted.replay(&SessionId::new("s1"), 0).unwrap().len(), 0);
    }

    // --- environment snapshot capture on settled turns ---

    /// A harness that keeps a counter in its state and can snapshot it — mirrors
    /// the `ReversibleHarness` in `goble-harness-runtime`. Used to verify that a
    /// settled turn records an environment snapshot when the harness is
    /// reversible.
    struct ReversibleHarness {
        id: HarnessId,
        counter: Arc<std::sync::Mutex<i32>>,
    }

    impl ReversibleHarness {
        fn new(id: HarnessId) -> Self {
            Self {
                id,
                counter: Arc::new(std::sync::Mutex::new(0)),
            }
        }
    }

    impl HarnessRuntime for ReversibleHarness {
        fn id(&self) -> HarnessId {
            self.id.clone()
        }
        fn capabilities(&self) -> HarnessCapabilities {
            let mut caps = HarnessCapabilities::internal();
            caps.reversible = true;
            caps
        }
        fn run(&self, turn: HarnessTurn, _cancel: Arc<AtomicBool>) -> HarnessRun {
            *self.counter.lock().unwrap() += 1;
            let session_id = turn.session_id;
            let count = *self.counter.lock().unwrap();
            HarnessRun {
                events: Box::pin(futures::stream::iter(vec![
                    HarnessServerEvent::AssistantDelta {
                        session_id: session_id.clone(),
                        delta: format!("count={count}"),
                    },
                    HarnessServerEvent::Done { session_id },
                ])),
            }
        }
        fn snapshot(&self, _session_id: &SessionId) -> Option<HarnessSnapshot> {
            Some(HarnessSnapshot::new(
                "counter",
                serde_json::json!({ "count": *self.counter.lock().unwrap() }),
            ))
        }
        fn restore(&self, session_id: &SessionId, snapshot: &HarnessSnapshot) -> Option<HarnessRun> {
            let count = snapshot.data.get("count").and_then(|v| v.as_i64())? as i32;
            *self.counter.lock().unwrap() = count;
            let session_id = session_id.clone();
            Some(HarnessRun {
                events: Box::pin(futures::stream::iter(vec![
                    HarnessServerEvent::AssistantDelta {
                        session_id: session_id.clone(),
                        delta: format!("restored={count}"),
                    },
                    HarnessServerEvent::Done { session_id },
                ])),
            })
        }
    }

    #[tokio::test]
    async fn settled_turn_records_environment_snapshot_when_reversible() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(ReversibleHarness::new(HarnessId::new("rev"))));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        let turn = HarnessTurn::new(HarnessId::new("rev"), SessionId::new("rev"), "scenario");
        daemon.run(turn).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let log = daemon.snapshot_log(&SessionId::new("rev"));
        assert_eq!(
            log,
            vec![Some(HarnessSnapshot::new(
                "counter",
                serde_json::json!({ "count": 1 })
            ))]
        );
    }

    #[tokio::test]
    async fn settled_turn_records_none_when_not_reversible() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        daemon.run(turn("mock")).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        assert_eq!(daemon.snapshot_log(&SessionId::new("mock")), vec![None]);
    }

    #[tokio::test]
    async fn rewind_restores_environment_from_snapshot_when_reversible() {
        let registry = HarnessRegistry::new();
        let rev = Arc::new(ReversibleHarness::new(HarnessId::new("rev")));
        let counter = Arc::clone(&rev.counter);
        registry.register(Arc::clone(&rev) as Arc<dyn HarnessRuntime>);
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        // Two turns advance the counter to 2; each settling turn records a
        // snapshot. The turns settle sequentially (a session runs one turn at a
        // time), so the counter is 1 when the first turn's snapshot is taken.
        daemon
            .run(HarnessTurn::new(HarnessId::new("rev"), SessionId::new("rev"), "first"))
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        daemon
            .run(HarnessTurn::new(HarnessId::new("rev"), SessionId::new("rev"), "second"))
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(*counter.lock().unwrap(), 2);
        assert_eq!(
            daemon.snapshot_log(&SessionId::new("rev")),
            vec![
                Some(HarnessSnapshot::new("counter", serde_json::json!({ "count": 1 }))),
                Some(HarnessSnapshot::new("counter", serde_json::json!({ "count": 2 }))),
            ]
        );

        // Rewinding to keep the first turn restores the harness environment to the
        // snapshot captured when that kept turn settled (count = 1), then drops the
        // later turn from the transcript.
        let removed = daemon.rewind(&SessionId::new("rev"), 1).unwrap();
        assert_eq!(removed, 1);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(
            *counter.lock().unwrap(),
            1,
            "rewind restores the harness environment to the kept turn's snapshot"
        );
        assert_eq!(
            daemon.replay(&SessionId::new("rev"), 0).unwrap().len(),
            2,
            "only the kept turn survives in the transcript"
        );
    }

    #[tokio::test]
    async fn rewind_stays_transcript_only_without_a_snapshot() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        daemon.run(turn("mock")).unwrap();
        daemon.run(turn("mock")).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(daemon.snapshot_log(&SessionId::new("mock")), vec![None, None]);

        // No environment snapshot exists, so rewinding keeps to the transcript
        // only: the later turn's events are dropped, nothing is restored.
        let removed = daemon.rewind(&SessionId::new("mock"), 1).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(daemon.replay(&SessionId::new("mock"), 0).unwrap().len(), 2);
    }

    // --- settlement: select/apply/release/discard on the transcript ---

    #[tokio::test]
    async fn settlement_select_then_apply_commits_a_candidate() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        for _ in 0..4 {
            daemon.run(turn("s5")).unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Four settled turns (delta+done each) accumulate in one transcript.
        assert_eq!(daemon.replay(&SessionId::new("s5"), 0).unwrap().len(), 8);

        // Select turn index 2 and commit it: keep the candidate and its prefix,
        // dropping the single turn after it.
        daemon.select(&SessionId::new("s5"), 2).unwrap();
        let removed = daemon.apply(&SessionId::new("s5"), 2).unwrap();
        assert_eq!(removed, 1, "only the turn after the candidate is dropped");
        assert_eq!(daemon.replay(&SessionId::new("s5"), 0).unwrap().len(), 6);
    }

    #[tokio::test]
    async fn settlement_release_drops_a_pending_candidate_without_rewind() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        for _ in 0..3 {
            daemon.run(turn("s6")).unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let before = daemon.replay(&SessionId::new("s6"), 0).unwrap().len();

        daemon.select(&SessionId::new("s6"), 1).unwrap();
        assert!(daemon.release(&SessionId::new("s6"), 1).unwrap());
        assert_eq!(
            daemon.replay(&SessionId::new("s6"), 0).unwrap().len(),
            before,
            "release must not rewind the transcript"
        );
    }

    #[tokio::test]
    async fn settlement_discard_throws_away_a_candidate_and_its_tail() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        for _ in 0..4 {
            daemon.run(turn("s7")).unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        daemon.select(&SessionId::new("s7"), 2).unwrap();
        let removed = daemon.discard(&SessionId::new("s7"), 2).unwrap();
        assert_eq!(removed, 2, "the candidate and the tail after it are dropped");
        assert_eq!(daemon.replay(&SessionId::new("s7"), 0).unwrap().len(), 4);
    }

    #[tokio::test]
    async fn settlement_apply_rejects_an_unselected_candidate() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        daemon.run(turn("s8")).unwrap();
        daemon.run(turn("s8")).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // No candidate has been selected, so apply must refuse.
        assert!(daemon.apply(&SessionId::new("s8"), 1).is_err());
    }

    // --- workflow wiring: DaemonPort::run_workflow + the harness run seam ---

    /// The daemon runs a workflow end-to-end: the host journals the request,
    /// runs the referenced workflow through the engine, and persists the run.
    #[test]
    fn run_workflow_journals_and_runs_through_the_host() {
        use goble_workflow::{MockStep, Step, Trigger, Workflow, WorkflowHostRequest, WorkflowId};

        let workflow = Workflow::new(WorkflowId::new("build"), "Build & release")
            .with_step(Step::new("checkout", "Checkout source"))
            .with_step(Step::new("build", "Compile artifacts"))
            .with_trigger(Trigger::Manual);

        let host = WorkflowHost::new(MockStep::new().fail_step("build"));
        let daemon = DaemonState::new(HarnessRegistry::new(), Arc::new(crate::sink::NoopSink));
        daemon.install_workflow_host(Arc::new(host));
        daemon.register_workflow(workflow).unwrap();

        let request = WorkflowHostRequest::new(
            0,
            "req-build-0",
            WorkflowId::new("build"),
            Trigger::Manual,
            serde_json::json!({}),
        );
        let run = daemon.run_workflow(request).unwrap();

        assert_eq!(
            run.status,
            goble_workflow::RunStatus::Failed("forced failure for build".into())
        );
        assert_eq!(run.steps.len(), 2);
        assert_eq!(run.steps[0].status, goble_workflow::StepStatus::Succeeded);
        assert_eq!(
            run.steps[1].status,
            goble_workflow::StepStatus::Failed("forced failure for build".into())
        );
    }

    #[test]
    fn run_workflow_errors_without_an_installed_host() {
        use goble_workflow::{Trigger, WorkflowHostRequest, WorkflowId};
        let daemon = DaemonState::new(HarnessRegistry::new(), Arc::new(crate::sink::NoopSink));
        let request = WorkflowHostRequest::new(
            0,
            "req-0",
            WorkflowId::new("build"),
            Trigger::Manual,
            serde_json::json!({}),
        );
        assert!(daemon.run_workflow(request).is_err());
    }

    /// The installed executor runs each step by driving a harness turn through
    /// the daemon run seam, so a real harness is invoked per step.
    #[test]
    fn installed_executor_runs_steps_through_a_real_harness() {
        use goble_workflow::{Step, Trigger, Workflow, WorkflowHostRequest, WorkflowId};

        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));
        daemon.install_workflow_executor(
            HarnessId::new("mock"),
            ProjectId::new("p1"),
            MediumId::new("local"),
            "wf",
        );
        daemon
            .register_workflow(
                Workflow::new(WorkflowId::new("greet"), "Greet")
                    .with_step(Step::new("say", "Say hi"))
                    .with_trigger(Trigger::Manual),
            )
            .unwrap();

        let request = WorkflowHostRequest::new(
            0,
            "req-greet-0",
            WorkflowId::new("greet"),
            Trigger::Manual,
            serde_json::json!({}),
        );
        // MockHarness emits a delta then Done, so the step settles as succeeded.
        let run = daemon.run_workflow(request).unwrap();
        assert_eq!(run.status, goble_workflow::RunStatus::Succeeded);
        assert_eq!(run.steps.len(), 1);
        assert_eq!(run.steps[0].status, goble_workflow::StepStatus::Succeeded);
    }

    #[test]
    fn run_turn_blocking_reports_success_and_failure() {
        use goble_workflow::StepOutcome;

        struct ErrorHarness {
            id: HarnessId,
        }
        impl ErrorHarness {
            fn new(id: HarnessId) -> Self {
                Self { id }
            }
        }
        impl HarnessRuntime for ErrorHarness {
            fn id(&self) -> HarnessId {
                self.id.clone()
            }
            fn capabilities(&self) -> HarnessCapabilities {
                HarnessCapabilities::internal()
            }
            fn run(&self, turn: HarnessTurn, _cancel: Arc<AtomicBool>) -> HarnessRun {
                let session_id = turn.session_id;
                HarnessRun {
                    events: Box::pin(futures::stream::iter(vec![
                        HarnessServerEvent::AssistantDelta {
                            session_id: session_id.clone(),
                            delta: "oops".to_string(),
                        },
                        HarnessServerEvent::Error {
                            session_id,
                            message: "step boom".to_string(),
                        },
                    ])),
                }
            }
        }

        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("ok"), "hi")));
        registry.register(Arc::new(ErrorHarness::new(HarnessId::new("err"))));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink));

        let ok = daemon.run_turn_blocking(HarnessTurn::new(
            HarnessId::new("ok"),
            SessionId::new("s1"),
            "x",
        ));
        assert_eq!(ok, StepOutcome::Succeeded);

        let err = daemon.run_turn_blocking(HarnessTurn::new(
            HarnessId::new("err"),
            SessionId::new("s2"),
            "x",
        ));
        assert_eq!(err, StepOutcome::Failed("step boom".to_string()));

        let missing = daemon.run_turn_blocking(HarnessTurn::new(
            HarnessId::new("nope"),
            SessionId::new("s3"),
            "x",
        ));
        assert_eq!(missing, StepOutcome::Failed("no harness resolved for nope".to_string()));
    }

    // --- per-session agent workspace ---

    /// A harness that supports a per-run workspace: it receives the session's
    /// workspace dir through [`HarnessRuntime::set_workspace_dir`] and, when
    /// running, writes a marker file into that directory. Used to verify that a
    /// configured `workspace_root` materializes a per-session agent workspace.
    struct WorkspaceHarness {
        id: HarnessId,
        workspace_dir: Arc<Mutex<Option<PathBuf>>>,
    }

    impl WorkspaceHarness {
        fn new(id: HarnessId) -> Self {
            Self {
                id,
                workspace_dir: Arc::new(Mutex::new(None)),
            }
        }
    }

    impl HarnessRuntime for WorkspaceHarness {
        fn id(&self) -> HarnessId {
            self.id.clone()
        }
        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::internal()
        }
        fn set_workspace_dir(&self, dir: &std::path::Path) -> bool {
            *self.workspace_dir.lock().unwrap() = Some(dir.to_path_buf());
            true
        }
        fn run(&self, turn: HarnessTurn, _cancel: Arc<AtomicBool>) -> HarnessRun {
            let dir = self
                .workspace_dir
                .lock()
                .unwrap()
                .clone()
                .expect("the daemon sets the session workspace before run");
            std::fs::write(dir.join("agent.txt"), "agent workspace").unwrap();
            let session_id = turn.session_id;
            HarnessRun {
                events: Box::pin(futures::stream::iter(vec![
                    HarnessServerEvent::AssistantDelta {
                        session_id: session_id.clone(),
                        delta: "worked".to_string(),
                    },
                    HarnessServerEvent::Done { session_id },
                ])),
            }
        }
    }

    /// With a `workspace_root` set, running a session materializes
    /// `<workspace_root>/harness/<trace_id>` and a harness that supports a
    /// per-run workspace writes its files there.
    #[tokio::test]
    async fn workspace_root_materializes_the_session_agent_workspace() {
        let root = std::env::temp_dir().join(format!("goble-daemon-ws-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let registry = HarnessRegistry::new();
        registry.register(Arc::new(WorkspaceHarness::new(HarnessId::new("mock"))));
        let daemon = DaemonState::new(registry, Arc::new(crate::sink::NoopSink))
            .with_workspace_root(root.clone());

        // Session "s1" maps to trace id "trace-s1", so its workspace is
        // `<root>/harness/trace-s1`.
        daemon.run(turn("s1")).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let trace_dir = root.join("harness").join("trace-s1");
        assert!(
            trace_dir.is_dir(),
            "running a session must create its per-session agent workspace"
        );
        assert!(
            trace_dir.join("agent.txt").is_file(),
            "a harness-backed file must land inside the session workspace"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
