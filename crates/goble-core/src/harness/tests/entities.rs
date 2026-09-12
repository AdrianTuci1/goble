use super::*;
use crate::harness::*;

use crate::agent::{AgentId, AgentSpec, Trigger};
use futures::StreamExt;

#[tokio::test]
async fn test_harness_list_entities_and_search() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let now = Utc::now().to_rfc3339();
    let spec = AgentSpec {
        id: AgentId("agent-x".to_string()),
        name: "Xavier".to_string(),
        description: "".to_string(),
        prompt: "".to_string(),
        tools: vec![],
        triggers: vec![Trigger::Manual],
        mcp_ids: vec![],
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    let spec_json = serde_json::to_string(&spec).unwrap();
    store
        .insert_agent(
            "agent-x",
            "Xavier",
            &spec_json,
            &spec.created_at,
            &spec.updated_at,
        )
        .unwrap();
    store.insert_team("team-1", "X-Men", "{}", &now).unwrap();

    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![
                LlmToolCall {
                    id: "tc6".to_string(),
                    name: "list_entities".to_string(),
                    arguments: serde_json::json!({"entity_type": "agents"}),
                },
                LlmToolCall {
                    id: "tc7".to_string(),
                    name: "search_store".to_string(),
                    arguments: serde_json::json!({"query": "x", "entity_types": ["agents", "teams"]}),
                },
            ],
            usage: None,
        },
    ));
    let harness = Harness::new(store).with_llm(llm);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "list and search", "mock", "mock")
        .collect()
        .await;
    let finished: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            HarnessEvent::ToolCallFinished { result, .. } => Some(result.clone()),
            _ => None,
        })
        .collect();
    assert!(finished.iter().any(|r| r.contains("agent-x")));
    assert!(finished
        .iter()
        .any(|r| r.contains("team-1") && r.contains("X-Men")));
}
