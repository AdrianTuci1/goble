//! Integration tests for the daemon reversibility + workflow surface exposed on
//! the worker's WebSocket.
//!
//! These drive the embedded daemon directly through the shared
//! `goblin_worker::websocket::handle_daemon_desktop_message` handler rather than
//! over a live socket, so they do not need a built `goblin` binary and can assert
//! the exact [`WorkflowRun`]/result the worker forwards onto its event channel.

use std::sync::Arc;
use std::time::Duration;

use goble_core::protocol::{DesktopMessage, WorkerMessage};
use goble_core::worker::WorkerId;
use goble_daemon::{DaemonPort, WorkflowHost};
use goble_harness_runtime::MockHarness;
use goble_harness_types::{HarnessId, HarnessTurn, SessionId};
use goble_workflow::{
    MockStep, RunStatus, Step, Trigger, Workflow, WorkflowHostRequest, WorkflowId, WorkflowRun,
};
use goblin_worker::state::AppState;
use goblin_worker::websocket::handle_daemon_desktop_message;

/// A rewind request against a settled session is accepted and the worker reports
/// how many transcript turns were dropped.
#[tokio::test]
async fn rewind_action_is_accepted() {
    let state = AppState::new(WorkerId::generate());
    // The daemon materializes per-session harness workspaces, so point it at a
    // temp dir before the (lazily built) daemon reads the configured root.
    let tmp = tempfile::tempdir().unwrap();
    state.config.lock().workspace_root = tmp.path().join("workspaces");
    // Seed a harness so a turn can settle and populate a transcript ledger.
    let daemon = state.daemon_state();
    daemon.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));

    // Run one turn on session "s1"; the transcript retains the settled record.
    let turn = HarnessTurn::new(HarnessId::new("mock"), SessionId::new("s1"), "say hi");
    daemon.run(turn).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Issue the rewind request through the worker's shared handler.
    let mut rx = state.event_tx.subscribe();
    let handled = handle_daemon_desktop_message(
        &state,
        &DesktopMessage::Rewind {
            session_id: "s1".to_string(),
            at: 0,
        },
    )
    .unwrap();
    assert!(handled, "a rewind request must be recognized by the worker");

    let msg = rx.try_recv().unwrap();
    match msg {
        WorkerMessage::RewindResult { session_id, removed } => {
            assert_eq!(session_id, "s1");
            assert_eq!(removed, 1, "one recorded turn is dropped keeping `at = 0`");
        }
        other => panic!("expected RewindResult, got {other:?}"),
    }
}

/// A workflow run request is routed to the daemon's workflow host and the
/// resulting [`WorkflowRun`] is emitted on the worker's event channel.
#[test]
fn run_workflow_request_returns_a_workflow_run() {
    let state = AppState::new(WorkerId::generate());
    // Install a workflow host over a scripted MockStep and register a workflow.
    let daemon = state.daemon_state();
    daemon.install_workflow_host(Arc::new(WorkflowHost::new(MockStep::new())));
    daemon
        .register_workflow(
            Workflow::new(WorkflowId::new("build"), "Build & release")
                .with_step(Step::new("checkout", "Checkout source"))
                .with_step(Step::new("build", "Compile artifacts")),
        )
        .unwrap();

    let request = WorkflowHostRequest::new(
        0,
        "req-build-1",
        WorkflowId::new("build"),
        Trigger::Manual,
        serde_json::json!({}),
    );

    let mut rx = state.event_tx.subscribe();
    let handled = handle_daemon_desktop_message(
        &state,
        &DesktopMessage::RunWorkflow {
            request: serde_json::to_value(&request).unwrap(),
        },
    )
    .unwrap();
    assert!(handled, "a run_workflow request must be recognized by the worker");

    let msg = rx.try_recv().unwrap();
    match msg {
        WorkerMessage::WorkflowRun { run } => {
            let run: WorkflowRun = serde_json::from_value(run).unwrap();
            assert_eq!(run.workflow_id, WorkflowId::new("build"));
            assert_eq!(run.status, RunStatus::Succeeded);
            assert_eq!(run.steps.len(), 2);
            assert!(
                run.steps
                    .iter()
                    .all(|s| s.status == goble_workflow::StepStatus::Succeeded)
            );
        }
        other => panic!("expected WorkflowRun, got {other:?}"),
    }
}
