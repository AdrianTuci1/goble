//! The daemon domain: the app's seam to the daemon (local or remote).
//!
//! The GUI is a thin client over [`DaemonModel`], which owns the routing
//! decision and the [`DaemonClient`] that backs it. Local turns run on the
//! embedded daemon the desktop service owns; remote turns need a remote client
//! and otherwise fail loudly rather than silently degrading.

mod state;

pub use state::DaemonModel;

/// The daemon client boundary the overall app uses to drive chat turns.
///
/// Re-exported so callers (`runtime`, actions, root view) can name the type
/// without importing the crate directly.
pub use goble_daemon_client::DaemonClient;
