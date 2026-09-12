use crate::harness::ToolCallStatus;

use super::*;

use futures::StreamExt;

use crate::llm::openai::{
    flush_tool_call_buffer, into_openai_messages, OpenAiStreamChunk, OpenAiStreamFunctionCall,
    OpenAiStreamToolCall,
};

#[tokio::test]
async fn test_mock_provider() {
    let provider = MockProvider::new(
        "mock",
        CompletionResponse {
            content: "hello".to_string(),
            tool_calls: Vec::new(),
            usage: None,
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
            usage: None,
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
            usage: None,
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

/// The provider protocols have no status slot on a tool message, so a failed
/// call carries an explicit marker in the content the model reads.
#[test]
fn failed_tool_message_carries_a_failure_marker_on_the_wire() {
    let message = Message {
        role: Role::Tool,
        content: "unknown tool".to_string(),
        tool_calls: None,
        tool_call_id: Some("call_1".to_string()),
        tool_status: Some(ToolCallStatus::Error),
    };
    let wire = into_openai_messages(vec![message]);
    assert_eq!(
        wire[0].content.as_deref(),
        Some("[tool call failed]\nunknown tool")
    );
    assert_eq!(wire[0].tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn a_successful_tool_message_is_sent_as_its_body() {
    let message = Message {
        role: Role::Tool,
        content: "ok".to_string(),
        tool_calls: None,
        tool_call_id: Some("call_1".to_string()),
        tool_status: Some(ToolCallStatus::Finished),
    };
    let wire = into_openai_messages(vec![message]);
    assert_eq!(wire[0].content.as_deref(), Some("ok"));
}
