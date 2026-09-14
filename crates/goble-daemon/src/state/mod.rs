//! The daemon state: harness registry + per-session execution ledger + sink.
//!
//! One module per surface of the daemon: its construction, registry and workflow
//! wiring ([`core`]), the per-session stream plumbing ([`streams`]), the
//! [`DaemonPort`] implementation that drives turns ([`port`]), and the mapping
//! from harness events to wire events ([`events`]). What the surfaces share —
//! [`DaemonState`], the live [`SessionState`] it keeps while a turn runs and the
//! [`ExecutionRecord`] it snapshots into — lives here.
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
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use goble_daemon_protocol::DaemonEvent;
use goble_harness_runtime::{HarnessRegistry, HarnessRuntime};
use goble_harness_types::{HarnessSnapshot, MediumId, ProjectId, SessionId};
use goble_replay::{CheckpointSink, ReplayLedger};
use serde::{Deserialize, Serialize};

use crate::sink::DaemonEventSink;
use crate::workflow::WorkflowHost;

mod core;
mod events;
mod port;
mod streams;

#[cfg(test)]
mod tests;

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
