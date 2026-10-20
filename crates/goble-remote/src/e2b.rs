use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::SandboxError;
use crate::provider::{SandboxHandle, SandboxProvider, SandboxRequest, SandboxStatus};

/// E2B's public REST endpoint.
pub const DEFAULT_API_URL: &str = "https://api.e2b.dev";

const PROVIDER: &str = "e2b";

/// E2B through its REST API.
///
/// E2B templates own the start command, so a request that carries an explicit
/// command is rejected instead of silently dropped.
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
}

#[async_trait]
impl SandboxProvider for E2BProvider {
    fn kind(&self) -> &'static str {
        PROVIDER
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
        if (200..300).contains(&status) {
            return Ok(SandboxStatus::Running);
        }
        Err(SandboxError::Http {
            provider: PROVIDER,
            status,
            body,
        })
    }
}
