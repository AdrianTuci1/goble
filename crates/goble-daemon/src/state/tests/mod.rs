//! Unit tests for the daemon state, grouped by the surface they cover.

use crate::port::DaemonPort;
use goble_harness_protocol::HarnessServerEvent;
use goble_harness_runtime::{HarnessRun, MockHarness};
use goble_harness_types::{HarnessCapabilities, HarnessId, HarnessTurn};
use goble_replay::Checkpoint;

use super::*;

mod events;
mod reversal;
mod run;
mod workflow;
mod workspace;

/// A harness whose run never yields, so the daemon session stays live.
struct BlockingHarness {
    id: HarnessId,
}

impl BlockingHarness {
    fn new(id: HarnessId) -> Self {
        Self { id }
    }
}

impl HarnessRuntime for BlockingHarness {
    fn id(&self) -> HarnessId {
        self.id.clone()
    }
    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::internal()
    }
    fn run(&self, _turn: HarnessTurn, _cancel: Arc<AtomicBool>) -> HarnessRun {
        HarnessRun {
            events: Box::pin(futures::stream::pending()),
        }
    }
}

/// A harness whose run pauses on an `AskUser` then ends (a suspension), and
/// whose `resume` streams the answer then `Done`.
struct AskHarness {
    id: HarnessId,
}

impl AskHarness {
    fn new(id: HarnessId) -> Self {
        Self { id }
    }
}

impl HarnessRuntime for AskHarness {
    fn id(&self) -> HarnessId {
        self.id.clone()
    }
    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::internal()
    }
    fn run(&self, turn: HarnessTurn, _cancel: Arc<AtomicBool>) -> HarnessRun {
        let session_id = turn.session_id;
        HarnessRun {
            events: Box::pin(futures::stream::iter(vec![
                HarnessServerEvent::AskUser {
                    session_id: session_id.clone(),
                    question: "continue?".to_string(),
                    quick_replies: vec!["yes".to_string()],
                },
            ])),
        }
    }
    fn resume(
        &self,
        session_id: &SessionId,
        response: &str,
        _credential: Option<(String, String)>,
    ) -> Option<HarnessRun> {
        Some(HarnessRun {
            events: Box::pin(futures::stream::iter(vec![
                HarnessServerEvent::AssistantDelta {
                    session_id: session_id.clone(),
                    delta: response.to_string(),
                },
                HarnessServerEvent::Done {
                    session_id: session_id.clone(),
                },
            ])),
        })
    }
}

#[derive(Default)]
struct CollectingSink {
    events: Mutex<Vec<DaemonEvent>>,
}

impl DaemonEventSink for CollectingSink {
    fn emit(&self, event: DaemonEvent) {
        self.events.lock().unwrap().push(event);
    }
}

fn turn(session: &str) -> HarnessTurn {
    HarnessTurn::new(HarnessId::new("mock"), SessionId::new(session), "say hi")
}
