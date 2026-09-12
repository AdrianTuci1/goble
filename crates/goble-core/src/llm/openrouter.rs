use std::pin::Pin;

use anyhow::Result;
use futures::Stream;

use crate::llm::{
    CompletionRequest, CompletionResponse, CompletionStreamEvent, LlmProvider, OpenAiProvider,
};

// --- OpenRouter provider ---

#[derive(Clone)]
pub struct OpenRouterProvider {
    inner: OpenAiProvider,
}

impl OpenRouterProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            inner: OpenAiProvider::new("openrouter", api_key, "https://openrouter.ai/api/v1"),
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for OpenRouterProvider {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        self.inner.complete(request).await
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = CompletionStreamEvent> + Send>>> {
        self.inner.complete_stream(request).await
    }
}
