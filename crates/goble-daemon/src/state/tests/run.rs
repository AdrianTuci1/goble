use super::*;

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
