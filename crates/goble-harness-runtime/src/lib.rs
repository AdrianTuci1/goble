//! Execution contract for a harness.
//!
//! This is the `runtime` layer of the `types <- protocol <- runtime` split. It
//! owns the object-safe [`HarnessRuntime`] trait plus [`HarnessRun`]/[`HarnessRegistry`].
//! A harness (internal or an external CLI) is *any* type implementing this trait;
//! the host drives it through the trait and never sees transport details.
//!
//! This crate is framework-agnostic: no `wgpu`/`winit`, no UI. It is compiled
//! into the daemon, which is deployed either embedded in the GUI or headless on
//! a remote machine.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use futures::Stream;
use goble_harness_protocol::HarnessServerEvent;
use goble_harness_types::{HarnessCapabilities, HarnessId, HarnessSnapshot, HarnessTurn, SessionId};

/// A stream of events a harness produces for one run.
pub struct HarnessRun {
    pub events: Pin<Box<dyn Stream<Item = HarnessServerEvent> + Send>>,
}

/// An object-safe contract a harness implements. `run` is called once per turn;
/// the returned stream is consumed by the host, which interprets the events.
///
/// [`Self::cancel`] and [`Self::resume`] have built-in defaults so a harness
/// that never pauses (or that cancels through its own flag) does not have to
/// implement them. [`Self::cancel`] is invoked by the daemon to stop a running
/// turn; [`Self::resume`] continues a turn that suspended on a user answer.
pub trait HarnessRuntime: Send + Sync {
    fn id(&self) -> HarnessId;
    fn capabilities(&self) -> HarnessCapabilities;
    fn run(&self, turn: HarnessTurn, cancel: Arc<AtomicBool>) -> HarnessRun;

    /// Cancel a running turn. The default is a no-op; a harness that drives its
    /// own cancellation flag should override it.
    fn cancel(&self) {}

    /// Resume a turn that suspended on a user answer. `credential` carries an
    /// optional SSH-style credential the harness may use. Returns `None` if the
    /// harness does not support resumption.
    fn resume(
        &self,
        session_id: &SessionId,
        response: &str,
        credential: Option<(String, String)>,
    ) -> Option<HarnessRun> {
        let _ = (session_id, response, credential);
        None
    }

    /// Capture the harness's execution state for a session, so the daemon can
    /// rewind to that point without re-executing. Returns `None` if the harness
    /// does not support environment-level snapshots; reversibility then falls
    /// back to the transcript-only ledger (which works for every harness).
    fn snapshot(&self, _session_id: &SessionId) -> Option<HarnessSnapshot> {
        None
    }

    /// Restore a previously captured snapshot and resume from that point.
    /// Returns the event stream the host should drive, or `None` if the harness
    /// does not support restore.
    fn restore(&self, _session_id: &SessionId, _snapshot: &HarnessSnapshot) -> Option<HarnessRun> {
        None
    }

    /// Point the harness's per-run agent workspace at `dir`.
    ///
    /// The daemon calls this before [`Self::run`] when a per-session agent
    /// workspace is configured, having already created `dir`. This is the
    /// harness-agnostic seam: a concrete harness that supports a per-run
    /// workspace overrides it (redirecting its agent workspace to `dir` and
    /// returning `true`); any harness that does not keep the default, which
    /// returns `false`. The daemon ignores the returned flag and simply leaves
    /// unsupporting harnesses untouched.
    fn set_workspace_dir(&self, dir: &std::path::Path) -> bool {
        let _ = dir;
        false
    }
}

/// A registry of harnesses, keyed by id. Cloneable and shareable across threads
/// (interior mutability). The daemon seeds it with the internal harness and any
/// discovered external harnesses; the GUI selects one by id.
#[derive(Clone, Default)]
pub struct HarnessRegistry {
    inner: Arc<RwLock<HashMap<HarnessId, Arc<dyn HarnessRuntime>>>>,
}

impl HarnessRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, harness: Arc<dyn HarnessRuntime>) {
        self.inner
            .write()
            .unwrap()
            .insert(harness.id(), harness);
    }

    pub fn resolve(&self, id: &HarnessId) -> Option<Arc<dyn HarnessRuntime>> {
        self.inner.read().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<HarnessId> {
        self.inner
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>()
    }
}

/// A scripted harness for tests and demos: emits one assistant delta then `Done`.
pub struct MockHarness {
    id: HarnessId,
    reply: String,
}

impl MockHarness {
    pub fn new(id: HarnessId, reply: &str) -> Self {
        Self {
            id,
            reply: reply.to_string(),
        }
    }
}

impl HarnessRuntime for MockHarness {
    fn id(&self) -> HarnessId {
        self.id.clone()
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities::internal()
    }

    fn run(&self, turn: HarnessTurn, _cancel: Arc<AtomicBool>) -> HarnessRun {
        let reply = self.reply.clone();
        let session_id = turn.session_id;
        let events = vec![
            HarnessServerEvent::AssistantDelta {
                session_id: session_id.clone(),
                delta: reply,
            },
            HarnessServerEvent::Done { session_id },
        ];
        HarnessRun {
            events: Box::pin(futures::stream::iter(events)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use goble_harness_types::SessionId;

    #[tokio::test]
    async fn mock_harness_emits_delta_then_done() {
        let harness = MockHarness::new(HarnessId::new("mock"), "hi");
        let mut run = harness.run(
            HarnessTurn::new(HarnessId::new("mock"), SessionId::new("s1"), "goal"),
            Arc::new(AtomicBool::new(false)),
        );
        let mut seen = Vec::new();
        while let Some(ev) = run.events.next().await {
            seen.push(ev);
        }
        assert_eq!(seen.len(), 2);
        assert!(matches!(
            &seen[0],
            HarnessServerEvent::AssistantDelta { delta, .. } if delta == "hi"
        ));
        assert!(matches!(&seen[1], HarnessServerEvent::Done { .. }));
    }

    #[test]
    fn registry_resolves_by_id() {
        let reg = HarnessRegistry::new();
        reg.register(Arc::new(MockHarness::new(HarnessId::new("a"), "x")));
        reg.register(Arc::new(MockHarness::new(HarnessId::new("b"), "y")));

        assert_eq!(reg.list().len(), 2);
        let resolved = reg.resolve(&HarnessId::new("a")).unwrap();
        assert_eq!(resolved.id(), HarnessId::new("a"));
        assert!(reg.resolve(&HarnessId::new("nope")).is_none());
    }

    /// A harness that keeps a counter in its state and can snapshot/restore it —
    /// a stand-in for a BYOH harness that offers environment-level reversibility.
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
    async fn reversible_harness_snapshots_and_restores_state() {
        let harness = ReversibleHarness::new(HarnessId::new("rev"));
        assert!(harness.capabilities().reversible);

        // run mutates the counter.
        let mut run = harness.run(
            HarnessTurn::new(HarnessId::new("rev"), SessionId::new("s1"), "x"),
            Arc::new(AtomicBool::new(false)),
        );
        while let Some(ev) = run.events.next().await {
            if let HarnessServerEvent::AssistantDelta { delta, .. } = ev {
                assert_eq!(delta, "count=1");
            }
        }
        assert_eq!(*harness.counter.lock().unwrap(), 1);

        // Capture a snapshot, then diverge the state.
        let snap = harness.snapshot(&SessionId::new("s1")).unwrap();
        *harness.counter.lock().unwrap() = 99;

        // Restore rolls the state back to the snapshot.
        let mut run = harness.restore(&SessionId::new("s1"), &snap).unwrap();
        while let Some(ev) = run.events.next().await {
            if let HarnessServerEvent::AssistantDelta { delta, .. } = ev {
                assert_eq!(delta, "restored=1");
            }
        }
        assert_eq!(*harness.counter.lock().unwrap(), 1);
    }
}
