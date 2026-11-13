//! Google Cloud Run, through the Cloud Run Admin API v2.
//!
//! Cloud Run is request-scoped: a job runs one container task and exits, with
//! no attachable PTY and no pause/resume. That is the shape of a one-shot,
//! non-interactive worker run, which is what this adapter provisions. An
//! interactive workspace on GCP is a GCE VM or a GKE pod running
//! `goblin-worker`, reached through the `ssh` adapter or the relay.
//!
//! The config table names the GCP project in `environment` and the Cloud Run
//! location in `region`; `image` is the worker image, which must live in a
//! registry Cloud Run can pull from (Artifact Registry or GCR). The bearer
//! token comes from `api_key` when it is set; otherwise the adapter mints one
//! with `gcloud auth print-access-token`, using the session the machine already
//! has, and caches it for fifty minutes.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::command::{CommandRunner, ProcessCommandRunner};
use crate::error::SandboxError;
use crate::provider::{SandboxHandle, SandboxProvider, SandboxRequest, SandboxStatus};

/// Cloud Run's regional endpoint.
pub const DEFAULT_API_URL: &str = "https://run.googleapis.com";

const PROVIDER: &str = "cloud_run";
const GCLOUD: &str = "gcloud";

/// Cloud Run's own ceiling for a job task timeout, one day.
const MAX_TIMEOUT_SECS: u64 = 86_400;

/// Cloud Run job ids stop at 49 characters.
const MAX_JOB_ID: usize = 49;

/// How long a minted `gcloud` token is reused. Google's tokens last an hour;
/// the margin keeps a token from expiring in flight.
const TOKEN_TTL: Duration = Duration::from_secs(50 * 60);

/// The job id Cloud Run accepts for a stable workspace name.
///
/// A job id is lowercase alphanumeric and hyphens, starts with a letter and
/// stops at 49 characters; a workspace name is `goble-<slug>-<key12>` and a
/// long slug can pass that. The truncation keeps the `goble-` prefix and the
/// key, so two long slugs that agree on their first characters still name two
/// jobs.
pub fn job_id(sandbox_name: &str) -> String {
    let sanitized: String = sandbox_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let mut name = sanitized.trim_matches('-').to_string();
    while name.contains("--") {
        name = name.replace("--", "-");
    }
    if name.is_empty() {
        return "goble-job".to_string();
    }
    if !name.starts_with(|c: char| c.is_ascii_lowercase()) {
        name = format!("goble-{name}");
    }
    if name.len() <= MAX_JOB_ID {
        return name;
    }

    let tail = name.rsplit('-').next().unwrap_or_default().to_string();
    let prefix = "goble-";
    let budget = MAX_JOB_ID
        .saturating_sub(prefix.len())
        .saturating_sub(tail.len())
        .saturating_sub(1);
    let head_start = if name.starts_with(prefix) { prefix.len() } else { 0 };
    let head: String = name[head_start..].chars().take(budget).collect();
    let head = head.trim_end_matches('-');
    if tail.is_empty() || head.is_empty() {
        let truncated: String = name.chars().take(MAX_JOB_ID).collect();
        let truncated = truncated.trim_end_matches('-');
        return truncated.to_string();
    }
    format!("{prefix}{head}-{tail}")
}

/// Labels Cloud Run accepts: lowercase alphanumerics, `_` and `-`, at most 63
/// characters, starting and ending with an alphanumeric.
fn label(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else if c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches(|c: char| c == '-' || c == '_');
    let capped: String = trimmed.chars().take(63).collect();
    capped
        .trim_end_matches(|c: char| c == '-' || c == '_')
        .to_string()
}

/// The job labels for a request's metadata. A key that sanitizes to nothing is
/// dropped rather than sent empty.
fn labels(metadata: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    for (key, value) in metadata {
        let (key, value) = (label(key), label(value));
        if key.is_empty() || value.is_empty() {
            continue;
        }
        labels.insert(key, value);
    }
    labels
}

#[derive(Debug, Clone)]
struct CachedToken {
    value: String,
    expires: Instant,
}

/// Cloud Run jobs behind [`SandboxProvider`].
pub struct CloudRunProvider {
    project: String,
    location: String,
    api_key: Option<String>,
    base_url: String,
    client: reqwest::Client,
    runner: Arc<dyn CommandRunner>,
    token: Mutex<Option<CachedToken>>,
}

impl CloudRunProvider {
    /// Build from a `[remote_agents.<name>]` table, minting tokens with the
    /// machine's own `gcloud`.
    pub fn from_config(
        config: &goble_core::config::RemoteAgentConfig,
    ) -> Result<Self, SandboxError> {
        Self::with_runner(config, Arc::new(ProcessCommandRunner))
    }

    /// Build with an injected command runner, so a test can answer the
    /// `gcloud` call without one being installed.
    pub fn with_runner(
        config: &goble_core::config::RemoteAgentConfig,
        runner: Arc<dyn CommandRunner>,
    ) -> Result<Self, SandboxError> {
        let project = filled(config.environment.as_deref()).ok_or(SandboxError::MissingCredential {
            provider: PROVIDER,
            key: "GOOGLE_CLOUD_PROJECT",
        })?;
        let location = filled(config.region.as_deref()).ok_or(SandboxError::MissingCredential {
            provider: PROVIDER,
            key: "GOOGLE_CLOUD_REGION",
        })?;
        let base_url = config
            .api_url
            .as_deref()
            .map(str::trim)
            .map(|url| url.trim_end_matches('/'))
            .filter(|url| !url.is_empty())
            .unwrap_or(DEFAULT_API_URL)
            .to_string();
        Ok(Self {
            project: project.to_string(),
            location: location.to_string(),
            api_key: filled(config.api_key.as_deref()).map(str::to_string),
            base_url,
            client: reqwest::Client::new(),
            runner,
            token: Mutex::new(None),
        })
    }

    fn jobs_url(&self) -> String {
        format!(
            "{}/v2/projects/{}/locations/{}/jobs",
            self.base_url, self.project, self.location
        )
    }

    fn job_url(&self, job: &str) -> String {
        format!("{}/{job}", self.jobs_url())
    }

    /// An execution resource's own name (`projects/…/executions/…`).
    fn execution_url(&self, name: &str) -> String {
        format!("{}/v2/{}", self.base_url, name.trim_start_matches('/'))
    }

    /// The bearer token for this call: `api_key` when the table carries one,
    /// otherwise a cached token minted from the machine's gcloud session.
    async fn bearer(&self) -> Result<String, SandboxError> {
        if let Some(key) = &self.api_key {
            return Ok(key.clone());
        }
        if let Some(cached) = self.token.lock().ok().and_then(|slot| slot.clone()) {
            if cached.expires > Instant::now() {
                return Ok(cached.value);
            }
        }
        let args = ["auth".to_string(), "print-access-token".to_string()];
        let output = self.runner.run(GCLOUD, &args, None, &BTreeMap::new()).await?;
        if !output.success() {
            return Err(SandboxError::Command {
                command: GCLOUD.to_string(),
                message: format!(
                    "no access token: {}. Run `gcloud auth login`, or set `api_key` in the \
                     agent table to a bearer token",
                    output.failure_message()
                ),
            });
        }
        let token = output.stdout.trim().to_string();
        if token.is_empty() {
            return Err(SandboxError::Command {
                command: GCLOUD.to_string(),
                message: "gcloud printed an empty access token".to_string(),
            });
        }
        if let Ok(mut slot) = self.token.lock() {
            *slot = Some(CachedToken {
                value: token.clone(),
                expires: Instant::now() + TOKEN_TTL,
            });
        }
        Ok(token)
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, SandboxError> {
        let token = self.bearer().await?;
        request
            .bearer_auth(token)
            .send()
            .await
            .map_err(|error| SandboxError::Transport {
                provider: PROVIDER,
                message: error.to_string(),
            })
    }

    async fn read(&self, response: reqwest::Response) -> Result<(u16, String), SandboxError> {
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

    fn job_body<'a>(&self, request: &'a SandboxRequest) -> Job<'a> {
        Job {
            labels: labels(&request.metadata),
            template: ExecutionTemplate {
                template: TaskTemplate {
                    containers: vec![Container {
                        image: &request.image,
                        env: request
                            .env
                            .iter()
                            .map(|(name, value)| EnvVar {
                                name: name.as_str(),
                                value: value.as_str(),
                            })
                            .collect(),
                        args: request.command.iter().map(String::as_str).collect(),
                    }],
                    timeout: format!("{}s", request.timeout_secs.clamp(1, MAX_TIMEOUT_SECS)),
                    max_retries: 0,
                },
                task_count: 1,
                parallelism: 1,
            },
        }
    }
}

fn filled(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[derive(Debug, Serialize)]
struct Job<'a> {
    labels: BTreeMap<String, String>,
    template: ExecutionTemplate<'a>,
}

#[derive(Debug, Serialize)]
struct ExecutionTemplate<'a> {
    template: TaskTemplate<'a>,
    #[serde(rename = "taskCount")]
    task_count: u32,
    parallelism: u32,
}

#[derive(Debug, Serialize)]
struct TaskTemplate<'a> {
    containers: Vec<Container<'a>>,
    timeout: String,
    #[serde(rename = "maxRetries")]
    max_retries: u32,
}

#[derive(Debug, Serialize)]
struct Container<'a> {
    image: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    env: Vec<EnvVar<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    args: Vec<&'a str>,
}

#[derive(Debug, Serialize)]
struct EnvVar<'a> {
    name: &'a str,
    value: &'a str,
}

#[derive(Debug, Default, Deserialize)]
struct Execution {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "completionTime")]
    completion_time: Option<String>,
    #[serde(default)]
    status: Option<ExecutionStatus>,
}

impl Execution {
    /// A one-shot execution has no paused state: it is running until it
    /// completes, and a completed execution leaves nothing to attach to.
    fn is_finished(&self) -> bool {
        if self
            .completion_time
            .as_deref()
            .is_some_and(|time| !time.trim().is_empty())
        {
            return true;
        }
        self.status.as_ref().is_some_and(|status| {
            status.conditions.iter().any(|condition| {
                condition.kind == "Completed" && condition.status == "True"
            })
        })
    }
}

#[derive(Debug, Default, Deserialize)]
struct ExecutionStatus {
    #[serde(default)]
    conditions: Vec<Condition>,
}

#[derive(Debug, Default, Deserialize)]
struct Condition {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    status: String,
}

#[async_trait]
impl SandboxProvider for CloudRunProvider {
    fn kind(&self) -> &'static str {
        PROVIDER
    }

    async fn create(&self, request: &SandboxRequest) -> Result<SandboxHandle, SandboxError> {
        let job = job_id(&request.sandbox_name);
        let body = self.job_body(request);

        // A stable workspace name reuses one job: the first run creates it and
        // every later run replaces its template, so no revision piles up.
        let existing = self.send(self.client.get(self.job_url(&job))).await?;
        let (status, existing_body) = self.read(existing).await?;
        let (method, url) = match status {
            404 => (reqwest::Method::POST, format!("{}?jobId={job}", self.jobs_url())),
            200..=299 => (reqwest::Method::PUT, self.job_url(&job)),
            _ => {
                return Err(SandboxError::Http {
                    provider: PROVIDER,
                    status,
                    body: existing_body,
                })
            }
        };
        let response = self.send(self.client.request(method, url).json(&body)).await?;
        let (status, body) = self.read(response).await?;
        if !(200..300).contains(&status) {
            return Err(SandboxError::Http {
                provider: PROVIDER,
                status,
                body,
            });
        }

        let run_url = format!("{}:run", self.job_url(&job));
        let response = self
            .send(self.client.post(run_url).json(&serde_json::json!({})))
            .await?;
        let (status, body) = self.read(response).await?;
        if !(200..300).contains(&status) {
            return Err(SandboxError::Http {
                provider: PROVIDER,
                status,
                body,
            });
        }
        let execution: Execution =
            serde_json::from_str(&body).map_err(|error| SandboxError::Parse {
                context: "Cloud Run execution response",
                message: format!("{error}: {body}"),
            })?;
        let execution = execution.name.trim();
        if execution.is_empty() {
            return Err(SandboxError::Parse {
                context: "Cloud Run execution response",
                message: "name is empty".to_string(),
            });
        }
        Ok(SandboxHandle::from_request(PROVIDER, execution, request))
    }

    async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
        // Deleting the job stops its executions; a job that is already gone
        // needs no further destruction.
        let job = job_id(&handle.sandbox_name);
        let url = format!("{}?force=true", self.job_url(&job));
        let response = self.send(self.client.delete(url)).await?;
        let (status, body) = self.read(response).await?;
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
        let execution = handle.sandbox_id.trim();
        if execution.is_empty() {
            return Ok(SandboxStatus::Missing);
        }
        let response = self
            .send(self.client.get(self.execution_url(execution)))
            .await?;
        if response.status().as_u16() == 404 {
            return Ok(SandboxStatus::Missing);
        }
        let (status, body) = self.read(response).await?;
        if !(200..300).contains(&status) {
            return Err(SandboxError::Http {
                provider: PROVIDER,
                status,
                body,
            });
        }
        let execution: Execution =
            serde_json::from_str(&body).map_err(|error| SandboxError::Parse {
                context: "Cloud Run execution response",
                message: format!("{error}: {body}"),
            })?;
        Ok(if execution.is_finished() {
            SandboxStatus::Missing
        } else {
            SandboxStatus::Running
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_workspace_name_is_its_own_job_id() {
        assert_eq!(job_id("goble-goble-2fbfef8fa780"), "goble-goble-2fbfef8fa780");
        assert_eq!(job_id("Goble_Goble"), "goble-goble");
        assert_eq!(job_id("  "), "goble-job");
        assert_eq!(job_id("9lives"), "goble-9lives");
    }

    #[test]
    fn a_long_slug_keeps_the_prefix_and_the_key_inside_49_characters() {
        let long = format!("goble-{}-2fbfef8fa780", "a-very-long-workspace-slug".repeat(4));
        let id = job_id(&long);
        assert!(id.len() <= MAX_JOB_ID, "{id} is {} chars", id.len());
        assert!(id.starts_with("goble-"), "{id}");
        assert!(id.ends_with("-2fbfef8fa780"), "{id}");
        assert!(!id.contains("--"), "{id}");
        assert!(id.starts_with(|c: char| c.is_ascii_lowercase()), "{id}");
        assert!(
            id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{id}"
        );

        // Two slugs that agree on their first characters still differ.
        let sibling = format!("goble-{}-2fbfef8fa780", "a-very-long-workspace-slug".repeat(4).replace('a', "b"));
        assert_ne!(job_id(&sibling), id);
    }

    #[test]
    fn labels_are_sanitized_to_what_cloud_run_accepts() {
        let metadata = BTreeMap::from([
            ("goble.project".to_string(), "projects/goble".to_string()),
            ("goble.workspace".to_string(), "Goble@Adrians-Mac".to_string()),
            ("".to_string(), "x".to_string()),
        ]);
        let labels = labels(&metadata);
        assert_eq!(
            labels.get("goble-project").map(String::as_str),
            Some("projects-goble")
        );
        assert_eq!(
            labels.get("goble-workspace").map(String::as_str),
            Some("goble-adrians-mac")
        );
        assert_eq!(labels.len(), 2, "a key with no usable characters is dropped");
    }

    #[test]
    fn a_finished_execution_reports_missing() {
        let completed = Execution {
            name: "e".to_string(),
            completion_time: Some("2026-01-01T00:00:00Z".to_string()),
            status: None,
        };
        assert!(completed.is_finished());

        let condition = Execution {
            name: "e".to_string(),
            completion_time: None,
            status: Some(ExecutionStatus {
                conditions: vec![Condition {
                    kind: "Completed".to_string(),
                    status: "True".to_string(),
                }],
            }),
        };
        assert!(condition.is_finished());

        let running = Execution {
            name: "e".to_string(),
            completion_time: None,
            status: Some(ExecutionStatus {
                conditions: vec![Condition {
                    kind: "Ready".to_string(),
                    status: "True".to_string(),
                }],
            }),
        };
        assert!(!running.is_finished());
    }
}
