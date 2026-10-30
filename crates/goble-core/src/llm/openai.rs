use std::pin::Pin;

use crate::harness::ToolCallStatus;
use anyhow::{Context, Result};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};

use crate::llm::{
    CompletionRequest, CompletionResponse, CompletionStreamEvent, LlmProvider, LlmToolCall,
    Message, Role, TokenUsage, ToolDefinition,
};

#[derive(Clone)]
pub struct OpenAiProvider {
    name: String,
    api_key: String,
    base_url: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(
        name: impl Into<String>,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            api_key: api_key.into(),
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }

    pub fn openai(api_key: impl Into<String>) -> Self {
        Self::new("openai", api_key, "https://api.openai.com/v1")
    }

    /// Whether this endpoint is a DeepSeek thinking model, which expects the
    /// conversation's `reasoning_content` replayed on the next request.
    ///
    /// A `[model.deepseek]` entry names its own `base_url`, so the app reaches
    /// DeepSeek through the generic OpenAI-compatible provider and the
    /// provider is named `"openai"`. The name alone therefore misses the
    /// endpoint the model is actually served by; the base URL and the model id
    /// both identify it as well.
    pub(super) fn replays_reasoning(&self, model: &str) -> bool {
        let named = |value: &str| value.to_ascii_lowercase();
        named(&self.name) == "deepseek"
            || named(&self.base_url).contains("deepseek")
            || named(model).contains("deepseek")
    }
}

#[derive(Debug, Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    tools: Option<Vec<OpenAiTool>>,
    temperature: Option<f32>,
    stream: bool,
    /// Asks the OpenAI-compatible endpoint for a final `usage` frame on the
    /// stream. Without it the stream carries no token counts at all.
    stream_options: OpenAiStreamOptions,
}

#[derive(Debug, Serialize)]
struct OpenAiStreamOptions {
    include_usage: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OpenAiMessage {
    role: String,
    pub(super) content: Option<String>,
    /// DeepSeek's thinking mode requires the field back on every assistant
    /// message. A tool-call turn carries the empty string; the reply that
    /// ended the turn carries the chain of thought it streamed. Other
    /// endpoints never receive the key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Serialize)]
struct OpenAiTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAiFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiToolCall {
    id: String,
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAiFunctionCall,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAiStreamChunk {
    pub(super) choices: Vec<OpenAiStreamChoice>,
    /// Present on the final frame when `stream_options.include_usage` was set.
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAiUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    prompt_tokens_details: Option<OpenAiPromptTokensDetails>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAiPromptTokensDetails {
    cached_tokens: Option<u64>,
}

impl OpenAiUsage {
    /// Map the provider's wire shape onto the app's token accounting. A field
    /// the provider left out stays out (it is not a zero).
    fn into_usage(self) -> TokenUsage {
        TokenUsage {
            input: self.prompt_tokens.unwrap_or(0),
            cached: self
                .prompt_tokens_details
                .and_then(|details| details.cached_tokens),
            output: self.completion_tokens.unwrap_or(0),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAiStreamChoice {
    pub(super) delta: OpenAiStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct OpenAiStreamDelta {
    pub(super) content: Option<String>,
    /// A thinking model's chain of thought, streamed before the answer. It is
    /// not part of the reply, but DeepSeek's next request must carry it back.
    #[serde(default)]
    pub(super) reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiStreamToolCall>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAiStreamToolCall {
    pub(super) index: usize,
    pub(super) id: Option<String>,
    #[serde(rename = "type")]
    pub(super) tool_type: Option<String>,
    pub(super) function: Option<OpenAiStreamFunctionCall>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OpenAiStreamFunctionCall {
    pub(super) name: Option<String>,
    pub(super) arguments: Option<String>,
}

/// The text a provider puts on the wire for one message. A tool result whose
/// call failed carries a fixed failure line: the provider protocols have no
/// status slot on a tool message, so the marker is the model's only signal.
pub(super) fn provider_content(message: &Message) -> String {
    match message.tool_status {
        Some(ToolCallStatus::Error) => format!("[tool call failed]\n{}", message.content),
        _ => message.content.clone(),
    }
}

pub(super) fn into_openai_messages(
    messages: Vec<Message>,
    thinking_provider: bool,
) -> Vec<OpenAiMessage> {
    messages
        .into_iter()
        .map(|m| {
            let role = match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            }
            .to_string();
            let tool_calls = m.tool_calls.as_ref().map(|calls| {
                calls
                    .iter()
                    .map(|c| OpenAiToolCall {
                        id: c.id.clone(),
                        tool_type: "function".to_string(),
                        function: OpenAiFunctionCall {
                            name: c.name.clone(),
                            arguments: c.arguments.to_string(),
                        },
                    })
                    .collect()
            });
            // DeepSeek checks the assistant turns of a thinking-mode
            // conversation for this field, and a tool-call turn is not exempt:
            // the API wants the chain of thought it streamed for that call.
            // Echoing an empty string where the model reasoned is itself the
            // rejection it answers with "must be passed back". A row from
            // before the field was stored, or a turn that reasoned nothing,
            // still satisfies the check with the empty string, so an old
            // conversation is not bricked.
            let reasoning_content = (thinking_provider && m.role == Role::Assistant)
                .then(|| m.reasoning_content.clone().unwrap_or_default());
            let tool_call_id = m.tool_call_id.clone();
            let content = provider_content(&m);
            OpenAiMessage {
                role,
                content: if content.is_empty() {
                    None
                } else {
                    Some(content)
                },
                reasoning_content,
                tool_call_id,
                tool_calls,
            }
        })
        .collect()
}

/// The shape of a request, for logs: role, whether a chain of thought is
/// replayed, tool calls, and whether the row answers a call. Content is never
/// included.
pub(super) fn request_shape(messages: &[OpenAiMessage]) -> String {
    messages
        .iter()
        .map(|message| {
            let reasoning = message
                .reasoning_content
                .as_ref()
                .map_or("reasoning:absent".to_string(), |text| {
                    format!("reasoning:{}b", text.len())
                });
            let calls = message.tool_calls.as_ref().map_or(0, Vec::len);
            format!(
                "{}[{reasoning},calls:{calls},tool_result:{}]",
                message.role,
                message.tool_call_id.is_some()
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn into_openai_tools(tools: Vec<ToolDefinition>) -> Option<Vec<OpenAiTool>> {
    if tools.is_empty() {
        None
    } else {
        Some(
            tools
                .into_iter()
                .map(|t| OpenAiTool {
                    tool_type: "function".to_string(),
                    function: OpenAiFunction {
                        name: t.name,
                        description: t.description,
                        parameters: t.parameters,
                    },
                })
                .collect(),
        )
    }
}

#[allow(dead_code)]
fn parse_tool_calls(tool_calls: Vec<OpenAiToolCall>) -> Vec<LlmToolCall> {
    tool_calls
        .into_iter()
        .map(|tc| {
            let args = serde_json::from_str(&tc.function.arguments).unwrap_or_default();
            LlmToolCall {
                id: tc.id,
                name: tc.function.name,
                arguments: args,
            }
        })
        .collect()
}

#[async_trait::async_trait]
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        let mut stream = self.complete_stream(request).await?;
        let mut content = String::new();
        let mut tool_calls = Vec::new();
        let mut usage = None;
        while let Some(event) = stream.next().await {
            match event {
                CompletionStreamEvent::AssistantDelta(delta) => content.push_str(&delta),
                CompletionStreamEvent::ReasoningDelta(_) => {}
                CompletionStreamEvent::ToolCalls(calls) => tool_calls = calls,
                CompletionStreamEvent::Usage(reported) => usage = Some(reported),
                CompletionStreamEvent::Done => {}
                CompletionStreamEvent::Error(message) => anyhow::bail!(message),
            }
        }
        Ok(CompletionResponse {
            content,
            tool_calls,
            usage,
        })
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = CompletionStreamEvent> + Send>>> {
        let thinking_provider = self.replays_reasoning(&request.model);
        let body = OpenAiRequest {
            model: request.model,
            messages: into_openai_messages(request.messages, thinking_provider),
            tools: into_openai_tools(request.tools),
            temperature: request.temperature,
            stream: true,
            stream_options: OpenAiStreamOptions { include_usage: true },
        };

        log::debug!("LLM request shape: {}", request_shape(&body.messages));
        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                crate::llm::log_transport_failure(&self.name, &body.model, &url, &error);
                error
            })
            .with_context(|| format!("failed to POST to {url}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text: String = resp.text().await.unwrap_or_default();
            crate::llm::log_http_failure(&self.name, &body.model, &url, status.as_u16(), &text);
            // The reasoning rejection names the field but not the message that
            // is missing it; the shape says which turns the endpoint counted as
            // thinking turns. No content is logged.
            log::error!("LLM request shape: {}", request_shape(&body.messages));
            anyhow::bail!("LLM request failed: {status} {text}");
        }

        let stream = async_stream::stream! {
            let mut tool_call_buffer: Vec<Option<OpenAiStreamToolCall>> = Vec::new();
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
                    if !line.starts_with("data: ") {
                        continue;
                    }
                    let data = &line[6..];
                    if data == "[DONE]" {
                        let tool_calls = flush_tool_call_buffer(&tool_call_buffer);
                        if !tool_calls.is_empty() {
                            yield CompletionStreamEvent::ToolCalls(tool_calls);
                        }
                        yield CompletionStreamEvent::Done;
                        return;
                    }

                    let parsed: OpenAiStreamChunk = match serde_json::from_str(data) {
                        Ok(c) => c,
                        Err(e) => {
                            yield CompletionStreamEvent::Error(format!("[parse error: {e}]"));
                            continue;
                        }
                    };

                    // The usage frame is its own chunk (empty `choices`), sent
                    // once per call just before the terminator.
                    if let Some(usage) = parsed.usage {
                        yield CompletionStreamEvent::Usage(usage.into_usage());
                    }

                    for choice in parsed.choices {
                        if let Some(reasoning) = choice.delta.reasoning_content {
                            if !reasoning.is_empty() {
                                yield CompletionStreamEvent::ReasoningDelta(reasoning);
                            }
                        }
                        if let Some(content) = choice.delta.content {
                            if !content.is_empty() {
                                yield CompletionStreamEvent::AssistantDelta(content);
                            }
                        }
                        if let Some(calls) = choice.delta.tool_calls {
                            for call in calls {
                                if tool_call_buffer.len() <= call.index {
                                    tool_call_buffer.resize_with(call.index + 1, || None);
                                }
                                let existing = tool_call_buffer[call.index].get_or_insert(OpenAiStreamToolCall {
                                    index: call.index,
                                    id: None,
                                    tool_type: None,
                                    function: None,
                                });
                                if let Some(id) = call.id {
                                    existing.id = Some(id);
                                }
                                if let Some(tool_type) = call.tool_type {
                                    existing.tool_type = Some(tool_type);
                                }
                                if let Some(function) = call.function {
                                    let existing_function = existing.function.get_or_insert(OpenAiStreamFunctionCall {
                                        name: None,
                                        arguments: None,
                                    });
                                    if let Some(name) = function.name {
                                        existing_function.name = Some(name);
                                    }
                                    if let Some(args) = function.arguments {
                                        existing_function.arguments = Some(
                                            existing_function.arguments.clone().unwrap_or_default() + &args,
                                        );
                                    }
                                }
                            }
                        }
                        if choice.finish_reason.is_some() {
                            let tool_calls = flush_tool_call_buffer(&tool_call_buffer);
                            if !tool_calls.is_empty() {
                                yield CompletionStreamEvent::ToolCalls(tool_calls);
                            }
                        }
                    }
                }
            }

            let tool_calls = flush_tool_call_buffer(&tool_call_buffer);
            if !tool_calls.is_empty() {
                yield CompletionStreamEvent::ToolCalls(tool_calls);
            }
            yield CompletionStreamEvent::Done;
        };

        Ok(Box::pin(stream))
    }
}

pub(super) fn flush_tool_call_buffer(buffer: &[Option<OpenAiStreamToolCall>]) -> Vec<LlmToolCall> {
    buffer
        .iter()
        .filter_map(|call| {
            let call = call.as_ref()?;
            let function = call.function.as_ref()?;
            let name = function.name.as_ref()?.clone();
            let arguments_str = function.arguments.as_ref()?.clone();
            let args = serde_json::from_str(&arguments_str).unwrap_or_default();
            Some(LlmToolCall {
                id: call.id.clone().unwrap_or_default(),
                name,
                arguments: args,
            })
        })
        .collect()
}
