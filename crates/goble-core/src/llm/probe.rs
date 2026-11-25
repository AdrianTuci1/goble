//! The real connection probe for a `[model.<slug>]` entry.
//!
//! Settings -> Providers' "Test connection" must report what the endpoint
//! actually says, never a canned success. One request per backend:
//!
//! - OpenAI-compatible (OpenAI, DeepSeek, OpenRouter, any custom `base_url`):
//!   `GET {base}/models` with `Authorization: Bearer <key>`;
//! - Anthropic: `GET {base}/v1/models` with `x-api-key` and `anthropic-version`;
//! - Ollama: `GET {base}/api/tags` (no auth).
//!
//! A non-2xx answer is reported verbatim — status and a body snippet — because
//! that is the only place the reason exists (a bad key is a 401, a wrong URL is
//! a 404). A transport failure (DNS, TLS, timeout) is an error. Timeout is
//! [`PROBE_TIMEOUT`]; the probe is only ever called from an explicit Test, never
//! at startup.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::ModelConfig;

/// How long a probe may take before it is reported as a timeout.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// The most model names a report keeps (a provider may list hundreds).
const MAX_MODELS: usize = 100;

/// The most body bytes kept in a failure note.
const MAX_NOTE_BODY: usize = 400;

/// Which wire format the probe speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeBackend {
    OpenAi,
    Anthropic,
    Ollama,
}

impl ProbeBackend {
    /// The name a settings row shows.
    pub fn label(self) -> &'static str {
        match self {
            ProbeBackend::OpenAi => "openai",
            ProbeBackend::Anthropic => "anthropic",
            ProbeBackend::Ollama => "ollama",
        }
    }

    /// Pick the backend the way the resolver does: an explicit provider hint
    /// wins, otherwise the configured `base_url` decides, and a plain custom
    /// endpoint is treated as OpenAI-compatible.
    pub fn for_model(provider_hint: Option<&str>, base_url: Option<&str>) -> Self {
        match provider_hint
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("anthropic") => return ProbeBackend::Anthropic,
            Some("ollama") => return ProbeBackend::Ollama,
            Some("openai") | Some("openrouter") | Some("deepseek") | Some("") => {
                // Fall through to the base_url check: a DeepSeek/OpenAI hint with
                // an Ollama base URL is still Ollama.
            }
            _ => {}
        }
        let base = base_url.unwrap_or_default().to_ascii_lowercase();
        if base.contains("anthropic") {
            ProbeBackend::Anthropic
        } else if base.contains("ollama") || base.contains(":11434") {
            ProbeBackend::Ollama
        } else {
            ProbeBackend::OpenAi
        }
    }
}

/// What one probe found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProbeReport {
    pub backend: ProbeBackend,
    /// The URL actually requested.
    pub url: String,
    /// Whether the endpoint answered 2xx with a model list.
    pub ok: bool,
    /// The HTTP status, when a response was received.
    pub status: Option<u16>,
    /// Round-trip time to the response headers.
    pub latency_ms: u64,
    /// The model ids the endpoint listed (capped).
    pub models: Vec<String>,
    /// What the settings row shows: the count, the HTTP error, or a note.
    pub note: String,
}

impl ProbeReport {
    /// A one-line status for a row's status line.
    pub fn line(&self) -> String {
        if self.ok {
            format!(
                "connected · {} ms · {} model(s)",
                self.latency_ms,
                self.models.len()
            )
        } else {
            self.note.clone()
        }
    }
}

/// Why a probe could not be attempted (or could not complete at the transport
/// level). An HTTP failure is **not** one of these: it is a report with
/// `ok: false` and the status, so the UI can show it.
#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    /// The model has no inline key and no `api_key_env` value set.
    #[error("no API key configured for this model")]
    MissingCredential,
    /// The configured base URL is not a usable endpoint.
    #[error("invalid endpoint: {0}")]
    InvalidEndpoint(String),
    /// The request failed before a response (DNS, TLS, connect, timeout).
    #[error("the endpoint did not answer: {0}")]
    Transport(String),
    /// A 2xx body was not the model list the backend's API returns.
    #[error("the endpoint answered with something that is not a model list: {0}")]
    UnexpectedBody(String),
}

/// Probe one configured model. `provider_hint` is the provider the caller
/// resolved for the entry (the same rule `resolve_llm_provider` uses); it may be
/// `None` for a custom endpoint.
pub async fn probe_model(
    model: &ModelConfig,
    provider_hint: Option<&str>,
) -> Result<ProbeReport, ProbeError> {
    let backend = ProbeBackend::for_model(provider_hint, model.base_url.as_deref());
    let url = models_url(backend, provider_hint, model.base_url.as_deref());
    let key = probe_key(model);
    if backend != ProbeBackend::Ollama && key.is_none() {
        return Err(ProbeError::MissingCredential);
    }
    probe(backend, &url, key.as_deref()).await
}

/// The model-list URL for a backend and a configured base URL. Pure, so the URL
/// rule is tested without a network.
pub fn models_url(
    backend: ProbeBackend,
    provider_hint: Option<&str>,
    base_url: Option<&str>,
) -> String {
    let base = base_url
        .map(str::trim)
        .filter(|base| !base.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_base(backend, provider_hint).to_string());
    let base = base.trim_end_matches('/');
    match backend {
        ProbeBackend::Ollama => {
            let base = base.strip_suffix("/v1").unwrap_or(base);
            format!("{base}/api/tags")
        }
        ProbeBackend::Anthropic => {
            if base.ends_with("/v1") {
                format!("{base}/models")
            } else {
                format!("{base}/v1/models")
            }
        }
        ProbeBackend::OpenAi => format!("{base}/models"),
    }
}

fn default_base(backend: ProbeBackend, provider_hint: Option<&str>) -> &'static str {
    match backend {
        ProbeBackend::Anthropic => "https://api.anthropic.com",
        ProbeBackend::Ollama => "http://localhost:11434",
        ProbeBackend::OpenAi => match provider_hint
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("deepseek") => "https://api.deepseek.com/v1",
            _ => "https://api.openai.com/v1",
        },
    }
}

/// The key a probe sends: the inline key when it is set, else the value of the
/// `api_key_env` variable, looked up at probe time.
fn probe_key(model: &ModelConfig) -> Option<String> {
    if let Some(key) = model.resolved_key() {
        return Some(key.to_string());
    }
    let name = model
        .api_key_env
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())?;
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

async fn probe(
    backend: ProbeBackend,
    url: &str,
    key: Option<&str>,
) -> Result<ProbeReport, ProbeError> {
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .connect_timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|error| ProbeError::Transport(error.to_string()))?;
    let mut request = client.get(url);
    match backend {
        ProbeBackend::OpenAi => {
            let key = key.ok_or(ProbeError::MissingCredential)?;
            request = request.header("Authorization", format!("Bearer {key}"));
        }
        ProbeBackend::Anthropic => {
            let key = key.ok_or(ProbeError::MissingCredential)?;
            request = request
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01");
        }
        ProbeBackend::Ollama => {}
    }

    let started = Instant::now();
    let response = request
        .send()
        .await
        .map_err(|error| ProbeError::Transport(format!("{error}")))?;
    let latency_ms = started.elapsed().as_millis() as u64;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| ProbeError::Transport(error.to_string()))?;

    if !status.is_success() {
        return Ok(ProbeReport {
            backend,
            url: url.to_string(),
            ok: false,
            status: Some(status.as_u16()),
            latency_ms,
            models: Vec::new(),
            note: format!("HTTP {} · {}", status.as_u16(), snippet(&body)),
        });
    }

    let models = parse_models(backend, &body)?;
    let note = if models.is_empty() {
        "the endpoint answered but listed no models".to_string()
    } else {
        format!("{} model(s) available", models.len())
    };
    Ok(ProbeReport {
        backend,
        url: url.to_string(),
        ok: true,
        status: Some(status.as_u16()),
        latency_ms,
        models,
        note,
    })
}

fn snippet(body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return "(empty body)".to_string();
    }
    let mut short: String = body.chars().take(MAX_NOTE_BODY).collect();
    if body.chars().count() > MAX_NOTE_BODY {
        short.push('…');
    }
    short.replace('\n', " ")
}

fn parse_models(backend: ProbeBackend, body: &str) -> Result<Vec<String>, ProbeError> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| ProbeError::UnexpectedBody(error.to_string()))?;
    let entries = match backend {
        ProbeBackend::Ollama => value.get("models").and_then(|value| value.as_array()),
        ProbeBackend::OpenAi | ProbeBackend::Anthropic => {
            value.get("data").and_then(|value| value.as_array())
        }
    };
    let Some(entries) = entries else {
        return Err(ProbeError::UnexpectedBody(snippet(body)));
    };
    let mut models = Vec::new();
    for entry in entries {
        let name = match backend {
            ProbeBackend::Ollama => entry.get("name").and_then(|value| value.as_str()),
            ProbeBackend::OpenAi | ProbeBackend::Anthropic => {
                entry.get("id").and_then(|value| value.as_str())
            }
        };
        if let Some(name) = name {
            models.push(name.to_string());
            if models.len() >= MAX_MODELS {
                break;
            }
        }
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// One-shot HTTP server: answers the first request with `status`/`body` and
    /// hands the raw request text back over a channel.
    async fn mock_server(
        status: u16,
        body: &'static str,
    ) -> (String, tokio::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("addr");
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                match stream.read(&mut buffer).await {
                    Ok(0) => break,
                    Ok(read) => {
                        request.extend_from_slice(&buffer[..read]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&request).to_string()).await;
            let response = format!(
                "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
        });
        (format!("http://{address}"), rx)
    }

    fn entry(base_url: &str, api_key: Option<&str>) -> ModelConfig {
        ModelConfig {
            model: "test-model".to_string(),
            name: None,
            base_url: Some(base_url.to_string()),
            api_key: api_key.map(str::to_string),
            api_key_env: None,
            max_completion_tokens: None,
            context_window: None,
            hidden: None,
        }
    }

    /// A URL nothing is listening on: bind, take the address, drop the listener.
    async fn closed_port() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("addr");
        drop(listener);
        format!("http://{address}")
    }

    #[test]
    fn the_backend_is_picked_from_the_hint_then_the_base_url() {
        assert_eq!(
            ProbeBackend::for_model(Some("anthropic"), Some("https://api.openai.com/v1")),
            ProbeBackend::Anthropic
        );
        assert_eq!(
            ProbeBackend::for_model(None, Some("https://api.anthropic.com/v1")),
            ProbeBackend::Anthropic
        );
        assert_eq!(
            ProbeBackend::for_model(None, Some("http://localhost:11434")),
            ProbeBackend::Ollama
        );
        assert_eq!(
            ProbeBackend::for_model(Some("deepseek"), Some("https://api.deepseek.com/v1")),
            ProbeBackend::OpenAi
        );
        assert_eq!(
            ProbeBackend::for_model(Some("custom"), None),
            ProbeBackend::OpenAi
        );
        // A hint with an Ollama base URL is still Ollama.
        assert_eq!(
            ProbeBackend::for_model(Some("openai"), Some("http://localhost:11434/v1")),
            ProbeBackend::Ollama
        );
    }

    #[test]
    fn the_models_url_follows_the_backend() {
        assert_eq!(
            models_url(
                ProbeBackend::OpenAi,
                None,
                Some("https://api.openai.com/v1/")
            ),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            models_url(ProbeBackend::OpenAi, Some("deepseek"), None),
            "https://api.deepseek.com/v1/models"
        );
        assert_eq!(
            models_url(ProbeBackend::Anthropic, Some("anthropic"), None),
            "https://api.anthropic.com/v1/models"
        );
        assert_eq!(
            models_url(
                ProbeBackend::Anthropic,
                Some("anthropic"),
                Some("https://api.anthropic.com/v1")
            ),
            "https://api.anthropic.com/v1/models"
        );
        assert_eq!(
            models_url(ProbeBackend::Ollama, None, None),
            "http://localhost:11434/api/tags"
        );
        assert_eq!(
            models_url(
                ProbeBackend::Ollama,
                None,
                Some("http://localhost:11434/v1")
            ),
            "http://localhost:11434/api/tags"
        );
        assert_eq!(
            models_url(ProbeBackend::OpenAi, None, Some("https://example.test/v1/")),
            "https://example.test/v1/models"
        );
    }

    #[tokio::test]
    async fn a_good_openai_endpoint_lists_its_models() {
        let (base, mut captured) = mock_server(
            200,
            r#"{"object":"list","data":[{"id":"deepseek-flash"},{"id":"deepseek-reasoner"}]}"#,
        )
        .await;
        let report = probe_model(&entry(&base, Some("sk-test")), Some("deepseek"))
            .await
            .expect("probe");
        assert!(report.ok, "{report:?}");
        assert_eq!(report.status, Some(200));
        assert_eq!(report.models, ["deepseek-flash", "deepseek-reasoner"]);
        assert!(report.url.ends_with("/models"), "{}", report.url);

        let request = captured.recv().await.expect("request captured");
        let lower = request.to_ascii_lowercase();
        assert!(lower.starts_with("get /models "), "{request}");
        assert!(lower.contains("authorization: bearer sk-test"), "{request}");
    }

    #[tokio::test]
    async fn an_anthropic_endpoint_uses_the_v1_path_and_x_api_key() {
        let (base, mut captured) = mock_server(
            200,
            r#"{"data":[{"id":"claude-sonnet-4-20250514"}],"has_more":false}"#,
        )
        .await;
        let report = probe_model(&entry(&base, Some("sk-ant")), Some("anthropic"))
            .await
            .expect("probe");
        assert!(report.ok, "{report:?}");
        assert_eq!(report.models, ["claude-sonnet-4-20250514"]);

        let request = captured.recv().await.expect("request captured");
        let lower = request.to_ascii_lowercase();
        assert!(lower.starts_with("get /v1/models "), "{request}");
        assert!(lower.contains("x-api-key: sk-ant"), "{request}");
        assert!(lower.contains("anthropic-version: 2023-06-01"), "{request}");
    }

    #[tokio::test]
    async fn an_ollama_endpoint_uses_api_tags_and_needs_no_key() {
        let (base, mut captured) =
            mock_server(200, r#"{"models":[{"name":"llama3.1:latest"}]}"#).await;
        let report = probe_model(&entry(&base, None), Some("ollama"))
            .await
            .expect("probe");
        assert!(report.ok, "{report:?}");
        assert_eq!(report.models, ["llama3.1:latest"]);

        let request = captured.recv().await.expect("request captured");
        let lower = request.to_ascii_lowercase();
        assert!(lower.starts_with("get /api/tags "), "{request}");
        assert!(!lower.contains("authorization"), "{request}");
    }

    #[tokio::test]
    async fn a_bad_key_is_reported_with_its_status_and_body() {
        let (base, _captured) = mock_server(401, r#"{"error":"invalid api key"}"#).await;
        let report = probe_model(&entry(&base, Some("sk-wrong")), Some("deepseek"))
            .await
            .expect("an HTTP failure is a report, not an error");
        assert!(!report.ok);
        assert_eq!(report.status, Some(401));
        assert!(report.models.is_empty());
        assert!(report.note.contains("401"), "{}", report.note);
        assert!(report.note.contains("invalid api key"), "{}", report.note);
        assert!(!report.line().contains("connected"));
    }

    #[tokio::test]
    async fn a_closed_port_is_a_transport_error() {
        let base = closed_port().await;
        let error = probe_model(&entry(&base, Some("sk-test")), Some("openai"))
            .await
            .expect_err("no endpoint");
        assert!(matches!(error, ProbeError::Transport(_)), "{error:?}");
        assert!(!matches!(error, ProbeError::MissingCredential));
    }

    #[tokio::test]
    async fn a_model_without_a_key_cannot_be_probed() {
        let error = probe_model(
            &entry("https://api.deepseek.com/v1", None),
            Some("deepseek"),
        )
        .await
        .expect_err("no key");
        assert!(matches!(error, ProbeError::MissingCredential), "{error:?}");
    }

    #[tokio::test]
    async fn a_key_can_come_from_the_named_environment_variable() {
        let (base, mut captured) = mock_server(200, r#"{"data":[{"id":"kimi-k2"}]}"#).await;
        let name = "GOBLE_PROBE_TEST_KEY";
        std::env::set_var(name, "env-key");
        let mut model = entry(&base, None);
        model.api_key_env = Some(name.to_string());
        let report = probe_model(&model, Some("openai")).await.expect("probe");
        std::env::remove_var(name);
        assert!(report.ok, "{report:?}");
        let request = captured.recv().await.expect("request captured");
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer env-key"),
            "{request}"
        );
    }

    #[tokio::test]
    async fn a_2xx_that_is_not_a_model_list_is_an_error() {
        let (base, _captured) = mock_server(200, "<html>not an api</html>").await;
        let error = probe_model(&entry(&base, Some("sk-test")), None)
            .await
            .expect_err("not JSON");
        assert!(matches!(error, ProbeError::UnexpectedBody(_)), "{error:?}");
    }

    #[tokio::test]
    async fn an_empty_model_list_is_still_a_connection() {
        let (base, _captured) = mock_server(200, r#"{"data":[]}"#).await;
        let report = probe_model(&entry(&base, Some("sk-test")), None)
            .await
            .expect("probe");
        assert!(report.ok);
        assert!(report.models.is_empty());
        assert!(report.note.contains("no models"), "{}", report.note);
    }
}
