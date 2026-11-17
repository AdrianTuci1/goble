//! The model providers the agent talks to.
//!
//! One module per surface: the request and response types a completion is
//! asked for and answered with ([`types`]), one module per backend — [`openai`]
//! (and its DeepSeek-compatible endpoint), [`ollama`], [`anthropic`] and
//! [`openrouter`] — and the factory that picks one from the configured provider
//! name ([`providers`]). What the backends share is [`types`]' request and
//! response types and the [`LlmProvider`] trait.

mod anthropic;
mod ollama;
mod openai;
mod openrouter;
mod probe;
mod providers;
mod types;

#[cfg(test)]
mod tests;

pub use anthropic::AnthropicProvider;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;
pub use openrouter::OpenRouterProvider;
pub use probe::{
    models_url, probe_model, ProbeBackend, ProbeError, ProbeReport, PROBE_TIMEOUT,
};
pub use providers::{create_provider, default_model_for, provider_models};
pub use types::{
    CompletionRequest, CompletionResponse, CompletionStreamEvent, LlmProvider, LlmToolCall,
    Message, MockProvider, Role, TokenUsage, ToolDefinition,
};

/// The most provider error body ever put in one log line. A provider that
/// echoes a large request keeps the line readable and the log bounded.
const MAX_BODY_CHARS: usize = 4_000;

/// How long a model endpoint may take to accept a connection.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// How long a streamed completion may go silent between bytes. The read timeout
/// applies between reads, so a long answer that keeps arriving is never cut off;
/// a request the endpoint stops answering becomes an error instead of a turn
/// that never settles and leaves its pane busy forever.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

/// The HTTP client the streaming backends use: fail fast on an unreachable
/// endpoint, and never wait forever on a silent one.
pub(crate) fn streaming_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

fn truncate_body(body: &str) -> String {
    if body.chars().count() <= MAX_BODY_CHARS {
        return body.to_string();
    }
    let mut cut: String = body.chars().take(MAX_BODY_CHARS).collect();
    cut.push_str("…[truncated]");
    cut
}

/// Record a rejected model call before the adapter turns it into an error.
///
/// The message the provider returned is the only place the reason exists (a
/// 400 explains which field it rejected), so it is logged verbatim, capped,
/// with the provider and model that produced it. The caller still returns the
/// same error for the UI.
pub(crate) fn log_http_failure(provider: &str, model: &str, url: &str, status: u16, body: &str) {
    log::error!(
        "LLM request failed: provider={provider} model={model} url={url} status={status} body={}",
        truncate_body(body.trim())
    );
}

/// Whether a failed model call was the endpoint refusing the request's size.
///
/// The provider's body cap is not the model's context window: a 256k-token
/// model answered 413 Request Entity Too Large (an openresty page, so the cap
/// is the endpoint's own) to a conversation still well inside the window. The
/// status leads the error message; a proxy in front of the endpoint answers
/// with its own words, so the phrases are matched too. Nothing here reads the
/// body as data: it only classifies a message the app already holds.
pub fn is_payload_too_large(message: &str) -> bool {
    const MARKERS: [&str; 5] = [
        "413",
        "412",
        "payload too large",
        "request entity too large",
        "content too large",
    ];
    let lower = message.to_ascii_lowercase();
    MARKERS.iter().any(|marker| lower.contains(marker))
}

/// Record a model call that never produced a response (DNS, TLS, connection
/// reset, timeout). The returned error carries the same text to the UI.
pub(crate) fn log_transport_failure(
    provider: &str,
    model: &str,
    url: &str,
    error: &reqwest::Error,
) {
    log::error!(
        "LLM request failed before a response: provider={provider} model={model} url={url}: {error}"
    );
}
