use super::*;
use crate::harness::*;

use futures::StreamExt;

#[tokio::test]
async fn test_execute_open_screen_acknowledges_handoff() {
    let (_, chat_id, harness) = harness_with_tool(
        "open_screen",
        serde_json::json!({ "host": "vm.example.com" }),
    );
    let events: Vec<_> = harness
        .run_turn(&chat_id, "open the remote desktop", "mock", "mock")
        .collect()
        .await;
    let finished_tool = events.iter().any(
        |e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result.contains("handoff requested to vm.example.com:3389")),
    );
    assert!(finished_tool);
}
