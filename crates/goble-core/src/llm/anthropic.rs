use std::pin::Pin;

use anyhow::{Context, Result};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};

use super::openai::provider_content;
use crate::llm::{CompletionRequest, CompletionResponse, CompletionStreamEvent, LlmProvider, Role};

// --- Anthropic provider ---

#[derive(Clone)]
pub struct AnthropicProvider {
    name: String,
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            name: "anthropic".to_string(),
            api_key: api_key.into(),
            base_url: "https://api.anthropic.com/v1".to_string(),
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: i32,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicStreamChunk {
    #[serde(rename = "type")]
    chunk_type: String,
    delta: Option<AnthropicDelta>,
    /// `message_start` carries the prompt accounting inside its message.
    message: Option<AnthropicMessageEnvelope>,
    /// `message_delta` carries the running output count.
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageEnvelope {
    usage: Option<AnthropicUsage>,
}

/// Anthropic splits its accounting across frames: `message_start` reports the
/// prompt (including the part served from the cache), `message_delta` the
/// output so far.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
struct AnthropicUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AnthropicDelta {
    text: Option<String>,
}

#[async_trait::async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        let mut stream = self.complete_stream(request).await?;
        let mut content = String::new();
        let mut usage = None;
        while let Some(event) = stream.next().await {
            match event {
                CompletionStreamEvent::AssistantDelta(delta) => content.push_str(&delta),
                CompletionStreamEvent::Usage(reported) => usage = Some(reported),
                CompletionStreamEvent::Done => {}
                CompletionStreamEvent::Error(message) => anyhow::bail!(message),
                _ => {}
            }
        }
        Ok(CompletionResponse {
            content,
            tool_calls: Vec::new(),
            usage,
        })
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = CompletionStreamEvent> + Send>>> {
        let system = request
            .messages
            .iter()
            .find(|m| m.role == Role::System)
            .map(|m| m.content.clone());
        let messages: Vec<AnthropicMessage> = request
            .messages
            .into_iter()
            .filter(|m| m.role != Role::System)
            .map(|m| AnthropicMessage {
                role: match m.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    _ => "user",
                }
                .to_string(),
                content: provider_content(&m),
            })
            .collect();
        let body = AnthropicRequest {
            model: request.model,
            max_tokens: 4096,
            messages,
            temperature: request.temperature,
            system,
            stream: true,
        };
        let url = format!("{}/messages", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("x-api-key", self.api_key.clone())
            .header("anthropic-version", "2023-06-01")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("failed to POST to {url}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text: String = resp.text().await.unwrap_or_default();
            anyhow::bail!("LLM request failed: {status} {text}");
        }

        let stream = async_stream::stream! {
            let mut bytes_stream = resp.bytes_stream();
            let mut buffer = String::new();
            // Prompt accounting from `message_start`, held until the closing
            // `message_delta` reports the output and the call's usage is known
            // in full.
            let mut prompt: Option<(u64, Option<u64>)> = None;
            while let Some(result) = bytes_stream.next().await {
                let chunk: bytes::Bytes = match result {
                    Ok(c) => c,
                    Err(e) => {
                        yield CompletionStreamEvent::Error(e.to_string());
                        return;
                    }
                };
                let text = String::from_utf8_lossy(&chunk);
                buffer.push_str(&text);
                while let Some(pos) = buffer.find('\n') {
                    let line = buffer.drain(..=pos).collect::<String>();
                    let line = line.trim();
                    if !line.starts_with("data: ") {
                        continue;
                    }
                    let data = &line[6..];
                    if data == "[DONE]" {
                        yield CompletionStreamEvent::Done;
                        return;
                    }
                    let parsed: AnthropicStreamChunk = match serde_json::from_str(data) {
                        Ok(c) => c,
                        Err(e) => {
                            yield CompletionStreamEvent::Error(format!("[parse error: {e}]"));
                            continue;
                        }
                    };
                    if parsed.chunk_type == "content_block_delta" {
                        if let Some(text) = parsed.delta.and_then(|d| d.text) {
                            yield CompletionStreamEvent::AssistantDelta(text);
                        }
                    }
                    if let Some(usage) = parsed.message.and_then(|m| m.usage) {
                        prompt = Some((
                            usage.input_tokens.unwrap_or(0),
                            usage.cache_read_input_tokens,
                        ));
                    }
                    if let Some(usage) = parsed.usage {
                        let (input, cached) = prompt.unwrap_or((0, None));
                        yield CompletionStreamEvent::Usage(crate::llm::TokenUsage {
                            input,
                            cached,
                            output: usage.output_tokens.unwrap_or(0),
                        });
                    }
                }
            }
            yield CompletionStreamEvent::Done;
        };
        Ok(Box::pin(stream))
    }
}
