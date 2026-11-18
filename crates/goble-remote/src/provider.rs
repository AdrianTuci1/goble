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

/// A provider-side durable snapshot of a sandbox's filesystem.
///
/// The pair `(provider, snapshot_id)` is the whole identity: a snapshot id is
/// only meaningful to the adapter kind that minted it, so a binding that moves
/// to another provider cannot restore it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRef {
    /// The provider kind that owns the snapshot (`modal`, …).
    pub provider: String,
    /// The provider's own snapshot/image id.
    pub snapshot_id: String,
}

impl SnapshotRef {
    /// A snapshot of `provider`'s filesystem.
    pub fn new(provider: impl Into<String>, snapshot_id: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            snapshot_id: snapshot_id.into(),
        }
    }

    /// Whether the reference names anything (a blank id is not a snapshot).
    pub fn is_filled(&self) -> bool {
        !self.snapshot_id.trim().is_empty()
    }
}

/// What an adapter can preserve past a sandbox's lifetime.
///
/// This is the honest per-provider answer the provisioner and the docs read:
/// only [`SnapshotSupport::Filesystem`] restores a filesystem into a *new*
/// sandbox from a snapshot id; the others keep some state without being able to
/// hand a snapshot to another sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotSupport {
    /// The provider can snapshot the sandbox filesystem and create a new
    /// sandbox from that snapshot (Modal through its Python SDK).
    Filesystem,
    /// The provider keeps the sandbox itself paused and resumes the same
    /// sandbox on demand, with no separate snapshot object (E2B pause/resume).
    PauseResume,
    /// The workspace is a durable volume or directory that outlives the
    /// sandbox, but the provider hands out no snapshot (Docker named volume,
    /// SSH install directory).
    Workspace,
    /// Neither: a new sandbox starts from the image. The harness conversation
    /// can still resume from its native session id, but files written in the
    /// dead sandbox are gone (Cloud Run, Daytona and Box today).
    None,
}

impl SnapshotSupport {
    /// The word the deploy report and the docs use for this level.
    pub fn as_str(&self) -> &'static str {
        match self {
            SnapshotSupport::Filesystem => "filesystem",
            SnapshotSupport::PauseResume => "pause_resume",
            SnapshotSupport::Workspace => "workspace",
            SnapshotSupport::None => "none",
        }
    }

    /// Whether the provider preserves anything past a sandbox's lifetime.
    pub fn is_durable(&self) -> bool {
        !matches!(self, SnapshotSupport::None)
    }
}

/// The durable state of one project's sandbox: what lets a session outlive it.
///
/// A session is a binding, not a process: the snapshot id names the filesystem
/// the next sandbox starts from, and the native session id names the harness
/// conversation the next worker resumes. Both are optional and independent —
/// a provider may have one, the other, both or neither.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SandboxBinding {
    /// The provider kind the binding is for (`modal`, `docker`, …).
    pub provider: String,
    /// The stable name the request asked for, per project.
    pub sandbox_name: String,
    /// The provider's handle for the sandbox that exists now, when one does.
    pub sandbox_id: Option<String>,
    /// The last filesystem snapshot of the sandbox, when the provider takes one.
    pub snapshot: Option<SnapshotRef>,
    /// The harness conversation to resume (`--session-id` for a DSH harness).
    pub native_session_id: Option<String>,
    /// Where the checkout lives inside the sandbox (`/workspace/<slug>`).
    pub workspace_path: String,
}

impl SandboxBinding {
    /// A binding for a project's stable sandbox name and workspace path.
    pub fn new(
        provider: impl Into<String>,
        sandbox_name: impl Into<String>,
        workspace_path: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            sandbox_name: sandbox_name.into(),
            workspace_path: workspace_path.into(),
            ..Self::default()
        }
    }

    /// The binding a provider handle implies, before any snapshot or session
    /// id is known.
    pub fn from_handle(handle: &SandboxHandle) -> Self {
        Self {
            provider: handle.provider.clone(),
            sandbox_name: handle.sandbox_name.clone(),
            sandbox_id: Some(handle.sandbox_id.clone()),
            snapshot: None,
            native_session_id: None,
            workspace_path: handle.workspace_path.clone(),
        }
    }

    /// The snapshot for `provider`, when the binding holds one it can restore.
    pub fn snapshot_for(&self, provider: &str) -> Option<&SnapshotRef> {
        self.snapshot
            .as_ref()
            .filter(|snapshot| snapshot.provider == provider && snapshot.is_filled())
    }

    /// A binding is resumable when it names something to restore: a snapshot,
    /// a sandbox the provider may still know, or a harness conversation.
    pub fn is_resumable(&self) -> bool {
        self.snapshot_for(&self.provider).is_some()
            || self
                .sandbox_id
                .as_deref()
                .is_some_and(|id| !id.trim().is_empty())
            || self
                .native_session_id
                .as_deref()
                .is_some_and(|id| !id.trim().is_empty())
    }
}

/// What a teardown left behind.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReleaseOutcome {
    /// The filesystem snapshot taken before teardown, when the provider takes
    /// one. The binding keeps it so the next sandbox restores it.
    pub snapshot: Option<SnapshotRef>,
    /// Whether the provider kept the sandbox itself under the same id (paused,
    /// stopped or archived), so the binding must keep its `sandbox_id` and the
    /// next run resumes that sandbox instead of creating one.
    pub sandbox_kept: bool,
}

impl ReleaseOutcome {
    /// A teardown that snapshotted and destroyed.
    pub fn destroyed(snapshot: Option<SnapshotRef>) -> Self {
        Self {
            snapshot,
            sandbox_kept: false,
        }
    }

    /// A teardown that left the sandbox itself alive but stopped.
    pub fn kept() -> Self {
        Self {
            snapshot: None,
            sandbox_kept: true,
        }
    }
}

/// One way to create, destroy, and inspect worker sandboxes.
///
/// Implementations are object-safe so [`crate::provider_for`] can return the
/// adapter a config table selects. A new provider plugs in by implementing
/// this trait and adding one match arm to the factory.
///
/// The snapshot half of the trait is optional and additive on purpose: every
/// method has a default that means "this provider has no snapshots", so an
/// adapter that was written before durable sessions existed keeps its exact
/// behaviour. [`snapshot_support`] is the machine-readable version of that
/// answer, [`release`] is the teardown that keeps state where the provider can,
/// and [`resume`] is the create path that restores it.
///
/// [`snapshot_support`]: SandboxProvider::snapshot_support
/// [`release`]: SandboxProvider::release
/// [`resume`]: SandboxProvider::resume
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

    /// What this adapter can preserve past a sandbox's lifetime.
    ///
    /// The default is [`SnapshotSupport::None`]: a provider that never
    /// implemented snapshots behaves exactly as it did before this method
    /// existed.
    fn snapshot_support(&self) -> SnapshotSupport {
        SnapshotSupport::None
    }

    /// Snapshot the sandbox's filesystem.
    ///
    /// `Ok(None)` means the provider has no filesystem snapshot capability (or
    /// the sandbox is already gone); it is not an error. A provider that does
    /// support snapshots returns the reference or the error the provider gave.
    async fn snapshot(&self, _handle: &SandboxHandle) -> Result<Option<SnapshotRef>, SandboxError> {
        Ok(None)
    }

    /// Create a new sandbox whose filesystem starts as `snapshot`.
    ///
    /// The default is [`SandboxError::Unsupported`]: an adapter cannot restore
    /// a snapshot it never takes, and it says so instead of silently starting
    /// from the image.
    async fn create_from_snapshot(
        &self,
        _request: &SandboxRequest,
        snapshot: &SnapshotRef,
    ) -> Result<SandboxHandle, SandboxError> {
        Err(SandboxError::Unsupported {
            provider: self.kind().to_string(),
            message: format!(
                "cannot create a sandbox from snapshot {}; start from the image instead",
                snapshot.snapshot_id
            ),
        })
    }

    /// Create a sandbox for a binding: restore its snapshot when the provider
    /// can, otherwise create a fresh one.
    ///
    /// The snapshot is only used when it belongs to this adapter's provider
    /// kind, so a binding re-pointed at a different provider starts cleanly.
    async fn resume(
        &self,
        request: &SandboxRequest,
        binding: &SandboxBinding,
    ) -> Result<SandboxHandle, SandboxError> {
        match binding.snapshot_for(self.kind()) {
            Some(snapshot) => self.create_from_snapshot(request, snapshot).await,
            None => self.create(request).await,
        }
    }

    /// Tear the sandbox down while keeping what can be kept.
    ///
    /// The default snapshots first (a provider with no snapshots answers
    /// `Ok(None)`) and then destroys, both steps attempted even when the
    /// snapshot failed: the caller asked for a teardown, and a snapshot failure
    /// is the error that comes back. An adapter whose provider keeps state
    /// itself — a paused sandbox, a stopped process with its directory — calls
    /// [`ReleaseOutcome::kept`] instead and skips the destroy.
    async fn release(&self, handle: &SandboxHandle) -> Result<ReleaseOutcome, SandboxError> {
        let snapshot = self.snapshot(handle).await;
        let destroyed = self.destroy(handle).await;
        match (snapshot, destroyed) {
            (Ok(snapshot), Ok(())) => Ok(ReleaseOutcome::destroyed(snapshot)),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
    }
}
