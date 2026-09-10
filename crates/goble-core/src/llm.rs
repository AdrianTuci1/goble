use std::pin::Pin;

use anyhow::{Context, Result};
use futures::{Stream, StreamExt};
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
        });
        self
    }

    pub fn with_user(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message {
            role: Role::User,
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
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
}

#[derive(Debug, Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    tools: Option<Vec<OpenAiTool>>,
    temperature: Option<f32>,
    stream: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiMessage {
    role: String,
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
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
struct OpenAiStreamChunk {
    choices: Vec<OpenAiStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChoice {
    delta: OpenAiStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct OpenAiStreamDelta {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiStreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamToolCall {
    index: usize,
    id: Option<String>,
    #[serde(rename = "type")]
    tool_type: Option<String>,
    function: Option<OpenAiStreamFunctionCall>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiStreamFunctionCall {
    name: Option<String>,
    arguments: Option<String>,
}

fn into_openai_messages(messages: Vec<Message>) -> Vec<OpenAiMessage> {
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
            let tool_call_id = m.tool_call_id.clone();
            OpenAiMessage {
                role,
                content: if m.content.is_empty() {
                    None
                } else {
                    Some(m.content)
                },
                tool_call_id,
                tool_calls,
            }
        })
        .collect()
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
        while let Some(event) = stream.next().await {
            match event {
                CompletionStreamEvent::AssistantDelta(delta) => content.push_str(&delta),
                CompletionStreamEvent::ToolCalls(calls) => tool_calls = calls,
                CompletionStreamEvent::Done => {}
                CompletionStreamEvent::Error(message) => anyhow::bail!(message),
            }
        }
        Ok(CompletionResponse {
            content,
            tool_calls,
        })
    }

    async fn complete_stream(
        &self,
        request: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = CompletionStreamEvent> + Send>>> {
        let body = OpenAiRequest {
            model: request.model,
            messages: into_openai_messages(request.messages),
            tools: into_openai_tools(request.tools),
            temperature: request.temperature,
            stream: true,
        };

        let url = format!("{}/chat/completions", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
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

                    for choice in parsed.choices {
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

fn flush_tool_call_buffer(buffer: &[Option<OpenAiStreamToolCall>]) -> Vec<LlmToolCall> {
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
            content: m.content,
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
        while let Some(event) = stream.next().await {
            match event {
                CompletionStreamEvent::AssistantDelta(delta) => content.push_str(&delta),
                CompletionStreamEvent::Done => {}
                CompletionStreamEvent::Error(message) => anyhow::bail!(message),
                _ => {}
            }
        }
        Ok(CompletionResponse {
            content,
            tool_calls: Vec::new(),
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
        while let Some(event) = stream.next().await {
            match event {
                CompletionStreamEvent::AssistantDelta(delta) => content.push_str(&delta),
                CompletionStreamEvent::Done => {}
                CompletionStreamEvent::Error(message) => anyhow::bail!(message),
                _ => {}
            }
        }
        Ok(CompletionResponse {
            content,
            tool_calls: Vec::new(),
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
                content: m.content,
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
                }
            }
            yield CompletionStreamEvent::Done;
        };
        Ok(Box::pin(stream))
    }
}

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
            CompletionResponse {
                content: format!("unknown provider {provider}"),
                tool_calls: Vec::new(),
            },
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
        "ollama" => vec!["llama3.1".to_string(), "qwen2.5".to_string(), "mistral".to_string()],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_provider() {
        let provider = MockProvider::new(
            "mock",
            CompletionResponse {
                content: "hello".to_string(),
                tool_calls: Vec::new(),
            },
        );
        let result = provider
            .complete(CompletionRequest::new("mock", "m"))
            .await
            .unwrap();
        assert_eq!(result.content, "hello");
    }

    #[tokio::test]
    async fn test_mock_provider_stream() {
        let provider = MockProvider::new(
            "mock",
            CompletionResponse {
                content: "hello".to_string(),
                tool_calls: Vec::new(),
            },
        );
        let mut stream = provider
            .complete_stream(CompletionRequest::new("mock", "m"))
            .await
            .unwrap();
        let mut out = String::new();
        while let Some(event) = stream.next().await {
            match event {
                CompletionStreamEvent::AssistantDelta(delta) => out.push_str(&delta),
                CompletionStreamEvent::Done => break,
                _ => {}
            }
        }
        assert_eq!(out, "hello");
    }

    #[tokio::test]
    async fn test_mock_provider_tool_calls() {
        let provider = MockProvider::new(
            "mock",
            CompletionResponse {
                content: "done".to_string(),
                tool_calls: vec![LlmToolCall {
                    id: "c1".to_string(),
                    name: "create_agent".to_string(),
                    arguments: serde_json::json!({"name": "a"}),
                }],
            },
        );
        let result = provider
            .complete(CompletionRequest::new("mock", "m"))
            .await
            .unwrap();
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].name, "create_agent");
    }

    #[test]
    fn test_request_builder() {
        let request = CompletionRequest::new("openai", "gpt-4o-mini")
            .with_system("sys")
            .with_user("hi")
            .with_tool(ToolDefinition {
                name: "x".to_string(),
                description: "y".to_string(),
                parameters: serde_json::json!({}),
            });
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.tools.len(), 1);
    }

    #[test]
    fn test_stream_chunk_deserialization() {
        let json = serde_json::json!({
            "choices": [{"delta": {"content": "hi"}, "finish_reason": null}]
        });
        let chunk: OpenAiStreamChunk = serde_json::from_value(json).unwrap();
        assert_eq!(chunk.choices[0].delta.content, Some("hi".to_string()));
    }

    #[test]
    fn test_parse_sse_lines() {
        let raw = r#"data: {"choices":[{"delta":{"content":"hello "}}]}

data: {"choices":[{"delta":{"content":"world"}}]}

data: [DONE]

"#;
        let mut contents = Vec::new();
        for line in raw.lines() {
            if line.starts_with("data: ") {
                let data = &line[6..];
                if data == "[DONE]" {
                    break;
                }
                let chunk: OpenAiStreamChunk = serde_json::from_str(data).unwrap();
                if let Some(c) = chunk.choices.get(0).and_then(|c| c.delta.content.clone()) {
                    contents.push(c);
                }
            }
        }
        assert_eq!(contents, vec!["hello ".to_string(), "world".to_string()]);
    }

    #[test]
    fn test_stream_tool_call_buffer_flush() {
        let buffer = vec![Some(OpenAiStreamToolCall {
            index: 0,
            id: Some("call_1".to_string()),
            tool_type: Some("function".to_string()),
            function: Some(OpenAiStreamFunctionCall {
                name: Some("create_agent".to_string()),
                arguments: Some(r#"{"name":"Agent"}"#.to_string()),
            }),
        })];
        let calls = flush_tool_call_buffer(&buffer);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "create_agent");
    }

    #[test]
    fn test_provider_factory_unknown() {
        let provider = create_provider("unknown", "", None);
        assert_eq!(provider.name(), "unknown");
    }

    #[test]
    fn test_provider_factory_openai() {
        let provider = create_provider("openai", "key", None);
        assert_eq!(provider.name(), "openai");
    }
}
