use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::SandboxError;
use crate::provider::{SandboxHandle, SandboxProvider, SandboxRequest, SandboxStatus};

/// Boat's public REST endpoint. Box became Boat in September 2026; the legacy
/// `https://ascii.dev/api/box/v1` routes still answer but carry deprecation
/// headers, so new deployments use the current endpoint.
pub const DEFAULT_API_URL: &str = "https://boat.dev/api/v1";

const PROVIDER: &str = "box";
const SANDBOXES_PATH: &str = "/sandboxes";

/// Box (now Boat) through its public REST API.
///
/// A sandbox is created with the worker environment, the requested TTL, the
/// configured environment, and the image as a named snapshot (`from`), then
/// renamed to the stable sandbox name with a follow-up `PATCH`. Account secrets
/// stay out: `noEnv` is always sent, while the worker's explicit environment
/// variables are passed.
pub struct BoxProvider {
    api_key: String,
    base_url: String,
    environment: Option<String>,
    image: Option<String>,
    client: reqwest::Client,
}

impl BoxProvider {
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
            environment: None,
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
                key: "BOX_API_KEY",
            })?;
        Ok(Self::new(
            api_key,
            config.api_url.as_deref().unwrap_or(DEFAULT_API_URL),
        )
        .with_environment(config.environment.clone())
        .with_image(config.image.clone()))
    }

    /// Set the Box environment attached to new boxes.
    pub fn with_environment(mut self, environment: Option<String>) -> Self {
        self.environment = environment
            .as_deref()
            .map(str::trim)
            .filter(|environment| !environment.is_empty())
            .map(str::to_string);
        self
    }

    /// Set the fallback snapshot name when a request does not carry an image.
    pub fn with_image(mut self, image: Option<String>) -> Self {
        self.image = image
            .as_deref()
            .map(str::trim)
            .filter(|image| !image.is_empty())
            .map(str::to_string);
        self
    }

    fn url(&self, suffix: &str) -> String {
        format!("{}{}{}", self.base_url, SANDBOXES_PATH, suffix)
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateBox<'a> {
    ttl_seconds: u64,
    no_env: bool,
    env: &'a BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    environment: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct RenameBox<'a> {
    name: &'a str,
}

#[derive(Debug, Deserialize)]
struct SandboxResponse {
    sandbox: SandboxInfo,
}

#[derive(Debug, Deserialize)]
struct SandboxInfo {
    id: String,
    #[serde(default)]
    state: Option<String>,
}

#[async_trait]
impl SandboxProvider for BoxProvider {
    fn kind(&self) -> &'static str {
        PROVIDER
    }

    async fn create(&self, request: &SandboxRequest) -> Result<SandboxHandle, SandboxError> {
        let image = if request.image.trim().is_empty() {
            self.image.as_deref()
        } else {
            Some(request.image.trim())
        };
        let payload = CreateBox {
            ttl_seconds: request.timeout_secs,
            no_env: true,
            env: &request.env,
            from: image,
            environment: self.environment.as_deref(),
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

        let created: SandboxResponse =
            serde_json::from_str(&body).map_err(|error| SandboxError::Parse {
                context: "Boat create response",
                message: format!("{error}: {body}"),
            })?;
        let sandbox_id = created.sandbox.id.trim();
        if sandbox_id.is_empty() {
            return Err(SandboxError::Parse {
                context: "Boat create response",
                message: "sandbox.id is empty".to_string(),
            });
        }

        // The create body has no name field; Boat exposes the stable name as a
        // `PATCH` on the new sandbox.
        let rename = RenameBox {
            name: &request.sandbox_name,
        };
        let response = self
            .send(
                self.client
                    .patch(self.url(&format!("/{sandbox_id}")))
                    .json(&rename),
            )
            .await?;
        let (status, body) = self.body_of(response).await?;
        if !(200..300).contains(&status) {
            return Err(SandboxError::Http {
                provider: PROVIDER,
                status,
                body,
            });
        }

        Ok(SandboxHandle::from_request(PROVIDER, sandbox_id, request))
    }

    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        let response = self
            .send(
                self.client
                    .post(self.url(&format!("/{}/stop", handle.sandbox_id)))
                    .json(&serde_json::json!({ "force": true })),
            )
            .await?;
        let (status, body) = self.body_of(response).await?;
        if status == 404 {
            // The box is already gone; deleting it again is a no-op.
            return Ok(());
        }
        if !(200..300).contains(&status) {
            return Err(SandboxError::Http {
                provider: PROVIDER,
                status,
                body,
            });
        }

        let response = self
            .send(
                self.client
                    .delete(self.url(&format!("/{}", handle.sandbox_id)))
                    .header("X-Ascii-Confirm-Delete", &handle.sandbox_id),
            )
            .await?;
        let (status, body) = self.body_of(response).await?;
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
        let info: SandboxResponse =
            serde_json::from_str(&body).map_err(|error| SandboxError::Parse {
                context: "Boat status response",
                message: format!("{error}: {body}"),
            })?;
        Ok(match info.sandbox.state.as_deref().unwrap_or_default() {
            "ready" | "idle" | "running" | "provisioned" => SandboxStatus::Running,
            "archived" | "archiving" => SandboxStatus::Paused,
            "error" | "cancelled" => SandboxStatus::Missing,
            _ => SandboxStatus::Unknown,
        })
    }
}
