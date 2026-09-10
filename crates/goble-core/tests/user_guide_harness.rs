//! Integration test for the `user_guide` tool: with a deterministic `MockProvider`
//! (no network), the model asks for the mobile-access topic and the harness returns
//! the seeded user-guide text so the agent can answer correctly.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use futures::StreamExt;
use goble_core::harness::{Harness, HarnessEvent};
use goble_core::llm::{CompletionResponse, LlmToolCall, MockProvider};
use goble_core::store::Store;

fn create_chat(store: &Store, title: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    store
        .insert_chat(&id, title, Some("mock"), Some("m"), &now, &now)
        .expect("insert chat");
    id
}

#[tokio::test]
async fn user_guide_tool_returns_mobile_access_doc() {
    // A docs dir shaped like the seeded ~/.goble/docs/user-guide.
    let docs = tempfile::TempDir::new().unwrap();
    std::fs::write(
        docs.path().join("07-mobile-access.md"),
        "# Mobile Access\n\nYou can expose this machine via Tailscale and connect from the Goble mobile app.\n",
    )
    .unwrap();

    let store = Store::open_in_memory().unwrap();
    let chat_id = create_chat(&store, "guide test");
    let harness = Harness::new(store)
        .with_llm(Arc::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: "".to_string(),
                tool_calls: vec![LlmToolCall {
                    id: "tc1".to_string(),
                    name: "user_guide".to_string(),
                    arguments: serde_json::json!({ "topic": "mobile-access" }),
                }],
            },
        )))
        .with_docs_dir(docs.path())
        .with_cancel(Arc::new(AtomicBool::new(false)));

    let mut finished = false;
    let mut tool_results = Vec::new();
    let mut errors = Vec::new();
    let mut stream = harness.run_turn(
        &chat_id,
        "How do I reach this machine from the mobile app?",
        "mock",
        "m",
    );
    while let Some(event) = stream.next().await {
        match event {
            HarnessEvent::Done => finished = true,
            HarnessEvent::Error(e) => errors.push(e),
            HarnessEvent::ToolCallFinished { result, .. } => tool_results.push(result),
            _ => {}
        }
    }

    assert!(finished, "stream did not finish; errors: {errors:?}");
    assert!(errors.is_empty(), "harness errors: {errors:?}");
    assert!(
        tool_results
            .iter()
            .any(|r| r.contains("Tailscale") && r.contains("mobile app")),
        "user_guide result missing mobile/tailscale text: {tool_results:?}"
    );
}
