//! Transcript bubble: an assistant/user message and the tool calls it carries.
//!
//! One module per surface: the bubble itself ([`bubble`]), the tool-call rows
//! ([`tool_call`]), the bodies those rows unfold to ([`tool_body`]), the
//! numbered read view ([`read_excerpt`]) and the quote rail a quoted row is
//! drawn with ([`quote_rail`]). The re-exports below keep the module's public
//! path (`elements::chat_message_bubble::ChatMessageBubble`) unchanged.

mod bubble;
mod quote_rail;
mod read_excerpt;
mod tool_body;
mod tool_call;
#[cfg(test)]
mod tests;

pub use bubble::ChatMessageBubble;
pub(crate) use quote_rail::QuoteRail;

#[cfg(test)]
pub(crate) use quote_rail::QUOTE_RAIL_WIDTH;
#[cfg(test)]
pub(crate) use read_excerpt::read_excerpt;
#[cfg(test)]
pub(crate) use tool_body::SUB_AGENT_TRANSCRIPT_NOTE;
