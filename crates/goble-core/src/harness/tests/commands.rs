use super::*;
use crate::harness::*;

use futures::StreamExt;

#[tokio::test]
async fn test_harness_run_command_mock() {
    let (_store, chat_id, harness) = harness_with_tool(
        "run_command",
        serde_json::json!({
            "command": "echo",
            "args": ["hi"]
        }),
    );
    // The immediate path is the auto-approved one; approval suspension is
    // covered by the harness approval tests.
    let harness = harness.with_auto_approve(true);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "run echo hi", "mock", "mock")
        .collect()
        .await;
    let finished = events.iter().any(|e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result.contains("mock ran")));
    assert!(finished);
}

#[tokio::test]
async fn test_harness_unknown_command() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: "ok".to_string(),
            tool_calls: vec![LlmToolCall {
                id: "tc3".to_string(),
                name: "not_a_tool".to_string(),
                arguments: serde_json::json!({}),
            }],
            usage: None,
        },
    ));
    let harness = Harness::new(store).with_llm(llm);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "do it", "mock", "mock")
        .collect()
        .await;
    assert!(events
        .iter()
        .any(|e| matches!(e, HarnessEvent::ToolCallError { .. })));
}
