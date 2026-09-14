use std::pin::Pin;

use crate::harness::ToolCallStatus;
use anyhow::Result;
use futures::Stream;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub tool_calls: Option<Vec<LlmToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// The outcome of the call a `Role::Tool` message answers. A tool row is
    /// persisted as `<call_id>\n<body>` with the status on the call record, not
    /// in its text, so the history builder resolves the status and carries it
    /// here — otherwise the model reads a failure as an ordinary result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_status: Option<ToolCallStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub provider: String,
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub temperature: Option<f32>,
}

impl CompletionRequest {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            messages: Vec::new(),
            tools: Vec::new(),
            temperature: None,
        }
    }

    pub fn with_system(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message {
            role: Role::System,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_status: None,
        });
        self
    }

    pub fn with_user(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message {
            role: Role::User,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
            tool_status: None,
        });
        self
    }

    pub fn with_tool(mut self, tool: ToolDefinition) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub content: String,
    pub tool_calls: Vec<LlmToolCall>,
    /// What the provider said the call cost in tokens, when it said anything at
    /// all. Streamed calls surface this as [`CompletionStreamEvent::Usage`
    /// instead; a caller that collects the whole response reads it here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
}

impl CompletionResponse {
    /// A response that reported no usage — the shape every caller that only
    /// carries content and tool calls wants.
    pub fn new(content: impl Into<String>, tool_calls: Vec<LlmToolCall>) -> Self {
        Self {
            content: content.into(),
            tool_calls,
            usage: None,
        }
    }

    pub fn with_usage(mut self, usage: Option<TokenUsage>) -> Self {
        self.usage = usage;
        self
    }
}

/// How many tokens one model call used, as the provider itself reported it.
///
/// This is accounting, not billing: it carries no price. A provider that does
/// not report usage leaves the whole thing absent, and the UI shows nothing
/// rather than a guessed number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Prompt tokens, including any the provider served from its cache.
    pub input: u64,
    /// The part of `input` the provider served from its prompt cache, when it
    /// distinguishes it (OpenAI's `cached_tokens`, Anthropic's
    /// `cache_read_input_tokens`). `None` when the provider reports no cache
    /// accounting at all — which is not the same as a cache hit of zero.
    pub cached: Option<u64>,
    /// Generated tokens.
    pub output: u64,
}

impl TokenUsage {
    /// The tokens billed against the context window and the completion.
    pub fn total(&self) -> u64 {
        self.input.saturating_add(self.output)
    }

    /// Whether the provider reported anything at all. A call with no accounted
    /// tokens draws no usage affordance instead of a zero.
    pub fn is_empty(&self) -> bool {
        self.total() == 0 && self.cached.is_none()
    }
}

#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = CompletionStreamEvent> + Send>>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionStreamEvent {
    AssistantDelta(String),
    ToolCalls(Vec<LlmToolCall>),
    /// What the call cost in tokens, when the provider reported it. It arrives
    /// before [`Self::Done`] on the providers that send a usage frame; one per
    /// model call.
    Usage(TokenUsage),
    Done,
    Error(String),
}

pub struct MockProvider {
    name: String,
    response: CompletionResponse,
}

impl MockProvider {
    pub fn new(name: impl Into<String>, response: CompletionResponse) -> Self {
        Self {
            name: name.into(),
            response,
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for MockProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse> {
        Ok(self.response.clone())
    }

    async fn complete_stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = CompletionStreamEvent> + Send>>> {
        let events = vec![
            CompletionStreamEvent::AssistantDelta(self.response.content.clone()),
            CompletionStreamEvent::ToolCalls(self.response.tool_calls.clone()),
            CompletionStreamEvent::Done,
        ];
        Ok(Box::pin(futures::stream::iter(events)))
    }
}
