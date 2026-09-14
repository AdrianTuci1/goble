use super::*;

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
