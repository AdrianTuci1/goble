//! Compose a remote sandbox from a project, an agent table, and the gateway.
//!
//! The provider adapter only knows how to create and destroy a sandbox. This
//! module supplies the worker's connection settings, derives the stable name
//! and workspace path from [`WorkspaceIdentity`], and returns the durable
//! binding the caller persists. Every pane that opens the project then attaches
//! to the one sandbox the binding names.

use std::collections::BTreeMap;

use goble_core::config::RemoteAgentConfig;
use goble_core::workspace_identity::WorkspaceIdentity;
use goble_persistence::{Project, ProjectSandbox};
use goble_remote::{SandboxError, SandboxProvider, SandboxRequest};

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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use goble_core::config::RemoteAgentProvider;
    use goble_harness_types::ProjectId;
    use goble_remote::{SandboxHandle, SandboxStatus};
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
}
