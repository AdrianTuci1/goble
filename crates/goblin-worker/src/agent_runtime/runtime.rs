use std::sync::Arc;

use goble_core::agent::{AgentId, AgentSpec};
use goble_core::execution::{ExecutionStatus, ExecutionTrace, LogLevel};
use goble_core::llm::{CompletionRequest, LlmProvider, Message, Role};
use goble_core::protocol::WorkerMessage;
use goble_core::workspace::Workspace;

use crate::agent_runtime::state::RuntimeState;
use crate::agent_runtime::tools::{ToolContext, ToolRegistry};
use crate::state::AppState;

const DEFAULT_MAX_STEPS: usize = 50;
const MAX_HISTORY_MESSAGES: usize = 40;
const SUMMARIZE_THRESHOLD: usize = 80;
const SUMMARY_PROMPT: &str = "Summarize the following conversation into a concise paragraph that captures the task, key decisions, and remaining work.";

pub struct AgentRuntime {
    state: Arc<AppState>,
    max_steps: usize,
}

impl AgentRuntime {
    pub fn new(state: Arc<AppState>) -> Self {
        Self {
            state,
            max_steps: DEFAULT_MAX_STEPS,
        }
    }

    pub fn with_max_steps(mut self, max_steps: usize) -> Self {
        self.max_steps = max_steps;
        self
    }

    pub async fn run(
        &self,
        trace_id: String,
        agent_id: AgentId,
        spec: AgentSpec,
        user_prompt: Option<String>,
        provider: Box<dyn LlmProvider>,
    ) -> anyhow::Result<(ExecutionTrace, Option<String>)> {
        let mut trace = ExecutionTrace::new(agent_id.clone());
        trace.id = trace_id.clone();
        trace.worker_id = Some(self.state.worker_id.clone());
        trace.status = ExecutionStatus::Running;
        self.state.store_trace(trace.clone());
        self.state.emit(WorkerMessage::AgentStarted {
            trace_id: trace_id.clone(),
            agent_id: agent_id.clone(),
        });

        let workspace = Workspace::new(agent_id.clone(), &self.state.config.lock().workspace_root);
        workspace.ensure_exists()?;

        let root_id = trace.add_root_step("agent runtime").id.clone();
        trace
            .find_step_mut(&root_id)
            .unwrap()
            .log(LogLevel::Info, format!("workspace: {}", workspace.path.display()));
        self.state.store_trace(trace.clone());

        let mut runtime_state = match RuntimeState::load(&workspace.path) {
            Ok(state) => state,
            Err(e) => {
                trace.find_step_mut(&root_id).unwrap().log(
                    LogLevel::Warn,
                    format!("failed to load runtime state: {}; starting fresh", e),
                );
                RuntimeState::new()
            }
        };

        let mut ctx = ToolContext::new(workspace.path.clone());
        ctx.runtime_state = runtime_state.clone();

        let system_prompt = build_system_prompt(&spec, &runtime_state);
        let user_message = user_prompt.unwrap_or_else(|| "Please proceed with your task.".into());
        let mut messages = vec![
            Message {
                role: Role::System,
                content: system_prompt,
                tool_calls: None,
                tool_call_id: None,
            },
            Message {
                role: Role::User,
                content: user_message,
                tool_calls: None,
                tool_call_id: None,
            },
        ];

        let mut finished = false;
        let mut finish_summary: Option<String> = None;

        for _step in 0..self.max_steps {
            let request = CompletionRequest::new(provider.name(), "gpt-4o-mini")
                .with_messages(messages.clone())
                .with_tools(ToolRegistry::definitions());

            let response = provider.complete(request).await?;

            if !response.content.is_empty() {
                trace
                    .find_step_mut(&root_id)
                    .unwrap()
                    .log(LogLevel::Info, format!("assistant: {}", response.content));
                self.state.store_trace(trace.clone());
            }

            if response.tool_calls.is_empty() {
                finish_summary = Some(response.content.clone());
                trace
                    .find_step_mut(&root_id)
                    .unwrap()
                    .log(LogLevel::Info, "no tool calls; finishing with assistant message");
                finished = true;
                break;
            }

            let mut tool_results = Vec::new();
            for call in &response.tool_calls {
                let tool_log = format!("tool call {}: {}", call.id, call.name);
                trace.find_step_mut(&root_id).unwrap().log(
                    LogLevel::Info,
                    format!("{} with args {}", tool_log, call.arguments),
                );
                self.state.store_trace(trace.clone());

                let result = match ToolRegistry::execute(&mut ctx, &call.name, &call.arguments) {
                    Ok(result) => {
                        if result.finished {
                            finished = true;
                            finish_summary = result.finish_summary.clone();
                            trace
                                .find_step_mut(&root_id)
                                .unwrap()
                                .log(LogLevel::Info, format!("finished: {}", result.output));
                        }
                        if let Some(new_state) = result.state {
                            runtime_state = new_state;
                            if let Err(e) = runtime_state.save(&workspace.path) {
                                trace.find_step_mut(&root_id).unwrap().log(
                                    LogLevel::Warn,
                                    format!("failed to save runtime state: {}", e),
                                );
                            }
                            self.state.emit(WorkerMessage::AgentStateUpdate {
                                trace_id: trace_id.clone(),
                                state: runtime_state.clone(),
                            });
                        }

                        self.state.emit(WorkerMessage::AgentToolResult {
                            trace_id: trace_id.clone(),
                            step_id: root_id.clone(),
                            name: call.name.clone(),
                            result: result.output.clone(),
                        });
                        result.output
                    }
                    Err(e) => {
                        trace.find_step_mut(&root_id).unwrap().log(
                            LogLevel::Error,
                            format!("tool {} error: {}", call.name, e),
                        );
                        format!("error: {}", e)
                    }
                };
                self.state.store_trace(trace.clone());
                tool_results.push((call.id.clone(), result));
            }

            messages.push(Message {
                role: Role::Assistant,
                content: response.content,
                tool_calls: Some(response.tool_calls.clone()),
                tool_call_id: None,
            });
            for (tool_call_id, content) in tool_results {
                messages.push(Message {
                    role: Role::Tool,
                    content,
                    tool_calls: None,
                    tool_call_id: Some(tool_call_id),
                });
            }

            messages = summarize_if_needed(messages, provider.as_ref()).await?;
            messages = trim_history(messages);

            if finished {
                break;
            }
        }

        if !finished {
            trace
                .find_step_mut(&root_id)
                .unwrap()
                .log(LogLevel::Warn, "reached max steps without finish");
        }

        trace.find_step_mut(&root_id).unwrap().finish(ExecutionStatus::Success);
        let status = ExecutionStatus::Success;
        trace.finish(status.clone());
        self.state.store_trace(trace.clone());
        self.state.emit(WorkerMessage::AgentFinished {
            trace_id: trace_id.clone(),
            status,
        });

        if let Some(ref summary) = finish_summary {
            trace.find_step_mut(&root_id).unwrap().log(
                LogLevel::Info,
                format!("final summary: {}", summary),
            );
            self.state.store_trace(trace.clone());
        }

        Ok((trace, finish_summary))
    }
}

fn build_system_prompt(spec: &AgentSpec, runtime_state: &RuntimeState) -> String {
    let mut lines = Vec::new();
    lines.push(format!("You are an autonomous coding agent.{}", spec.prompt));
    lines.push("".into());
    lines.push("You have access to tools. Use them to complete the task.".into());
    lines.push("When you are done, call the `finish` tool with a summary.".into());
    lines.push("".into());
    if !runtime_state.checklist.is_empty() {
        lines.push("Current checklist:".into());
        for item in &runtime_state.checklist {
            let status = if item.done { "[x]" } else { "[ ]" };
            lines.push(format!("{} {} ({})", status, item.text, item.id));
        }
        lines.push("".into());
    }
    if !runtime_state.notes.is_empty() {
        lines.push("Notes:".into());
        for note in &runtime_state.notes {
            lines.push(format!("- {}", note));
        }
        lines.push("".into());
    }
    lines.join("\n")
}

async fn summarize_if_needed(
    mut messages: Vec<Message>,
    provider: &dyn LlmProvider,
) -> anyhow::Result<Vec<Message>> {
    let system_count = messages.iter().take_while(|m| m.role == Role::System).count();
    let non_system = messages.len() - system_count;
    if non_system < SUMMARIZE_THRESHOLD {
        return Ok(messages);
    }
    let to_summarize: Vec<Message> = messages.drain(system_count..).collect();
    let mut summary_request = CompletionRequest::new(provider.name(), "gpt-4o-mini")
        .with_system(SUMMARY_PROMPT)
        .with_user(serde_json::to_string(&to_summarize)?);
    summary_request.messages.extend(to_summarize);
    let summary_response = provider.complete(summary_request).await?;
    let summary = Message {
        role: Role::User,
        content: format!("Summary of prior work: {}", summary_response.content),
        tool_calls: None,
        tool_call_id: None,
    };
    messages.push(summary);
    Ok(messages)
}

fn trim_history(mut messages: Vec<Message>) -> Vec<Message> {
    let system_count = messages.iter().take_while(|m| m.role == Role::System).count();
    let non_system = messages.len() - system_count;
    if non_system > MAX_HISTORY_MESSAGES {
        let drop = non_system - MAX_HISTORY_MESSAGES;
        let mut kept = messages.drain(..system_count).collect::<Vec<_>>();
        let rest: Vec<Message> = messages.into_iter().skip(drop).collect();
        kept.extend(rest);
        kept
    } else {
        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::llm::{CompletionResponse, LlmToolCall, MockProvider};
    use goble_core::worker::WorkerId;
    use serde_json::json;
    use tempfile::tempdir;

    fn test_state(tmp: &tempfile::TempDir) -> Arc<AppState> {
        let state = AppState::new(WorkerId::generate());
        {
            let mut cfg = state.config.lock();
            cfg.workspace_root = tmp.path().to_path_buf();
        }
        state
    }

    #[tokio::test]
    async fn test_runtime_loop_finishes_via_tool() {
        let tmp = tempdir().unwrap();
        let state = test_state(&tmp);
        let runtime = AgentRuntime::new(state.clone()).with_max_steps(5);
        let spec = AgentSpec::new("demo", "Create a file with 'done' and finish.");

        let step1 = vec![LlmToolCall {
            id: "call_1".into(),
            name: "edit_file".into(),
            arguments: json!({
                "path": "result.txt",
                "old_string": "",
                "new_string": "done"
            }),
        }];
        let step2 = vec![LlmToolCall {
            id: "call_2".into(),
            name: "finish".into(),
            arguments: json!({"summary": "completed"}),
        }];

        let _provider = Box::new(MockProvider::new(
            "mock",
            CompletionResponse {
                content: "ok".into(),
                tool_calls: step1.clone(),
            },
        ));

        // First response is edit_file. After seeing its result, we need a second
        // provider call that returns finish. MockProvider always returns the same
        // response, so we use a custom provider that alternates.
        let provider = Box::new(AlternatingProvider {
            responses: vec![
                CompletionResponse {
                    content: "creating file".into(),
                    tool_calls: step1.clone(),
                },
                CompletionResponse {
                    content: "done".into(),
                    tool_calls: step2.clone(),
                },
            ],
            index: std::sync::atomic::AtomicUsize::new(0),
        });

        let (trace, _summary) = runtime
            .run("trace-1".into(), AgentId::generate(), spec, None, provider)
            .await
            .unwrap();
        assert_eq!(trace.status, ExecutionStatus::Success);
        assert!(trace.steps.iter().any(|s| s.logs.iter().any(|l| l.message.contains("finished:"))));
    }

    struct AlternatingProvider {
        responses: Vec<CompletionResponse>,
        index: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl LlmProvider for AlternatingProvider {
        fn name(&self) -> &str {
            "alternating"
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
}
