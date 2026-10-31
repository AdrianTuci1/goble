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

/// A file mutation records the change it made on its own tool call: a write
/// states what it added, and an edit states its hunk against the file it read,
/// with the line numbers the replacement sits at.
#[tokio::test]
async fn a_file_mutation_records_its_diff_on_the_tool_call() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let path = "harness_diff.txt";
    std::fs::remove_file(path).ok();
    let llm = Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: String::new(),
            tool_calls: vec![
                LlmToolCall {
                    id: "tc_write".to_string(),
                    name: "write_file".to_string(),
                    arguments: serde_json::json!({"path": path, "content": "one\ntwo\nthree\n"}),
                },
                LlmToolCall {
                    id: "tc_edit".to_string(),
                    name: "edit_file".to_string(),
                    arguments: serde_json::json!({"path": path, "old_text": "two", "new_text": "TWO"}),
                },
            ],
            usage: None,
        },
    ));
    let harness = Harness::new(store.clone()).with_llm(llm);
    harness
        .run_turn(&chat_id, "file diffs", "mock", "mock")
        .collect::<Vec<_>>()
        .await;

    let records: Vec<ChatToolCall> = store
        .list_chat_messages(&chat_id)
        .unwrap()
        .into_iter()
        .filter_map(|(_, _, _, tool_calls, _)| tool_calls)
        .flat_map(|json| serde_json::from_str::<Vec<ChatToolCall>>(&json).unwrap_or_default())
        .collect();
    let write = records
        .iter()
        .find(|record| record.id == "tc_write")
        .expect("the write was recorded");
    assert_eq!(
        write.diff.as_deref(),
        Some("@@ -0,0 +1,3 @@\n+one\n+two\n+three\n"),
        "a new file's diff is everything it added"
    );
    let edit = records
        .iter()
        .find(|record| record.id == "tc_edit")
        .expect("the edit was recorded");
    assert_eq!(
        edit.diff.as_deref(),
        Some("@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n"),
        "an edit's diff is its hunk against the file, with context"
    );
    // The diff is the row's to draw, not part of what the model reads back.
    assert!(
        write.result.as_deref().unwrap_or("").contains(path),
        "the result is the write's own confirmation, got {:?}",
        write.result
    );
    assert!(!write.result.as_deref().unwrap_or("").contains("+one"));

    std::fs::remove_file(path).ok();
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
