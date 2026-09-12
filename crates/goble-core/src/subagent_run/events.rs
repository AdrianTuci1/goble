use std::pin::Pin;

use futures::{Stream, StreamExt};

use crate::harness::HarnessEvent;
use crate::llm::TokenUsage;
use crate::subagent::{SubAgentRecord, SubAgentStatus};

use super::registry::SubAgentRegistry;

/// Merge the registry's sub-agent lifecycle events into a turn's own event
/// stream (S4). The child runs on its own task and transitions the registry
/// while the turn's generator is suspended inside the spawn tool — events the
/// generator could never `yield` itself. The stream ends with the turn: the
/// `Done`/`Error` that settles it stops the merge, after any lifecycle events
/// already in flight are drained. One turn at a time is attached (a later
/// `attach_event_sink` retires this one's sender); a child that outlives its
/// turn keeps updating its record in the registry, which stays the source.
pub(crate) fn with_subagent_lifecycle(
    registry: &SubAgentRegistry,
    turn: Pin<Box<dyn Stream<Item = HarnessEvent> + Send>>,
) -> Pin<Box<dyn Stream<Item = HarnessEvent> + Send>> {
    let mut lifecycle = registry.attach_event_sink();
    Box::pin(async_stream::stream! {
        futures::pin_mut!(turn);
        let mut lifecycle_live = true;
        loop {
            tokio::select! {
                biased;
                event = lifecycle.recv(), if lifecycle_live => {
                    match event {
                        Some(event) => yield event,
                        // The sink was retired by a newer turn; nothing left
                        // to merge from this end.
                        None => lifecycle_live = false,
                    }
                }
                event = turn.next() => {
                    match event {
                        Some(event) => {
                            let ends_turn = matches!(event, HarnessEvent::Done | HarnessEvent::Error(_));
                            yield event;
                            if ends_turn {
                                break;
                            }
                        }
                        // The turn returned without a terminal event (a
                        // suspension ends the stream the same way `Done` does).
                        None => break,
                    }
                }
            }
        }
        // A child's terminal transition can land while the turn is settling;
        // it belongs on the wire before the stream closes.
        while let Ok(event) = lifecycle.try_recv() {
            yield event;
        }
    })
}

/// The status name S4's wire payloads carry — the same snake_case kind as S1's
/// serde tag on [`SubAgentStatus`], as plain data for the protocol hops.
pub(super) fn status_kind(status: &SubAgentStatus) -> &'static str {
    match status {
        SubAgentStatus::Initializing => "initializing",
        SubAgentStatus::Running { .. } => "running",
        SubAgentStatus::Completed { .. } => "completed",
        SubAgentStatus::Failed { .. } => "failed",
        SubAgentStatus::Cancelled { .. } => "cancelled",
    }
}

/// The `SubAgentSpawned` payload: the child's [`SubAgentSpec`] identity, with
/// `chat_id` the parent conversation the transcript row lives in.
pub(super) fn subagent_spawned(record: &SubAgentRecord) -> HarnessEvent {
    HarnessEvent::SubAgentSpawned {
        chat_id: record.spec.parent_chat_id.clone(),
        subagent_id: record.spec.id.0.clone(),
        subagent_type: record.spec.subagent_type.clone(),
        description: record.spec.description.clone(),
        parent_call_id: record.spec.parent_call_id.clone(),
        run_in_background: record.spec.run_in_background,
    }
}

/// The `TokenUsage` payload for one of a child's model calls. It rides the
/// parent conversation's id, because that is the conversation the spend belongs
/// to: the child has no transcript of its own on the wire, only its record.
pub(super) fn subagent_token_usage(chat_id: &str, usage: &TokenUsage) -> HarnessEvent {
    HarnessEvent::TokenUsage {
        chat_id: chat_id.to_string(),
        input: usage.input,
        cached: usage.cached,
        output: usage.output,
    }
}

/// The `SubAgentProgress` payload: the live child's counters, activity line
/// and elapsed time, all read from the record itself.
pub(super) fn subagent_progress(record: &SubAgentRecord) -> HarnessEvent {
    let (turns, tool_calls, tokens) = record.status.counts();
    let activity = match &record.status {
        SubAgentStatus::Running { activity, .. } => activity.clone(),
        _ => String::new(),
    };
    HarnessEvent::SubAgentProgress {
        chat_id: record.spec.parent_chat_id.clone(),
        subagent_id: record.spec.id.0.clone(),
        status: status_kind(&record.status).to_string(),
        activity,
        turns,
        tool_calls,
        tokens,
        duration_ms: record.elapsed_ms(),
    }
}

/// The `SubAgentFinished` payload: the terminal outcome — `output` for
/// `completed`, `error` for `failed`/`cancelled` — with the final counters.
pub(super) fn subagent_finished(record: &SubAgentRecord) -> HarnessEvent {
    let (turns, tool_calls, tokens) = record.status.counts();
    let (output, error) = match &record.status {
        SubAgentStatus::Completed { output, .. } => (Some(output.clone()), None),
        SubAgentStatus::Failed { error } => (None, Some(error.clone())),
        SubAgentStatus::Cancelled { reason } => (None, Some(reason.clone())),
        other => unreachable!("only a terminal status finishes: {other:?}"),
    };
    HarnessEvent::SubAgentFinished {
        chat_id: record.spec.parent_chat_id.clone(),
        subagent_id: record.spec.id.0.clone(),
        status: status_kind(&record.status).to_string(),
        output,
        error,
        duration_ms: record.duration_ms().unwrap_or_default(),
        turns,
        tool_calls,
        tokens,
    }
}
