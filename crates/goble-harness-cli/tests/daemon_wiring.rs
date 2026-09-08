//! Proves a [`goble_harness_cli::CliHarness`] is drivable by the daemon core
//! with no changes to `goble-daemon`: register it, run a turn, and the daemon
//! streams the subprocess events through its own event sink.

use std::sync::Arc;
use std::time::Duration;

use goble_daemon::{DaemonEventSink, DaemonPort, DaemonState};
use goble_daemon_protocol::DaemonEvent;
use goble_harness_cli::{CliHarness, CliHarnessConfig};
use goble_harness_runtime::HarnessRegistry;
use goble_harness_types::{HarnessId, HarnessTurn, SessionId};
use parking_lot::Mutex;

struct Sink(Mutex<Vec<DaemonEvent>>);

impl DaemonEventSink for Sink {
    fn emit(&self, event: DaemonEvent) {
        self.0.lock().push(event);
    }
}

#[tokio::test]
async fn daemon_drives_a_cli_harness_to_completion() {
    let cli = CliHarness::new(CliHarnessConfig::new(
        HarnessId::new("cli"),
        vec![
            env!("CARGO_BIN_EXE_harness_fixture").to_string(),
            "happy".to_string(),
        ],
    ))
    .expect("build cli harness");

    let registry = HarnessRegistry::new();
    registry.register(Arc::new(cli));
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    let daemon = DaemonState::new(registry, sink.clone());

    let turn = HarnessTurn::new(HarnessId::new("cli"), SessionId::new("s1"), "do it");
    daemon.run(turn).unwrap();

    // Let the spawned drain consume the subprocess stream.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let events = sink.0.lock().clone();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DaemonEvent::AssistantDelta { delta, .. } if delta == "from-fixture")),
        "the CLI harness delta must reach the daemon sink"
    );
    assert!(
        events.iter().any(|e| matches!(e, DaemonEvent::TraceStarted { .. })),
        "the daemon opens a trace"
    );
    assert!(
        events.iter().any(|e| matches!(e, DaemonEvent::TraceFinished { .. })),
        "the daemon closes the trace once the CLI harness settles"
    );
}
