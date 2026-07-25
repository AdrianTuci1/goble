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
        });
        self
    }

    pub fn with_user(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message {
            role: Role::User,
            content: content.into(),
            tool_calls: None,
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
    pub fn new(name: impl Into<String>, api_key: impl Into<String>, base_url: impl Into<String>) -> Self {
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
    content: String,
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

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoiceMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct OpenAiToolCall {
    id: String,
    function: OpenAiFunctionCall,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
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

#[derive(Debug, Deserialize)]
struct OpenAiStreamFunctionCall {
    name: Option<String>,
    arguments: Option<String>,
}

fn into_openai_messages(messages: Vec<Message>) -> Vec<OpenAiMessage> {
    messages
        .into_iter()
        .map(|m| OpenAiMessage {
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

        // Return boxed stream; explicit lifetime matches the owned values above.
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
        let request = CompletionRequest::new("mock", "m")
            .with_system("sys")
            .with_user("hi");
        let response = provider.complete(request).await.unwrap();
        assert_eq!(response.content, "hello");
    }

    #[test]
    fn test_request_builder() {
        let req = CompletionRequest::new("p", "m")
            .with_system("s")
            .with_user("u")
            .with_tool(ToolDefinition {
                name: "x".to_string(),
                description: "y".to_string(),
                parameters: serde_json::Value::Object(Default::default()),
            });
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.messages[1].role, Role::User);
        assert_eq!(req.tools.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_provider_tool_calls() {
        let provider = MockProvider::new(
            "mock",
            CompletionResponse {
                content: String::new(),
                tool_calls: vec![LlmToolCall {
                    id: "tc1".to_string(),
                    name: "create_agent".to_string(),
                    arguments: serde_json::json!({"id": "a1", "name": "A", "prompt": "p"}),
                }],
            },
        );
        let request = CompletionRequest::new("mock", "m").with_user("create agent");
        let response = provider.complete(request).await.unwrap();
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "create_agent");
    }

    #[test]
    fn test_openai_response_deserialization() {
        let body = serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "hello",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "create_agent",
                            "arguments": "{\"id\":\"a1\"}"
                        }
                    }]
                }
            }]
        });
        let resp: OpenAiResponse = serde_json::from_value(body).unwrap();
        assert_eq!(resp.choices[0].message.content, Some("hello".to_string()));
        assert_eq!(resp.choices[0].message.tool_calls.as_ref().map(|v| v.len()), Some(1));
    }

    #[test]
    fn test_stream_chunk_deserialization() {
        let data = r#"{"choices":[{"delta":{"content":"hello"},"finish_reason":null}]}"#;
        let chunk: OpenAiStreamChunk = serde_json::from_str(data).unwrap();
        assert_eq!(chunk.choices[0].delta.content, Some("hello".to_string()));
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
        let buffer = vec![
            Some(OpenAiStreamToolCall {
                index: 0,
                id: Some("call_1".to_string()),
                tool_type: Some("function".to_string()),
                function: Some(OpenAiStreamFunctionCall {
                    name: Some("create_agent".to_string()),
                    arguments: Some("{\"id\":\"a1\"}".to_string()),
                }),
            }),
        ];
        let calls = flush_tool_call_buffer(&buffer);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "create_agent");
    }
}
