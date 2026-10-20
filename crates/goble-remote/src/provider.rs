use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::SandboxError;

/// Everything a provider needs to start one worker sandbox.
///
/// The caller fills `env` with the worker's connection settings (gateway URL,
/// worker token, worker id, repository and branch, workspace path); the adapter
/// passes them through in whatever form the provider understands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxRequest {
    /// The durable project this sandbox belongs to.
    pub project_id: String,
    /// The stable provider-visible name from [`goble_core::workspace_identity`].
    pub sandbox_name: String,
    /// Where the checkout lives inside the sandbox (`/workspace/<slug>`).
    pub workspace_path: String,
    /// The image that carries the worker.
    pub image: String,
    /// An explicit command; empty means the image entrypoint.
    pub command: Vec<String>,
    /// Environment variables for the worker.
    pub env: BTreeMap<String, String>,
    /// How long the provider keeps the sandbox alive, in seconds.
    pub timeout_secs: u64,
    /// Provider labels/tags carried on the sandbox.
    pub metadata: BTreeMap<String, String>,
}

/// The provider's receipt for a created sandbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxHandle {
    /// The provider kind (`e2b`, `modal`, `daytona`, `box`, `docker`, `ssh`).
    pub provider: String,
    /// The provider's own identifier.
    pub sandbox_id: String,
    /// The stable name the request asked for.
    pub sandbox_name: String,
    /// Where the checkout lives inside the sandbox.
    pub workspace_path: String,
}

impl SandboxHandle {
    /// Build a handle from the request that created it.
    pub fn from_request(
        provider: impl Into<String>,
        sandbox_id: impl Into<String>,
        request: &SandboxRequest,
    ) -> Self {
        Self {
            provider: provider.into(),
            sandbox_id: sandbox_id.into(),
            sandbox_name: request.sandbox_name.clone(),
            workspace_path: request.workspace_path.clone(),
        }
    }
}

/// What the provider says about a sandbox right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxStatus {
    /// The sandbox exists and is running.
    Running,
    /// The sandbox exists but is not running.
    Paused,
    /// The provider has no sandbox with that identifier.
    Missing,
    /// The provider answered in a way the adapter does not model.
    Unknown,
}

/// One way to create, destroy, and inspect worker sandboxes.
///
/// Implementations are object-safe so [`crate::provider_for`] can return the
/// adapter a config table selects. A new provider plugs in by implementing
/// this trait and adding one match arm to the factory.
#[async_trait]
pub trait SandboxProvider: Send + Sync {
    /// The provider kind, matching [`goble_core::config::RemoteAgentProvider::as_str`].
    fn kind(&self) -> &'static str;

    /// Create the sandbox and return its provider handle.
    async fn create(&self, request: &SandboxRequest) -> Result<SandboxHandle, SandboxError>;

    /// Destroy the sandbox. Implementations may treat an already-missing
    /// sandbox as success.
    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError>;

    /// Ask the provider whether the sandbox still exists.
    async fn status(&self, handle: &SandboxHandle) -> Result<SandboxStatus, SandboxError>;
}
