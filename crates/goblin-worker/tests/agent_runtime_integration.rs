use goble_core::agent::AgentSpec;
use goble_core::execution::ExecutionStatus;
use goble_core::llm::{CompletionResponse, LlmProvider};
use goble_core::worker::WorkerId;
use goble_core::llm::CompletionRequest;
use goble_core::llm::LlmToolCall;

use goblin_worker::runner::{ProviderFactory, Runner};
use goblin_worker::state::AppState;

/// Mock provider that alternates between two LLM responses:
/// 1. edit_file tool call to create result.txt
/// 2. finish tool call to end the loop
struct AlternatingProvider {
    responses: Vec<CompletionResponse>,
    index: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl LlmProvider for AlternatingProvider {
    fn name(&self) -> &str {
        "alternating-mock"
    }

    async fn complete(&self, _request: CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let idx = self.index.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.responses[idx.min(self.responses.len() - 1)].clone())
    }

    async fn complete_stream(
        &self,
        _request: CompletionRequest,
    ) -> anyhow::Result<std::pin::Pin<Box<dyn futures::Stream<Item = goble_core::llm::CompletionStreamEvent> + Send>>> {
        unimplemented!()
    }
}

fn mock_factory() -> ProviderFactory {
    Box::new(|| {
        let step1 = vec![LlmToolCall {
            id: "call_1".into(),
            name: "edit_file".into(),
            arguments: serde_json::json!({
                "path": "result.txt",
                "old_string": "",
                "new_string": "done"
            }),
        }];
        let step2 = vec![LlmToolCall {
            id: "call_2".into(),
            name: "finish".into(),
            arguments: serde_json::json!({"summary": "completed"}),
        }];
        Ok(Box::new(AlternatingProvider {
            responses: vec![
                CompletionResponse {
                    content: "creating file".into(),
                    tool_calls: step1,
                },
                CompletionResponse {
                    content: "done".into(),
                    tool_calls: step2,
                },
            ],
            index: std::sync::atomic::AtomicUsize::new(0),
        }))
    })
}

#[tokio::test]
async fn test_agent_runtime_integration_writes_file_and_finishes() {
    let state = AppState::new(WorkerId::generate());
    let tmp = std::env::temp_dir().join(format!("goble-e2e-{}", uuid::Uuid::new_v4()));
    *state.config.lock() = goblin_worker::state::WorkerConfig {
        workspace_root: tmp.clone(),
    };

    let runner = Runner::new_with_provider_factory(state.clone(), mock_factory());
    let mut spec = AgentSpec::new("file-writer", "Create a file named result.txt with 'done' and finish.");
    let agent_id = spec.id.clone();
    let trace_id = "trace-file-writer".to_string();

    runner.run_agent(trace_id.clone(), agent_id, spec).await.unwrap();

    let trace = state.get_trace(&trace_id).unwrap();
    assert_eq!(trace.status, ExecutionStatus::Success);

    let result_path = find_file_recursively(&tmp, "result.txt")
        .expect("result.txt should be created in the agent workspace");
    let content = std::fs::read_to_string(&result_path).unwrap();
    assert_eq!(content.trim(), "done");

    std::fs::remove_dir_all(&tmp).ok();
}

fn find_file_recursively(root: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.file_name()? == name {
                    return Some(path);
                }
            }
        }
    }
    None
}

#[tokio::test]
async fn test_agent_runtime_for_thread_reply_returns_summary() {
    let state = AppState::new(WorkerId::generate());
    let tmp = std::env::temp_dir().join(format!("goble-reply-{}", uuid::Uuid::new_v4()));
    *state.config.lock() = goblin_worker::state::WorkerConfig {
        workspace_root: tmp.clone(),
    };

    let runner = Runner::new_with_provider_factory(state.clone(), mock_factory());
    let spec = AgentSpec::new("replier", "Reply with a summary and finish.");
    let agent_id = spec.id.clone();
    let trace_id = "trace-reply".to_string();
    let prompt = "Please summarize your task.".to_string();

    let reply = runner
        .run_agent_for_thread_reply(trace_id.clone(), agent_id, spec, prompt)
        .await
        .unwrap();

    assert_eq!(reply, "completed");

    let trace = state.get_trace(&trace_id).unwrap();
    assert_eq!(trace.status, ExecutionStatus::Success);

    std::fs::remove_dir_all(&tmp).ok();
}
