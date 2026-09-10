//! Canonical per-agent memory that survives conversation summarization.
//!
//! Transcripts may be compacted freely (see [`compaction`]), but this state is
//! only changed through explicit writes, so the agent stays current with the
//! user's requirements no matter how often conversations are summarized.

pub mod compaction;
pub mod context;
pub mod memory;

#[cfg(test)]
mod compaction_tests;
#[cfg(test)]
mod context_tests;
#[cfg(test)]
mod memory_tests;

pub use compaction::{
    merge_compaction, CompactionResult, CompactedDecision, COMPACTION_PROMPT,
};
pub use context::ContextBuilder;
pub use memory::{
    AgentMemory, Decision, Goal, Milestone, SessionSummary, AGENT_MEMORY_VERSION,
};
