//! The seam between the daemon and any host that consumes its output.
//!
//! `goble-daemon` never emits Tauri events or writes to a WebSocket channel
//! directly. It calls a [`DaemonEventSink`]; the embedded wrapper supplies the
//! GUI event bus, and the remote worker supplies its broadcast channel. The
//! sink speaks the transport-agnostic [`DaemonEvent`] wire shape, which keeps
//! the daemon framework- and transport-agnostic.

use goble_daemon_protocol::DaemonEvent;

/// A consumer of daemon output, in the [`DaemonEvent`] shape.
///
/// Implemented by the embedded wrapper (GUI event bus) and by a remote worker
/// (broadcast channel / WebSocket).
pub trait DaemonEventSink: Send + Sync {
    fn emit(&self, event: DaemonEvent);
}

/// A sink that drops everything. Useful for tests and headless runs.
#[derive(Clone, Copy, Default)]
pub struct NoopSink;

impl DaemonEventSink for NoopSink {
    fn emit(&self, _event: DaemonEvent) {}
}
