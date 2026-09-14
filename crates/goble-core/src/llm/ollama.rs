use std::pin::Pin;

use anyhow::{Context, Result};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};

use crate::llm::{
    CompletionRequest, CompletionResponse, CompletionStreamEvent, LlmProvider, Message, Role,
};

use super::openai::provider_content;

// --- Ollama provider ---

#[derive(Clone)]
pub struct OllamaProvider {
    name: String,
    base_url: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            name: "ollama".to_string(),
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }

    pub fn localhost() -> Self {
        Self::new("http://localhost:11434")
    }
}

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    options: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct OllamaMessage {
    #[serde(default)]
    role: String,
    #[serde(default)]
    content: String,
}

#[derive(Debug, Deserialize)]
struct OllamaStreamChunk {
    #[serde(default)]
    message: OllamaMessage,
    #[serde(default)]
    done: bool,
    /// The final chunk counts the prompt and the generation. Ollama has no
    /// prompt cache, so the cached part stays absent.
    #[serde(default)]
    prompt_eval_count: Option<u64>,
    #[serde(default)]
    eval_count: Option<u64>,
}

fn into_ollama_messages(messages: Vec<Message>) -> Vec<OllamaMessage> {
    messages
        .into_iter()
        .map(|m| OllamaMessage {
            role: match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            }
            .to_string(),
            content: provider_content(&m),
        })
        .collect()
}

#[async_trait::async_trait]
impl LlmProvider for OllamaProvider {
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
        let mut options = serde_json::Map::new();
        if let Some(t) = request.temperature {
            options.insert("temperature".to_string(), serde_json::json!(t));
        }
        let body = OllamaRequest {
            model: request.model,
            messages: into_ollama_messages(request.messages),
            stream: true,
            options: if options.is_empty() {
                None
            } else {
                Some(options)
            },
        };
        let url = format!("{}/api/chat", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Accept", "application/x-ndjson")
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
                    if line.is_empty() {
                        continue;
                    }
                    let parsed: OllamaStreamChunk = match serde_json::from_str(line) {
                        Ok(c) => c,
                        Err(e) => {
                            yield CompletionStreamEvent::Error(format!("[parse error: {e}]"));
                            continue;
                        }
                    };
                    yield CompletionStreamEvent::AssistantDelta(parsed.message.content);
                    if parsed.done {
                        if parsed.prompt_eval_count.is_some() || parsed.eval_count.is_some() {
                            yield CompletionStreamEvent::Usage(crate::llm::TokenUsage {
                                input: parsed.prompt_eval_count.unwrap_or(0),
                                cached: None,
                                output: parsed.eval_count.unwrap_or(0),
                            });
                        }
                        yield CompletionStreamEvent::Done;
                        return;
                    }
                }
            }
            yield CompletionStreamEvent::Done;
        };
        Ok(Box::pin(stream))
    }
}
