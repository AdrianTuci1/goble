use super::*;
use crate::harness::*;

use futures::StreamExt;

#[tokio::test]
async fn test_execute_open_screen_acknowledges_handoff() {
    let (_, chat_id, harness) = harness_with_tool(
        "open_screen",
        serde_json::json!({ "host": "vm.example.com", "credential": "desktop-account" }),
    );
    let events: Vec<_> = harness
        .run_turn(&chat_id, "open the remote desktop", "mock", "mock")
        .collect()
        .await;
    let finished_tool = events.iter().any(|e| {
        matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result
            == "handoff requested to vm.example.com:3389 with stored credential `desktop-account`")
    });
    assert!(finished_tool);
    // The acknowledgement carries the reference only: no account, no secret.
    assert!(
        !events.iter().any(|e| matches!(
            e,
            HarnessEvent::ToolCallFinished { result, .. } if result.contains("password")
        )),
        "the tool result must not mention a password: {events:?}"
    );
}

/// A handoff with no credential is refused at the tool, so the model is told
/// there is nothing to resolve instead of a desktop opening without an account.
#[tokio::test]
async fn open_screen_without_a_credential_fails_the_tool_call() {
    let (_, chat_id, harness) = harness_with_tool(
        "open_screen",
        serde_json::json!({ "host": "vm.example.com" }),
    );
    let events: Vec<_> = harness
        .run_turn(&chat_id, "open the remote desktop", "mock", "mock")
        .collect()
        .await;
    assert!(
        events.iter().any(|e| matches!(
            e,
            HarnessEvent::ToolCallError { message, .. }
                if message.contains("requires a `credential`")
                    && message.contains("stored desktop credential")
        )),
        "the missing credential must fail the call: {events:?}"
    );
}

/// The reference names a credential; an account line or a value in its place is
/// refused rather than echoed into the tool's result and the handoff event.
#[tokio::test]
async fn open_screen_refuses_an_account_value_in_the_credential_field() {
    for written in ["goble:hunter2", "goble hunter2"] {
        let (_, chat_id, harness) = harness_with_tool(
            "open_screen",
            serde_json::json!({ "host": "vm.example.com", "credential": written }),
        );
        let events: Vec<_> = harness
            .run_turn(&chat_id, "open the remote desktop", "mock", "mock")
            .collect()
            .await;
        assert!(
            events.iter().any(|e| matches!(
                e,
                HarnessEvent::ToolCallError { message, .. }
                    if message.contains("NAME of a credential stored on the host")
            )),
            "`{written}` must be refused as a reference: {events:?}"
        );
        assert!(
            !events.iter().any(|e| matches!(
                e,
                HarnessEvent::ToolCallFinished { result, .. } if result.contains(written)
            )),
            "nothing may echo `{written}` back: {events:?}"
        );
    }
}
