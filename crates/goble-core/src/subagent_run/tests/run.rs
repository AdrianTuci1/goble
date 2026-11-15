use std::sync::Arc;

use crate::harness::{ChatToolCall, ToolCallStatus, SPAWN_SUBAGENT_TOOL};
use crate::llm::{
    CompletionRequest, CompletionResponse, CompletionStreamEvent, LlmProvider, Message,
};
use crate::store::Store;
use crate::subagent::{SubAgentBudget, SubAgentStatus};
use futures::StreamExt;
use std::pin::Pin;

use super::{
    background_args, child_of, completion, harness_on, host_for, parent_chat, roles, run_direct,
    spawn_args, spawn_through_tool, spec_for, tool_call, wait_until_terminal, PanickingProvider,
    ScriptedProvider,
};

#[tokio::test]
async fn a_spawned_child_gets_its_own_chat_and_returns_its_final_text() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let parent_chat = parent_chat(&store);
    let llm = ScriptedProvider::scripted(
        vec![
            // The parent's turn asks for a child.
            completion(
                "",
                vec![tool_call(
                    "tc1",
                    SPAWN_SUBAGENT_TOOL,
                    spawn_args("review it"),
                )],
            ),
            // The child's one and only turn: a final answer, no tools.
            completion("child final answer", vec![]),
        ],
        completion("", vec![]),
    );
    let harness = harness_on(store.clone(), workspace.path(), llm);

    let events: Vec<_> = harness
        .run_turn(&parent_chat, "spawn a reviewer", "mock", "model")
        .collect()
        .await;

    // The child's final text came back as the parent's tool result.
    assert!(
        events.iter().any(|e| matches!(
            e,
            crate::harness::HarnessEvent::ToolCallFinished { result, .. }
                if result == "child final answer"
        )),
        "{events:?}"
    );

    // The child owns a chats row carrying the parent id...
    let child_chat = child_of(&store, &parent_chat);
    assert_eq!(
        store.get_chat_parent(&child_chat).unwrap().as_deref(),
        Some(parent_chat.as_str())
    );
    assert_eq!(
        store.get_chat_parent(&parent_chat).unwrap(),
        None,
        "the parent's own row has no parent"
    );
    // ...and wrote its messages under its own id: user prompt, then reply.
    let child_rows = roles(&store, &child_chat);
    assert_eq!(child_rows.len(), 2);
    assert_eq!(child_rows[0], ("user".to_string(), "review it".to_string()));
    assert_eq!(
        child_rows[1],
        ("assistant".to_string(), "child final answer".to_string())
    );
    // The parent's conversation carries no child rows of its own: its only
    // trace of the child is the tool call and its result.
    let parent_rows = store.list_chat_messages(&parent_chat).unwrap();
    assert!(
        !parent_rows
            .iter()
            .any(|(_, role, content, _, _)| role == "user" && content == "review it"),
        "the child's prompt row must not land in the parent's transcript: {parent_rows:?}"
    );
    // The child got its per-id subdir under the workspace root, created by
    // the spawner.
    assert!(workspace
        .path()
        .join(crate::subagent::SUBAGENTS_DIR)
        .join(&child_chat)
        .is_dir());
}

#[tokio::test]
async fn a_child_dispatches_its_tool_calls_under_its_own_cwd_and_completes() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let cwd = workspace.path().join("child-files");
    let llm = ScriptedProvider::scripted(
        vec![
            completion(
                "writing it now",
                vec![tool_call(
                    "call-write",
                    "write_file",
                    serde_json::json!({"path": "notes.txt", "content": "hi from child"}),
                )],
            ),
            completion("done: notes.txt written", vec![]),
        ],
        completion("", vec![]),
    );
    let spec = spec_for("child-files", &cwd, SubAgentBudget::new(10, 10, 100_000));
    let record = run_direct(&store, workspace.path(), llm, spec).await;

    match &record.status {
        SubAgentStatus::Completed {
            output,
            turns,
            tool_calls,
            ..
        } => {
            assert_eq!(output, "done: notes.txt written");
            assert_eq!((*turns, *tool_calls), (2, 1));
        }
        other => panic!("expected Completed, got {other:?}"),
    }
    // The child's tool worked in its own cwd, not the workspace root.
    assert_eq!(
        std::fs::read_to_string(cwd.join("notes.txt")).unwrap(),
        "hi from child"
    );
    assert!(!workspace.path().join("notes.txt").exists());
    let rows = roles(&store, "child-files");
    let tool_row = rows
        .iter()
        .find(|(role, _)| role == "tool")
        .expect("a persisted tool row")
        .1
        .clone();
    assert!(
        tool_row.starts_with("call-write\n"),
        "tool rows keep the <call_id>\\n<body> shape: {tool_row}"
    );
    assert!(tool_row.contains("wrote"));
    // The assistant row carries the call record with its finished status.
    let calls: Vec<ChatToolCall> = store
        .list_chat_messages("child-files")
        .unwrap()
        .into_iter()
        .find(|(_, role, ..)| role == "assistant")
        .and_then(|(_, _, _, tool_calls, _)| tool_calls)
        .map(|json| serde_json::from_str(&json).unwrap())
        .unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].status, ToolCallStatus::Finished);
}

#[tokio::test]
async fn a_panicking_child_becomes_failed_and_never_hangs() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let parent_chat = parent_chat(&store);
    let host = host_for(&store, workspace.path(), Arc::new(PanickingProvider));
    spawn_through_tool(
        &host,
        &store,
        workspace.path(),
        &parent_chat,
        background_args("this will unwind"),
    )
    .await
    .expect("a background child");

    let id = host.registry.records()[0].spec.id.clone();
    let done = wait_until_terminal(&host, &id).await;
    match &done.status {
        SubAgentStatus::Failed { error } => {
            assert!(error.contains("panicked"), "{error}");
            assert!(error.contains("child exploded"), "{error}");
        }
        other => panic!("a panicking child must be Failed, got {other:?}"),
    }
    assert!(done.finished_at.is_some());
}

/// The endpoint's body cap as a child meets it: the summariser's call is
/// answered with a summary, the child's third model call is refused for the
/// size of its conversation, and the call after the retry answers. Every
/// request is kept so a test can read what the retry carried.
struct BodyCapOnce {
    calls: std::sync::atomic::AtomicUsize,
    seen: std::sync::Mutex<Vec<Vec<Message>>>,
}

impl BodyCapOnce {
    fn new() -> Self {
        Self {
            calls: std::sync::atomic::AtomicUsize::new(0),
            seen: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn model_calls(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl LlmProvider for BodyCapOnce {
    fn name(&self) -> &str {
        "body-cap"
    }

    async fn complete(&self, request: CompletionRequest) -> anyhow::Result<CompletionResponse> {
        // The compaction prompt is the one call whose text is a transcript.
        if request
            .messages
            .iter()
            .any(|message| message.content.starts_with("Conversation so far:"))
        {
            return Ok(CompletionResponse::new(
                r#"{"session_id":"c","summary":"The earlier turns.","facts":[],"decisions":[],"goals_completed":[],"new_goals":[],"milestones_completed":[],"next_steps":[],"message_count":2}"#,
                Vec::new(),
            ));
        }
        self.seen.lock().unwrap().push(request.messages.clone());
        match self
            .calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        {
            0 => Ok(completion(
                "",
                vec![tool_call("tc1", "credentials", serde_json::json!({}))],
            )),
            1 => Ok(completion(
                "",
                vec![tool_call("tc2", "credentials", serde_json::json!({}))],
            )),
            2 => anyhow::bail!(
                "LLM request failed: 413 Request Entity Too Large <html><center><h1>413 Request Entity Too Large</h1></center><hr><center>openresty</center></html>"
            ),
            _ => Ok(completion("done: read both", vec![])),
        }
    }

    async fn complete_stream(
        &self,
        _request: CompletionRequest,
    ) -> anyhow::Result<Pin<Box<dyn futures::Stream<Item = CompletionStreamEvent> + Send>>> {
        Err(anyhow::anyhow!("this stub only answers a child's call"))
    }
}

/// A child has no compaction of its own until it is refused: its conversation
/// is compacted through the same path a parent turn uses, rebuilt from its own
/// stored rows, and the refused call retried exactly once.
#[tokio::test]
async fn a_child_refused_for_its_body_size_compacts_and_retries_once() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let cwd = workspace.path().join("child-cap");
    let spec = spec_for("child-cap", &cwd, SubAgentBudget::new(10, 10, 100_000));
    let llm = Arc::new(BodyCapOnce::new());
    let record = run_direct(&store, workspace.path(), llm.clone(), spec).await;

    match &record.status {
        SubAgentStatus::Completed { output, .. } => assert_eq!(output, "done: read both"),
        other => panic!("the retried child completes, got {other:?}"),
    }
    assert_eq!(
        llm.model_calls(),
        4,
        "the refused call is retried exactly once"
    );

    // The retry's history came back from the child's own store: the summary
    // and the last exchange, not the turns the summary replaced.
    let requests = llm.seen.lock().unwrap().clone();
    let retry = requests.last().expect("the retry is recorded");
    assert!(
        retry[0].content.contains("You are a sub-agent"),
        "the child's system prompt stays: {:?}",
        retry[0].content
    );
    assert!(
        retry[1].content.contains("Summary of the earlier turns"),
        "the retry reads the compaction's summary: {:?}",
        retry[1].content
    );
    assert!(
        !retry
            .iter()
            .any(|message| message.tool_call_id.as_deref() == Some("tc1")),
        "the summarised exchange left the request: {retry:?}"
    );

    // The child's own transcript says what happened.
    let rows = roles(&store, "child-cap");
    assert!(
        rows.iter()
            .any(|(role, content)| role == "notice" && content.contains("Context compacted:")),
        "the child's transcript carries the compaction notice: {rows:?}"
    );
}
