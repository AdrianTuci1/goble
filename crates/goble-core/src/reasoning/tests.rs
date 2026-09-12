use super::history::build_history;
use super::*;
use crate::harness::Harness;
use crate::llm::{CompletionResponse, MockProvider};
use futures::StreamExt;
use std::sync::Arc;

use chrono::Utc;

use crate::harness::{ChatToolCall, CommandDecision, HarnessEvent, ThinkingMode, ToolCallStatus};
use crate::llm::{CompletionRequest, CompletionStreamEvent, LlmProvider, LlmToolCall, Role};
use crate::store::Store;

fn chat(store: &Store) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    store
        .insert_chat(&id, "test", None, None, &now, &now)
        .unwrap();
    id
}

fn llm_with_reasoning_tools(
    content: impl Into<String>,
    tool_calls: Vec<LlmToolCall>,
) -> Arc<dyn LlmProvider> {
    Arc::new(MockProvider::new(
        "mock",
        CompletionResponse {
            content: content.into(),
            tool_calls,
            usage: None,
        },
    ))
}

#[tokio::test]
async fn test_reasoning_mode_switch_and_execute() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = llm_with_reasoning_tools(
        "I will switch to planning and execute.",
        vec![
            LlmToolCall {
                id: "tc1".to_string(),
                name: "set_thinking_mode".to_string(),
                arguments: serde_json::json!({"mode": "planning"}),
            },
            LlmToolCall {
                id: "tc2".to_string(),
                name: "execute".to_string(),
                arguments: serde_json::json!({}),
            },
        ],
    );
    let harness = Harness::new(store).with_llm(llm).with_reasoning(true);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "build a daily report workflow", "mock", "mock")
        .collect()
        .await;
    assert!(events
        .iter()
        .any(|e| matches!(e, HarnessEvent::ReasoningStarted { mode, .. } if mode == "direct")));
    assert!(events.iter().any(|e| matches!(e, HarnessEvent::ReasoningDone { decision, .. } if decision == "\"execute\"")));
}

#[tokio::test]
async fn test_ask_user_suspends_and_creates_pending_ask() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = llm_with_reasoning_tools(
        "Need more info.",
        vec![LlmToolCall {
            id: "tc3".to_string(),
            name: "ask_user".to_string(),
            arguments: serde_json::json!({
                "question": "Which database should I query?",
                "quick_replies": ["postgres", "mysql"]
            }),
        }],
    );
    let harness = Harness::new(store.clone())
        .with_llm(llm)
        .with_reasoning(true);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "automate reports", "mock", "mock")
        .collect()
        .await;
    assert!(events.iter().any(|e| matches!(e, HarnessEvent::AskUser { question, .. } if question == "Which database should I query?")));
    assert!(store.get_pending_ask(&chat_id).unwrap().is_some());

    let llm2 = llm_with_reasoning_tools(
        "Got it, executing now.",
        vec![LlmToolCall {
            id: "tc4".to_string(),
            name: "execute".to_string(),
            arguments: serde_json::json!({}),
        }],
    );
    let harness2 = Harness::new(store.clone()).with_llm(llm2);
    let events2: Vec<_> = harness2
        .resume_turn(&chat_id, "postgres", None, "mock", "mock")
        .collect()
        .await;
    assert!(events2
        .iter()
        .any(|e| matches!(e, HarnessEvent::ReasoningDone { .. })));
    assert!(store.get_pending_ask(&chat_id).unwrap().is_none());
}

#[tokio::test]
async fn test_resume_stores_credential_by_name_not_value() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = llm_with_reasoning_tools(
        "Need a token.",
        vec![LlmToolCall {
            id: "tc3".to_string(),
            name: "ask_user".to_string(),
            arguments: serde_json::json!({
                "question": "What's the GitHub token?",
                "quick_replies": []
            }),
        }],
    );
    let harness = Harness::new(store.clone()).with_llm(llm).with_reasoning(true);
    let _: Vec<_> = harness
        .run_turn(&chat_id, "set up a deploy", "mock", "mock")
        .collect()
        .await;

    let llm2 = llm_with_reasoning_tools(
        "Got it.",
        vec![LlmToolCall {
            id: "tc4".to_string(),
            name: "execute".to_string(),
            arguments: serde_json::json!({}),
        }],
    );
    let harness2 = Harness::new(store.clone()).with_llm(llm2);
    let _: Vec<_> = harness2
        .resume_turn(
            &chat_id,
            "here you go",
            Some(("github_token".to_string(), "ghs_secret".to_string())),
            "mock",
            "mock",
        )
        .collect()
        .await;

    // The secret is stored by name and only the name enters the transcript.
    assert_eq!(
        store.get_credential("github_token").unwrap(),
        Some("ghs_secret".to_string())
    );
    let rows = store.list_chat_messages(&chat_id).unwrap();
    let user_turn = rows
        .iter()
        .filter(|(_, role, _, _, _)| role == "user")
        .map(|(_, _, content, _, _)| content.clone())
        .find(|c| c.contains("Answer to question"))
        .unwrap_or_default();
    assert!(user_turn.contains("Credential stored as github_token"));
    assert!(user_turn.contains("{{credential:github_token}}"));
    assert!(!user_turn.contains("ghs_secret"));
}

#[tokio::test]
async fn test_auto_approve_skips_ask_user() {
    // With auto-approve on, the harness must not suspend on `ask_user`: it
    // neither emits an AskUser event nor persists a pending ask.
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = llm_with_reasoning_tools(
        "Need more info.",
        vec![LlmToolCall {
            id: "tc5".to_string(),
            name: "ask_user".to_string(),
            arguments: serde_json::json!({
                "question": "Which database should I query?",
                "quick_replies": ["postgres"]
            }),
        }],
    );
    let harness = Harness::new(store.clone())
        .with_llm(llm)
        .with_reasoning(true)
        .with_auto_approve(true);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "automate reports", "mock", "mock")
        .collect()
        .await;
    assert!(!events.iter().any(|e| matches!(e, HarnessEvent::AskUser { .. })));
    assert!(store.get_pending_ask(&chat_id).unwrap().is_none());
    // The harness records that the question was auto-approved so the next
    // reasoning/execution step sees an answer in history.
    let messages = store.list_chat_messages(&chat_id).unwrap();
    assert!(messages
        .iter()
        .any(|(_, role, content, _, _)| role == "user" && content.contains("auto-approved")));
}

/// Records each command line the harness runs, so the approval tests assert
/// exactly what a decision executed.
struct RecordingRunner {
    runs: std::sync::Mutex<Vec<String>>,
}

impl RecordingRunner {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            runs: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn lines(&self) -> Vec<String> {
        self.runs.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl crate::harness::CommandRunner for RecordingRunner {
    async fn run(&self, command: &str, args: &[String]) -> anyhow::Result<String> {
        let line = if args.is_empty() {
            command.to_string()
        } else {
            format!("{command} {}", args.join(" "))
        };
        self.runs.lock().unwrap().push(line.clone());
        Ok(format!("ran: {line}"))
    }
}

/// Drive a turn until its `run_command` call suspends on approval, returning
/// the store, chat id, recording runner and the events observed so far.
async fn turn_suspends_on_command() -> (Store, String, Arc<RecordingRunner>, Vec<HarnessEvent>)
{
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = llm_with_reasoning_tools(
        "",
        vec![LlmToolCall {
            id: "tc_cmd".to_string(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command": "echo", "args": ["hi"]}),
        }],
    );
    let runner = RecordingRunner::new();
    let harness = Harness::new(store.clone())
        .with_llm(llm)
        .with_runner(runner.clone())
        .with_workspace_dir("/tmp/workspace");
    let events: Vec<_> = harness
        .run_turn(&chat_id, "run echo hi", "mock", "mock")
        .collect()
        .await;
    (store, chat_id, runner, events)
}

/// A `run_command` call suspends before it runs: the harness emits a
/// `CommandProposed` with the candidate line and the cwd, persists the
/// proposal, and leaves the call running until a decision arrives.
#[tokio::test]
async fn test_command_suspends_before_it_runs() {
    let (store, chat_id, runner, events) = turn_suspends_on_command().await;
    let (id, candidates, cwd) = events
        .iter()
        .find_map(|e| match e {
            HarnessEvent::CommandProposed {
                id,
                candidates,
                cwd,
            } => Some((id.clone(), candidates.clone(), cwd.clone())),
            _ => None,
        })
        .expect("a command tool must suspend on a proposal");
    assert_eq!(id, "tc_cmd");
    assert_eq!(candidates, vec!["echo hi".to_string()]);
    assert_eq!(cwd, "/tmp/workspace");
    assert!(runner.lines().is_empty(), "nothing runs while suspended");
    assert!(store.get_pending_command(&chat_id).unwrap().is_some());
    assert_eq!(
        tool_call_records(&store, &chat_id)[0].status,
        ToolCallStatus::Running
    );
    assert!(!events.iter().any(|e| matches!(e, HarnessEvent::Done)));
}

/// Approving runs the chosen command text verbatim.
#[tokio::test]
async fn test_approve_runs_chosen_command() {
    let (store, chat_id, runner, _) = turn_suspends_on_command().await;
    let events: Vec<_> = Harness::new(store.clone())
        .with_runner(runner.clone())
        .resume_command(&chat_id, CommandDecision::Approve("echo hi".to_string()))
        .collect()
        .await;
    assert_eq!(runner.lines(), vec!["echo hi".to_string()]);
    assert!(events.iter().any(
        |e| matches!(e, HarnessEvent::ToolCallFinished { id, result } if id == "tc_cmd" && result == "ran: echo hi")
    ));
    assert!(
        events.iter().any(|e| matches!(e, HarnessEvent::Done)),
        "the resumed turn finishes after the approved command runs"
    );
    assert!(store.get_pending_command(&chat_id).unwrap().is_none());
    let records = tool_call_records(&store, &chat_id);
    assert_eq!(records[0].status, ToolCallStatus::Finished);
    assert_eq!(records[0].result.as_deref(), Some("ran: echo hi"));
}

/// Editing runs the edited text instead of the proposal.
#[tokio::test]
async fn test_edit_runs_edited_command() {
    let (store, chat_id, runner, _) = turn_suspends_on_command().await;
    let events: Vec<_> = Harness::new(store.clone())
        .with_runner(runner.clone())
        .resume_command(&chat_id, CommandDecision::Edit("echo edited".to_string()))
        .collect()
        .await;
    assert_eq!(runner.lines(), vec!["echo edited".to_string()]);
    assert!(events.iter().any(
        |e| matches!(e, HarnessEvent::ToolCallFinished { id, result } if id == "tc_cmd" && result == "ran: echo edited")
    ));
}

/// Rejecting never runs the command; the tool call fails instead.
#[tokio::test]
async fn test_reject_errors() {
    let (store, chat_id, runner, _) = turn_suspends_on_command().await;
    let events: Vec<_> = Harness::new(store.clone())
        .with_runner(runner.clone())
        .resume_command(&chat_id, CommandDecision::Reject("too risky".to_string()))
        .collect()
        .await;
    assert!(runner.lines().is_empty(), "a rejected command never runs");
    assert!(events.iter().any(
        |e| matches!(e, HarnessEvent::ToolCallError { id, message } if id == "tc_cmd" && message.contains("rejected by user: too risky"))
    ));
    assert!(
        events.iter().any(|e| matches!(e, HarnessEvent::Done)),
        "a rejected command still finishes the turn"
    );
    assert_eq!(
        tool_call_records(&store, &chat_id)[0].status,
        ToolCallStatus::Error
    );
    let tool_rows: Vec<String> = store
        .list_chat_messages(&chat_id)
        .unwrap()
        .into_iter()
        .filter(|(_, role, _, _, _)| role == "tool")
        .map(|(_, _, content, _, _)| content)
        .collect();
    assert!(
        tool_rows
            .iter()
            .any(|c| c.contains("rejected by user: too risky")),
        "the rejection is the result body: {tool_rows:?}"
    );
    assert!(
        tool_rows.iter().all(|c| !c.contains("ERROR:")),
        "the resumed path carries the status as data too: {tool_rows:?}"
    );
}

/// The execution phase streams assistant deltas into a single message row as
/// they arrive (so the renderer can show the reply progressively), and the
/// final content is the full concatenation.
#[tokio::test]
async fn test_execution_streams_assistant_deltas_into_one_message() {
    use std::pin::Pin;
    use futures::Stream;
    use crate::llm::CompletionResponse;

    struct SplitProvider;
    #[async_trait::async_trait]
    impl LlmProvider for SplitProvider {
        fn name(&self) -> &str {
            "split"
        }
        async fn complete(&self, _req: CompletionRequest) -> anyhow::Result<CompletionResponse> {
            Ok(CompletionResponse {
                content: "Hello world".to_string(),
                tool_calls: Vec::new(),
                usage: None,
            })
        }
        async fn complete_stream(
            &self,
            _req: CompletionRequest,
        ) -> anyhow::Result<Pin<Box<dyn Stream<Item = CompletionStreamEvent> + Send>>> {
            let events = vec![
                CompletionStreamEvent::AssistantDelta("Hello ".to_string()),
                CompletionStreamEvent::AssistantDelta("world".to_string()),
                CompletionStreamEvent::Done,
            ];
            Ok(Box::pin(futures::stream::iter(events)))
        }
    }

    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let harness = Harness::new(store.clone()).with_llm(Arc::new(SplitProvider));
    let events: Vec<_> = harness
        .run_turn(&chat_id, "hi", "mock", "mock")
        .collect()
        .await;
    assert!(events.iter().any(|e| matches!(e, HarnessEvent::AssistantDelta(d) if d == "Hello ")));
    assert!(events.iter().any(|e| matches!(e, HarnessEvent::AssistantDelta(d) if d == "world")));

    let msgs = store.list_chat_messages(&chat_id).unwrap();
    let assistant = msgs.into_iter().find(|m| m.1 == "assistant").unwrap();
    assert_eq!(assistant.2, "Hello world", "deltas should be concatenated into one assistant message");
}

/// Collect the persisted tool-call records from every row of a chat.
fn tool_call_records(store: &Store, chat_id: &str) -> Vec<ChatToolCall> {
    store
        .list_chat_messages(chat_id)
        .unwrap()
        .into_iter()
        .filter_map(|(_, _, _, tool_calls, _)| tool_calls)
        .flat_map(|json| serde_json::from_str::<Vec<ChatToolCall>>(&json).unwrap_or_default())
        .collect()
}

/// A started call is readable from the store while the turn is still running,
/// and the same row carries the outcome once it finishes.
#[tokio::test]
async fn test_tool_call_is_running_in_store_before_turn_ends() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = llm_with_reasoning_tools(
        "",
        vec![LlmToolCall {
            id: "tc_live".to_string(),
            name: "credentials".to_string(),
            arguments: serde_json::json!({}),
        }],
    );
    let harness = Harness::new(store.clone()).with_llm(llm);
    let mut events = harness.run_turn(&chat_id, "list credentials", "mock", "mock");

    // Drive the turn only up to the start event: the call must already be
    // persisted as running, well before the turn ends.
    let mut saw_started = false;
    while let Some(event) = events.next().await {
        if matches!(event, HarnessEvent::ToolCallStarted { .. }) {
            saw_started = true;
            break;
        }
    }
    assert!(saw_started, "the turn should have started a tool call");

    let records = tool_call_records(&store, &chat_id);
    assert_eq!(records.len(), 1, "the started call is persisted");
    assert_eq!(records[0].id, "tc_live");
    assert_eq!(records[0].status, ToolCallStatus::Running);
    assert_eq!(records[0].result, None, "a running call has no result yet");

    // Finish the turn; the same row now carries the terminal state.
    let rest: Vec<_> = events.collect().await;
    assert!(rest.iter().any(|e| matches!(e, HarnessEvent::Done)));

    let records = tool_call_records(&store, &chat_id);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, ToolCallStatus::Finished);
    assert!(records[0].result.is_some(), "a finished call keeps its result");
}

/// A call that fails is persisted with the error state and the message.
#[tokio::test]
async fn test_failed_tool_call_is_persisted_as_error() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = llm_with_reasoning_tools(
        "",
        vec![LlmToolCall {
            id: "tc_bad".to_string(),
            name: "no_such_tool".to_string(),
            arguments: serde_json::json!({}),
        }],
    );
    let harness = Harness::new(store.clone()).with_llm(llm);
    let events: Vec<_> = harness
        .run_turn(&chat_id, "do the impossible", "mock", "mock")
        .collect()
        .await;
    assert!(events
        .iter()
        .any(|e| matches!(e, HarnessEvent::ToolCallError { .. })));

    let records = tool_call_records(&store, &chat_id);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, ToolCallStatus::Error);
    assert!(records[0]
        .result
        .as_deref()
        .unwrap_or_default()
        .contains("unknown tool"));
}

/// A failed result is a `role="tool"` row of `<call_id>\n<message>`, not a
/// status encoded as an `ERROR: ` prefix; the outcome is the call's
/// persisted `status`, which the renderer reads.
#[tokio::test]
async fn test_failed_tool_result_row_does_not_encode_status_as_text() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = llm_with_reasoning_tools(
        "",
        vec![LlmToolCall {
            id: "tc_bad".to_string(),
            name: "no_such_tool".to_string(),
            arguments: serde_json::json!({}),
        }],
    );
    let harness = Harness::new(store.clone()).with_llm(llm);
    let _: Vec<_> = harness
        .run_turn(&chat_id, "do the impossible", "mock", "mock")
        .collect()
        .await;

    let tool_rows: Vec<String> = store
        .list_chat_messages(&chat_id)
        .unwrap()
        .into_iter()
        .filter(|(_, role, _, _, _)| role == "tool")
        .map(|(_, _, content, _, _)| content)
        .collect();
    assert_eq!(tool_rows.len(), 1, "one result row per failed call");
    assert!(
        tool_rows[0].starts_with("tc_bad\n"),
        "the result row names the call it belongs to"
    );
    assert!(
        !tool_rows[0].contains("ERROR:"),
        "the status must not travel as a text prefix: {tool_rows:?}"
    );
    assert_eq!(
        tool_call_records(&store, &chat_id)[0].status,
        ToolCallStatus::Error,
        "the outcome is on the call record the renderer reads"
    );
}

/// The persisted row stays `<call_id>\n<body>`, but the message the model
/// reads carries the outcome: a failed call is a `Role::Tool` message whose
/// `tool_status` is `Error`, so the model does not read a failure as an
/// ordinary result.
#[tokio::test]
async fn test_failed_tool_call_is_failed_in_the_llm_history() {
    let store = Store::open_in_memory().unwrap();
    let chat_id = chat(&store);
    let llm = llm_with_reasoning_tools(
        "",
        vec![LlmToolCall {
            id: "tc_bad".to_string(),
            name: "no_such_tool".to_string(),
            arguments: serde_json::json!({}),
        }],
    );
    let harness = Harness::new(store.clone()).with_llm(llm);
    let _: Vec<_> = harness
        .run_turn(&chat_id, "do the impossible", "mock", "mock")
        .collect()
        .await;

    let mission = MissionState {
        id: "mission-1".to_string(),
        chat_id: chat_id.clone(),
        goal: "do the impossible".to_string(),
        status: "running".to_string(),
        plan: None,
        workflow_id: None,
        reasoning_steps: Vec::new(),
        pending_ask: None,
    };
    let history =
        build_history(&store, &chat_id, &mission, &[], &[], ThinkingMode::Direct).await;
    let tool_message = history
        .iter()
        .find(|m| m.role == Role::Tool)
        .expect("a failed call leaves a tool message in the history");
    assert_eq!(tool_message.tool_call_id.as_deref(), Some("tc_bad"));
    assert_eq!(
        tool_message.tool_status,
        Some(ToolCallStatus::Error),
        "the model must be able to read the call as failed"
    );
    assert!(
        tool_message.content.contains("unknown tool"),
        "the result body is still what the model reads: {:?}",
        tool_message.content
    );
    assert!(
        !tool_message.content.starts_with("tc_bad"),
        "the call id belongs on tool_call_id, not in the content"
    );
}

/// A `tool_calls` row written by an older build carries only
/// `id`/`name`/`arguments`; the new fields must default rather than fail.
#[test]
fn test_tool_call_row_from_older_build_still_parses() {
    let old = r#"[{"id":"call_1","name":"ls","arguments":{"path":"/tmp"}}]"#;
    let records: Vec<ChatToolCall> = serde_json::from_str(old).expect("old row must parse");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].name, "ls");
    assert_eq!(records[0].status, ToolCallStatus::Pending);
    assert_eq!(records[0].result, None);
}
