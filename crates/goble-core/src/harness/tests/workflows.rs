use super::*;
use crate::harness::*;

use crate::agent::{AgentId, AgentSpec, Trigger};
use crate::workflow::Workflow;
use futures::StreamExt;

#[tokio::test]
async fn test_harness_create_workflow_and_team() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let now = Utc::now().to_rfc3339();
    let spec_a = AgentSpec {
        id: AgentId("agent-a".to_string()),
        name: "A".to_string(),
        description: "".to_string(),
        prompt: "".to_string(),
        tools: vec![],
        triggers: vec![Trigger::Manual],
        mcp_ids: vec![],
        created_at: now.clone(),
        updated_at: now,
    };
    let spec_json = serde_json::to_string(&spec_a).unwrap();
    store
        .insert_agent(
            "agent-a",
            "A",
            &spec_json,
            &spec_a.created_at,
            &spec_a.updated_at,
        )
        .unwrap();
    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![
                LlmToolCall {
                    id: "tc4".to_string(),
                    name: "create_workflow".to_string(),
                    arguments: serde_json::json!({
                        "name": "wf1",
                        "steps": [{"name": "step1", "agent_id": "agent-a", "input_template": "go"}]
                    }),
                },
                LlmToolCall {
                    id: "tc5".to_string(),
                    name: "create_team".to_string(),
                    arguments: serde_json::json!({"id": "team-1", "name": "T1", "agent_ids": ["agent-a"]}),
                },
            ],
            usage: None,
        },
    ));
    let harness = Harness::new(store.clone()).with_llm(llm);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "make workflow and team", "mock", "mock")
        .collect()
        .await;
    assert!(events.iter().any(|e| matches!(e, HarnessEvent::Done)));
    assert_eq!(store.list_workflows().unwrap().len(), 1);
    assert_eq!(store.list_teams().unwrap().len(), 1);
}

#[tokio::test]
async fn test_harness_deploy_agent_without_sender() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let _now = Utc::now().to_rfc3339();
    let spec = AgentSpec::new("A", "p");
    let spec_json = serde_json::to_string(&spec).unwrap();
    store
        .insert_agent(
            "agent-1",
            "A",
            &spec_json,
            &spec.created_at,
            &spec.updated_at,
        )
        .unwrap();

    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![LlmToolCall {
                id: "tc8".to_string(),
                name: "deploy_agent".to_string(),
                arguments: serde_json::json!({"agent_id": "agent-1", "worker_id": "w-1"}),
            }],
            usage: None,
        },
    ));
    let harness = Harness::new(store).with_llm(llm);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "deploy", "mock", "mock")
        .collect()
        .await;
    let finished = events.iter().any(|e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result.contains("no deploy channel")));
    assert!(finished);
}

#[tokio::test]
async fn test_harness_schedule_workflow_and_get_execution() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
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
            tool_calls: vec![LlmToolCall {
                id: "tc9".to_string(),
                name: "schedule_workflow".to_string(),
                arguments: serde_json::json!({"workflow_id": wf.id.to_string(), "trigger_type": "cron", "trigger_value": "0 9 * * *"}),
            }],
            usage: None,
        },
    ));
    let harness = Harness::new(store.clone()).with_llm(llm);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "schedule", "mock", "mock")
        .collect()
        .await;
    let finished = events.iter().any(|e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result.contains("scheduled")));
    assert!(finished);

    let rows = store.list_workflows().unwrap();
    assert!(rows[0].4.contains("Cron"));
}
