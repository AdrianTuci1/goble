//! Compose a remote sandbox from a project, an agent table, and the gateway.
//!
//! The provider adapter only knows how to create and destroy a sandbox. This
//! module supplies the worker's connection settings, derives the stable name
//! and workspace path from [`WorkspaceIdentity`], and returns the durable
//! binding the caller persists. Every pane that opens the project then attaches
//! to the one sandbox the binding names.
//!
//! # Durable sessions
//!
//! A session here is a binding, not a process. The binding carries the
//! provider's sandbox id, the last snapshot id and the harness's native session
//! id, so the provisioner can behave like the reference control plane:
//!
//! - a deploy whose binding names a *live* sandbox reuses it instead of
//!   creating a second one ([`SandboxResume::Reused`]);
//! - a deploy whose sandbox is dead restores the binding's snapshot when the
//!   provider can take one ([`SandboxResume::Restored`]), and otherwise starts
//!   from the image and says so ([`SandboxResume::Created`]);
//! - a close or an idle reap snapshots (or pauses, or keeps the workspace) and
//!   then stops the sandbox, so the next deploy resumes it
//!   ([`DesktopState::release_remote_agent`], [`DesktopState::reap_idle_remote_agents`]);
//! - the worker always receives the recorded native session id as
//!   `GOBLE_NATIVE_SESSION_ID`, so a harness can resume its conversation after
//!   the sandbox that ran it is gone.
//!
//! [`DesktopState::deploy_remote_agent`] is the one caller of all of it: it
//! resolves the `[remote_agents.<name>]` table and the `[remote]` gateway,
//! picks the project out of the persistence store, selects the adapter with
//! [`goble_remote::provider_for`], resumes or creates the sandbox, records the
//! binding and reports the outcome as [`REMOTE_DEPLOY_EVENT`].

use std::collections::BTreeMap;

use goble_core::config::{GobleConfig, RemoteAgentConfig};
use goble_core::workspace_identity::WorkspaceIdentity;
use goble_persistence::{CheckpointStore, Project, ProjectSandbox};
use goble_remote::{
    SandboxBinding, SandboxError, SandboxHandle, SandboxLifecycle, SandboxProvider, SandboxRequest,
    SandboxStatus, SnapshotRef, SnapshotSupport,
};
use serde::Serialize;

use super::DesktopState;

/// The event a deploy reports on: `ready` with the binding it persisted, or
/// `failed` with the sentence naming what stopped it. The app refreshes Remote's
/// row from it.
pub const REMOTE_DEPLOY_EVENT: &str = "remote:deploy";

/// The event a release (close or idle reap) reports on.
pub const REMOTE_RELEASE_EVENT: &str = "remote:release";

/// The environment a worker image reads at startup.
pub const GATEWAY_URL_ENV: &str = "GOBLE_GATEWAY_URL";
pub const WORKER_TOKEN_ENV: &str = "GOBLE_WORKER_TOKEN";
pub const WORKER_ID_ENV: &str = "GOBLE_WORKER_ID";
pub const PROJECT_ID_ENV: &str = "GOBLE_PROJECT_ID";
pub const WORKSPACE_PATH_ENV: &str = "GOBLE_WORKSPACE_PATH";
pub const REPO_URL_ENV: &str = "GOBLE_REPO_URL";
pub const REPO_BRANCH_ENV: &str = "GOBLE_REPO_BRANCH";
/// The harness conversation to resume, when the binding knows one. A DSH
/// harness takes it as `--session-id`; another harness maps it to its own
/// resume flag.
pub const NATIVE_SESSION_ENV: &str = "GOBLE_NATIVE_SESSION_ID";

/// Everything the deploy needs beyond the provider adapter.
pub struct RemoteDeploy<'a> {
    pub project: &'a Project,
    pub agent: &'a RemoteAgentConfig,
    pub gateway_url: &'a str,
    pub worker_token: &'a str,
    pub repo_url: Option<&'a str>,
    pub repo_branch: Option<&'a str>,
    pub host: &'a str,
    pub timeout_secs: u64,
    /// The harness conversation to resume, carried to the worker in
    /// [`NATIVE_SESSION_ENV`]. `None` starts a new conversation.
    pub native_session_id: Option<&'a str>,
}

impl<'a> RemoteDeploy<'a> {
    /// A deploy with the standard one-hour sandbox lifetime.
    pub fn new(
        project: &'a Project,
        agent: &'a RemoteAgentConfig,
        gateway_url: &'a str,
        worker_token: &'a str,
        host: &'a str,
    ) -> Self {
        Self {
            project,
            agent,
            gateway_url,
            worker_token,
            repo_url: None,
            repo_branch: None,
            host,
            timeout_secs: 3600,
            native_session_id: None,
        }
    }

    pub fn with_repo(mut self, url: &'a str, branch: Option<&'a str>) -> Self {
        self.repo_url = Some(url);
        self.repo_branch = branch;
        self
    }

    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    /// Resume the harness conversation `native_session_id` on the worker.
    pub fn with_native_session_id(mut self, native_session_id: Option<&'a str>) -> Self {
        self.native_session_id = native_session_id.map(str::trim).filter(|id| !id.is_empty());
        self
    }
}

/// Build the request a provider adapter receives. The image comes from the
/// agent table; the worker connects out to `gateway_url` with `worker_token`,
/// so no inbound port or tunnel is needed.
pub fn sandbox_request(deploy: &RemoteDeploy<'_>) -> Result<SandboxRequest, SandboxError> {
    if !deploy.agent.enabled {
        return Err(SandboxError::InvalidRequest {
            message: "the remote agent is disabled in config.toml".to_string(),
        });
    }
    let image = deploy
        .agent
        .image
        .as_deref()
        .map(str::trim)
        .filter(|image| !image.is_empty())
        .ok_or_else(|| SandboxError::InvalidRequest {
            message: format!(
                "the remote agent needs an `image` that carries the goble worker: {}",
                deploy.project.project_id.0
            ),
        })?;
    if deploy.gateway_url.trim().is_empty() {
        return Err(SandboxError::InvalidRequest {
            message: "the gateway URL is empty; start goble-gateway and configure its address"
                .to_string(),
        });
    }
    if deploy.worker_token.trim().is_empty() {
        return Err(SandboxError::InvalidRequest {
            message: "the worker token is empty; use the token goble-gateway printed".to_string(),
        });
    }

    let identity = WorkspaceIdentity::new(
        &deploy.project.project_id.0,
        &deploy.project.directory,
        deploy.host,
    );
    let mut env = BTreeMap::from([
        (GATEWAY_URL_ENV.to_string(), deploy.gateway_url.to_string()),
        (
            WORKER_TOKEN_ENV.to_string(),
            deploy.worker_token.to_string(),
        ),
        (WORKER_ID_ENV.to_string(), identity.sandbox_name.clone()),
        (
            PROJECT_ID_ENV.to_string(),
            deploy.project.project_id.0.clone(),
        ),
        (
            WORKSPACE_PATH_ENV.to_string(),
            identity.sandbox_path.clone(),
        ),
    ]);
    if let Some(url) = deploy.repo_url.filter(|url| !url.trim().is_empty()) {
        env.insert(REPO_URL_ENV.to_string(), url.to_string());
    }
    if let Some(branch) = deploy
        .repo_branch
        .filter(|branch| !branch.trim().is_empty())
    {
        env.insert(REPO_BRANCH_ENV.to_string(), branch.to_string());
    }
    if let Some(session_id) = deploy
        .native_session_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        env.insert(NATIVE_SESSION_ENV.to_string(), session_id.to_string());
    }

    Ok(SandboxRequest {
        project_id: deploy.project.project_id.0.clone(),
        sandbox_name: identity.sandbox_name,
        workspace_path: identity.sandbox_path,
        image: image.to_string(),
        command: Vec::new(),
        env,
        timeout_secs: deploy.timeout_secs,
        metadata: BTreeMap::from([
            (
                "goble.project".to_string(),
                deploy.project.project_id.0.clone(),
            ),
            ("goble.workspace".to_string(), identity.prompt_name.clone()),
        ]),
    })
}

/// How a deploy obtained the project's sandbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxResume {
    /// The provider still had the sandbox running; it was shared, not replaced.
    Reused,
    /// A new sandbox started from durable state: a filesystem snapshot, a
    /// paused sandbox's state, or a workspace that outlives the sandbox.
    Restored,
    /// A new sandbox started from the image with no prior state to restore.
    Created,
    /// The binding named a snapshot, but the provider cannot restore it, so a
    /// fresh sandbox was created. The snapshot is kept in the binding.
    CreatedWithoutSnapshot,
}

impl SandboxResume {
    /// The word the deploy report and the docs use.
    pub fn as_str(&self) -> &'static str {
        match self {
            SandboxResume::Reused => "reused",
            SandboxResume::Restored => "restored",
            SandboxResume::Created => "created",
            SandboxResume::CreatedWithoutSnapshot => "created_without_snapshot",
        }
    }

    /// Whether the sandbox the run got holds the project's earlier state.
    pub fn kept_state(&self) -> bool {
        matches!(self, SandboxResume::Reused | SandboxResume::Restored)
    }
}

/// What [`acquire`] produced: the sandbox to run in and how it was obtained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcquiredSandbox {
    /// The provider handle for the sandbox the deploy runs in.
    pub handle: SandboxHandle,
    /// Whether it was reused, restored from durable state, or created fresh.
    pub resume: SandboxResume,
}

/// The provider-neutral binding a persisted [`ProjectSandbox`] row describes.
///
/// The row's `snapshot_id` is turned into a [`SnapshotRef`] for the row's own
/// provider, so an adapter only ever sees a snapshot it can restore.
pub fn sandbox_binding(binding: &ProjectSandbox) -> SandboxBinding {
    let snapshot = binding
        .snapshot_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| SnapshotRef::new(binding.provider.clone(), id));
    SandboxBinding {
        provider: binding.provider.clone(),
        sandbox_name: binding.sandbox_name.clone(),
        sandbox_id: binding.sandbox_id.clone(),
        snapshot,
        native_session_id: binding.native_session_id.clone(),
        workspace_path: binding.workspace_path.clone(),
    }
}

/// The binding to persist after a deploy: the new handle plus the durable state
/// the binding keeps.
fn binding_from_acquired(
    project: &Project,
    provider: &str,
    acquired: &AcquiredSandbox,
    previous: Option<&ProjectSandbox>,
    native_session_id: Option<&str>,
) -> ProjectSandbox {
    // A snapshot is only meaningful to the provider kind that minted it, so a
    // binding that moved to another provider does not carry the old one.
    let snapshot_id = previous
        .filter(|previous| previous.provider == provider)
        .and_then(|previous| previous.snapshot_id.clone())
        .filter(|id| !id.trim().is_empty());
    let native_session_id = native_session_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| previous.and_then(|previous| previous.native_session_id.clone()));
    ProjectSandbox {
        project_id: project.project_id.clone(),
        provider: provider.to_string(),
        sandbox_name: acquired.handle.sandbox_name.clone(),
        sandbox_id: Some(acquired.handle.sandbox_id.clone()),
        workspace_path: acquired.handle.workspace_path.clone(),
        snapshot_id,
        native_session_id,
        updated_at: chrono::Utc::now().to_rfc3339(),
    }
}

/// Create the sandbox and return the binding to persist, with no previous
/// binding to resume. The deploy path uses [`acquire`] instead, so a durable
/// session is reused or restored before a fresh create is considered.
pub async fn deploy(
    provider: &dyn SandboxProvider,
    deploy: &RemoteDeploy<'_>,
) -> Result<ProjectSandbox, SandboxError> {
    let request = sandbox_request(deploy)?;
    let acquired = acquire(provider, &request, None).await?;
    Ok(binding_from_acquired(
        deploy.project,
        provider.kind(),
        &acquired,
        None,
        deploy.native_session_id,
    ))
}

/// Get the project's sandbox for `request`: reuse the live one, restore the
/// dead one from the binding's durable state, or create a fresh one.
///
/// The reference control plane keeps one sandbox per project alive between
/// runs. This does the same with what it has persisted: a running sandbox is
/// shared; a paused one is resumed; a dead one is recreated from its snapshot
/// when the provider can take snapshots, from its workspace when the workspace
/// outlives the sandbox, and from the image (say so honestly) when neither is
/// possible.
pub async fn acquire(
    provider: &dyn SandboxProvider,
    request: &SandboxRequest,
    previous: Option<&ProjectSandbox>,
) -> Result<AcquiredSandbox, SandboxError> {
    let Some(previous) = previous else {
        return Ok(AcquiredSandbox {
            handle: provider.create(request).await?,
            resume: SandboxResume::Created,
        });
    };
    // A binding written by another provider names state this adapter cannot
    // read, so the deploy starts a fresh sandbox rather than guessing.
    if previous.provider != provider.kind() {
        return Ok(AcquiredSandbox {
            handle: provider.create(request).await?,
            resume: SandboxResume::Created,
        });
    }

    let binding = sandbox_binding(previous);
    if let Some(sandbox_id) = binding
        .sandbox_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        let handle = SandboxHandle {
            provider: provider.kind().to_string(),
            sandbox_id: sandbox_id.to_string(),
            sandbox_name: binding.sandbox_name.clone(),
            workspace_path: binding.workspace_path.clone(),
        };
        // One sandbox per project: a live one is shared, never replaced.
        if matches!(provider.status(&handle).await, Ok(SandboxStatus::Running)) {
            return Ok(AcquiredSandbox {
                handle,
                resume: SandboxResume::Reused,
            });
        }
        // Dead, paused or unreachable: a resume may wake the recorded sandbox or
        // replace it from durable state.
        return resume_or_create(provider, request, &binding, Some(sandbox_id)).await;
    }
    resume_or_create(provider, request, &binding, None).await
}

/// The resume half of [`acquire`]: restore what the binding names, falling back
/// to a fresh sandbox when the provider cannot restore it.
async fn resume_or_create(
    provider: &dyn SandboxProvider,
    request: &SandboxRequest,
    binding: &SandboxBinding,
    recorded_sandbox_id: Option<&str>,
) -> Result<AcquiredSandbox, SandboxError> {
    let snapshot = binding.snapshot_for(provider.kind());

    // A provider that cannot restore snapshots must not claim it did: create a
    // fresh sandbox and report that the snapshot was not used.
    if snapshot.is_some() && provider.snapshot_support() != SnapshotSupport::Filesystem {
        return Ok(AcquiredSandbox {
            handle: provider.create(request).await?,
            resume: SandboxResume::CreatedWithoutSnapshot,
        });
    }

    let handle = match provider.resume(request, binding).await {
        Ok(handle) => handle,
        Err(SandboxError::Unsupported { .. }) if snapshot.is_some() => {
            return Ok(AcquiredSandbox {
                handle: provider.create(request).await?,
                resume: SandboxResume::CreatedWithoutSnapshot,
            })
        }
        Err(error) => return Err(error),
    };

    let resume = if snapshot.is_some() {
        SandboxResume::Restored
    } else if recorded_sandbox_id.is_some_and(|id| id == handle.sandbox_id) {
        // The same sandbox came back, so its state came with it (E2B pause).
        SandboxResume::Reused
    } else if provider.snapshot_support() == SnapshotSupport::Workspace {
        // A new sandbox mounted the durable workspace the last one wrote.
        SandboxResume::Restored
    } else {
        SandboxResume::Created
    };
    Ok(AcquiredSandbox { handle, resume })
}

/// What a deploy produced: the binding a pane attaches through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteSandbox {
    /// The provider kind that created the sandbox (`modal`, `docker`, …).
    pub provider: String,
    /// The stable name the binding holds, derived from the project id.
    pub sandbox_name: String,
    /// Where the checkout lives inside the sandbox (`/workspace/<slug>`).
    pub workspace_path: String,
    /// The provider's handle for the sandbox that exists now.
    pub sandbox_id: String,
    /// Whether the deploy reused, restored or created the sandbox.
    pub resumed: SandboxResume,
    /// The snapshot the binding keeps, when the provider takes snapshots.
    pub snapshot_id: Option<String>,
    /// The harness conversation the next worker resumes.
    pub native_session_id: Option<String>,
    /// What this provider can preserve past a sandbox's lifetime, as a word.
    pub snapshot_support: &'static str,
}

/// The payload of [`REMOTE_DEPLOY_EVENT`].
#[derive(Debug, Serialize)]
struct DeployReport<'a> {
    /// `ready` once the binding is persisted, `failed` otherwise.
    status: &'a str,
    /// The `[remote_agents.<name>]` table the deploy was for.
    agent: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sandbox_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_path: Option<&'a str>,
    /// `reused`, `restored`, `created` or `created_without_snapshot`.
    #[serde(skip_serializing_if = "Option::is_none")]
    resumed: Option<&'a str>,
    /// The snapshot the binding keeps, when the provider takes snapshots.
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_id: Option<&'a str>,
    /// The harness conversation the worker resumes.
    #[serde(skip_serializing_if = "Option::is_none")]
    native_session_id: Option<&'a str>,
    /// The provider's durable-state level: `filesystem`, `pause_resume`,
    /// `workspace` or `none`.
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_support: Option<&'a str>,
    /// Why it failed, as Remote's row shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
}

/// What a release (close or idle reap) did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteRelease {
    /// The project whose sandbox was stopped.
    pub project_id: String,
    /// The provider kind that stopped it (`modal`, `docker`, …).
    pub provider: String,
    /// The handle the sandbox had before the release.
    pub sandbox_id: String,
    /// The snapshot the binding now keeps, when the release took one.
    pub snapshot_id: Option<String>,
    /// Whether the provider kept the sandbox itself (a paused sandbox) instead
    /// of destroying it.
    pub sandbox_kept: bool,
    /// The provider's durable-state level: `filesystem`, `pause_resume`,
    /// `workspace` or `none`.
    pub snapshot_support: &'static str,
}

/// The payload of [`REMOTE_RELEASE_EVENT`].
#[derive(Debug, Serialize)]
struct ReleaseReport<'a> {
    /// `released` once the binding is updated, `failed` otherwise.
    status: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sandbox_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sandbox_kept: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot_support: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
}

impl DesktopState {
    /// Ship the worker for `[remote_agents.<agent_name>]` to its platform and
    /// record the sandbox the project now runs in.
    ///
    /// `project` names a durable project id; `None` takes the store's single
    /// project and fails, naming them, when the store holds several. The
    /// outcome is reported as [`REMOTE_DEPLOY_EVENT`] either way.
    pub async fn deploy_remote_agent(
        &self,
        project: Option<&str>,
        agent_name: &str,
    ) -> anyhow::Result<RemoteSandbox> {
        // The adapter is built only for a table that is ready, so its own
        // missing-credential error never stands in for this module's sentence.
        let agent = match remote_agent_for_deploy(&self.config(), agent_name) {
            Ok(agent) => agent,
            Err(error) => {
                self.report_deploy(agent_name, Err(&error));
                return Err(error);
            }
        };
        match goble_remote::provider_for(&agent) {
            Ok(provider) => {
                self.deploy_remote_agent_with(&*provider, project, agent_name)
                    .await
            }
            Err(error) => {
                let error = anyhow::Error::new(error);
                self.report_deploy(agent_name, Err(&error));
                Err(error)
            }
        }
    }

    /// [`Self::deploy_remote_agent`] with the adapter already selected. The
    /// tests call this with a fake so nothing reaches a provider CLI.
    async fn deploy_remote_agent_with(
        &self,
        provider: &dyn SandboxProvider,
        project: Option<&str>,
        agent_name: &str,
    ) -> anyhow::Result<RemoteSandbox> {
        let result = self.run_remote_deploy(provider, project, agent_name).await;
        self.report_deploy(agent_name, result.as_ref());
        result
    }

    /// Resolve the table, the gateway, the project and its workspace, create
    /// the sandbox through `provider`, and persist the binding.
    async fn run_remote_deploy(
        &self,
        provider: &dyn SandboxProvider,
        project: Option<&str>,
        agent_name: &str,
    ) -> anyhow::Result<RemoteSandbox> {
        let config = self.config();
        let agent = remote_agent_for_deploy(&config, agent_name)?;
        let store = self.sandbox_store().ok_or_else(|| {
            anyhow::anyhow!(
                "this state has no persistence store, so the sandbox binding has nowhere to be recorded"
            )
        })?;
        let gateway_url = config.remote.gateway_url().ok_or_else(|| {
            anyhow::anyhow!(
                "the [remote] gateway URL is empty; set `gateway_url` in [remote] to the address goble-gateway listens on"
            )
        })?;
        let worker_token = config.remote.worker_token().ok_or_else(|| {
            anyhow::anyhow!(
                "the [remote] worker token is empty; set `worker_token` in [remote] to the token goble-gateway printed at startup"
            )
        })?;
        let project = resolve_project(&store, project)?;

        // The previous binding is the durable session: a sandbox to share, a
        // snapshot to restore and a conversation to resume. It is read before
        // the deploy so the plan can carry the native session id to the worker.
        let previous = store.load_project_sandbox(&project.project_id)?;
        let native_session_id = previous
            .as_ref()
            .and_then(|previous| previous.native_session_id.as_deref());

        let host = WorkspaceIdentity::local_host();
        let mut plan = RemoteDeploy::new(&project, &agent, gateway_url, worker_token, &host)
            .with_native_session_id(native_session_id);
        // A checkout with an origin is cloned inside the sandbox; one without
        // deploys to an empty workspace rather than failing outright.
        let repo_url = git_origin(&project.directory).await;
        if let Some(repo_url) = repo_url.as_deref() {
            plan = plan.with_repo(repo_url, None);
        }

        // Resume or create through the binding: a live sandbox is reused, a
        // dead one is restored from its snapshot, and only a project with no
        // durable state gets a fresh create.
        let request = sandbox_request(&plan)?;
        let acquired = acquire(provider, &request, previous.as_ref()).await?;
        // The handle carries the provider's id; the name and the path stay the
        // ones the project derives, so a rebind finds the same workspace.
        let binding = binding_from_acquired(
            &project,
            provider.kind(),
            &acquired,
            previous.as_ref(),
            plan.native_session_id,
        );
        store.upsert_project_sandbox(&binding)?;
        Ok(RemoteSandbox {
            provider: binding.provider,
            sandbox_name: binding.sandbox_name,
            workspace_path: binding.workspace_path,
            sandbox_id: acquired.handle.sandbox_id,
            resumed: acquired.resume,
            snapshot_id: binding.snapshot_id,
            native_session_id: binding.native_session_id,
            snapshot_support: provider.snapshot_support().as_str(),
        })
    }

    /// The lifecycle knobs, read from the environment: the idle window before a
    /// reaper snapshots and stops a sandbox, and whether a close keeps state.
    pub fn sandbox_lifecycle(&self) -> SandboxLifecycle {
        SandboxLifecycle::from_env()
    }

    /// Keep the project's sandbox state and stop the sandbox: snapshot when the
    /// provider can, pause when it has pause/resume, keep the workspace when it
    /// does not, and destroy accordingly.
    ///
    /// The binding is updated, not deleted: `sandbox_id` is cleared when the
    /// sandbox is destroyed (kept when it was paused) and `snapshot_id` keeps
    /// the last snapshot, so the next [`Self::deploy_remote_agent`] restores it.
    ///
    /// `project` names a durable project id; `None` takes the store's only
    /// project. A project with no live sandbox is a no-op that reports nothing.
    pub async fn release_remote_agent(
        &self,
        project: Option<&str>,
    ) -> anyhow::Result<Option<RemoteRelease>> {
        let lifecycle = self.sandbox_lifecycle();
        let result = self.release_remote_agent_at(&lifecycle, project).await;
        self.report_release(result.as_ref());
        result
    }

    /// [`Self::release_remote_agent`] with an explicit lifecycle, for tests and
    /// for a caller that already read the settings.
    async fn release_remote_agent_at(
        &self,
        lifecycle: &SandboxLifecycle,
        project: Option<&str>,
    ) -> anyhow::Result<Option<RemoteRelease>> {
        let store = self.sandbox_store().ok_or_else(|| {
            anyhow::anyhow!(
                "this state has no persistence store, so the sandbox binding cannot be updated"
            )
        })?;
        let project = resolve_project(&store, project)?;
        let Some(binding) = store.load_project_sandbox(&project.project_id)? else {
            return Ok(None);
        };
        let Some(provider) = self.provider_for_binding(&binding) else {
            return Ok(None);
        };
        self.release_binding(&store, provider.as_ref(), &binding, lifecycle)
            .await
    }

    /// Stop one binding's sandbox under the lifecycle rules, updating the row.
    async fn release_binding(
        &self,
        store: &CheckpointStore,
        provider: &dyn SandboxProvider,
        binding: &ProjectSandbox,
        lifecycle: &SandboxLifecycle,
    ) -> anyhow::Result<Option<RemoteRelease>> {
        let Some(sandbox_id) = binding
            .sandbox_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            return Ok(None);
        };
        let handle = SandboxHandle {
            provider: binding.provider.clone(),
            sandbox_id: sandbox_id.to_string(),
            sandbox_name: binding.sandbox_name.clone(),
            workspace_path: binding.workspace_path.clone(),
        };
        // A close that keeps state snapshots/pauses/keeps the workspace; one
        // that does not is a plain destroy, so the preference is honoured.
        let (snapshot_id, sandbox_kept) = if lifecycle.snapshots_on_close() {
            let outcome = provider.release(&handle).await?;
            (
                outcome.snapshot.map(|snapshot| snapshot.snapshot_id),
                outcome.sandbox_kept,
            )
        } else {
            provider.destroy(&handle).await?;
            (None, false)
        };

        let mut updated = binding.clone();
        // Keep the last snapshot: a restore that starts a new sandbox may die
        // again before it takes a fresh one.
        if let Some(snapshot_id) = snapshot_id {
            updated.snapshot_id = Some(snapshot_id);
        }
        if !sandbox_kept {
            updated.sandbox_id = None;
        }
        updated.updated_at = chrono::Utc::now().to_rfc3339();
        store.upsert_project_sandbox(&updated)?;
        Ok(Some(RemoteRelease {
            project_id: binding.project_id.0.clone(),
            provider: binding.provider.clone(),
            sandbox_id: handle.sandbox_id,
            snapshot_id: updated.snapshot_id,
            sandbox_kept,
            snapshot_support: provider.snapshot_support().as_str(),
        }))
    }

    /// Snapshot and stop every project sandbox idle past the configured window.
    ///
    /// The idle window comes from [`SandboxLifecycle`] (`GOBLE_SANDBOX_IDLE_SECS`,
    /// default thirty minutes, `0` disables the reaper). Returns the project ids
    /// that were released, so a caller can report what the reaper did.
    pub async fn reap_idle_remote_agents(&self) -> anyhow::Result<Vec<String>> {
        let lifecycle = self.sandbox_lifecycle();
        self.reap_idle_remote_agents_at(&lifecycle, chrono::Utc::now().timestamp())
            .await
    }

    /// [`Self::reap_idle_remote_agents`] at an explicit time, so a test does not
    /// have to age rows by waiting.
    async fn reap_idle_remote_agents_at(
        &self,
        lifecycle: &SandboxLifecycle,
        now: i64,
    ) -> anyhow::Result<Vec<String>> {
        let Some(store) = self.sandbox_store() else {
            return Ok(Vec::new());
        };
        let provider_for = |binding: &ProjectSandbox| self.provider_for_binding(binding);
        self.reap_idle_bindings(&store, lifecycle, now, &provider_for)
            .await
    }

    /// The reaper over an explicit store and provider resolver.
    ///
    /// The resolver is a parameter so a test can hand in mock adapters; the
    /// public path resolves them from whichever `[remote_agents.*]` table names
    /// the binding's provider kind.
    async fn reap_idle_bindings(
        &self,
        store: &CheckpointStore,
        lifecycle: &SandboxLifecycle,
        now: i64,
        provider_for: &dyn Fn(&ProjectSandbox) -> Option<Box<dyn SandboxProvider>>,
    ) -> anyhow::Result<Vec<String>> {
        // A disabled reaper (`GOBLE_SANDBOX_IDLE_SECS=0`) does nothing at all.
        if lifecycle.idle_timeout().is_none() {
            return Ok(Vec::new());
        }
        let mut reaped = Vec::new();
        for binding in store.list_project_sandboxes()? {
            let last_used = match chrono::DateTime::parse_from_rfc3339(binding.updated_at.trim()) {
                Ok(at) => at.timestamp(),
                // A row with an unreadable timestamp cannot be proven idle.
                Err(_) => continue,
            };
            if !lifecycle.is_idle(now, last_used) {
                continue;
            }
            let Some(provider) = provider_for(&binding) else {
                continue;
            };
            if let Some(release) = self
                .release_binding(store, provider.as_ref(), &binding, lifecycle)
                .await?
            {
                // Every reaped sandbox is a release like any other, so surfaces
                // watching Remote refresh from the same event.
                self.report_release(Ok(&Some(release)));
                reaped.push(binding.project_id.0);
            }
        }
        Ok(reaped)
    }

    /// Remember the harness conversation a worker reported, so the next deploy
    /// resumes it.
    ///
    /// A project has one binding, so the most recent report wins: the harness
    /// session the user is working in is the one the next run should resume.
    /// An unknown project, or a project with no binding, is a no-op.
    pub fn record_remote_native_session(
        &self,
        project: Option<&str>,
        native_session_id: &str,
    ) -> anyhow::Result<Option<ProjectSandbox>> {
        let native_session_id = native_session_id.trim();
        if native_session_id.is_empty() {
            return Err(anyhow::anyhow!("the harness session id is empty"));
        }
        let store = self.sandbox_store().ok_or_else(|| {
            anyhow::anyhow!(
                "this state has no persistence store, so the session id has nowhere to be recorded"
            )
        })?;
        let project = resolve_project(&store, project)?;
        let Some(mut binding) = store.load_project_sandbox(&project.project_id)? else {
            return Ok(None);
        };
        if binding.native_session_id.as_deref() == Some(native_session_id) {
            return Ok(Some(binding));
        }
        binding.native_session_id = Some(native_session_id.to_string());
        // The harness is activity: a conversation report keeps the sandbox from
        // looking idle.
        binding.updated_at = chrono::Utc::now().to_rfc3339();
        store.upsert_project_sandbox(&binding)?;
        Ok(Some(binding))
    }

    /// The persisted binding for a project, when it has one.
    pub fn remote_sandbox_binding(
        &self,
        project: Option<&str>,
    ) -> anyhow::Result<Option<ProjectSandbox>> {
        let store = self.sandbox_store().ok_or_else(|| {
            anyhow::anyhow!("this state has no persistence store, so it has no sandbox bindings")
        })?;
        let project = resolve_project(&store, project)?;
        Ok(store.load_project_sandbox(&project.project_id)?)
    }

    /// The adapter a binding's provider kind selects, from whichever enabled
    /// `[remote_agents.*]` table names that provider.
    fn provider_for_binding(&self, binding: &ProjectSandbox) -> Option<Box<dyn SandboxProvider>> {
        let config = self.config();
        let agent = config
            .remote_agents
            .values()
            .find(|agent| agent.enabled && agent.provider.as_str() == binding.provider)?;
        goble_remote::provider_for(agent).ok()
    }

    /// Publish what a deploy did, so every surface reading Remote refreshes
    /// from one report instead of polling.
    fn report_deploy(&self, agent_name: &str, outcome: Result<&RemoteSandbox, &anyhow::Error>) {
        match outcome {
            Ok(sandbox) => self.emit(
                REMOTE_DEPLOY_EVENT,
                DeployReport {
                    status: "ready",
                    agent: agent_name,
                    provider: Some(&sandbox.provider),
                    sandbox_name: Some(&sandbox.sandbox_name),
                    workspace_path: Some(&sandbox.workspace_path),
                    resumed: Some(sandbox.resumed.as_str()),
                    snapshot_id: sandbox.snapshot_id.as_deref(),
                    native_session_id: sandbox.native_session_id.as_deref(),
                    snapshot_support: Some(sandbox.snapshot_support),
                    message: None,
                },
            ),
            Err(error) => {
                // The whole chain, so the row names the cause and not only the
                // outermost `context`.
                let message = format!("{error:#}");
                self.emit(
                    REMOTE_DEPLOY_EVENT,
                    DeployReport {
                        status: "failed",
                        agent: agent_name,
                        provider: None,
                        sandbox_name: None,
                        workspace_path: None,
                        resumed: None,
                        snapshot_id: None,
                        native_session_id: None,
                        snapshot_support: None,
                        message: Some(&message),
                    },
                );
            }
        }
    }

    /// Publish what a release did, on [`REMOTE_RELEASE_EVENT`].
    fn report_release(&self, outcome: Result<&Option<RemoteRelease>, &anyhow::Error>) {
        match outcome {
            Ok(Some(release)) => self.emit(
                REMOTE_RELEASE_EVENT,
                ReleaseReport {
                    status: "released",
                    project_id: Some(&release.project_id),
                    provider: Some(&release.provider),
                    sandbox_id: Some(&release.sandbox_id),
                    snapshot_id: release.snapshot_id.as_deref(),
                    sandbox_kept: Some(release.sandbox_kept),
                    snapshot_support: Some(release.snapshot_support),
                    message: None,
                },
            ),
            Ok(None) => {}
            Err(error) => {
                let message = format!("{error:#}");
                self.emit(
                    REMOTE_RELEASE_EVENT,
                    ReleaseReport {
                        status: "failed",
                        project_id: None,
                        provider: None,
                        sandbox_id: None,
                        snapshot_id: None,
                        sandbox_kept: None,
                        snapshot_support: None,
                        message: Some(&message),
                    },
                );
            }
        }
    }
}

/// The `[remote_agents.<name>]` table a deploy runs on, refused when it is
/// disabled or still missing a credential the provider needs.
fn remote_agent_for_deploy(config: &GobleConfig, name: &str) -> anyhow::Result<RemoteAgentConfig> {
    let Some(agent) = config.remote_agents.get(name) else {
        return Err(anyhow::anyhow!(
            "there is no [remote_agents.{name}] table in config.toml"
        ));
    };
    if !agent.enabled {
        return Err(anyhow::anyhow!(
            "the remote agent `{name}` is disabled; set `enabled = true` in [remote_agents.{name}]"
        ));
    }
    let missing = agent.missing_credentials();
    if !missing.is_empty() {
        return Err(anyhow::anyhow!(
            "the remote agent `{name}` is missing {}; add them to [remote_agents.{name}]",
            missing.join(", ")
        ));
    }
    Ok(agent.clone())
}

/// The project a deploy works on: the named one, or the store's only project.
fn resolve_project(store: &CheckpointStore, named: Option<&str>) -> anyhow::Result<Project> {
    let projects = store.list_projects()?;
    let Some(named) = named.map(str::trim).filter(|id| !id.is_empty()) else {
        return match projects.len() {
            0 => Err(anyhow::anyhow!(
                "no project to deploy yet; open a project in Goble first"
            )),
            1 => Ok(projects.into_iter().next().expect("one project")),
            _ => {
                let mut ids: Vec<String> = projects.into_iter().map(|p| p.project_id.0).collect();
                ids.sort();
                Err(anyhow::anyhow!(
                    "the store holds several projects ({}); name the one to deploy",
                    ids.join(", ")
                ))
            }
        };
    };
    projects
        .into_iter()
        .find(|project| project.project_id.0 == named)
        .ok_or_else(|| anyhow::anyhow!("there is no project `{named}` to deploy"))
}

/// The `origin` URL of the project's checkout, when it has one. A project
/// without git, or without a remote, deploys with no repository.
async fn git_origin(directory: &str) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(["remote", "get-url", "origin"])
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!url.is_empty()).then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use goble_core::config::RemoteAgentProvider;
    use goble_harness_types::ProjectId;
    use goble_remote::{SandboxHandle, SandboxStatus};
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::Duration;

    /// A provider whose every answer a test decides, and whose calls it records.
    ///
    /// It does not override the snapshot half of the trait beyond `snapshot`,
    /// so `release` runs the default "snapshot then destroy" path a provider
    /// without its own atomic teardown takes.
    struct MockProvider {
        kind: &'static str,
        support: SnapshotSupport,
        status: SandboxStatus,
        /// The id `create` answers with.
        created_id: String,
        /// The id `create_from_snapshot` answers with.
        restored_id: String,
        /// Whether `resume` wakes the binding's recorded sandbox (pause/resume).
        wakes_recorded: bool,
        seen: Mutex<Option<SandboxRequest>>,
        /// The snapshot `create_from_snapshot` was asked to restore.
        restored_snapshot: Mutex<Option<SnapshotRef>>,
        /// The calls the provisioner made, in order.
        calls: Mutex<Vec<&'static str>>,
    }

    impl MockProvider {
        fn fake() -> Self {
            Self {
                kind: "fake",
                support: SnapshotSupport::None,
                status: SandboxStatus::Running,
                created_id: "sb-1".to_string(),
                restored_id: "sb-restored".to_string(),
                wakes_recorded: false,
                seen: Mutex::new(None),
                restored_snapshot: Mutex::new(None),
                calls: Mutex::new(Vec::new()),
            }
        }

        /// A provider with real filesystem snapshots (the Modal shape).
        fn filesystem() -> Self {
            Self {
                support: SnapshotSupport::Filesystem,
                status: SandboxStatus::Missing,
                ..Self::fake()
            }
        }

        /// A provider that wakes the same recorded sandbox (the E2B shape).
        fn pause_resume() -> Self {
            Self {
                support: SnapshotSupport::PauseResume,
                status: SandboxStatus::Paused,
                wakes_recorded: true,
                ..Self::fake()
            }
        }

        fn with_status(mut self, status: SandboxStatus) -> Self {
            self.status = status;
            self
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, call: &'static str) {
            self.calls.lock().unwrap().push(call);
        }
    }

    #[async_trait]
    impl SandboxProvider for MockProvider {
        fn kind(&self) -> &'static str {
            self.kind
        }

        fn snapshot_support(&self) -> SnapshotSupport {
            self.support
        }

        async fn create(&self, request: &SandboxRequest) -> Result<SandboxHandle, SandboxError> {
            self.record("create");
            *self.seen.lock().unwrap() = Some(request.clone());
            Ok(SandboxHandle::from_request(
                self.kind,
                &self.created_id,
                request,
            ))
        }

        async fn create_from_snapshot(
            &self,
            request: &SandboxRequest,
            snapshot: &SnapshotRef,
        ) -> Result<SandboxHandle, SandboxError> {
            if self.support != SnapshotSupport::Filesystem {
                return Err(SandboxError::Unsupported {
                    provider: self.kind.to_string(),
                    message: "fake provider has no filesystem snapshots".to_string(),
                });
            }
            self.record("create_from_snapshot");
            *self.seen.lock().unwrap() = Some(request.clone());
            *self.restored_snapshot.lock().unwrap() = Some(snapshot.clone());
            Ok(SandboxHandle::from_request(
                self.kind,
                &self.restored_id,
                request,
            ))
        }

        async fn resume(
            &self,
            request: &SandboxRequest,
            binding: &SandboxBinding,
        ) -> Result<SandboxHandle, SandboxError> {
            if self.wakes_recorded {
                self.record("resume");
                *self.seen.lock().unwrap() = Some(request.clone());
                let id = binding
                    .sandbox_id
                    .clone()
                    .unwrap_or_else(|| self.created_id.clone());
                return Ok(SandboxHandle::from_request(self.kind, id, request));
            }
            match binding.snapshot_for(self.kind) {
                Some(snapshot) => self.create_from_snapshot(request, snapshot).await,
                None => self.create(request).await,
            }
        }

        async fn snapshot(
            &self,
            _handle: &SandboxHandle,
        ) -> Result<Option<SnapshotRef>, SandboxError> {
            self.record("snapshot");
            Ok(Some(SnapshotRef::new(self.kind, "im-1")))
        }

        async fn destroy(&self, _handle: &SandboxHandle) -> Result<(), SandboxError> {
            self.record("destroy");
            Ok(())
        }

        async fn status(&self, _handle: &SandboxHandle) -> Result<SandboxStatus, SandboxError> {
            self.record("status");
            Ok(self.status)
        }
    }

    fn project() -> Project {
        Project {
            project_id: ProjectId::new("projects/goble"),
            directory: "/Users/me/Projects/goble".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn agent() -> RemoteAgentConfig {
        RemoteAgentConfig {
            provider: RemoteAgentProvider::Docker,
            enabled: true,
            api_key: None,
            token_id: None,
            token_secret: None,
            api_url: None,
            image: Some("ghcr.io/example/goble-worker:latest".to_string()),
            region: None,
            environment: None,
        }
    }

    #[tokio::test]
    async fn the_request_carries_one_stable_workspace_and_its_gateway() {
        let project = project();
        let agent = agent();
        let provider = MockProvider::fake();
        let plan = RemoteDeploy::new(
            &project,
            &agent,
            "wss://gateway.example/ws/worker",
            "worker-secret",
            "Adrians-MacBook-Pro",
        );

        let binding = deploy(&provider, &plan).await.unwrap();
        assert_eq!(binding.project_id, ProjectId::new("projects/goble"));
        assert_eq!(binding.sandbox_name, "goble-goble-2fbfef8fa780");
        assert_eq!(binding.workspace_path, "/workspace/goble");
        assert_eq!(binding.sandbox_id.as_deref(), Some("sb-1"));
        assert_eq!(binding.provider, "fake");

        let request = provider.seen.lock().unwrap().clone().unwrap();
        assert_eq!(request.project_id, "projects/goble");
        assert_eq!(request.image, "ghcr.io/example/goble-worker:latest");
        assert!(request.command.is_empty());
        assert_eq!(
            request.env.get(GATEWAY_URL_ENV).map(String::as_str),
            Some("wss://gateway.example/ws/worker")
        );
        assert_eq!(
            request.env.get(WORKER_ID_ENV).map(String::as_str),
            Some("goble-goble-2fbfef8fa780")
        );
        assert_eq!(
            request.env.get(WORKSPACE_PATH_ENV).map(String::as_str),
            Some("/workspace/goble")
        );
        assert_eq!(
            request.env.get(WORKER_TOKEN_ENV).map(String::as_str),
            Some("worker-secret")
        );
    }

    #[test]
    fn a_missing_image_or_gateway_is_refused_before_any_call() {
        let project = project();
        let mut missing_image = agent();
        missing_image.image = None;
        let plan = RemoteDeploy::new(&project, &missing_image, "wss://gateway", "token", "host");
        assert!(matches!(
            sandbox_request(&plan),
            Err(SandboxError::InvalidRequest { .. })
        ));

        let configured = agent();
        let plan = RemoteDeploy::new(&project, &configured, " ", "token", "host");
        assert!(matches!(
            sandbox_request(&plan),
            Err(SandboxError::InvalidRequest { .. })
        ));

        let mut disabled = agent();
        disabled.enabled = false;
        let plan = RemoteDeploy::new(&project, &disabled, "wss://gateway", "token", "host");
        let error = sandbox_request(&plan).expect_err("a disabled agent must not deploy");
        assert!(error.to_string().contains("disabled"));
    }

    #[test]
    fn the_repo_is_passed_only_when_named() {
        let project = project();
        let agent = agent();
        let deploy = RemoteDeploy::new(&project, &agent, "wss://gateway", "token", "host")
            .with_repo("git@github.com:me/goble.git", Some("main"));
        let request = sandbox_request(&deploy).unwrap();
        assert_eq!(
            request.env.get(REPO_URL_ENV).map(String::as_str),
            Some("git@github.com:me/goble.git")
        );
        assert_eq!(
            request.env.get(REPO_BRANCH_ENV).map(String::as_str),
            Some("main")
        );
    }

    // ---- The deploy entry point ------------------------------------------

    /// A state whose sandbox store holds `projects/goble`, whose directory is
    /// `<temp>/goble` (so the stable name and path match the ones above), and
    /// whose config names a `[remote]` gateway and one ready platform.
    fn remote_state_with(
        adjust: impl FnOnce(&mut GobleConfig),
    ) -> (tempfile::TempDir, Arc<DesktopState>, Arc<CheckpointStore>) {
        let dir = tempfile::tempdir().unwrap();
        let state = DesktopState::new(
            goble_core::store::Store::open_in_memory().unwrap(),
            crate::thread_store::ThreadStore::new(dir.path()).unwrap(),
        );
        let store = Arc::new(CheckpointStore::in_memory());
        state.set_sandbox_store(Arc::clone(&store));
        store
            .upsert_project(&Project {
                project_id: ProjectId::new("projects/goble"),
                directory: dir.path().join("goble").display().to_string(),
                created_at: "2026-01-01T00:00:00Z".to_string(),
            })
            .unwrap();

        let mut config = GobleConfig::default();
        config.remote.gateway_url = Some("wss://gateway.example/ws/worker".to_string());
        config.remote.worker_token = Some("worker-secret".to_string());
        config.remote_agents.insert("local".to_string(), agent());
        adjust(&mut config);
        state.set_config_path(dir.path().join("config.toml"));
        state.save_config(&config).unwrap();
        (dir, state, store)
    }

    fn remote_state() -> (tempfile::TempDir, Arc<DesktopState>, Arc<CheckpointStore>) {
        remote_state_with(|_| {})
    }

    fn fake() -> MockProvider {
        MockProvider::fake()
    }

    #[tokio::test]
    async fn a_deploy_creates_the_sandbox_and_records_its_binding() {
        let (_dir, state, store) = remote_state();
        let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
        state.set_event_bus(bus.clone());
        let provider = fake();

        let deployed = state
            .deploy_remote_agent_with(&provider, None, "local")
            .await
            .expect("the ready table deploys");

        assert_eq!(deployed.provider, "fake");
        assert_eq!(deployed.sandbox_name, "goble-goble-2fbfef8fa780");
        assert_eq!(deployed.workspace_path, "/workspace/goble");
        assert_eq!(deployed.sandbox_id, "sb-1");
        assert_eq!(deployed.resumed, SandboxResume::Created);
        assert_eq!(deployed.snapshot_support, "none");
        assert_eq!(deployed.snapshot_id, None);
        assert_eq!(deployed.native_session_id, None);

        // What the adapter was asked for comes from the table and `[remote]`.
        let request = provider.seen.lock().unwrap().clone().expect("one create");
        assert_eq!(request.project_id, "projects/goble");
        assert_eq!(request.image, "ghcr.io/example/goble-worker:latest");
        assert_eq!(
            request.env.get(GATEWAY_URL_ENV).map(String::as_str),
            Some("wss://gateway.example/ws/worker")
        );
        assert_eq!(
            request.env.get(WORKER_TOKEN_ENV).map(String::as_str),
            Some("worker-secret")
        );
        assert_eq!(
            request.env.get(WORKER_ID_ENV).map(String::as_str),
            Some("goble-goble-2fbfef8fa780")
        );
        assert!(
            request.env.get(REPO_URL_ENV).is_none(),
            "a directory with no origin deploys without a repository"
        );
        assert!(
            request.env.get(NATIVE_SESSION_ENV).is_none(),
            "a first deploy has no conversation to resume"
        );

        // The binding is durable, with the handle's id and the stable name.
        let binding = store
            .load_project_sandbox(&ProjectId::new("projects/goble"))
            .unwrap()
            .expect("the binding was persisted");
        assert_eq!(binding.provider, "fake");
        assert_eq!(binding.sandbox_name, "goble-goble-2fbfef8fa780");
        assert_eq!(binding.sandbox_id.as_deref(), Some("sb-1"));
        assert_eq!(binding.workspace_path, "/workspace/goble");
        assert_eq!(binding.snapshot_id, None);
        assert_eq!(binding.native_session_id, None);
        assert!(!binding.updated_at.is_empty());

        let report = bus
            .events()
            .into_iter()
            .find(|(name, _)| name == REMOTE_DEPLOY_EVENT)
            .map(|(_, payload)| payload)
            .expect("the deploy was reported");
        assert_eq!(report.get("status").and_then(|v| v.as_str()), Some("ready"));
        assert_eq!(
            report.get("sandbox_name").and_then(|v| v.as_str()),
            Some("goble-goble-2fbfef8fa780")
        );
        assert_eq!(
            report.get("resumed").and_then(|v| v.as_str()),
            Some("created")
        );
        assert_eq!(
            report.get("snapshot_support").and_then(|v| v.as_str()),
            Some("none")
        );
    }

    #[tokio::test]
    async fn a_disabled_table_deploys_nothing_and_says_so() {
        let (_dir, state, _store) = remote_state_with(|config| {
            config.remote_agents.get_mut("local").unwrap().enabled = false;
        });
        let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
        state.set_event_bus(bus.clone());
        let provider = fake();

        let error = state
            .deploy_remote_agent_with(&provider, None, "local")
            .await
            .expect_err("a disabled table must not deploy");

        assert!(
            error.to_string().contains("disabled"),
            "the sentence names the cause: {error}"
        );
        assert!(
            provider.seen.lock().unwrap().is_none(),
            "nothing was created"
        );
        let report = bus
            .events()
            .into_iter()
            .find(|(name, _)| name == REMOTE_DEPLOY_EVENT)
            .map(|(_, payload)| payload)
            .expect("the failure was reported");
        assert_eq!(
            report.get("status").and_then(|v| v.as_str()),
            Some("failed")
        );
    }

    #[tokio::test]
    async fn a_table_missing_a_credential_names_the_key() {
        let (_dir, state, _store) = remote_state_with(|config| {
            config.remote_agents.insert(
                "builder".to_string(),
                RemoteAgentConfig {
                    provider: RemoteAgentProvider::Modal,
                    enabled: true,
                    api_key: None,
                    token_id: None,
                    token_secret: None,
                    api_url: None,
                    image: Some("ghcr.io/example/goble-worker:latest".to_string()),
                    region: None,
                    environment: None,
                },
            );
        });
        let provider = fake();

        let error = state
            .deploy_remote_agent_with(&provider, None, "builder")
            .await
            .expect_err("a table without its tokens must not deploy");

        assert!(
            error.to_string().contains("MODAL_TOKEN_ID")
                && error.to_string().contains("MODAL_TOKEN_SECRET"),
            "the sentence names the missing keys: {error}"
        );
        assert!(
            provider.seen.lock().unwrap().is_none(),
            "nothing was created"
        );
    }

    #[tokio::test]
    async fn an_empty_gateway_setting_is_refused_before_the_provider() {
        // A write through the settings editor refuses a blank URL, so the test
        // loads a hand-written file with the blank value: the deploy still has
        // to name the setting instead of handing it to a provider.
        let blanked = |state: &DesktopState, section: &str, from: &str, to: &str| {
            let path = state.config_path().expect("the state pins a config path");
            let text = std::fs::read_to_string(&path).expect("read the config");
            let text = text.replace(from, to);
            assert_ne!(
                text,
                std::fs::read_to_string(&path).unwrap(),
                "{section} was in the file"
            );
            std::fs::write(&path, text).expect("write the config");
            state.reload_config(&path).expect("reload the config");
        };

        let (_dir, state, _store) = remote_state();
        blanked(
            &state,
            "gateway URL",
            "gateway_url = \"wss://gateway.example/ws/worker\"",
            "gateway_url = \"\"",
        );
        let provider = fake();
        let error = state
            .deploy_remote_agent_with(&provider, None, "local")
            .await
            .expect_err("a deploy needs the gateway URL");
        assert!(
            error.to_string().contains("gateway URL"),
            "the sentence names the setting: {error}"
        );
        assert!(
            provider.seen.lock().unwrap().is_none(),
            "nothing was created"
        );

        let (_dir, state, _store) = remote_state();
        blanked(
            &state,
            "worker token",
            "worker_token = \"worker-secret\"",
            "worker_token = \"  \"",
        );
        let error = state
            .deploy_remote_agent_with(&provider, None, "local")
            .await
            .expect_err("a deploy needs the gateway token");
        assert!(
            error.to_string().contains("worker token"),
            "the sentence names the setting: {error}"
        );
        assert!(
            provider.seen.lock().unwrap().is_none(),
            "nothing was created"
        );
    }

    #[tokio::test]
    async fn an_unknown_project_is_named() {
        let (_dir, state, _store) = remote_state();
        let provider = fake();

        let error = state
            .deploy_remote_agent_with(&provider, Some("projects/other"), "local")
            .await
            .expect_err("a project that is not in the store must not deploy");

        assert!(
            error.to_string().contains("projects/other"),
            "the sentence names the project: {error}"
        );
        assert!(
            provider.seen.lock().unwrap().is_none(),
            "nothing was created"
        );
    }

    #[tokio::test]
    async fn several_projects_are_named_before_one_is_chosen() {
        let (_dir, state, store) = remote_state();
        store
            .upsert_project(&Project {
                project_id: ProjectId::new("projects/second"),
                directory: "/Users/me/Projects/second".to_string(),
                created_at: "2026-01-02T00:00:00Z".to_string(),
            })
            .unwrap();
        let provider = fake();

        let error = state
            .deploy_remote_agent_with(&provider, None, "local")
            .await
            .expect_err("two projects need one named");

        let sentence = error.to_string();
        assert!(
            sentence.contains("projects/goble") && sentence.contains("projects/second"),
            "the sentence names the choice: {sentence}"
        );
    }

    #[tokio::test]
    async fn a_named_project_deploys_onto_its_own_binding() {
        let (_dir, state, store) = remote_state();
        store
            .upsert_project(&Project {
                project_id: ProjectId::new("projects/second"),
                directory: "/Users/me/Projects/second".to_string(),
                created_at: "2026-01-02T00:00:00Z".to_string(),
            })
            .unwrap();
        let provider = fake();

        state
            .deploy_remote_agent_with(&provider, Some("projects/second"), "local")
            .await
            .expect("the named project deploys");

        let binding = store
            .load_project_sandbox(&ProjectId::new("projects/second"))
            .unwrap()
            .expect("the named project's binding");
        assert_eq!(binding.sandbox_name, "goble-second-efb6b76d7851");
        assert!(
            store
                .load_project_sandbox(&ProjectId::new("projects/goble"))
                .unwrap()
                .is_none(),
            "the other project gets no binding"
        );
    }

    // ---- Durable sessions: resume, release, reap -------------------------

    /// The binding a previous deploy would have left in the store.
    fn previous_binding(provider: &str) -> ProjectSandbox {
        ProjectSandbox {
            project_id: ProjectId::new("projects/goble"),
            provider: provider.to_string(),
            sandbox_name: "goble-goble-2fbfef8fa780".to_string(),
            sandbox_id: Some("sb-old".to_string()),
            workspace_path: "/workspace/goble".to_string(),
            snapshot_id: None,
            native_session_id: None,
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn seed_binding(store: &CheckpointStore, binding: ProjectSandbox) {
        store.upsert_project_sandbox(&binding).unwrap();
    }

    fn stored_binding(store: &CheckpointStore, project: &str) -> ProjectSandbox {
        store
            .load_project_sandbox(&ProjectId::new(project))
            .unwrap()
            .expect("the project's binding")
    }

    #[tokio::test]
    async fn a_second_deploy_reuses_the_live_sandbox() {
        let (_dir, state, store) = remote_state();
        let mut previous = previous_binding("fake");
        previous.native_session_id = Some("conversation-1".to_string());
        seed_binding(&store, previous);

        let provider = MockProvider::fake().with_status(SandboxStatus::Running);
        let deployed = state
            .deploy_remote_agent_with(&provider, None, "local")
            .await
            .expect("the live sandbox is reused");

        assert_eq!(deployed.resumed, SandboxResume::Reused);
        assert_eq!(deployed.sandbox_id, "sb-old");
        assert_eq!(
            provider.calls(),
            vec!["status"],
            "a live sandbox is shared, not replaced"
        );

        let binding = stored_binding(&store, "projects/goble");
        assert_eq!(binding.sandbox_id.as_deref(), Some("sb-old"));
        assert_eq!(
            binding.native_session_id.as_deref(),
            Some("conversation-1"),
            "the conversation survives the reuse"
        );
    }

    #[tokio::test]
    async fn a_dead_sandbox_with_a_snapshot_is_restored() {
        let (_dir, state, store) = remote_state();
        let mut previous = previous_binding("fake");
        previous.snapshot_id = Some("im-1".to_string());
        previous.native_session_id = Some("conversation-1".to_string());
        seed_binding(&store, previous);

        // The adapter with real filesystem snapshots answers Missing.
        let provider = MockProvider::filesystem();
        let deployed = state
            .deploy_remote_agent_with(&provider, None, "local")
            .await
            .expect("the snapshot is restored");

        assert_eq!(deployed.resumed, SandboxResume::Restored);
        assert_eq!(deployed.sandbox_id, "sb-restored");
        assert_eq!(deployed.snapshot_id.as_deref(), Some("im-1"));
        assert_eq!(
            deployed.native_session_id.as_deref(),
            Some("conversation-1")
        );
        assert_eq!(deployed.snapshot_support, "filesystem");
        assert_eq!(
            provider.calls(),
            vec!["status", "create_from_snapshot"],
            "the dead sandbox is not recreated from the image"
        );
        assert_eq!(
            provider
                .restored_snapshot
                .lock()
                .unwrap()
                .as_ref()
                .map(|snapshot| snapshot.snapshot_id.as_str()),
            Some("im-1")
        );

        // The worker is told which conversation to resume.
        let request = provider.seen.lock().unwrap().clone().expect("one create");
        assert_eq!(
            request.env.get(NATIVE_SESSION_ENV).map(String::as_str),
            Some("conversation-1")
        );

        // The binding keeps the last snapshot and records the new handle.
        let binding = stored_binding(&store, "projects/goble");
        assert_eq!(binding.sandbox_id.as_deref(), Some("sb-restored"));
        assert_eq!(binding.snapshot_id.as_deref(), Some("im-1"));
        assert_eq!(binding.native_session_id.as_deref(), Some("conversation-1"));
    }

    #[tokio::test]
    async fn a_paused_sandbox_is_woken_under_the_same_id() {
        let (_dir, state, store) = remote_state();
        seed_binding(&store, previous_binding("fake"));

        let provider = MockProvider::pause_resume();
        let deployed = state
            .deploy_remote_agent_with(&provider, None, "local")
            .await
            .expect("the paused sandbox is woken");

        assert_eq!(deployed.resumed, SandboxResume::Reused);
        assert_eq!(deployed.sandbox_id, "sb-old");
        assert_eq!(provider.calls(), vec!["status", "resume"]);
    }

    #[tokio::test]
    async fn a_snapshot_the_provider_cannot_restore_is_reported_not_faked() {
        let (_dir, state, store) = remote_state();
        let mut previous = previous_binding("fake");
        previous.sandbox_id = None;
        previous.snapshot_id = Some("im-1".to_string());
        seed_binding(&store, previous);

        let provider = MockProvider::fake().with_status(SandboxStatus::Missing);
        let deployed = state
            .deploy_remote_agent_with(&provider, None, "local")
            .await
            .expect("a fresh sandbox is created");

        assert_eq!(deployed.resumed, SandboxResume::CreatedWithoutSnapshot);
        assert_eq!(
            provider.calls(),
            vec!["create"],
            "a provider without filesystem snapshots never restores one"
        );
        // The last snapshot is kept: a later capable adapter could restore it.
        assert_eq!(
            stored_binding(&store, "projects/goble")
                .snapshot_id
                .as_deref(),
            Some("im-1")
        );
    }

    #[tokio::test]
    async fn a_release_snapshots_before_it_destroys() {
        let (_dir, state, store) = remote_state();
        let mut previous = previous_binding("fake");
        previous.native_session_id = Some("conversation-1".to_string());
        seed_binding(&store, previous);
        let provider = MockProvider::filesystem();
        let binding = stored_binding(&store, "projects/goble");

        let released = state
            .release_binding(&store, &provider, &binding, &SandboxLifecycle::default())
            .await
            .unwrap()
            .expect("the sandbox was released");

        assert_eq!(
            provider.calls(),
            vec!["snapshot", "destroy"],
            "the snapshot is taken before the sandbox dies"
        );
        assert_eq!(released.snapshot_id.as_deref(), Some("im-1"));
        assert!(!released.sandbox_kept);
        assert_eq!(released.snapshot_support, "filesystem");

        let binding = stored_binding(&store, "projects/goble");
        assert_eq!(binding.sandbox_id, None, "the handle is cleared");
        assert_eq!(binding.snapshot_id.as_deref(), Some("im-1"));
        assert_eq!(
            binding.native_session_id.as_deref(),
            Some("conversation-1"),
            "the conversation survives the teardown"
        );
    }

    #[tokio::test]
    async fn a_close_without_state_keeping_destroys_without_a_snapshot() {
        let (_dir, state, store) = remote_state();
        seed_binding(&store, previous_binding("fake"));
        let provider = MockProvider::filesystem();
        let binding = stored_binding(&store, "projects/goble");
        let lifecycle = SandboxLifecycle::new(Some(Duration::from_secs(600)), false);

        let released = state
            .release_binding(&store, &provider, &binding, &lifecycle)
            .await
            .unwrap()
            .expect("the sandbox was released");

        assert_eq!(
            provider.calls(),
            vec!["destroy"],
            "the configured close is a plain destroy"
        );
        assert_eq!(released.snapshot_id, None);
    }

    #[tokio::test]
    async fn the_idle_reaper_releases_only_rows_past_the_window() {
        let (_dir, state, store) = remote_state();
        // projects/goble was last used at 00:00 (two hours ago); projects/second
        // five minutes ago, inside the ten-minute window.
        seed_binding(&store, previous_binding("fake"));
        let mut fresh = previous_binding("fake");
        fresh.project_id = ProjectId::new("projects/second");
        fresh.sandbox_name = "goble-second-efb6b76d7851".to_string();
        fresh.updated_at = "2026-01-01T01:55:00Z".to_string();
        store.upsert_project_sandbox(&fresh).unwrap();

        let lifecycle = SandboxLifecycle::new(Some(Duration::from_secs(600)), true);
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-01T02:00:00Z")
            .unwrap()
            .timestamp();
        let resolver = |_binding: &ProjectSandbox| {
            Some(Box::new(MockProvider::filesystem()) as Box<dyn SandboxProvider>)
        };

        let reaped = state
            .reap_idle_bindings(&store, &lifecycle, now, &resolver)
            .await
            .unwrap();

        assert_eq!(reaped, vec!["projects/goble".to_string()]);
        let idle = stored_binding(&store, "projects/goble");
        assert_eq!(idle.sandbox_id, None);
        assert_eq!(idle.snapshot_id.as_deref(), Some("im-1"));
        let fresh = stored_binding(&store, "projects/second");
        assert_eq!(
            fresh.sandbox_id.as_deref(),
            Some("sb-old"),
            "a sandbox inside the window is untouched"
        );
    }

    #[tokio::test]
    async fn a_disabled_reaper_releases_nothing() {
        let (_dir, state, store) = remote_state();
        seed_binding(&store, previous_binding("fake"));
        // GOBLE_SANDBOX_IDLE_SECS=0 is represented by `None` here, so the test
        // never mutates the process environment.
        let lifecycle = SandboxLifecycle::new(None, true);
        let resolver = |_binding: &ProjectSandbox| {
            Some(Box::new(MockProvider::filesystem()) as Box<dyn SandboxProvider>)
        };
        let reaped = state
            .reap_idle_bindings(&store, &lifecycle, i64::MAX, &resolver)
            .await
            .unwrap();
        assert!(reaped.is_empty());
        assert_eq!(
            stored_binding(&store, "projects/goble")
                .sandbox_id
                .as_deref(),
            Some("sb-old")
        );
    }

    #[test]
    fn recording_the_harness_session_keeps_it_for_the_next_deploy() {
        let (_dir, state, store) = remote_state();
        seed_binding(&store, previous_binding("fake"));

        let recorded = state
            .record_remote_native_session(Some("projects/goble"), "conversation-9")
            .unwrap()
            .expect("the binding was updated");
        assert_eq!(
            recorded.native_session_id.as_deref(),
            Some("conversation-9")
        );
        assert_eq!(
            stored_binding(&store, "projects/goble")
                .native_session_id
                .as_deref(),
            Some("conversation-9")
        );

        // A blank id is refused rather than recorded as the conversation.
        assert!(state.record_remote_native_session(None, "  ").is_err());
        // A project with no binding records nothing.
        assert!(state
            .record_remote_native_session(Some("projects/missing"), "conversation-9")
            .is_err());
    }

    #[tokio::test]
    async fn a_release_is_reported_on_the_release_event() {
        let (_dir, state, _store) = remote_state();
        let bus = Arc::new(crate::event_bus::CollectingEventBus::new());
        state.set_event_bus(bus.clone());
        let released = RemoteRelease {
            project_id: "projects/goble".to_string(),
            provider: "modal".to_string(),
            sandbox_id: "sb-1".to_string(),
            snapshot_id: Some("im-1".to_string()),
            sandbox_kept: false,
            snapshot_support: "filesystem",
        };
        let outcome = Some(released);
        state.report_release(Ok(&outcome));

        let (_, payload) = bus
            .events()
            .into_iter()
            .find(|(name, _)| name == REMOTE_RELEASE_EVENT)
            .expect("the release was reported");
        assert_eq!(
            payload.get("status").and_then(|v| v.as_str()),
            Some("released")
        );
        assert_eq!(
            payload.get("project_id").and_then(|v| v.as_str()),
            Some("projects/goble")
        );
        assert_eq!(
            payload.get("snapshot_id").and_then(|v| v.as_str()),
            Some("im-1")
        );
        assert_eq!(
            payload.get("sandbox_kept").and_then(|v| v.as_bool()),
            Some(false)
        );
    }
}
