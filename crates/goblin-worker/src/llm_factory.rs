use std::sync::Arc;

use goble_core::llm::{CompletionResponse, LlmProvider, MockProvider};
use goble_core::secret::Secret;

/// Create an LLM provider from worker secrets and env.
///
/// When `LLM_PROVIDER=mock` the provider is instantiated without an API key,
/// which is convenient for end-to-end tests that exercise the worker binary.
pub fn default_provider_factory(
    secrets: std::collections::HashMap<String, Secret>,
) -> anyhow::Result<Arc<dyn LlmProvider>> {
    let provider = std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "openai".into());
    let base_url = std::env::var("LLM_BASE_URL").ok();

    if provider.eq_ignore_ascii_case("mock") {
        return Ok(Arc::new(goble_core::llm::MockProvider::new(
            "mock",
            goble_core::llm::CompletionResponse {
                content: "ok".to_string(),
                tool_calls: vec![],
            },
        )));
    }

    let key = secrets
        .get("llm_api_key")
        .and_then(|s| String::from_utf8(s.encrypted_value.clone()).ok())
        .ok_or_else(|| anyhow::anyhow!("no llm_api_key secret available"))?;
    let boxed = goble_core::llm::create_provider(&provider, &key, base_url.as_deref());
    Ok(Arc::from(boxed))
}

/// Create an LLM provider from explicit configuration (used by the worker runtime).
pub fn provider_from_config(
    provider: &str,
    api_key: &str,
    base_url: Option<&str>,
) -> anyhow::Result<Arc<dyn LlmProvider>> {
    let boxed = goble_core::llm::create_provider(provider, api_key, base_url);
    Ok(Arc::from(boxed))
}

/// Mock factory for tests.
pub fn mock_provider_factory() -> Arc<dyn LlmProvider> {
    Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: "ok".to_string(),
            tool_calls: vec![],
        },
    ))
}
