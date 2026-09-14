use std::collections::HashMap;

use crate::harness::{ChatToolCall, ThinkingMode, ToolCallStatus};
use crate::llm::{Message, Role, ToolDefinition};
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
    match store.list_chat_messages(chat_id) {
        Ok(rows) => {
            // A tool row is `<call_id>\n<body>` with no status in its text; the
            // outcome lives on the assistant row's call records. Resolve it here
            // so the model reads a failed call as failed, not as a plain result.
            let statuses: HashMap<String, ToolCallStatus> = rows
                .iter()
                .filter_map(|(_, _, _, tool_calls, _)| tool_calls.as_deref())
                .flat_map(|json| {
                    serde_json::from_str::<Vec<ChatToolCall>>(json).unwrap_or_default()
                })
                .map(|record| (record.id, record.status))
                .collect();
            rows.into_iter()
                .map(|(_, role, content, tool_calls, _)| {
                    let (content, tool_call_id, tool_status) = if role.as_str() == "tool" {
                        if let Some((id, rest)) = content.split_once('\n') {
                            (
                                rest.to_string(),
                                Some(id.to_string()),
                                statuses.get(id).copied(),
                            )
                        } else {
                            (content, None, None)
                        }
                    } else {
                        (content, None, None)
                    };
                    Message {
                        role: match role.as_str() {
                            "system" => Role::System,
                            "assistant" => Role::Assistant,
                            "tool_calls" => Role::Assistant,
                            "tool" => Role::Tool,
                            _ => Role::User,
                        },
                        content,
                        tool_calls: tool_calls.and_then(|t| serde_json::from_str(&t).ok()),
                        tool_call_id,
                        tool_status,
                    }
                })
                .collect::<Vec<_>>()
        }
        Err(_) => Vec::new(),
    }
}
