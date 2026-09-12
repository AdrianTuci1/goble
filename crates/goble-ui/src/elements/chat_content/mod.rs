//! Chat transcript content: the fragments a message is made of.
//!
//! One module per surface: a message and its role/action ([`message`]), the
//! flat fragments a message carries ([`fragment`]), how those fragments group
//! into renderable blocks ([`block`]), a tool invocation ([`tool_call`]) and a
//! live sub-agent row ([`sub_agent`]). The re-exports below keep the module's
//! public paths (`elements::chat_content::ChatFragment`, ...) unchanged.

mod block;
mod fragment;
mod message;
mod sub_agent;
mod tool_call;
#[cfg(test)]
mod tests;

pub use block::{group_fragments_into_blocks, ChatBlock, InlineSpan, InlineStyle};
pub use fragment::{ChatFragment, ChatFragmentKind, ListItem};
pub use message::{ChatAction, ChatMessage, ChatRole};
pub use sub_agent::{SubAgentRow, SubAgentRowStatus};
pub use tool_call::{tool_fold_key, ToolCall, ToolDisplayMode};
