use goble_daemon_protocol::DaemonEvent;
use goble_harness_protocol::HarnessServerEvent;

/// Map a harness [`HarnessServerEvent`] into the daemon [`DaemonEvent`] shape.
///
/// The host-discovery frames (`Ready`, `ToolList`) are not part of the daemon
/// stream and are dropped.
pub(super) fn map_event(ev: HarnessServerEvent) -> Option<DaemonEvent> {
    match ev {
        HarnessServerEvent::Ready
        | HarnessServerEvent::ToolList { .. }
        | HarnessServerEvent::Checkpoint { .. } => None,
        HarnessServerEvent::AssistantDelta { session_id, delta } => {
            Some(DaemonEvent::AssistantDelta { session_id, delta })
        }
        HarnessServerEvent::ToolCallStarted {
            session_id,
            id,
            name,
            arguments,
        } => Some(DaemonEvent::ToolCallStarted {
            session_id,
            id,
            name,
            arguments,
        }),
        HarnessServerEvent::ToolCallFinished { session_id, id, result } => {
            Some(DaemonEvent::ToolCallFinished {
                session_id,
                id,
                result,
            })
        }
        HarnessServerEvent::ToolCallError {
            session_id,
            id,
            message,
        } => Some(DaemonEvent::ToolCallError {
            session_id,
            id,
            message,
        }),
        HarnessServerEvent::AskUser {
            session_id,
            question,
            quick_replies,
        } => Some(DaemonEvent::AskUser {
            session_id,
            question,
            quick_replies,
        }),
        HarnessServerEvent::CommandProposed {
            session_id,
            id,
            candidates,
            cwd,
        } => Some(DaemonEvent::CommandProposed {
            session_id,
            id,
            candidates,
            cwd,
        }),
        HarnessServerEvent::SubAgentSpawned {
            session_id,
            chat_id,
            subagent_id,
            subagent_type,
            description,
            parent_call_id,
            run_in_background,
        } => Some(DaemonEvent::SubAgentSpawned {
            session_id,
            chat_id,
            subagent_id,
            subagent_type,
            description,
            parent_call_id,
            run_in_background,
        }),
        HarnessServerEvent::SubAgentProgress {
            session_id,
            chat_id,
            subagent_id,
            status,
            activity,
            turns,
            tool_calls,
            tokens,
            duration_ms,
        } => Some(DaemonEvent::SubAgentProgress {
            session_id,
            chat_id,
            subagent_id,
            status,
            activity,
            turns,
            tool_calls,
            tokens,
            duration_ms,
        }),
        HarnessServerEvent::SubAgentFinished {
            session_id,
            chat_id,
            subagent_id,
            status,
            output,
            error,
            duration_ms,
            turns,
            tool_calls,
            tokens,
        } => Some(DaemonEvent::SubAgentFinished {
            session_id,
            chat_id,
            subagent_id,
            status,
            output,
            error,
            duration_ms,
            turns,
            tool_calls,
            tokens,
        }),
        HarnessServerEvent::MissionUpdated {
            session_id,
            mission_id,
            status,
        } => Some(DaemonEvent::MissionUpdated {
            session_id,
            mission_id,
            status,
        }),
        HarnessServerEvent::TokenUsage {
            session_id,
            chat_id,
            input,
            cached,
            output,
        } => Some(DaemonEvent::TokenUsage {
            session_id,
            chat_id,
            input,
            cached,
            output,
        }),
        HarnessServerEvent::ReasoningStarted {
            session_id,
            step,
            mode,
        } => Some(DaemonEvent::ReasoningStarted {
            session_id,
            step,
            mode,
        }),
        HarnessServerEvent::ReasoningDelta { session_id, delta } => {
            Some(DaemonEvent::ReasoningDelta { session_id, delta })
        }
        HarnessServerEvent::ReasoningDone {
            session_id,
            step,
            mode,
            content,
            decision,
        } => Some(DaemonEvent::ReasoningDone {
            session_id,
            step,
            mode,
            content,
            decision,
        }),
        HarnessServerEvent::Done { session_id } => Some(DaemonEvent::Done { session_id }),
        HarnessServerEvent::Error { session_id, message } => {
            Some(DaemonEvent::Error { session_id, message })
        }
        HarnessServerEvent::ScreenHandoff { session_id, config } => {
            Some(DaemonEvent::ScreenHandoff { session_id, config })
        }
    }
}
