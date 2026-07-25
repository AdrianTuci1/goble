use goble_core::llm::{CompletionRequest, LlmProvider, Message, Role, ToolDefinition};
use serde_json::json;

#[tokio::test]
async fn test_deepseek_streaming_hello() {
    let api_key = std::fs::read_to_string("/tmp/.goble_deepseek_key")
        .expect("deepseek key file")
        .trim()
        .to_string();
    let provider = goble_core::llm::OpenAiProvider::new(
        "deepseek",
        api_key,
        "https://api.deepseek.com/v1",
    );
    let request = CompletionRequest::new("deepseek", "deepseek-v4-pro")
        .with_system("You are a concise assistant.")
        .with_user("Say exactly 'hello goble' and nothing else.");
    let mut stream = provider.complete_stream(request).await.expect("stream request");
    let mut content = String::new();
    let mut got_delta = false;
    while let Some(event) = futures::StreamExt::next(&mut stream).await {
        match event {
            goble_core::llm::CompletionStreamEvent::AssistantDelta(delta) => {
                got_delta = true;
                content.push_str(&delta);
            }
            goble_core::llm::CompletionStreamEvent::Done => break,
            goble_core::llm::CompletionStreamEvent::Error(e) => panic!("stream error: {e}"),
            _ => {}
        }
    }
    assert!(got_delta, "no delta received");
    let lower = content.to_lowercase();
    assert!(lower.contains("hello goble"), "unexpected response: {content}");
}

#[tokio::test]
async fn test_deepseek_tool_call() {
    let api_key = std::fs::read_to_string("/tmp/.goble_deepseek_key")
        .expect("deepseek key file")
        .trim()
        .to_string();
    let provider = goble_core::llm::OpenAiProvider::new(
        "deepseek",
        api_key,
        "https://api.deepseek.com/v1",
    );
    let tool = ToolDefinition {
        name: "greet".to_string(),
        description: "Return a greeting for a name".to_string(),
        parameters: json!({
            "type": "object",
            "properties": { "name": { "type": "string" } },
            "required": ["name"]
        }),
    };
    let request = CompletionRequest::new("deepseek", "deepseek-v4-pro")
        .with_system("Use the greet tool when asked to greet someone.")
        .with_user("Greet Ada with the greet tool.")
        .with_tool(tool);
    let response = provider.complete(request).await.expect("complete request");
    assert!(
        !response.tool_calls.is_empty(),
        "expected a tool call, got: {}",
        response.content
    );
    let call = &response.tool_calls[0];
    assert_eq!(call.name, "greet");
    let name = call
        .arguments
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert_eq!(name, "Ada", "got args {:?}", call.arguments);
}

#[tokio::test]
async fn test_provider_factory_deepseek() {
    let provider = goble_core::llm::create_provider(
        "openai",
        "sk-fake",
        Some("https://api.deepseek.com/v1"),
    );
    assert_eq!(provider.name(), "openai");
}

#[tokio::test]
async fn test_deepseek_with_conversation_history() {
    let api_key = std::fs::read_to_string("/tmp/.goble_deepseek_key")
        .expect("deepseek key file")
        .trim()
        .to_string();
    let provider = goble_core::llm::OpenAiProvider::new(
        "deepseek",
        api_key,
        "https://api.deepseek.com/v1",
    );
    let messages = vec![
        Message {
            role: Role::System,
            content: "Answer in one word.".to_string(),
            tool_calls: None,
            tool_call_id: None,
        },
        Message {
            role: Role::User,
            content: "Capital of France?".to_string(),
            tool_calls: None,
            tool_call_id: None,
        },
    ];
    let request = CompletionRequest::new("deepseek", "deepseek-v4-pro").with_messages(messages);
    let response = provider.complete(request).await.expect("complete request");
    assert!(response.content.to_lowercase().contains("paris"));
}
