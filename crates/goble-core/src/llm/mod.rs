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
