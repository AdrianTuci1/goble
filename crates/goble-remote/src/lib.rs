//! Sandbox provider adapters for remote workers.
//!
//! A worker image runs inside a provider sandbox, connects out to the gateway,
//! and speaks the WebSocket protocol in `goble_core::protocol`. This crate owns
//! the provider side of that arrangement:
//!
//! - [`SandboxProvider`] is the object-safe seam. [`provider_for`] selects an
//!   implementation from a [`goble_core::config::RemoteAgentConfig`].
//! - [`E2BProvider`] talks to E2B's REST API.
//! - [`ModalProvider`] drives Modal's official Python SDK through a bundled
//!   bridge script, because Modal has no public REST API.
//! - [`DaytonaProvider`] talks to Daytona's REST API.
//! - [`BoxProvider`] talks to Box's REST API.
//! - [`DockerProvider`] runs the local Docker CLI.
//! - [`SshProvider`] installs and starts the worker on an existing host through
//!   the local `ssh` and `scp` executables.

pub mod r#box;
pub mod command;
pub mod daytona;
pub mod docker;
pub mod e2b;
pub mod error;
mod factory;
pub mod modal;
mod provider;
pub mod ssh;

pub use command::{CommandOutput, CommandRunner, ProcessCommandRunner};
pub use daytona::DaytonaProvider;
pub use docker::DockerProvider;
pub use e2b::E2BProvider;
pub use error::SandboxError;
pub use factory::provider_for;
pub use modal::ModalProvider;
pub use provider::{SandboxHandle, SandboxProvider, SandboxRequest, SandboxStatus};
pub use r#box::BoxProvider;
pub use ssh::SshProvider;
