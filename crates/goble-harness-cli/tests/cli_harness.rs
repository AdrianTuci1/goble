//! End-to-end tests for [`goble_harness_cli::CliHarness`] driving a real
//! subprocess (the `harness_fixture` bin in this crate).

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use goble_harness_cli::{CliHarness, CliHarnessConfig};
use goble_harness_protocol::HarnessServerEvent;
use goble_harness_runtime::HarnessRuntime;
use goble_harness_types::{HarnessId, HarnessTurn, SessionId};

fn fixture_path() -> &'static str {
    env!("CARGO_BIN_EXE_harness_fixture")
}

fn harness(mode: &str) -> CliHarness {
    CliHarness::new(CliHarnessConfig::new(
        HarnessId::new("cli"),
        vec![fixture_path().to_string(), mode.to_string()],
    ))
    .expect("build cli harness")
}

fn turn(session: &str) -> HarnessTurn {
    HarnessTurn::new(HarnessId::new("cli"), SessionId::new(session), "do it")
}

#[tokio::test]
async fn run_streams_delta_then_done() {
    let h = harness("happy");
    let run = h.run(turn("s1"), Arc::new(AtomicBool::new(false)));

    let events: Vec<_> = tokio::time::timeout(
        Duration::from_secs(5),
        run.events.collect(),
    )
    .await
    .expect("turn should finish");

    assert!(
        events.iter().any(|e| matches!(e, HarnessServerEvent::AssistantDelta { delta, .. } if delta == "from-fixture")),
        "expected the fixture delta"
    );
    assert!(
        events.iter().any(|e| matches!(e, HarnessServerEvent::Done { session_id } if session_id == &SessionId::new("s1"))),
        "expected a Done for the session"
    );
}

#[tokio::test]
async fn cancel_ends_a_suspended_stream() {
    let h = harness("suspend");
    let cancel = Arc::new(AtomicBool::new(false));
    let mut run = h.run(turn("s1"), Arc::clone(&cancel));

    // First event is the delta the fixture emits on Run.
    let first = tokio::time::timeout(Duration::from_secs(5), run.events.next())
        .await
        .expect("delta should arrive")
        .expect("stream not empty");
    assert!(matches!(first, HarnessServerEvent::AssistantDelta { .. }));

    // Cancel kills the child; the stream must terminate without a Done.
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    h.cancel();

    let rest: Vec<_> = tokio::time::timeout(
        Duration::from_secs(5),
        run.events.collect(),
    )
    .await
    .expect("cancelled stream should end");

    assert!(
        !rest.iter().any(|e| matches!(e, HarnessServerEvent::Done { .. })),
        "a cancelled turn must not report Done"
    );
}

#[tokio::test]
async fn resume_streams_response_then_done() {
    let h = CliHarness::new(CliHarnessConfig::new(
        HarnessId::new("cli"),
        vec![fixture_path().to_string(), "resume".to_string()],
    ))
    .expect("build cli harness");

    let run = h
        .resume(&SessionId::new("s1"), "my answer", None)
        .expect("resume is supported");

    let events: Vec<_> = tokio::time::timeout(
        Duration::from_secs(5),
        run.events.collect(),
    )
    .await
    .expect("resumed turn should finish");

    assert!(
        events.iter().any(|e| matches!(e, HarnessServerEvent::AssistantDelta { delta, .. } if delta == "my answer")),
        "resume should stream the response"
    );
    assert!(
        events.iter().any(|e| matches!(e, HarnessServerEvent::Done { .. })),
        "resume should settle with Done"
    );
}
