use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::SandboxError;
use crate::provider::{
    ReleaseOutcome, SandboxBinding, SandboxHandle, SandboxProvider, SandboxRequest, SandboxStatus,
    SnapshotSupport,
};

/// E2B's public REST endpoint.
pub const DEFAULT_API_URL: &str = "https://api.e2b.dev";

const PROVIDER: &str = "e2b";

/// E2B's shortest resume timeout, in seconds. The API rejects anything below
/// it, and the create request keeps its own (longer) budget.
const MIN_RESUME_TIMEOUT_SECS: u64 = 300;

/// E2B through its REST API.
///
/// E2B templates own the start command, so a request that carries an explicit
/// command is rejected instead of silently dropped.
///
/// E2B is the pause/resume provider: its sandbox persistence keeps the whole
/// sandbox — filesystem *and* memory — and `POST /sandboxes/{id}/resume` brings
/// the same sandbox back, with the same id. The binding therefore keeps the
/// sandbox id rather than a snapshot; a release pauses instead of destroying,
/// and a resume re-uses the recorded sandbox before considering a fresh create.
/// See the [E2B persistence documentation](https://docs.e2b.dev/sandbox/persistence).
pub struct E2BProvider {
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl E2BProvider {
    /// Build an adapter for `api_key`, talking to `api_url` (empty uses
    /// [`DEFAULT_API_URL`]).
    pub fn new(api_key: impl Into<String>, api_url: impl Into<String>) -> Self {
        let api_url = api_url.into();
        let base_url = api_url.trim().trim_end_matches('/');
        let base_url = if base_url.is_empty() {
            DEFAULT_API_URL
        } else {
            base_url
        };
        Self {
            api_key: api_key.into(),
            base_url: base_url.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Build from a `[remote_agents.<name>]` table.
    pub fn from_config(
        config: &goble_core::config::RemoteAgentConfig,
    ) -> Result<Self, SandboxError> {
        let api_key = config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .ok_or(SandboxError::MissingCredential {
                provider: PROVIDER,
                key: "E2B_API_KEY",
            })?;
        Ok(Self::new(
            api_key,
            config.api_url.as_deref().unwrap_or(DEFAULT_API_URL),
        ))
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}/sandboxes{}", self.base_url, suffix)
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, SandboxError> {
        request
            .header("X-API-KEY", &self.api_key)
            .send()
            .await
            .map_err(|error| SandboxError::Transport {
                provider: PROVIDER,
                message: error.to_string(),
            })
    }

    async fn body_of(&self, response: reqwest::Response) -> Result<(u16, String), SandboxError> {
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|error| SandboxError::Transport {
                provider: PROVIDER,
                message: error.to_string(),
            })?;
        Ok((status, body))
    }

    /// Wake a paused sandbox back up, keeping its id.
    ///
    /// `Ok(None)` means E2B no longer knows the sandbox (404), so the caller
    /// starts a fresh one; a sandbox that is already running (409) is a success
    /// because it is exactly the state the caller wanted.
    async fn wake_sandbox(
        &self,
        sandbox_id: &str,
        timeout_secs: u64,
    ) -> Result<Option<()>, SandboxError> {
        let timeout = timeout_secs.max(MIN_RESUME_TIMEOUT_SECS);
        let response = self
            .send(
                self.client
                    .post(self.url(&format!("/{sandbox_id}/resume")))
                    .json(&serde_json::json!({
                        "timeout": timeout,
                        "autoPause": true,
                    })),
            )
            .await?;
        let status = response.status().as_u16();
        if status == 404 {
            return Ok(None);
        }
        let (status, body) = self.body_of(response).await?;
        if (200..300).contains(&status) || status == 409 {
            return Ok(Some(()));
        }
        Err(SandboxError::Http {
            provider: PROVIDER,
            status,
            body,
        })
    }
}

#[derive(Debug, Serialize)]
struct CreateSandbox<'a> {
    #[serde(rename = "templateID")]
    template_id: &'a str,
    #[serde(rename = "envVars")]
    env_vars: &'a BTreeMap<String, String>,
    metadata: &'a BTreeMap<String, String>,
    timeout: u64,
    #[serde(rename = "autoPause")]
    auto_pause: bool,
    #[serde(rename = "autoResume")]
    auto_resume: AutoResume,
}

#[derive(Debug, Serialize)]
struct AutoResume {
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct CreatedSandbox {
    #[serde(rename = "sandboxID")]
    sandbox_id: String,
    /// `running` / `paused` on providers that report it; E2B omits it on some
    /// responses, in which case a 200 is a live sandbox.
    #[serde(default)]
    state: Option<String>,
}

#[async_trait]
impl SandboxProvider for E2BProvider {
    fn kind(&self) -> &'static str {
        PROVIDER
    }

    /// E2B pauses the sandbox itself; there is no snapshot object to hand over.
    fn snapshot_support(&self) -> SnapshotSupport {
        SnapshotSupport::PauseResume
    }

    async fn create(&self, request: &SandboxRequest) -> Result<SandboxHandle, SandboxError> {
        if !request.command.is_empty() {
            return Err(SandboxError::InvalidRequest {
                message: format!(
                    "E2B sandboxes use the template entrypoint; command {:?} cannot be passed",
                    request.command
                ),
            });
        }

        let payload = CreateSandbox {
            template_id: &request.image,
            env_vars: &request.env,
            metadata: &request.metadata,
            timeout: request.timeout_secs,
            auto_pause: true,
            auto_resume: AutoResume { enabled: true },
        };
        let response = self
            .send(self.client.post(self.url("")).json(&payload))
            .await?;
        let (status, body) = self.body_of(response).await?;
        if !(200..300).contains(&status) {
            return Err(SandboxError::Http {
                provider: PROVIDER,
                status,
                body,
            });
        }

        let created: CreatedSandbox =
            serde_json::from_str(&body).map_err(|error| SandboxError::Parse {
                context: "E2B create response",
                message: format!("{error}: {body}"),
            })?;
        let sandbox_id = created.sandbox_id.trim();
        if sandbox_id.is_empty() {
            return Err(SandboxError::Parse {
                context: "E2B create response",
                message: "sandboxID is empty".to_string(),
            });
        }
        Ok(SandboxHandle::from_request(PROVIDER, sandbox_id, request))
    }

    /// Resume the recorded sandbox when E2B still knows it; create a fresh one
    /// when it does not. The harness conversation is resumed by the worker, not
    /// here: a paused E2B sandbox keeps its memory too, so nothing is lost by
    /// waking it.
    async fn resume(
        &self,
        request: &SandboxRequest,
        binding: &SandboxBinding,
    ) -> Result<SandboxHandle, SandboxError> {
        let recorded = binding
            .sandbox_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty());
        if let Some(sandbox_id) = recorded {
            if self
                .wake_sandbox(sandbox_id, request.timeout_secs)
                .await?
                .is_some()
            {
                return Ok(SandboxHandle::from_request(PROVIDER, sandbox_id, request));
            }
        }
        self.create(request).await
    }

    /// Pause instead of destroying: E2B keeps a paused sandbox indefinitely with
    /// its filesystem and memory, and the binding keeps its id to resume it.
    async fn release(&self, handle: &SandboxHandle) -> Result<ReleaseOutcome, SandboxError> {
        let response = self
            .send(
                self.client
                    .post(self.url(&format!("/{}/pause", handle.sandbox_id))),
            )
            .await?;
        let status = response.status().as_u16();
        if status == 404 {
            // The sandbox is gone, so there is nothing left to keep or destroy.
            return Ok(ReleaseOutcome::destroyed(None));
        }
        let (status, body) = self.body_of(response).await?;
        // 409 is E2B's "already paused", which is the state asked for.
        if (200..300).contains(&status) || status == 409 {
            return Ok(ReleaseOutcome::kept());
        }
        Err(SandboxError::Http {
            provider: PROVIDER,
            status,
            body,
        })
    }

    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        let response = self
            .send(
                self.client
                    .delete(self.url(&format!("/{}", handle.sandbox_id))),
            )
            .await?;
        let (status, body) = self.body_of(response).await?;
        // A sandbox that is already gone needs no further destruction.
        if (200..300).contains(&status) || status == 404 {
            return Ok(());
        }
        Err(SandboxError::Http {
            provider: PROVIDER,
            status,
            body,
        })
    }

    async fn status(&self, handle: &SandboxHandle) -> Result<SandboxStatus, SandboxError> {
        let response = self
            .send(
                self.client
                    .get(self.url(&format!("/{}", handle.sandbox_id))),
            )
            .await?;
        if response.status().as_u16() == 404 {
            return Ok(SandboxStatus::Missing);
        }
        let (status, body) = self.body_of(response).await?;
        if !(200..300).contains(&status) {
            return Err(SandboxError::Http {
                provider: PROVIDER,
                status,
                body,
            });
        }
        // A paused sandbox may be reported with a state field; without one a
        // 200 is a live sandbox, which is what the endpoint used to imply.
        let state = serde_json::from_str::<CreatedSandbox>(&body)
            .ok()
            .and_then(|sandbox| sandbox.state)
            .unwrap_or_default();
        Ok(match state.trim().to_ascii_lowercase().as_str() {
            "paused" => SandboxStatus::Paused,
            _ => SandboxStatus::Running,
        })
    }
}
