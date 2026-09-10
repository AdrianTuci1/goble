use anyhow::Result;
use goble_core::agent_memory::{AgentMemory, ContextBuilder};
use goble_core::store::Store;

/// Build the system prompt for a harness turn: identity + memory + transcript tail.
pub fn build_context(spec_prompt: &str, memory: &AgentMemory, tail: &str) -> String {
    ContextBuilder::build(spec_prompt, memory, tail)
}

/// Render the most recent messages of a chat as plain text (sliding window).
pub fn transcript_tail(store: &Store, chat_id: &str, max_messages: usize) -> Result<String> {
    let rows = store.list_chat_messages(chat_id)?;
    let tail_rows: Vec<_> = rows.into_iter().rev().take(max_messages).collect();
    let mut out = String::new();
    for (_, role, content, _, _) in tail_rows.into_iter().rev() {
        out.push_str(&format!("{role}: {content}\n"));
    }
    Ok(out)
}

/// True when a chat's message count exceeds the compaction threshold.
pub fn should_compact(store: &Store, chat_id: &str, threshold: usize) -> Result<bool> {
    Ok(store.list_chat_messages(chat_id)?.len() > threshold)
}
