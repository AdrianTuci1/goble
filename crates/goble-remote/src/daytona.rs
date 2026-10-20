use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::SandboxError;
use crate::provider::{SandboxHandle, SandboxProvider, SandboxRequest, SandboxStatus};

/// Daytona's public REST endpoint.
pub const DEFAULT_API_URL: &str = "https://app.daytona.io/api";

const PROVIDER: &str = "daytona";
const SANDBOX_PATH: &str = "/sandbox";

/// Daytona through its public REST API.
///
/// A create request sends the stable sandbox name, the image as a Dockerfile
/// `FROM` build (or as a snapshot name when [`request.image`] is not a registry
/// reference), the worker environment, labels, the configured region as
/// `target`, and the requested timeout as a Daytona auto-stop interval.
///
/// [`request.image`]: crate::provider::SandboxRequest::image
pub struct DaytonaProvider {
    api_key: String,
    base_url: String,
    region: Option<String>,
    image: Option<String>,
    client: reqwest::Client,
}

impl DaytonaProvider {
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
            region: None,
            image: None,
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
                key: "DAYTONA_API_KEY",
            })?;
        Ok(Self::new(
            api_key,
            config.api_url.as_deref().unwrap_or(DEFAULT_API_URL),
        )
        .with_region(config.region.clone())
        .with_image(config.image.clone()))
    }

    /// Set the region sent as Daytona's `target`.
    pub fn with_region(mut self, region: Option<String>) -> Self {
        self.region = region
            .as_deref()
            .map(str::trim)
            .filter(|region| !region.is_empty())
            .map(str::to_string);
        self
    }

    /// Set the fallback image when a request does not carry one.
    pub fn with_image(mut self, image: Option<String>) -> Self {
        self.image = image
            .as_deref()
            .map(str::trim)
            .filter(|image| !image.is_empty())
            .map(str::to_string);
        self
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}{}{}", self.base_url, SANDBOX_PATH, suffix)
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, SandboxError> {
        request
            .header("Authorization", format!("Bearer {}", self.api_key))
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

/// A registry reference is built into a Dockerfile, matching the official SDK's
/// `Image.base`; anything else is treated as a Daytona snapshot name.
fn is_registry_reference(image: &str) -> bool {
    image.contains('/') || image.contains(':') || image.contains('@')
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateSandbox<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    build_info: Option<BuildInfo>,
    env: &'a BTreeMap<String, String>,
    labels: &'a BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<&'a str>,
    auto_stop_interval: u64,
    auto_delete_interval: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildInfo {
    dockerfile_content: String,
}

#[derive(Debug, Deserialize)]
struct CreatedSandbox {
    id: String,
}

#[derive(Debug, Deserialize)]
struct SandboxState {
    #[serde(default)]
    state: Option<String>,
}

#[async_trait]
impl SandboxProvider for DaytonaProvider {
    fn kind(&self) -> &'static str {
        PROVIDER
    }

    async fn create(&self, request: &SandboxRequest) -> Result<SandboxHandle, SandboxError> {
        let image = if request.image.trim().is_empty() {
            self.image.as_deref()
        } else {
            Some(request.image.trim())
        };
        let (snapshot, build_info) = match image {
            Some(image) if is_registry_reference(image) => (
                None,
                Some(BuildInfo {
                    dockerfile_content: format!("FROM {image}\n"),
                }),
            ),
            Some(image) => (Some(image), None),
            None => (None, None),
        };

        let payload = CreateSandbox {
            name: &request.sandbox_name,
            snapshot,
            build_info,
            env: &request.env,
            labels: &request.metadata,
            target: self.region.as_deref(),
            auto_stop_interval: request.timeout_secs.div_ceil(60),
            auto_delete_interval: 0,
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
                context: "Daytona create response",
                message: format!("{error}: {body}"),
            })?;
        let sandbox_id = created.id.trim();
        if sandbox_id.is_empty() {
            return Err(SandboxError::Parse {
                context: "Daytona create response",
                message: "id is empty".to_string(),
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
        if !(200..300).contains(&status) {
            return Err(SandboxError::Http {
                provider: PROVIDER,
                status,
                body,
            });
        }
        let state: SandboxState =
            serde_json::from_str(&body).map_err(|error| SandboxError::Parse {
                context: "Daytona status response",
                message: format!("{error}: {body}"),
            })?;
        Ok(match state.state.as_deref().unwrap_or_default() {
            "running" | "started" => SandboxStatus::Running,
            "stopped" | "paused" => SandboxStatus::Paused,
            _ => SandboxStatus::Unknown,
        })
    }
}
