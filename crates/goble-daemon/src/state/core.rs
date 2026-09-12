use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use goble_harness_protocol::HarnessServerEvent;
use goble_harness_runtime::{HarnessRegistry, HarnessRuntime};
use goble_harness_types::{HarnessId, HarnessSnapshot, HarnessTurn, MediumId, ProjectId, SessionId};
use goble_replay::{Checkpoint, CheckpointSink, ReplayLedger};
use goble_workflow::{StepOutcome, Workflow};

use crate::sink::DaemonEventSink;
use crate::workflow::{HarnessStepExecutor, WorkflowHost};

use super::DaemonState;

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
    pub(super) fn ledger(&self, session_id: &SessionId) -> ReplayLedger {
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
}
