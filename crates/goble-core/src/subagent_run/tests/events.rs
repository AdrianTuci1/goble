use crate::harness::{HarnessEvent, SPAWN_SUBAGENT_TOOL};
use crate::store::Store;
use crate::llm::TokenUsage;
use futures::StreamExt;

use super::{
    child_of, completion, completion_reporting, harness_on, parent_chat, spawn_args, tool_call,
    ScriptedProvider,
};

/// S4: the registry's transitions for a child — register, each publish,
/// the terminal write — surface on the turn's own event stream as
/// `SubAgentSpawned` / `SubAgentProgress` / `SubAgentFinished`, with the
/// record's fields intact.
#[tokio::test]
async fn the_child_lifecycle_crosses_the_turn_stream() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let parent_chat = parent_chat(&store);
    let llm = ScriptedProvider::scripted(
        vec![
            completion(
                "",
                vec![tool_call(
                    "tc1",
                    SPAWN_SUBAGENT_TOOL,
                    spawn_args("review it"),
                )],
            ),
            completion("child final answer", vec![]),
        ],
        completion("", vec![]),
    );
    let harness = harness_on(store.clone(), workspace.path(), llm);

    let events: Vec<_> = harness
        .run_turn(&parent_chat, "spawn a reviewer", "mock", "model")
        .collect()
        .await;

    let spawned: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, HarnessEvent::SubAgentSpawned { .. }))
        .collect();
    assert_eq!(spawned.len(), 1, "one spawn per child: {events:?}");
    let HarnessEvent::SubAgentSpawned {
        chat_id,
        subagent_id,
        subagent_type,
        description,
        parent_call_id,
        run_in_background,
    } = &spawned[0]
    else {
        unreachable!("filtered for the spawn event");
    };
    assert_eq!(chat_id, &parent_chat);
    assert_eq!(subagent_id, &child_of(&store, &parent_chat));
    assert_eq!(subagent_type, "reviewer");
    assert_eq!(description, "review the diff");
    assert_eq!(parent_call_id, "tc1");
    assert!(!run_in_background, "spawn_args awaits its child");

    let progress = events
        .iter()
        .filter(|e| matches!(e, HarnessEvent::SubAgentProgress { .. }))
        .count();
    assert!(progress >= 1, "a live child publishes progress: {events:?}");
    assert!(
        events.iter().any(|e| matches!(
            e,
            HarnessEvent::SubAgentProgress { status, turns: 1, .. }
                if status == "running"
        )),
        "the child's first charged turn publishes a Running record: {events:?}"
    );

    let finished: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, HarnessEvent::SubAgentFinished { .. }))
        .collect();
    assert_eq!(finished.len(), 1, "one terminal transition per child");
    let HarnessEvent::SubAgentFinished {
        chat_id,
        status,
        output,
        error,
        turns,
        ..
    } = &finished[0]
    else {
        unreachable!("filtered for the finish event");
    };
    assert_eq!(chat_id, &parent_chat);
    assert_eq!(status, "completed");
    assert_eq!(output.as_deref(), Some("child final answer"));
    assert_eq!(error, &None, "a completion carries no error");
    assert_eq!(*turns, 1, "the record's final turn count");

    // Spawned precedes progress precedes finished on the stream, and the
    // whole lifecycle crossed before the turn's own tool result.
    let index_of = |want: fn(&HarnessEvent) -> bool| {
        events
            .iter()
            .position(want)
            .expect("the event is on the stream")
    };
    assert!(
        index_of(|e| matches!(e, HarnessEvent::SubAgentSpawned { .. }))
            < index_of(|e| matches!(e, HarnessEvent::SubAgentProgress { .. }))
            && index_of(|e| matches!(e, HarnessEvent::SubAgentProgress { .. }))
                < index_of(|e| matches!(e, HarnessEvent::SubAgentFinished { .. })),
        "the lifecycle arrives in order: {events:?}"
    );
}

/// A child's model calls are the parent conversation's spend: each reported
/// call crosses the turn stream under the parent's chat id, buckets intact,
/// and the child's own budget is charged the provider's count rather than an
/// estimate of it.
#[tokio::test]
async fn a_childs_reported_usage_lands_on_the_parent_conversation() {
    let store = Store::open_in_memory().unwrap();
    let workspace = tempfile::TempDir::new().unwrap();
    let parent_chat = parent_chat(&store);
    let llm = ScriptedProvider::scripted(
        vec![
            completion(
                "",
                vec![tool_call(
                    "tc1",
                    SPAWN_SUBAGENT_TOOL,
                    spawn_args("review it"),
                )],
            ),
            completion_reporting(
                "child final answer",
                vec![],
                TokenUsage {
                    input: 900,
                    cached: Some(700),
                    output: 40,
                },
            ),
        ],
        completion("", vec![]),
    );
    let harness = harness_on(store.clone(), workspace.path(), llm);

    let events: Vec<_> = harness
        .run_turn(&parent_chat, "spawn a reviewer", "mock", "model")
        .collect()
        .await;

    let reported: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            HarnessEvent::TokenUsage {
                chat_id,
                input,
                cached,
                output,
            } => Some((chat_id.as_str(), *input, *cached, *output)),
            _ => None,
        })
        .collect();
    assert_eq!(
        reported,
        vec![(parent_chat.as_str(), 900, Some(700), 40)],
        "the child's call is the parent conversation's spend: {events:?}"
    );

    let HarnessEvent::SubAgentFinished { tokens, .. } = events
        .iter()
        .find(|e| matches!(e, HarnessEvent::SubAgentFinished { .. }))
        .expect("the child finished")
    else {
        unreachable!("filtered for the finish event");
    };
    assert_eq!(
        *tokens, 40,
        "the budget counts the provider's generated tokens, not the estimate"
    );
}
