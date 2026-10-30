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
mod providers;
mod types;

#[cfg(test)]
mod tests;

pub use anthropic::AnthropicProvider;
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;
pub use openrouter::OpenRouterProvider;
pub use providers::{create_provider, default_model_for, provider_models};
pub use types::{
    CompletionRequest, CompletionResponse, CompletionStreamEvent, LlmProvider, LlmToolCall,
    Message, MockProvider, Role, TokenUsage, ToolDefinition,
};

/// The most provider error body ever put in one log line. A provider that
/// echoes a large request keeps the line readable and the log bounded.
const MAX_BODY_CHARS: usize = 4_000;

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
