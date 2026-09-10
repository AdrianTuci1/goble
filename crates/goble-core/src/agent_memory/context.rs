use super::memory::AgentMemory;

/// Composes the system prompt for a harness turn from identity, persistent
/// memory and the recent transcript tail.
pub struct ContextBuilder;

impl ContextBuilder {
    /// Identity (spec prompt) + canonical memory block + recent transcript tail.
    /// Memory is always injected in full; only the transcript is a sliding window.
    pub fn build(spec_prompt: &str, memory: &AgentMemory, transcript_tail: &str) -> String {
        let mut out = String::new();
        out.push_str(spec_prompt.trim());
        out.push('\n');
        out.push_str("\n---\n");
        out.push_str(&memory.render_block());
        let tail = transcript_tail.trim();
        if !tail.is_empty() {
            out.push_str("\n---\nRecent conversation:\n");
            out.push_str(tail);
            out.push('\n');
        }
        out
    }
}
