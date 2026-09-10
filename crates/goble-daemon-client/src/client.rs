//! The [`DaemonClient`] facade and its concrete implementations.

use std::sync::Arc;

use anyhow::Result;
use goble_daemon::{Checkpoint, DaemonPort, ExecutionRecord};
use goble_daemon_protocol::DaemonEvent;
use goble_harness_types::{HarnessId, HarnessTurn, SessionId};

#[cfg(feature = "remote")]
use goble_daemon_protocol::{DaemonMessage, DaemonRequest};

/// GUI-side facade for driving a daemon, wherever it runs.
///
/// Both the embedded daemon (`InProcessClient`) and a remote daemon expose the
/// same boundary. `subscribe` returns a stream of [`DaemonEvent`]s the GUI uses
/// to render live state; the specific event routing differs per implementation.
pub trait DaemonClient: Send + Sync {
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

    // --- reversibility (harness-agnostic) ---
    fn rewind(&self, session_id: &SessionId, at: usize) -> Result<usize>;
    fn fork(
        &self,
        session_id: &SessionId,
        at: usize,
        new_session_id: SessionId,
    ) -> Result<SessionId>;
    fn replay(&self, session_id: &SessionId, at: usize) -> Result<Vec<DaemonEvent>>;
    fn checkpoints(&self, session_id: &SessionId) -> Result<Vec<Checkpoint>>;

    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DaemonEvent>;
}

/// In-process client: drives a local [`DaemonState`] directly and receives its
/// events over a broadcast channel.
pub struct InProcessClient {
    port: Arc<dyn DaemonPort>,
    events: tokio::sync::broadcast::Sender<DaemonEvent>,
}

impl InProcessClient {
    /// Wrap a local [`DaemonState`] (as a [`DaemonPort`]) plus the broadcast
    /// sender that the same daemon was built with (via [`crate::BroadcastSink`]).
    pub fn new(
        port: Arc<dyn DaemonPort>,
        events: tokio::sync::broadcast::Sender<DaemonEvent>,
    ) -> Self {
        Self { port, events }
    }
}

impl DaemonClient for InProcessClient {
    fn run(&self, turn: HarnessTurn) -> Result<()> {
        self.port.run(turn)
    }

    fn resume(
        &self,
        session_id: &SessionId,
        response: &str,
        credential: Option<(String, String)>,
    ) -> Result<()> {
        self.port.resume(session_id, response, credential)
    }

    fn cancel(&self, session_id: &SessionId) -> Result<()> {
        self.port.cancel(session_id)
    }

    fn list_harnesses(&self) -> Vec<HarnessId> {
        self.port.list_harnesses()
    }

    fn snapshot(&self) -> Vec<ExecutionRecord> {
        self.port.snapshot()
    }

    fn rewind(&self, session_id: &SessionId, at: usize) -> Result<usize> {
        self.port.rewind(session_id, at)
    }

    fn fork(
        &self,
        session_id: &SessionId,
        at: usize,
        new_session_id: SessionId,
    ) -> Result<SessionId> {
        self.port.fork(session_id, at, new_session_id)
    }

    fn replay(&self, session_id: &SessionId, at: usize) -> Result<Vec<DaemonEvent>> {
        self.port.replay(session_id, at)
    }

    fn checkpoints(&self, session_id: &SessionId) -> Result<Vec<Checkpoint>> {
        self.port.checkpoints(session_id)
    }

    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DaemonEvent> {
        self.events.subscribe()
    }
}

/// Remote (WebSocket) client facade. Owns the GUI<->daemon wire codec: requests
/// are framed as [`DaemonMessage::Request`] and pushed to the request channel,
/// while received [`DaemonMessage::Event`] frames are fanned into the event
/// broadcast via [`Self::on_frame`].
///
/// This is the codec/ownership layer only — the actual socket is injected by the
/// caller, which drives [`Self::on_frame`] from the transport's reader and calls
/// [`Self::send_request`] from its writer. Request/response methods are not yet
/// wired to a live transport; use it via the codec surface.
#[cfg(feature = "remote")]
pub struct WebSocketClient {
    request_tx: tokio::sync::mpsc::Sender<DaemonMessage>,
    events: tokio::sync::broadcast::Sender<DaemonEvent>,
}

#[cfg(feature = "remote")]
impl WebSocketClient {
    pub fn new(
        request_tx: tokio::sync::mpsc::Sender<DaemonMessage>,
        events: tokio::sync::broadcast::Sender<DaemonEvent>,
    ) -> Self {
        Self { request_tx, events }
    }

    /// Frame and send a request toward the daemon.
    pub fn send_request(&self, req: DaemonRequest) -> Result<()> {
        self.request_tx
            .try_send(DaemonMessage::Request(req))
            .map_err(|e| anyhow::anyhow!("daemon request channel closed: {e}"))
    }

    /// Subscribe to the event stream.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DaemonEvent> {
        self.events.subscribe()
    }

    /// Feed a frame read off the wire back into the event stream.
    pub fn on_frame(&self, frame: DaemonMessage) {
        if let DaemonMessage::Event(ev) = frame {
            let _ = self.events.send(ev);
        }
    }
}
