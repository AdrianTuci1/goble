use serde::{Deserialize, Serialize};

pub type ToolCall = String;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub provider: String,
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
}

impl CompletionRequest {
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            messages: Vec::new(),
            temperature: None,
        }
    }

    pub fn with_system(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message {
            role: Role::System,
            content: content.into(),
        });
        self
    }

    pub fn with_user(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message {
            role: Role::User,
            content: content.into(),
        });
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn complete(&self, request: CompletionRequest) -> anyhow::Result<CompletionResponse>;
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

    async fn complete(&self, _request: CompletionRequest) -> anyhow::Result<CompletionResponse> {
        Ok(self.response.clone())
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
            .with_user("u");
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.messages[1].role, Role::User);
    }

    #[test]
    fn test_serialization() {
        let msg = Message {
            role: Role::User,
            content: "x".to_string(),
        };
        let s = serde_json::to_string(&msg).unwrap();
        let d: Message = serde_json::from_str(&s).unwrap();
        assert_eq!(d, msg);
    }
}
