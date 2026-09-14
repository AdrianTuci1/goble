use crate::llm::LlmToolCall;

use super::types::PendingAsk;

pub(super) fn extract_ask_user(tool_calls: &[LlmToolCall]) -> Option<PendingAsk> {
    for call in tool_calls {
        if call.name == "ask_user" {
            let question = call.arguments["question"].as_str()?.to_string();
            let quick_replies: Vec<String> = call.arguments["quick_replies"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            return Some(PendingAsk {
                id: uuid::Uuid::new_v4().to_string(),
                question,
                quick_replies,
            });
        }
    }
    None
}

pub(super) fn is_orchestration_goal(prompt: &str) -> bool {
    let prompt_lower = prompt.to_lowercase();
    let triggers = [
        "agent",
        "workflow",
        "mcp",
        "deploy",
        "schedule",
        "cron",
        "worker",
        "orchestr",
        "automate",
        "build a",
        "create a",
        "mission",
        "plan",
        "multiple steps",
        "complex",
    ];
    triggers.iter().any(|t| prompt_lower.contains(t))
}
