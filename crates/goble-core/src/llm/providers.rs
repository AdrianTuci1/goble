use crate::llm::{
    AnthropicProvider, CompletionResponse, LlmProvider, MockProvider, OllamaProvider,
    OpenAiProvider, OpenRouterProvider,
};

// --- Provider factory ---

pub fn create_provider(
    provider: &str,
    api_key: &str,
    base_url: Option<&str>,
) -> Box<dyn LlmProvider> {
    match provider.to_lowercase().as_str() {
        "openai" => Box::new(OpenAiProvider::openai(api_key)),
        "ollama" => Box::new(OllamaProvider::new(
            base_url.unwrap_or("http://localhost:11434"),
        )),
        "anthropic" => Box::new(AnthropicProvider::new(api_key)),
        "openrouter" => Box::new(OpenRouterProvider::new(api_key)),
        _ => Box::new(MockProvider::new(
            provider,
            CompletionResponse::new(format!("unknown provider {provider}"), Vec::new()),
        )),
    }
}

/// Concrete model ids offered in the composer's model dropdown for a provider.
/// Callers ensure the currently configured model is present (the
/// desktop-service `available_models` promotion does that).
pub fn provider_models(provider: &str) -> Vec<String> {
    match provider.to_lowercase().as_str() {
        "openai" => vec![
            "gpt-4o".to_string(),
            "gpt-4o-mini".to_string(),
            "gpt-4.1".to_string(),
            "o3-mini".to_string(),
        ],
        "anthropic" => vec![
            "claude-sonnet-4-20250514".to_string(),
            "claude-3-7-sonnet-latest".to_string(),
            "claude-3-5-haiku-latest".to_string(),
        ],
        "ollama" => vec![
            "llama3.1".to_string(),
            "qwen2.5".to_string(),
            "mistral".to_string(),
        ],
        "deepseek" => vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
        "openrouter" => vec![
            "anthropic/claude-3.5-sonnet".to_string(),
            "openai/gpt-4o".to_string(),
            "meta-llama/llama-3.1-70b-instruct".to_string(),
        ],
        "mock" => vec!["mock".to_string()],
        _ => vec!["goble-agent".to_string()],
    }
}

/// A sane default model id for a provider when nothing is configured yet.
pub fn default_model_for(provider: &str) -> &'static str {
    match provider.to_lowercase().as_str() {
        "openai" => "gpt-4o",
        "anthropic" => "claude-sonnet-4-20250514",
        "ollama" => "llama3.1",
        "deepseek" => "deepseek-chat",
        "openrouter" => "anthropic/claude-3.5-sonnet",
        _ => "goble-agent",
    }
}
