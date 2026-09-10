use goble_core::agent::AgentSpec;
use goble_core::execution::ExecutionStatus;
use goble_core::llm::{CompletionRequest, CompletionResponse, LlmProvider, LlmToolCall};
use goble_core::worker::WorkerId;

use goblin_worker::runner::Runner;
use goblin_worker::state::AppState;
use std::sync::Arc;

/// Mock provider that returns the next response in a sequence.
struct SequentialProvider {
    responses: Vec<CompletionResponse>,
    index: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl LlmProvider for SequentialProvider {
    fn name(&self) -> &str {
        "sequential-mock"
    }

    async fn complete(&self, _request: CompletionRequest) -> anyhow::Result<CompletionResponse> {
        let idx = self.index.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.responses[idx.min(self.responses.len() - 1)].clone())
    }

    async fn complete_stream(
        &self,
        _request: CompletionRequest,
    ) -> anyhow::Result<
        std::pin::Pin<
            Box<dyn futures::Stream<Item = goble_core::llm::CompletionStreamEvent> + Send>,
        >,
    > {
        let idx = self.index.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let response = self.responses[idx.min(self.responses.len() - 1)].clone();
        let events = vec![
            goble_core::llm::CompletionStreamEvent::AssistantDelta(response.content.clone()),
            goble_core::llm::CompletionStreamEvent::ToolCalls(response.tool_calls),
            goble_core::llm::CompletionStreamEvent::Done,
        ];
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

fn file_writer_factory() -> goblin_worker::runner::ProviderFactory {
    Box::new(|| {
        let step1 = vec![LlmToolCall {
            id: "call_1".into(),
            name: "write_file".into(),
            arguments: serde_json::json!({
                "path": "result.txt",
                "content": "done"
            }),
        }];
        let provider = SequentialProvider {
            responses: vec![
                CompletionResponse {
                    content: "I will create the file.".to_string(),
                    tool_calls: step1,
                },
                CompletionResponse {
                    content: "I am done.".to_string(),
                    tool_calls: vec![],
                },
            ],
            index: std::sync::atomic::AtomicUsize::new(0),
        };
        Ok(std::sync::Arc::new(provider))
    })
}

fn simple_ok_factory() -> goblin_worker::runner::ProviderFactory {
    Box::new(|| {
        Ok(std::sync::Arc::new(goble_core::llm::MockProvider::new(
            "mock",
            CompletionResponse {
                content: "ok".to_string(),
                tool_calls: vec![],
            },
        )))
    })
}

fn setup_tmp_state() -> (std::path::PathBuf, Arc<AppState>) {
    let state = AppState::new(WorkerId::generate());
    let tmp = std::env::temp_dir().join(format!("goble-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).unwrap();
    state.set_store_path(tmp.join("worker.db")).unwrap();
    *state.config.lock() = goblin_worker::state::WorkerConfig {
        workspace_root: tmp.clone(),
        llm_provider: None,
        llm_model: None,
        llm_base_url: None,
    };
    (tmp, state)
}

#[tokio::test]
async fn test_harness_integration_writes_file_and_finishes() {
    let (tmp, state) = setup_tmp_state();
    let runner = Runner::new_with_provider_factory(state.clone(), file_writer_factory());
    let spec = AgentSpec::new(
        "file-writer",
        "Create a file named result.txt with 'done' and finish.",
    );
    let agent_id = spec.id.clone();
    let trace_id = "trace-file-writer".to_string();

    runner
        .run_agent(trace_id.clone(), agent_id, spec, vec![], vec![])
        .await
        .unwrap();

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
async fn test_harness_for_thread_reply_returns_summary() {
    let (tmp, state) = setup_tmp_state();
    let runner = Runner::new_with_provider_factory(state.clone(), simple_ok_factory());
    let spec = AgentSpec::new("replier", "Reply with a summary and finish.");
    let agent_id = spec.id.clone();
    let trace_id = "trace-reply".to_string();
    let prompt = "Please summarize your task.".to_string();

    let reply = runner
        .run_agent_for_thread_reply(trace_id.clone(), agent_id, spec, prompt, vec![], vec![])
        .await
        .unwrap();

    assert_eq!(reply, "reply submitted");

    let trace = state.get_trace(&trace_id).unwrap();
    assert_eq!(trace.status, ExecutionStatus::Success);

    std::fs::remove_dir_all(&tmp).ok();
}
