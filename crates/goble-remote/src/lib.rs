//! Sandbox provider adapters for remote workers.
//!
//! A worker image runs inside a provider sandbox, connects out to the gateway,
//! and speaks the WebSocket protocol in `goble_core::protocol`. This crate owns
//! the provider side of that arrangement:
//!
//! - [`SandboxProvider`] is the object-safe seam. [`provider_for`] selects an
//!   implementation from a [`goble_core::config::RemoteAgentConfig`].
//! - [`E2BProvider`] talks to E2B's REST API; its sandboxes pause and resume
//!   natively, so the binding keeps the sandbox id.
//! - [`ModalProvider`] drives Modal's official Python SDK through a bundled
//!   bridge script, because Modal has no public REST API. It is the adapter with
//!   real filesystem snapshots: [`SandboxProvider::snapshot`] and
//!   [`SandboxProvider::create_from_snapshot`] map onto the SDK's
//!   `snapshot_filesystem()` and `Image.from_id()`.
//! - [`DaytonaProvider`] talks to Daytona's REST API.
//! - [`BoxProvider`] talks to Box's REST API.
//! - [`CloudRunProvider`] runs one-shot Cloud Run jobs, authenticated with a
//!   bearer token or the machine's `gcloud` session.
//! - [`DockerProvider`] runs the local Docker CLI, with the workspace on a named
//!   volume so it outlives the container.
//! - [`SshProvider`] installs and starts the worker on an existing host through
//!   the local `ssh` and `scp` executables.
//!
//! A durable session is a [`SandboxBinding`], not a process: the binding carries
//! the provider's sandbox id, the last [`SnapshotRef`], and the harness's native
//! session id, so a terminated sandbox can be recreated from the snapshot and
//! the conversation resumed. [`SandboxLifecycle`] carries the configurable idle
//! window and close behaviour the provisioner applies.

pub mod r#box;
pub mod cloud_run;
pub mod command;
pub mod daytona;
pub mod docker;
pub mod e2b;
pub mod error;
mod factory;
mod lifecycle;
pub mod modal;
mod provider;
pub mod ssh;

pub use cloud_run::CloudRunProvider;
pub use command::{CommandOutput, CommandRunner, ProcessCommandRunner};
pub use daytona::DaytonaProvider;
pub use docker::DockerProvider;
pub use e2b::E2BProvider;
pub use error::SandboxError;
pub use factory::provider_for;
pub use lifecycle::{
    SandboxLifecycle, DEFAULT_IDLE_TIMEOUT_SECS, IDLE_TIMEOUT_ENV, SNAPSHOT_ON_CLOSE_ENV,
};
pub use modal::ModalProvider;
pub use provider::{
    ReleaseOutcome, SandboxBinding, SandboxHandle, SandboxProvider, SandboxRequest, SandboxStatus,
    SnapshotRef, SnapshotSupport,
};
pub use r#box::BoxProvider;
pub use ssh::SshProvider;
