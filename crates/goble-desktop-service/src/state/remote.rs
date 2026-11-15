//! Compose a remote sandbox from a project, an agent table, and the gateway.
//!
//! The provider adapter only knows how to create and destroy a sandbox. This
//! module supplies the worker's connection settings, derives the stable name
//! and workspace path from [`WorkspaceIdentity`], and returns the durable
//! binding the caller persists. Every pane that opens the project then attaches
//! to the one sandbox the binding names.
//!
//! [`DesktopState::deploy_remote_agent`] is the one caller of all of it: it
//! resolves the `[remote_agents.<name>]` table and the `[remote]` gateway,
//! picks the project out of the persistence store, selects the adapter with
//! [`goble_remote::provider_for`], creates the sandbox, records the binding and
//! reports the outcome as [`REMOTE_DEPLOY_EVENT`].

use std::collections::BTreeMap;

use goble_core::config::{GobleConfig, RemoteAgentConfig};
use goble_core::workspace_identity::WorkspaceIdentity;
use goble_persistence::{CheckpointStore, Project, ProjectSandbox};
use goble_remote::{SandboxError, SandboxProvider, SandboxRequest};
use serde::Serialize;

use super::{binding_for_project, DesktopState};

/// The event a deploy reports on: `ready` with the binding it persisted, or
/// `failed` with the sentence naming what stopped it. The app refreshes Remote's
/// row from it.
pub const REMOTE_DEPLOY_EVENT: &str = "remote:deploy";

/// The environment a worker image reads at startup.
pub const GATEWAY_URL_ENV: &str = "GOBLE_GATEWAY_URL";
pub const WORKER_TOKEN_ENV: &str = "GOBLE_WORKER_TOKEN";
pub const WORKER_ID_ENV: &str = "GOBLE_WORKER_ID";
pub const PROJECT_ID_ENV: &str = "GOBLE_PROJECT_ID";
pub const WORKSPACE_PATH_ENV: &str = "GOBLE_WORKSPACE_PATH";
pub const REPO_URL_ENV: &str = "GOBLE_REPO_URL";
pub const REPO_BRANCH_ENV: &str = "GOBLE_REPO_BRANCH";

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

/// Create the sandbox and return the binding to persist. The binding carries
/// the provider handle; the stable name and path stay the same across rebinds.
pub async fn deploy(
    provider: &dyn SandboxProvider,
    deploy: &RemoteDeploy<'_>,
) -> Result<ProjectSandbox, SandboxError> {
    let request = sandbox_request(deploy)?;
    let handle = provider.create(&request).await?;
    Ok(ProjectSandbox {
        project_id: deploy.project.project_id.clone(),
        provider: provider.kind().to_string(),
        sandbox_name: handle.sandbox_name,
        sandbox_id: Some(handle.sandbox_id),
        workspace_path: handle.workspace_path,
        updated_at: chrono::Utc::now().to_rfc3339(),
    })
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
    /// Why it failed, as Remote's row shows it.
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

        // The binding is what a pane attaches through: one stable name and path
        // per project, whoever creates the sandbox.
        let host = WorkspaceIdentity::local_host();
        let stable = binding_for_project(&project, provider.kind(), &host);
        let mut plan = RemoteDeploy::new(&project, &agent, gateway_url, worker_token, &host);
        // A checkout with an origin is cloned inside the sandbox; one without
        // deploys to an empty workspace rather than failing outright.
        let repo_url = git_origin(&project.directory).await;
        if let Some(repo_url) = repo_url.as_deref() {
            plan = plan.with_repo(repo_url, None);
        }

        // The handle carries the provider's id; the name and the path stay the
        // ones the project derives, so a rebind finds the same workspace.
        let ProjectSandbox { sandbox_id, .. } = deploy(provider, &plan).await?;
        let binding = ProjectSandbox {
            sandbox_id,
            ..stable
        };
        store.upsert_project_sandbox(&binding)?;
        Ok(RemoteSandbox {
            provider: binding.provider,
            sandbox_name: binding.sandbox_name,
            workspace_path: binding.workspace_path,
        })
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
                        message: Some(&message),
                    },
                );
            }
        }
    }
}

/// The `[remote_agents.<name>]` table a deploy runs on, refused when it is
/// disabled or still missing a credential the provider needs.
fn remote_agent_for_deploy(
    config: &GobleConfig,
    name: &str,
) -> anyhow::Result<RemoteAgentConfig> {
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
fn resolve_project(
    store: &CheckpointStore,
    named: Option<&str>,
) -> anyhow::Result<Project> {
    let projects = store.list_projects()?;
    let Some(named) = named.map(str::trim).filter(|id| !id.is_empty()) else {
        return match projects.len() {
            0 => Err(anyhow::anyhow!(
                "no project to deploy yet; open a project in Goble first"
            )),
            1 => Ok(projects.into_iter().next().expect("one project")),
            _ => {
                let mut ids: Vec<String> =
                    projects.into_iter().map(|p| p.project_id.0).collect();
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

    struct FakeProvider {
        seen: Mutex<Option<SandboxRequest>>,
    }

    #[async_trait]
    impl SandboxProvider for FakeProvider {
        fn kind(&self) -> &'static str {
            "fake"
        }

        async fn create(&self, request: &SandboxRequest) -> Result<SandboxHandle, SandboxError> {
            *self.seen.lock().unwrap() = Some(request.clone());
            Ok(SandboxHandle::from_request("fake", "sb-1", request))
        }

        async fn destroy(&self, _handle: &SandboxHandle) -> Result<(), SandboxError> {
            Ok(())
        }

        async fn status(&self, _handle: &SandboxHandle) -> Result<SandboxStatus, SandboxError> {
            Ok(SandboxStatus::Running)
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
        let provider = FakeProvider {
            seen: Mutex::new(None),
        };
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

    fn fake() -> FakeProvider {
        FakeProvider {
            seen: Mutex::new(None),
        }
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

        // The binding is durable, with the handle's id and the stable name.
        let binding = store
            .load_project_sandbox(&ProjectId::new("projects/goble"))
            .unwrap()
            .expect("the binding was persisted");
        assert_eq!(binding.provider, "fake");
        assert_eq!(binding.sandbox_name, "goble-goble-2fbfef8fa780");
        assert_eq!(binding.sandbox_id.as_deref(), Some("sb-1"));
        assert_eq!(binding.workspace_path, "/workspace/goble");

        let report = bus
            .events()
            .into_iter()
            .find(|(name, _)| name == REMOTE_DEPLOY_EVENT)
            .map(|(_, payload)| payload)
            .expect("the deploy was reported");
        assert_eq!(
            report.get("status").and_then(|v| v.as_str()),
            Some("ready")
        );
        assert_eq!(
            report.get("sandbox_name").and_then(|v| v.as_str()),
            Some("goble-goble-2fbfef8fa780")
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
        assert!(provider.seen.lock().unwrap().is_none(), "nothing was created");
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
        assert!(provider.seen.lock().unwrap().is_none(), "nothing was created");
    }

    #[tokio::test]
    async fn an_empty_gateway_setting_is_refused_before_the_provider() {
        let (_dir, state, _store) =
            remote_state_with(|config| config.remote.worker_token = Some("  ".to_string()));
        let provider = fake();
        let error = state
            .deploy_remote_agent_with(&provider, None, "local")
            .await
            .expect_err("a deploy needs the gateway token");
        assert!(
            error.to_string().contains("worker token"),
            "the sentence names the setting: {error}"
        );

        let (_dir, state, _store) =
            remote_state_with(|config| config.remote.gateway_url = Some(String::new()));
        let error = state
            .deploy_remote_agent_with(&provider, None, "local")
            .await
            .expect_err("a deploy needs the gateway URL");
        assert!(
            error.to_string().contains("gateway URL"),
            "the sentence names the setting: {error}"
        );
        assert!(provider.seen.lock().unwrap().is_none(), "nothing was created");
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
        assert!(provider.seen.lock().unwrap().is_none(), "nothing was created");
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
}
