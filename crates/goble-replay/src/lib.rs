//! Reversible-execution substrate: a per-turn transcript ledger with
//! rewind/revert, fork, and deterministic replay.
//!
//! Goble's moat is reversibility. `goble-replay` is the model layer that makes
//! execution reversible: it records each `HarnessTurn` alongside the events it
//! produced (the transcript), so a session can be rewound to an earlier point,
//! forked into an independent branch from any point, checkpointed/restored, and
//! replayed (re-emitted) deterministically without touching a harness.
//!
//! This is a pure model crate — no transport, no harness driving, no persistence.
//! It depends only on the harness seam (`types`/`protocol`), so it plugs into the
//! daemon's execution ledger. A live turn is recorded by streaming its events
//! into [`ReplayLedger::append_events`] and then settling it; the ledger is
//! internally synchronized (cloneable and shareable) so an async drain task can
//! record a turn while the rest of the host reads/writes it concurrently.

use chrono::{DateTime, Utc};
use goble_harness_protocol::HarnessServerEvent;
use goble_harness_types::{HarnessTurn, SessionId};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A durable hook called with a session's [`Checkpoint`] when the daemon
/// settles a turn, so the reversible history can be persisted (e.g. to SQLite).
/// Keeping it a trait leaves this model crate free of any concrete storage —
/// the daemon just forwards the checkpoint, and a consumer supplies the store.
pub trait CheckpointSink: Send + Sync {
    fn persist(&self, checkpoint: &Checkpoint);
}

/// Errors from replay operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    #[error("no turn has been recorded yet")]
    NoTurn,
    #[error("checkpoint index {requested} is out of range (ledger has {len} turns)")]
    IndexOutOfRange { requested: usize, len: usize },
    #[error("candidate turn {at} is not the currently selected candidate (selected: {selected:?})")]
    CandidateNotSelected { at: usize, selected: Option<usize> },
}

/// The life-cycle of one recorded turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Running,
    Success,
    Failed(String),
    Cancelled,
}

/// One turn in the transcript: the request handed to a harness plus everything
/// it produced, and how it settled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnRecord {
    /// Position of this turn in the ledger (0-based, contiguous).
    pub index: usize,
    /// The request that was handed to the harness for this turn.
    pub turn: HarnessTurn,
    /// When this turn started.
    pub started_at: DateTime<Utc>,
    /// When this turn settled, if it did.
    pub finished_at: Option<DateTime<Utc>>,
    /// The status the turn settled to.
    pub status: TurnStatus,
    /// The events the harness produced for this turn, in order.
    pub events: Vec<HarnessServerEvent>,
}

/// A cheap snapshot of the ledger at a point in its history. Serializable, so
/// it is what a consumer persists or ships across the wire. `at` is the number
/// of turns captured (the prefix length), which is the checkpoint index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub session_id: SessionId,
    /// Number of turns captured; a checkpoint index used for rewind/fork.
    pub at: usize,
    pub turns: Vec<TurnRecord>,
    pub created_at: DateTime<Utc>,
}

struct LedgerState {
    session_id: SessionId,
    turns: Vec<TurnRecord>,
    /// Index of the checkpoint turn currently marked as the settlement
    /// candidate (`None` when nothing is selected).
    selected: Option<usize>,
}

/// The reversibility ledger for one session. Cloneable and shareable (interior
/// mutability via [`parking_lot::Mutex`]), so a live turn can be recorded from
/// an async drain task while the rest of the host reads/writes concurrently.
///
/// - [`ReplayLedger::record_turn`] pushes a new turn and returns its index.
/// - [`ReplayLedger::append_events`] streams that turn's events into it.
/// - [`ReplayLedger::settle_turn`] finalizes the turn's status/timestamp.
/// - [`ReplayLedger::rewind_to`]/[`ReplayLedger::revert_to`] drop the tail.
/// - [`ReplayLedger::fork`] branches a fresh ledger from a checkpoint index.
/// - [`ReplayLedger::checkpoint`]/[`ReplayLedger::checkpoint_at`] snapshot it.
/// - [`ReplayLedger::replay`] re-emits a recorded trajectory deterministically.
/// - [`ReplayLedger::select`]/[`ReplayLedger::apply`]/[`ReplayLedger::release`]/
///   [`ReplayLedger::discard`] settle a chosen candidate turn.
#[derive(Clone)]
pub struct ReplayLedger {
    state: Arc<Mutex<LedgerState>>,
}

impl ReplayLedger {
    /// A fresh, empty ledger for `session_id`.
    pub fn new(session_id: SessionId) -> Self {
        Self {
            state: Arc::new(Mutex::new(LedgerState {
                session_id,
                turns: Vec::new(),
                selected: None,
            })),
        }
    }

    /// Restore a ledger from a previously saved [`Checkpoint`], so an execution
    /// can be resumed exactly where it was snapshotted.
    pub fn from_checkpoint(cp: &Checkpoint) -> Self {
        Self {
            state: Arc::new(Mutex::new(LedgerState {
                session_id: cp.session_id.clone(),
                turns: cp.turns.clone(),
                selected: None,
            })),
        }
    }

    pub fn session_id(&self) -> SessionId {
        self.state.lock().session_id.clone()
    }

    /// Number of turns recorded.
    pub fn len(&self) -> usize {
        self.state.lock().turns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Start recording a turn. Returns its index. The previous turn is left as
    /// recorded (so a non-settled turn's `status` is still `Running` until
    /// [`Self::settle_turn`] is called for it).
    pub fn record_turn(&self, turn: HarnessTurn) -> usize {
        let mut s = self.state.lock();
        let index = s.turns.len();
        s.turns.push(TurnRecord {
            index,
            turn,
            started_at: Utc::now(),
            finished_at: None,
            status: TurnStatus::Running,
            events: Vec::new(),
        });
        index
    }

    /// Append events to the most recently recorded turn (the one still running).
    pub fn append_events(
        &self,
        events: impl IntoIterator<Item = HarnessServerEvent>,
    ) -> Result<(), ReplayError> {
        let mut s = self.state.lock();
        let last = s.turns.last_mut().ok_or(ReplayError::NoTurn)?;
        last.events.extend(events);
        Ok(())
    }

    /// Settle the most recently recorded turn with `status`.
    pub fn settle_turn(&self, status: TurnStatus) -> Result<(), ReplayError> {
        let mut s = self.state.lock();
        let last = s.turns.last_mut().ok_or(ReplayError::NoTurn)?;
        last.status = status;
        last.finished_at = Some(Utc::now());
        Ok(())
    }

    /// Append events to a specific recorded turn by index. Prefer this over
    /// [`Self::append_events`] when a session can host multiple concurrent or
    /// rapid turns, so each turn's events land on its own record instead of the
    /// current tail.
    pub fn append_events_to(
        &self,
        index: usize,
        events: impl IntoIterator<Item = HarnessServerEvent>,
    ) -> Result<(), ReplayError> {
        let mut s = self.state.lock();
        let len = s.turns.len();
        let turn = s
            .turns
            .get_mut(index)
            .ok_or(ReplayError::IndexOutOfRange { requested: index, len })?;
        turn.events.extend(events);
        Ok(())
    }

    /// Settle a specific recorded turn by index.
    pub fn settle_turn_at(&self, index: usize, status: TurnStatus) -> Result<(), ReplayError> {
        let mut s = self.state.lock();
        let len = s.turns.len();
        let turn = s
            .turns
            .get_mut(index)
            .ok_or(ReplayError::IndexOutOfRange { requested: index, len })?;
        turn.status = status;
        turn.finished_at = Some(Utc::now());
        Ok(())
    }

    /// The index of the checkpoint turn currently marked as the selected
    /// settlement candidate, if any. A candidate is chosen with [`Self::select`]
    /// and either committed with [`Self::apply`], released with
    /// [`Self::release`], or discarded with [`Self::discard`].
    pub fn selected(&self) -> Option<usize> {
        self.state.lock().selected
    }

    /// Mark checkpoint turn `at` as the selected candidate. The transcript is
    /// otherwise unchanged: this only records the intended settlement target so
    /// a later [`Self::apply`]/[`Self::release`]/[`Self::discard`] can act on
    /// it. Errors if `at` is out of range.
    pub fn select(&self, at: usize) -> Result<(), ReplayError> {
        let mut s = self.state.lock();
        let len = s.turns.len();
        if at >= len {
            return Err(ReplayError::IndexOutOfRange { requested: at, len });
        }
        s.selected = Some(at);
        Ok(())
    }

    /// Commit the selected candidate at `at`: the transcript keeps the candidate
    /// and everything before it, dropping the tail after it. Returns how many
    /// turns were removed. Errors if `at` is out of range, or is not the
    /// currently selected candidate (call [`Self::select`] first).
    pub fn apply(&self, at: usize) -> Result<usize, ReplayError> {
        let mut s = self.state.lock();
        let len = s.turns.len();
        if at >= len {
            return Err(ReplayError::IndexOutOfRange { requested: at, len });
        }
        if s.selected != Some(at) {
            return Err(ReplayError::CandidateNotSelected { at, selected: s.selected });
        }
        let removed = len - at - 1;
        s.turns.truncate(at + 1);
        s.selected = None;
        Ok(removed)
    }

    /// Drop the pending selection at `at` without rewinding: the transcript is
    /// untouched, only the candidate marker is cleared. Returns whether a
    /// pending candidate was actually released. Errors if `at` is out of range.
    pub fn release(&self, at: usize) -> Result<bool, ReplayError> {
        let mut s = self.state.lock();
        let len = s.turns.len();
        if at >= len {
            return Err(ReplayError::IndexOutOfRange { requested: at, len });
        }
        let was = s.selected == Some(at);
        if was {
            s.selected = None;
        }
        Ok(was)
    }

    /// Discard the candidate at `at` and its whole tail: the transcript keeps
    /// `[0, at)` and everything from `at` onward is thrown away. Returns how
    /// many turns were removed. This is the abandonment counterpart to
    /// [`Self::apply`] (which keeps the candidate). Errors if `at` is out of
    /// range.
    pub fn discard(&self, at: usize) -> Result<usize, ReplayError> {
        let mut s = self.state.lock();
        let len = s.turns.len();
        if at >= len {
            return Err(ReplayError::IndexOutOfRange { requested: at, len });
        }
        let removed = len - at;
        s.turns.truncate(at);
        s.selected = None;
        Ok(removed)
    }

    /// Rewind the ledger so it keeps exactly `index` turns, dropping the tail.
    /// Returns how many turns were removed. An `index` at or beyond the current
    /// length is a no-op (returns 0). This is the "undo" primitive: go back to
    /// a checkpoint and discard everything after it.
    pub fn rewind_to(&self, index: usize) -> usize {
        let mut s = self.state.lock();
        let n = s.turns.len();
        if index >= n {
            return 0;
        }
        let removed = n - index;
        s.turns.truncate(index);
        removed
    }

    /// Alias for [`Self::rewind_to`] that reads as "revert the session to this
    /// point", discarding all later work.
    pub fn revert_to(&self, index: usize) -> usize {
        self.rewind_to(index)
    }

    /// Branch a fresh ledger from checkpoint index `at`, carrying the recorded
    /// prefix `[0, at)` into a new session that is free to diverge. The original
    /// ledger is untouched. `at` is clamped to the current length.
    pub fn fork(&self, at: usize, new_session_id: SessionId) -> ReplayLedger {
        let s = self.state.lock();
        let at = at.min(s.turns.len());
        ReplayLedger {
            state: Arc::new(Mutex::new(LedgerState {
                session_id: new_session_id,
                turns: s.turns[..at].to_vec(),
                selected: None,
            })),
        }
    }

    /// A full snapshot of the ledger.
    pub fn checkpoint(&self) -> Checkpoint {
        let s = self.state.lock();
        Checkpoint {
            session_id: s.session_id.clone(),
            at: s.turns.len(),
            turns: s.turns.clone(),
            created_at: Utc::now(),
        }
    }

    /// A snapshot capturing only the prefix `[0, at)`. `at` is clamped.
    pub fn checkpoint_at(&self, at: usize) -> Checkpoint {
        let s = self.state.lock();
        let at = at.min(s.turns.len());
        Checkpoint {
            session_id: s.session_id.clone(),
            at,
            turns: s.turns[..at].to_vec(),
            created_at: Utc::now(),
        }
    }

    /// Overwrite the ledger in place with a saved [`Checkpoint`], restoring an
    /// execution to exactly that point.
    pub fn restore(&self, cp: &Checkpoint) {
        let mut s = self.state.lock();
        s.session_id = cp.session_id.clone();
        s.turns = cp.turns.clone();
        s.selected = None;
    }

    /// Deterministically re-emit the recorded events from checkpoint index `at`
    /// onward, concatenated in order, without touching a harness. This is how a
    /// transcript is replayed (e.g. to rebuild a trace after a rewind/fork).
    /// `at` is clamped to `0..=len`.
    pub fn replay(&self, at: usize) -> Vec<HarnessServerEvent> {
        let s = self.state.lock();
        let at = at.min(s.turns.len());
        s.turns[at..].iter().flat_map(|t| t.events.iter().cloned()).collect()
    }

    /// A clone of all recorded turns.
    pub fn turns(&self) -> Vec<TurnRecord> {
        self.state.lock().turns.clone()
    }

    /// A clone of one recorded turn by index.
    pub fn turn(&self, index: usize) -> Option<TurnRecord> {
        self.state.lock().turns.get(index).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_harness_types::HarnessId;

    fn delta(session: &str, delta: &str) -> HarnessServerEvent {
        HarnessServerEvent::AssistantDelta {
            session_id: SessionId::new(session),
            delta: delta.to_string(),
        }
    }

    fn done(session: &str) -> HarnessServerEvent {
        HarnessServerEvent::Done {
            session_id: SessionId::new(session),
        }
    }

    fn turn(session: &str, goal: &str) -> HarnessTurn {
        HarnessTurn::new(HarnessId::new("mock"), SessionId::new(session), goal)
    }

    #[test]
    fn record_replay_single_turn() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        let idx = ledger.record_turn(turn("s1", "fix bug"));
        assert_eq!(idx, 0);
        ledger.append_events(vec![delta("s1", "hi"), done("s1")]).unwrap();
        ledger
            .settle_turn(TurnStatus::Success)
            .unwrap();

        assert_eq!(ledger.len(), 1);
        let replayed = ledger.replay(0);
        assert_eq!(replayed.len(), 2);
        assert!(matches!(
            &replayed[0],
            HarnessServerEvent::AssistantDelta { delta, .. } if delta == "hi"
        ));
        assert!(matches!(&replayed[1], HarnessServerEvent::Done { .. }));
    }

    #[test]
    fn rewind_truncates_history() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        for i in 0..3 {
            ledger.record_turn(turn("s1", &format!("goal {i}")));
            ledger.append_events(vec![delta("s1", &format!("d{i}"))]).unwrap();
            ledger.settle_turn(TurnStatus::Success).unwrap();
        }
        assert_eq!(ledger.len(), 3);

        let removed = ledger.rewind_to(1);
        assert_eq!(removed, 2);
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger.turn(0).unwrap().turn.goal, "goal 0");
        // The dropped turns' events are gone from the replay.
        assert_eq!(ledger.replay(0).len(), 1);
    }

    #[test]
    fn rewind_beyond_len_is_noop() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        ledger.record_turn(turn("s1", "a"));
        ledger.append_events(vec![delta("s1", "x")]).unwrap();
        assert_eq!(ledger.rewind_to(5), 0);
        assert_eq!(ledger.len(), 1);
    }

    #[test]
    fn revert_to_aliases_rewind() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        ledger.record_turn(turn("s1", "a"));
        ledger.record_turn(turn("s1", "b"));
        assert_eq!(ledger.revert_to(0), 2);
        assert!(ledger.is_empty());
    }

    #[test]
    fn select_marks_a_candidate() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        for i in 0..3 {
            ledger.record_turn(turn("s1", &format!("goal {i}")));
        }

        assert_eq!(ledger.selected(), None);
        ledger.select(1).unwrap();
        assert_eq!(ledger.selected(), Some(1));
        assert_eq!(ledger.len(), 3, "select must not change the transcript");

        assert!(matches!(
            ledger.select(9),
            Err(ReplayError::IndexOutOfRange { requested: 9, .. })
        ));
    }

    #[test]
    fn apply_commits_the_selected_candidate() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        for i in 0..4 {
            let idx = ledger.record_turn(turn("s1", &format!("goal {i}")));
            ledger.append_events_to(idx, vec![delta("s1", &format!("d{i}"))]).unwrap();
        }

        ledger.select(2).unwrap();
        let removed = ledger.apply(2).unwrap();
        assert_eq!(removed, 1, "drops the single turn after the candidate");
        assert_eq!(ledger.len(), 3, "keeps the candidate and its prefix");
        assert_eq!(ledger.turn(2).unwrap().turn.goal, "goal 2");
        assert_eq!(ledger.selected(), None, "applying clears the selection");
        assert_eq!(ledger.replay(0).len(), 3);
    }

    #[test]
    fn apply_requires_a_selected_candidate() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        ledger.record_turn(turn("s1", "a"));
        ledger.record_turn(turn("s1", "b"));

        // Nothing selected yet, so apply must refuse rather than act on an
        // arbitrary index.
        assert!(matches!(
            ledger.apply(0),
            Err(ReplayError::CandidateNotSelected { .. })
        ));

        // Selecting a different turn than the one applied also refuses.
        ledger.select(1).unwrap();
        assert!(matches!(
            ledger.apply(0),
            Err(ReplayError::CandidateNotSelected { .. })
        ));
    }

    #[test]
    fn release_drops_a_pending_candidate_without_rewinding() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        for i in 0..3 {
            ledger.record_turn(turn("s1", &format!("goal {i}")));
        }

        ledger.select(1).unwrap();
        assert!(ledger.release(1).unwrap(), "a pending candidate was released");
        assert_eq!(ledger.selected(), None);
        assert_eq!(ledger.len(), 3, "release leaves the transcript untouched");

        // Releasing again reports nothing was pending.
        assert!(!ledger.release(1).unwrap());

        assert!(matches!(
            ledger.release(9),
            Err(ReplayError::IndexOutOfRange { requested: 9, .. })
        ));
    }

    #[test]
    fn discard_throws_away_a_candidate_and_its_tail() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        for i in 0..4 {
            ledger.record_turn(turn("s1", &format!("goal {i}")));
        }

        ledger.select(2).unwrap();
        let removed = ledger.discard(2).unwrap();
        assert_eq!(removed, 2, "drops the candidate and everything after it");
        assert_eq!(ledger.len(), 2, "keeps only the prefix before the candidate");
        assert_eq!(ledger.turn(0).unwrap().turn.goal, "goal 0");
        assert_eq!(ledger.turn(1).unwrap().turn.goal, "goal 1");
        assert_eq!(ledger.selected(), None, "discarding clears the selection");
    }

    #[test]
    fn fork_creates_independent_branch() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        for i in 0..4 {
            ledger.record_turn(turn("s1", &format!("goal {i}")));
            ledger.append_events(vec![delta("s1", &format!("d{i}"))]).unwrap();
        }

        let branch = ledger.fork(2, SessionId::new("s1-fork"));
        assert_eq!(branch.session_id(), SessionId::new("s1-fork"));
        assert_eq!(branch.len(), 2);
        assert_eq!(branch.turn(0).unwrap().turn.goal, "goal 0");
        assert_eq!(branch.turn(1).unwrap().turn.goal, "goal 1");

        // The branch diverges without affecting the original.
        branch.record_turn(turn("s1-fork", "diverged"));
        assert_eq!(branch.len(), 3);
        assert_eq!(ledger.len(), 4, "fork must not mutate the original ledger");
    }

    #[test]
    fn checkpoints_capture_prefix() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        for i in 0..4 {
            ledger.record_turn(turn("s1", &format!("goal {i}")));
            ledger.append_events(vec![delta("s1", &format!("d{i}"))]).unwrap();
        }

        let cp = ledger.checkpoint_at(2);
        assert_eq!(cp.at, 2);
        assert_eq!(cp.turns.len(), 2);
        assert_eq!(cp.session_id, SessionId::new("s1"));

        let full = ledger.checkpoint();
        assert_eq!(full.at, 4);
        assert_eq!(full.turns.len(), 4);
    }

    #[test]
    fn restore_overwrites_ledger_in_place() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        ledger.record_turn(turn("s1", "a"));
        ledger.append_events(vec![delta("s1", "x")]).unwrap();

        let saved = ledger.checkpoint_at(1);

        // Add more, then restore back to the checkpoint.
        ledger.record_turn(turn("s1", "b"));
        assert_eq!(ledger.len(), 2);
        ledger.restore(&saved);
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger.turn(0).unwrap().turn.goal, "a");
    }

    #[test]
    fn from_checkpoint_resumes_a_saved_execution() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        ledger.record_turn(turn("s1", "a"));
        ledger.append_events(vec![delta("s1", "x")]).unwrap();
        let cp = ledger.checkpoint();

        let resumed = ReplayLedger::from_checkpoint(&cp);
        assert_eq!(resumed.session_id(), SessionId::new("s1"));
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed.replay(0), vec![delta("s1", "x")]);
    }

    #[test]
    fn index_aware_append_and_settle() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        let t0 = ledger.record_turn(turn("s1", "first"));
        let t1 = ledger.record_turn(turn("s1", "second"));
        assert_eq!(t0, 0);
        assert_eq!(t1, 1);

        // Each turn's events land on its own record, even though both are
        // recorded before either stream drains.
        ledger.append_events_to(t0, vec![delta("s1", "d0")]).unwrap();
        ledger.append_events_to(t1, vec![delta("s1", "d1")]).unwrap();
        ledger.settle_turn_at(t0, TurnStatus::Success).unwrap();
        ledger.settle_turn_at(t1, TurnStatus::Failed("boom".to_string())).unwrap();

        assert_eq!(ledger.turn(0).unwrap().events.len(), 1);
        assert_eq!(ledger.turn(0).unwrap().status, TurnStatus::Success);
        assert_eq!(ledger.turn(1).unwrap().events.len(), 1);
        assert_eq!(
            ledger.turn(1).unwrap().status,
            TurnStatus::Failed("boom".to_string())
        );
        assert_eq!(ledger.replay(0).len(), 2);

        // Out-of-range index errors.
        assert!(matches!(
            ledger.append_events_to(9, vec![delta("s1", "x")]),
            Err(ReplayError::IndexOutOfRange { .. })
        ));
    }

    #[test]
    fn append_to_no_turn_errors() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        assert!(matches!(
            ledger.append_events(vec![delta("s1", "x")]),
            Err(ReplayError::NoTurn)
        ));
        assert!(matches!(
            ledger.settle_turn(TurnStatus::Success),
            Err(ReplayError::NoTurn)
        ));
    }

    #[test]
    fn checkpoint_serializes() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        ledger.record_turn(turn("s1", "a"));
        ledger.append_events(vec![delta("s1", "x"), done("s1")]).unwrap();
        let cp = ledger.checkpoint();

        let json = serde_json::to_string(&cp).unwrap();
        let decoded: Checkpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, cp);
        assert_eq!(decoded.at, 1);
    }

    #[tokio::test]
    async fn concurrent_recording_is_safe() {
        let ledger = ReplayLedger::new(SessionId::new("s1"));
        ledger.record_turn(turn("s1", "a"));
        let mut handles = Vec::new();
        for i in 0..16 {
            let ledger = ledger.clone();
            handles.push(tokio::spawn(async move {
                ledger
                    .append_events(vec![delta("s1", &format!("d{i}"))])
                    .unwrap();
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        assert_eq!(ledger.turn(0).unwrap().events.len(), 16);
    }
}
