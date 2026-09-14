use super::*;
use crate::harness::*;

use crate::agent::AgentSpec;
use crate::workflow::Workflow;
use futures::StreamExt;

#[tokio::test]
async fn test_harness_create_agent() {
    let (store, chat_id, harness) = harness_with_tool(
        "create_agent",
        serde_json::json!({
            "id": "agent-1",
            "name": "Greeter",
            "prompt": "Say hello"
        }),
    );
    let events: Vec<_> = harness
        .run_turn(&chat_id, "make a greeter", "mock", "mock")
        .collect()
        .await;
    let started = events
        .iter()
        .any(|e| matches!(e, HarnessEvent::ToolCallStarted { name, .. } if name == "create_agent"));
    assert!(started);
    let agents = store.list_agents().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].0, "agent-1");
}

#[tokio::test]
async fn test_harness_delete_entities() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let now = Utc::now().to_rfc3339();
    let spec = AgentSpec::new("A", "p");
    store
        .insert_agent(
            "agent-1",
            "A",
            &serde_json::to_string(&spec).unwrap(),
            &spec.created_at,
            &spec.updated_at,
        )
        .unwrap();
    store.insert_team("team-1", "T", "{}", &now).unwrap();
    let wf = Workflow::new("wf", "").with_steps(vec![]);
    let spec_json = serde_json::to_string(&wf).unwrap();
    let trigger_str = serde_json::to_string(&wf.trigger).unwrap();
    store
        .insert_workflow(
            &wf.id.to_string(),
            &wf.name,
            &wf.description,
            &spec_json,
            &trigger_str,
            true,
            &wf.created_at,
            &wf.updated_at,
        )
        .unwrap();

    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![
                LlmToolCall {
                    id: "tc_del_a".to_string(),
                    name: "delete_agent".to_string(),
                    arguments: serde_json::json!({"id": "agent-1"}),
                },
                LlmToolCall {
                    id: "tc_del_t".to_string(),
                    name: "delete_team".to_string(),
                    arguments: serde_json::json!({"id": "team-1"}),
                },
                LlmToolCall {
                    id: "tc_del_w".to_string(),
                    name: "delete_workflow".to_string(),
                    arguments: serde_json::json!({"id": wf.id.to_string()}),
                },
            ],
            usage: None,
        },
    ));
    let harness = Harness::new(store.clone()).with_llm(llm);
    harness
        .run_turn(&chat_id, "delete all", "mock", "mock")
        .collect::<Vec<_>>()
        .await;
    assert!(store.list_agents().unwrap().is_empty());
    assert!(store.list_teams().unwrap().is_empty());
    assert!(store.list_workflows().unwrap().is_empty());
}
