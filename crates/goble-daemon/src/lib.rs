//! Framework-agnostic daemon core.
//!
//! This crate hosts the logic that both the embedded daemon (inside the GUI
//! binary) and the headless daemon (`goblin-worker` on a VPS) share: the
//! harness registry, the per-session execution ledger, and a transport-agnostic
//! event sink. It depends only on the harness seam crates and
//! `goble-daemon-protocol` — no UI, no Tauri, no transport, no `goble-core`.
//!
//! Dependency direction: `app` / `goble-desktop-service` → `goble-daemon` →
//! `goble-harness-*`; the single `goble-core` adapter lives in
//! `goble-harness-internal` and is wired in by a composition root.

pub mod port;
pub mod sink;
pub mod state;
pub mod workflow;

pub use goble_replay::Checkpoint;
pub use port::DaemonPort;
pub use sink::{DaemonEventSink, NoopSink};
pub use state::{DaemonState, ExecutionRecord};
pub use workflow::{HarnessStepExecutor, WorkflowHost};
