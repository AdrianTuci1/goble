use anyhow::{Context, Result};
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
}

/// OpenAI-compatible chat completions provider using `reqwest`.
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
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCall {
    id: String,
    function: OpenAiFunctionCall,
}

#[derive(Debug, Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

#[async_trait::async_trait]
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        let messages: Vec<OpenAiMessage> = request
            .messages
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
            .collect();

        let tools = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
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
        };

        let body = OpenAiRequest {
            model: request.model,
            messages,
            tools,
            temperature: request.temperature,
        };

        let url = format!("{}/chat/completions", self.base_url);
        let req = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body);
        let resp = req.send().await.with_context(|| format!("failed to POST to {url}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text: String = resp.text().await.unwrap_or_default();
            anyhow::bail!("LLM request failed: {status} {text}");
        }

        let bytes = resp.bytes().await.context("failed to read LLM response")?;
        let data: OpenAiResponse = serde_json::from_slice(&bytes).context("failed to parse LLM response")?;
        let choice = data.choices.into_iter().next().context("no choices returned")?;
        let content = choice.message.content.unwrap_or_default();
        let tool_calls = choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .map(|tc| {
                let args = serde_json::from_str(&tc.function.arguments).unwrap_or_default();
                LlmToolCall {
                    id: tc.id,
                    name: tc.function.name,
                    arguments: args,
                }
            })
            .collect();

        Ok(CompletionResponse {
            content,
            tool_calls,
        })
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
}
