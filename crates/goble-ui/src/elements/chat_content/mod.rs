//! Chat transcript content: the fragments a message is made of.
//!
//! One module per surface: a message and its role/action ([`message`]), the
//! flat fragments a message carries ([`fragment`]), how those fragments group
//! into renderable blocks ([`block`]), a tool invocation ([`tool_call`]), a
//! live sub-agent row ([`sub_agent`]) and the strip that owns a pane's
//! sub-agent controls ([`sub_agent_panel`]). The re-exports below keep the
//! module's public paths (`elements::chat_content::ChatFragment`, ...)
//! unchanged.

pub mod ansi;
mod block;
mod changed_files;
mod file_mention;
mod fragment;
mod message;
mod sub_agent;
mod sub_agent_panel;
#[cfg(test)]
mod tests;
mod tool_call;
mod work_dock;

pub use block::{group_fragments_into_blocks, ChatBlock, InlineSpan, InlineStyle};
pub use changed_files::{changed_files_for_message, ChangedFile, ChangedFiles};
pub use file_mention::inline_code_file_path;
pub use fragment::{ChatFragment, ChatFragmentKind, ListItem};
pub use message::{ChatAction, ChatMessage, ChatRole};
pub use sub_agent::{format_elapsed, SubAgentRow, SubAgentRowStatus};
pub use sub_agent_panel::SubAgentPanel;
pub use tool_call::{tool_fold_key, ToolCall, ToolDisplayMode};
pub use work_dock::{WorkDock, WorkGroup, WorkRow, WorkSection};

/// The elapsed time a sub-agent row states, shared by the transcript row's
/// status line and the strip's own column so the two read the same clock.
pub(crate) use sub_agent::format_subagent_elapsed;
