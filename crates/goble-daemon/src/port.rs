//! The object-safe facade a client uses to drive the daemon.
//!
//! The in-process composition root implements [`DaemonPort`] on [`DaemonState`];
//! a remote client (`goble-daemon-client`) drives it over the wire. Both expose
//! the same run/resume/cancel/introspection surface, so the GUI never sees where
//! the daemon actually runs.

use anyhow::Result;
use goble_daemon_protocol::DaemonEvent;
use goble_harness_types::{HarnessId, HarnessTurn, SessionId};
use goble_workflow::{WorkflowHostRequest, WorkflowRun};

use crate::state::ExecutionRecord;
use crate::Checkpoint;

/// A transport-agnostic daemon boundary.
///
/// Implemented by [`DaemonState`] for the embedded daemon and by the client
/// facade (`goble-daemon-client`) for a remote daemon. Events stream back to the
/// caller through the event sink the daemon was constructed with, not through
/// this trait.
pub trait DaemonPort: Send + Sync {
    fn run(&self, turn: HarnessTurn) -> Result<()>;
    fn resume(
        &self,
        session_id: &SessionId,
        response: &str,
        credential: Option<(String, String)>,
    ) -> Result<()>;
    fn cancel(&self, session_id: &SessionId) -> Result<()>;
    fn list_harnesses(&self) -> Vec<HarnessId>;
    fn snapshot(&self) -> Vec<ExecutionRecord>;

    /// Run a scripted workflow: journal `req` through the daemon's workflow
    /// host, run the referenced workflow, and return the persisted
    /// [`WorkflowRun`].
    fn run_workflow(&self, req: WorkflowHostRequest) -> Result<WorkflowRun>;

    // --- reversibility (harness-agnostic) ---
    // These operate purely on the recorded transcript, which every harness
    // produces through its event stream. They never call into the harness, so
    // rewind/fork/replay work regardless of which harness ran the turn.

    /// Rewind a (settled) session's transcript to keep `at` turns, dropping the
    /// tail. Returns how many turns were removed. A live session must be
    /// cancelled or allowed to settle first.
    fn rewind(&self, session_id: &SessionId, at: usize) -> Result<usize>;

    /// Fork a fresh session from a checkpoint index, carrying the recorded
    /// prefix. The source session is untouched.
    fn fork(&self, session_id: &SessionId, at: usize, new_session_id: SessionId)
        -> Result<SessionId>;

    /// Re-emit a session's recorded event transcript from checkpoint `at` onward,
    /// in the daemon wire shape.
    fn replay(&self, session_id: &SessionId, at: usize) -> Result<Vec<DaemonEvent>>;

    /// List the per-turn checkpoints of a session's transcript.
    fn checkpoints(&self, session_id: &SessionId) -> Result<Vec<Checkpoint>>;

    /// Mark checkpoint turn `at` as a session's settlement candidate. The
    /// transcript is unchanged; use [`Self::apply`]/[`Self::release`]/
    /// [`Self::discard`] to settle the candidate.
    fn select(&self, session_id: &SessionId, at: usize) -> Result<()>;

    /// Commit the selected candidate at `at`: keep the candidate and its prefix,
    /// dropping the tail after it. Returns how many turns were removed.
    fn apply(&self, session_id: &SessionId, at: usize) -> Result<usize>;

    /// Drop a pending candidate at `at` without rewinding. Returns whether a
    /// pending candidate was actually released.
    fn release(&self, session_id: &SessionId, at: usize) -> Result<bool>;

    /// Discard the candidate at `at` and its whole tail. Returns how many turns
    /// were removed.
    fn discard(&self, session_id: &SessionId, at: usize) -> Result<usize>;
}
