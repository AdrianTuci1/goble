use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::command::{CommandRunner, ProcessCommandRunner};
use crate::error::SandboxError;
use crate::provider::{
    ReleaseOutcome, SandboxBinding, SandboxHandle, SandboxProvider, SandboxRequest, SandboxStatus,
    SnapshotRef, SnapshotSupport,
};

const PROVIDER: &str = "modal";
const DEFAULT_PYTHON: &str = "python3";
const PYTHON_OVERRIDE: &str = "GOBLE_MODAL_PYTHON";
const BRIDGE_OVERRIDE: &str = "GOBLE_MODAL_BRIDGE";

/// Modal through its official Python SDK.
///
/// Modal exposes no public REST API, so the adapter runs
/// `scripts/modal_sandbox.py` with a JSON payload on stdin. The interpreter and
/// script are overridable with `GOBLE_MODAL_PYTHON` and `GOBLE_MODAL_BRIDGE`.
///
/// Modal is the one adapter with real filesystem snapshots: a teardown through
/// [`SandboxProvider::release`] snapshots and terminates in a single bridge
/// process (`snapshot_filesystem()` then `terminate()`), and
/// [`SandboxProvider::create_from_snapshot`] starts the replacement from the
/// recorded snapshot id with `modal.Image.from_id()`. The whole sandbox
/// filesystem — `/workspace`, the worker's home, the harness state — is what
/// the snapshot captures, which is exactly what a resumed conversation needs.
pub struct ModalProvider {
    token_id: String,
    token_secret: String,
    python: String,
    bridge: PathBuf,
    runner: Arc<dyn CommandRunner>,
}

impl ModalProvider {
    /// Build an adapter around an already-resolved runner.
    pub fn new(
        token_id: impl Into<String>,
        token_secret: impl Into<String>,
        runner: Arc<dyn CommandRunner>,
    ) -> Self {
        Self {
            token_id: token_id.into(),
            token_secret: token_secret.into(),
            python: default_python(),
            bridge: default_bridge(),
            runner,
        }
    }

    /// Build from a `[remote_agents.<name>]` table.
    pub fn from_config(
        config: &goble_core::config::RemoteAgentConfig,
    ) -> Result<Self, SandboxError> {
        let token_id = required_credential(config.token_id.as_deref(), "MODAL_TOKEN_ID")?;
        let token_secret =
            required_credential(config.token_secret.as_deref(), "MODAL_TOKEN_SECRET")?;
        Ok(Self::new(
            token_id,
            token_secret,
            Arc::new(ProcessCommandRunner),
        ))
    }

    /// Override the Python interpreter (tests use this to stay deterministic).
    pub fn with_python(mut self, python: impl Into<String>) -> Self {
        self.python = python.into();
        self
    }

    /// Override the bridge script path.
    pub fn with_bridge(mut self, bridge: impl Into<PathBuf>) -> Self {
        self.bridge = bridge.into();
        self
    }

    async fn bridge(&self, action: &str, payload: &impl Serialize) -> Result<String, SandboxError> {
        let stdin = serde_json::to_string(payload).map_err(|error| SandboxError::Parse {
            context: "Modal bridge request",
            message: error.to_string(),
        })?;
        let args = vec![self.bridge.display().to_string(), action.to_string()];
        let env = BTreeMap::from([
            ("MODAL_TOKEN_ID".to_string(), self.token_id.clone()),
            ("MODAL_TOKEN_SECRET".to_string(), self.token_secret.clone()),
        ]);
        let output = self
            .runner
            .run(&self.python, &args, Some(&stdin), &env)
            .await?;
        if !output.success() {
            return Err(SandboxError::Command {
                command: format!("modal {action}"),
                message: output.failure_message(),
            });
        }
        Ok(output.stdout)
    }

    /// Create the sandbox, optionally from a filesystem snapshot. The bridge
    /// chooses `Image.from_id(snapshot)` over the request's image when a
    /// snapshot is named.
    async fn create_inner(
        &self,
        request: &SandboxRequest,
        snapshot: Option<&SnapshotRef>,
    ) -> Result<SandboxHandle, SandboxError> {
        let snapshot_id = snapshot
            .map(|snapshot| snapshot.snapshot_id.trim())
            .filter(|id| !id.is_empty());
        let payload = CreatePayload {
            app_name: &request.sandbox_name,
            image: &request.image,
            command: &request.command,
            env: &request.env,
            timeout_secs: request.timeout_secs,
            snapshot_id,
        };
        let stdout = self.bridge("create", &payload).await?;
        let created: CreatedSandbox = parse_json(&stdout, "Modal create response")?;
        let sandbox_id = created.sandbox_id.trim();
        if sandbox_id.is_empty() {
            return Err(SandboxError::Parse {
                context: "Modal create response",
                message: "sandbox_id is empty".to_string(),
            });
        }
        Ok(SandboxHandle::from_request(PROVIDER, sandbox_id, request))
    }

    /// The snapshot id one bridge answer names, when it names one.
    ///
    /// A `null` or blank id means the bridge found nothing to snapshot (the
    /// sandbox was already gone or stopped), which is not an error.
    fn snapshot_from(payload: SnapshotPayload) -> Option<SnapshotRef> {
        let id = payload.snapshot_id?;
        let id = id.trim();
        if id.is_empty() {
            return None;
        }
        Some(SnapshotRef::new(PROVIDER, id))
    }
}

fn required_credential(value: Option<&str>, key: &'static str) -> Result<String, SandboxError> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or(SandboxError::MissingCredential {
            provider: PROVIDER,
            key,
        })
}

fn default_python() -> String {
    std::env::var(PYTHON_OVERRIDE)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_PYTHON.to_string())
}

fn default_bridge() -> PathBuf {
    std::env::var_os(BRIDGE_OVERRIDE)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("scripts")
                .join("modal_sandbox.py")
        })
}

#[derive(Debug, Serialize)]
struct CreatePayload<'a> {
    app_name: &'a str,
    image: &'a str,
    command: &'a [String],
    env: &'a BTreeMap<String, String>,
    timeout_secs: u64,
    /// Present only when the new sandbox restores a filesystem snapshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct SandboxIdPayload<'a> {
    sandbox_id: &'a str,
}

#[derive(Debug, Deserialize)]
struct CreatedSandbox {
    sandbox_id: String,
}

#[derive(Debug, Deserialize)]
struct StatusPayload {
    status: String,
}

#[derive(Debug, Deserialize)]
struct SnapshotPayload {
    #[serde(default)]
    snapshot_id: Option<String>,
}

fn parse_json<T: DeserializeOwned>(text: &str, context: &'static str) -> Result<T, SandboxError> {
    serde_json::from_str(text.trim()).map_err(|error| SandboxError::Parse {
        context,
        message: format!("{error}: {text}"),
    })
}

#[async_trait]
impl SandboxProvider for ModalProvider {
    fn kind(&self) -> &'static str {
        PROVIDER
    }

    /// Modal hands out real filesystem snapshots (`Sandbox.snapshot_filesystem`).
    fn snapshot_support(&self) -> SnapshotSupport {
        SnapshotSupport::Filesystem
    }

    async fn create(&self, request: &SandboxRequest) -> Result<SandboxHandle, SandboxError> {
        self.create_inner(request, None).await
    }

    async fn create_from_snapshot(
        &self,
        request: &SandboxRequest,
        snapshot: &SnapshotRef,
    ) -> Result<SandboxHandle, SandboxError> {
        if !snapshot.is_filled() {
            return Err(SandboxError::InvalidRequest {
                message: "a Modal resume needs a non-empty snapshot id".to_string(),
            });
        }
        self.create_inner(request, Some(snapshot)).await
    }

    /// Resume the project's sandbox: restore the recorded snapshot when the
    /// binding has one, otherwise start from the request's image.
    async fn resume(
        &self,
        request: &SandboxRequest,
        binding: &SandboxBinding,
    ) -> Result<SandboxHandle, SandboxError> {
        match binding.snapshot_for(PROVIDER) {
            Some(snapshot) => self.create_from_snapshot(request, snapshot).await,
            None => self.create(request).await,
        }
    }

    /// Snapshot the sandbox filesystem. `Ok(None)` when Modal no longer knows
    /// the sandbox, or when it has already stopped and cannot be snapshotted.
    async fn snapshot(&self, handle: &SandboxHandle) -> Result<Option<SnapshotRef>, SandboxError> {
        let payload = SandboxIdPayload {
            sandbox_id: &handle.sandbox_id,
        };
        let stdout = self.bridge("snapshot", &payload).await?;
        let snapshot: SnapshotPayload = parse_json(&stdout, "Modal snapshot response")?;
        Ok(Self::snapshot_from(snapshot))
    }

    /// Snapshot and terminate in one bridge process, so an idle close cannot
    /// lose the snapshot to a half-finished teardown. A sandbox that is already
    /// gone reports no snapshot and is not an error.
    async fn release(&self, handle: &SandboxHandle) -> Result<ReleaseOutcome, SandboxError> {
        let payload = SandboxIdPayload {
            sandbox_id: &handle.sandbox_id,
        };
        let stdout = self.bridge("snapshot_and_terminate", &payload).await?;
        let snapshot: SnapshotPayload =
            parse_json(&stdout, "Modal snapshot_and_terminate response")?;
        Ok(ReleaseOutcome::destroyed(Self::snapshot_from(snapshot)))
    }

    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        let payload = SandboxIdPayload {
            sandbox_id: &handle.sandbox_id,
        };
        self.bridge("destroy", &payload).await?;
        Ok(())
    }

    async fn status(&self, handle: &SandboxHandle) -> Result<SandboxStatus, SandboxError> {
        let payload = SandboxIdPayload {
            sandbox_id: &handle.sandbox_id,
        };
        let stdout = self.bridge("status", &payload).await?;
        let status: StatusPayload = parse_json(&stdout, "Modal status response")?;
        Ok(match status.status.trim().to_ascii_lowercase().as_str() {
            "running" => SandboxStatus::Running,
            "paused" => SandboxStatus::Paused,
            "missing" => SandboxStatus::Missing,
            _ => SandboxStatus::Unknown,
        })
    }
}
