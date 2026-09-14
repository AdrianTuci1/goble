use super::*;
use crate::harness::*;

use crate::agent::AgentSpec;
use futures::StreamExt;
use std::path::PathBuf;

#[tokio::test]
async fn test_harness_read_write_edit_file() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![
                LlmToolCall {
                    id: "tc10".to_string(),
                    name: "write_file".to_string(),
                    arguments: serde_json::json!({"path": "harness_test.txt", "content": "hello world"}),
                },
                LlmToolCall {
                    id: "tc11".to_string(),
                    name: "edit_file".to_string(),
                    arguments: serde_json::json!({"path": "harness_test.txt", "old_text": "world", "new_text": "Goble"}),
                },
                LlmToolCall {
                    id: "tc12".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path": "harness_test.txt"}),
                },
            ],
            usage: None,
        },
    ));
    let harness = Harness::new(store).with_llm(llm);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "file ops", "mock", "mock")
        .collect()
        .await;
    let finished: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            HarnessEvent::ToolCallFinished { result, .. } => Some(result.clone()),
            _ => None,
        })
        .collect();
    assert!(finished.iter().any(|r| r.contains("harness_test.txt")));
    assert!(finished.last().unwrap().contains("hello Goble"));
    std::fs::remove_file("harness_test.txt").ok();
}

#[tokio::test]
async fn test_harness_rename_and_delete_file() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    std::fs::write("harness_tmp.txt", "data").unwrap();
    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![LlmToolCall {
                id: "tc_ren".to_string(),
                name: "rename_file".to_string(),
                arguments: serde_json::json!({"from": "harness_tmp.txt", "to": "harness_renamed.txt"}),
            }],
            usage: None,
        },
    ));
    let harness = Harness::new(store.clone()).with_llm(llm);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "rename", "mock", "mock")
        .collect()
        .await;
    assert!(events.iter().any(
        |e| matches!(e, HarnessEvent::ToolCallFinished { result, .. } if result.contains("renamed"))
    ));
    assert!(std::fs::metadata("harness_renamed.txt").is_ok());

    let llm2 = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![LlmToolCall {
                id: "tc_del".to_string(),
                name: "delete_file".to_string(),
                arguments: serde_json::json!({"path": "harness_renamed.txt"}),
            }],
            usage: None,
        },
    ));
    let harness2 = Harness::new(store).with_llm(llm2);
    harness2
        .run_turn(&chat_id, "delete", "mock", "mock")
        .collect::<Vec<_>>()
        .await;
    assert!(!PathBuf::from("harness_renamed.txt").exists());
}

#[tokio::test]
async fn test_harness_git_status_and_run_agent() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let spec = AgentSpec::new("Greeter", "say hello");
    store
        .insert_agent(
            "agent-1",
            "Greeter",
            &serde_json::to_string(&spec).unwrap(),
            &spec.created_at,
            &spec.updated_at,
        )
        .unwrap();

    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![
                LlmToolCall {
                    id: "tc_git".to_string(),
                    name: "git_status".to_string(),
                    arguments: serde_json::json!({}),
                },
                LlmToolCall {
                    id: "tc_run".to_string(),
                    name: "run_agent".to_string(),
                    arguments: serde_json::json!({"agent_id": "agent-1", "input": "hi"}),
                },
            ],
            usage: None,
        },
    ));
    let harness = Harness::new(store).with_llm(llm);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "git and run", "mock", "mock")
        .collect()
        .await;
    let finished: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            HarnessEvent::ToolCallFinished { result, .. } => Some(result.clone()),
            _ => None,
        })
        .collect();
    assert!(finished
        .iter()
        .any(|r| r.contains("mock ran") && r.contains("git")));
    assert!(finished
        .iter()
        .any(|r| r.contains("ran agent") && r.contains("say hello")));
}
