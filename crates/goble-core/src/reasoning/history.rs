use std::collections::{HashMap, HashSet};

use crate::harness::{ChatToolCall, ThinkingMode, ToolCallStatus};
use crate::llm::{LlmToolCall, Message, Role, ToolDefinition};
use crate::store::Store;

use super::types::MissionState;

pub(super) async fn build_history(
    store: &Store,
    chat_id: &str,
    _mission: &MissionState,
    _reasoning_tools: &[ToolDefinition],
    _execution_tools: &[ToolDefinition],
    _mode: ThinkingMode,
) -> Vec<Message> {
    let Ok(rows) = store.list_chat_messages(chat_id) else {
        return Vec::new();
    };
    // The chain of thought each assistant turn streamed. A thinking model
    // (DeepSeek) rejects the next request when an assistant message is replayed
    // without it, so the history carries it back exactly as it arrived.
    let reasoning = store.list_chat_message_reasoning(chat_id).unwrap_or_default();
    // A tool row is `<call_id>\n<body>` with no status in its text; the
    // outcome lives on the assistant row's call records. Resolve it here
    // so the model reads a failed call as failed, not as a plain result.
    let statuses: HashMap<String, ToolCallStatus> = rows
        .iter()
        .filter_map(|(_, _, _, tool_calls, _)| tool_calls.as_deref())
        .flat_map(|json| serde_json::from_str::<Vec<ChatToolCall>>(json).unwrap_or_default())
        .map(|record| (record.id, record.status))
        .collect();
    // The ids that have a tool result row. A suspended batch leaves every call
    // after the approved one unrun, so an assistant message can carry
    // `tool_calls` that no tool message answers; the provider rejects such a
    // history outright, which would brick the conversation. Keep only the
    // calls that were answered, and only the result rows that belong to a kept
    // call: a call without a result is the model's to plan again.
    let answered: HashSet<String> = rows
        .iter()
        .filter(|(_, role, _, _, _)| role == "tool")
        .filter_map(|(_, _, content, _, _)| {
            content.split_once('\n').map(|(id, _)| id.to_string())
        })
        .collect();
    let paired: HashSet<String> = rows
        .iter()
        .filter(|(_, role, _, _, _)| role == "assistant" || role == "tool_calls")
        .filter_map(|(_, _, _, tool_calls, _)| tool_calls.as_deref())
        .flat_map(|json| serde_json::from_str::<Vec<ChatToolCall>>(json).unwrap_or_default())
        .filter(|record| answered.contains(&record.id))
        .map(|record| record.id)
        .collect();

    rows.into_iter()
        .filter_map(|(id, role, content, tool_calls, _)| {
            let is_tool = role.as_str() == "tool";
            let (content, tool_call_id, tool_status) = if is_tool {
                match content.split_once('\n') {
                    Some((id, rest)) if paired.contains(id) => (
                        rest.to_string(),
                        Some(id.to_string()),
                        statuses.get(id).copied(),
                    ),
                    // An orphan result, or a row that carries no call id, has
                    // no `tool_call_id` the provider would accept.
                    _ => return None,
                }
            } else {
                (content, None, None)
            };
            let mut calls = if role.as_str() == "assistant" || role.as_str() == "tool_calls" {
                tool_calls
                    .and_then(|json| serde_json::from_str::<Vec<LlmToolCall>>(&json).ok())
                    .map(|calls| {
                        calls
                            .into_iter()
                            .filter(|call| paired.contains(&call.id))
                            .collect::<Vec<_>>()
                    })
            } else {
                None
            };
            if calls.as_ref().is_some_and(|calls| calls.is_empty()) {
                // Nothing calls this message anymore: an empty tool-call-only
                // message is not worth a request slot.
                if content.is_empty() {
                    return None;
                }
                calls = None;
            }
            Some(Message {
                role: match role.as_str() {
                    "system" => Role::System,
                    "assistant" => Role::Assistant,
                    "tool_calls" => Role::Assistant,
                    "tool" => Role::Tool,
                    _ => Role::User,
                },
                content,
                tool_calls: calls,
                tool_call_id,
                tool_status,
                reasoning_content: reasoning.get(&id).cloned(),
            })
        })
        .collect()
}
