//! A [`DaemonEventSink`] that fans daemon events out to a broadcast channel.
//!
//! This is the embedded daemon bridge: the daemon's [`DaemonState`] is built
//! with a [`BroadcastSink`] wrapping the same [`tokio::sync::broadcast::Sender`]
//! the GUI-side [`crate::InProcessClient`] subscribes to, so live harness events
//! reach the UI without the daemon knowing anything about the GUI.

use goble_daemon::DaemonEventSink;
use goble_daemon_protocol::DaemonEvent;

/// A [`DaemonEventSink`] that forwards every event to a broadcast channel.
pub struct BroadcastSink {
    tx: tokio::sync::broadcast::Sender<DaemonEvent>,
}

impl BroadcastSink {
    pub fn new(tx: tokio::sync::broadcast::Sender<DaemonEvent>) -> Self {
        Self { tx }
    }

    /// The sender to hand to [`crate::InProcessClient::new`] for subscription.
    pub fn sender(&self) -> tokio::sync::broadcast::Sender<DaemonEvent> {
        self.tx.clone()
    }
}

impl DaemonEventSink for BroadcastSink {
    fn emit(&self, event: DaemonEvent) {
        let _ = self.tx.send(event);
    }
}
