//! GUI-side daemon client facade.
//!
//! The GUI never talks to `goble-daemon` internals: it holds a [`DaemonClient`].
//! For the embedded (in-process) daemon the concrete type is [`InProcessClient`],
//! which drives a local [`DaemonState`] directly and receives its events over a
//! broadcast channel (via [`BroadcastSink`]). Behind the `remote` feature a
//! [`WebSocketClient`] facade owns the GUI<->daemon wire codec for the
//! headless/remote case.
//!
//! Dependency direction: `app` → `goble-daemon-client` → `goble-daemon`.

mod client;
mod sink;

pub use client::{DaemonClient, InProcessClient};
pub use sink::BroadcastSink;

#[cfg(feature = "remote")]
pub use client::WebSocketClient;

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use goble_daemon::{Checkpoint, DaemonEventSink, DaemonPort, DaemonState, ExecutionRecord};
    use goble_daemon_protocol::DaemonEvent;
    use goble_harness_runtime::{HarnessRegistry, MockHarness};
    use goble_harness_types::{HarnessId, HarnessTurn, SessionId};
    use std::sync::Arc;

    #[derive(Default)]
    struct NoopPort;

    impl goble_daemon::DaemonPort for NoopPort {
        fn run(&self, _turn: HarnessTurn) -> Result<()> {
            Ok(())
        }
        fn resume(
            &self,
            _session_id: &SessionId,
            _response: &str,
            _credential: Option<(String, String)>,
        ) -> Result<()> {
            Ok(())
        }
        fn cancel(&self, _session_id: &SessionId) -> Result<()> {
            Ok(())
        }
        fn list_harnesses(&self) -> Vec<HarnessId> {
            vec![]
        }
        fn snapshot(&self) -> Vec<ExecutionRecord> {
            vec![]
        }
        fn run_workflow(&self, _request: goble_workflow::WorkflowHostRequest) -> Result<goble_workflow::WorkflowRun> {
            anyhow::bail!("noop port cannot run workflows")
        }
        fn rewind(&self, _session_id: &SessionId, _at: usize) -> Result<usize> {
            Ok(0)
        }
        fn fork(
            &self,
            _session_id: &SessionId,
            _at: usize,
            new_session_id: SessionId,
        ) -> Result<SessionId> {
            Ok(new_session_id)
        }
        fn replay(&self, _session_id: &SessionId, _at: usize) -> Result<Vec<DaemonEvent>> {
            Ok(vec![])
        }
        fn checkpoints(&self, _session_id: &SessionId) -> Result<Vec<Checkpoint>> {
            Ok(vec![])
        }
        fn select(&self, _session_id: &SessionId, _at: usize) -> Result<()> {
            Ok(())
        }
        fn apply(&self, _session_id: &SessionId, _at: usize) -> Result<usize> {
            Ok(0)
        }
        fn release(&self, _session_id: &SessionId, _at: usize) -> Result<bool> {
            Ok(false)
        }
        fn discard(&self, _session_id: &SessionId, _at: usize) -> Result<usize> {
            Ok(0)
        }
    }

    #[test]
    fn in_process_client_delegates_run() {
        let (tx, _rx) = tokio::sync::broadcast::channel(16);
        let client = InProcessClient::new(Arc::new(NoopPort), tx);

        // run delegates to the port and returns Ok without a harness.
        let turn = HarnessTurn::new(HarnessId::new("mock"), SessionId::new("s1"), "hi");
        assert!(client.run(turn).is_ok());
    }

    #[test]
    fn broadcast_sink_forwards_events() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let sink = BroadcastSink::new(tx);
        sink.emit(DaemonEvent::Done {
            session_id: SessionId::new("s1"),
        });
        let got = rx.try_recv().unwrap();
        assert!(matches!(got, DaemonEvent::Done { .. }));
    }

    #[tokio::test]
    async fn daemon_state_wires_via_broadcast_sink() {
        let registry = HarnessRegistry::new();
        registry.register(Arc::new(MockHarness::new(HarnessId::new("mock"), "hi")));
        let (tx, mut rx) = tokio::sync::broadcast::channel(64);
        let daemon = DaemonState::new(registry, Arc::new(BroadcastSink::new(tx)));

        let turn = HarnessTurn::new(HarnessId::new("mock"), SessionId::new("s1"), "hi");
        daemon.run(turn).unwrap();
        let seen = rx.recv().await.unwrap();
        assert!(matches!(
            seen,
            DaemonEvent::TraceStarted { .. }
                | DaemonEvent::AssistantDelta { .. }
                | DaemonEvent::TraceFinished { .. }
        ));
    }
}
