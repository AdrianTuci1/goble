//! The command-block model: one command and everything it printed, as one unit.
//!
//! One module per surface of the model: the vocabulary a block is described
//! with ([`state`]), block identities ([`id`]) and the views a block belongs to
//! ([`visibility`]); the block itself ([`block`]) and the list that owns one
//! session's blocks ([`list`]); the events the list reports ([`event`]) and the
//! tool-result contract with the agent ([`tool`]). Every surface is re-exported
//! here, so `goble_terminal::blocks::…` names what it always did.
//!
//! A block is one command and everything it printed, kept as one unit so the
//! renderer can draw a command and its output together and never lets output
//! from one command bleed into another.
//!
//! Blocks are driven by shell-integration hooks (see [`crate::hooks`]), not by
//! the VT stream: a prompt inside a command's output does not start a block, and
//! a command that prints nothing at all still ends one. The lifecycle is
//!
//! ```text
//! BeforeExecution --Preexec--> Executing --CommandFinished--> Done...*
//! ```
//!
//! Every finished block is frozen ([`Screen::freeze`](crate::screen::Screen::freeze))
//! so that anything the shell writes afterwards — the next prompt, a background
//! job's output — stays out of it. The block that follows is created by
//! `CommandFinished` itself, the same way the shell's own `PROMPT_COMMAND` runs
//! before the next prompt.
//!
//! A block the agent claimed ([`BlockOwner::Agent`]) is also a tool result: the
//! moment it reaches a terminal state — its `CommandFinished`, or the `Precmd`
//! that promotes a still-running command to `Background` — the list hands its
//! output and exit code back as a [`ToolResult`].
//!
//! A block draws its screens' whole grids (`Screen::content_lines`), not their
//! viewports, so output taller than the window is kept rather than scrolled
//! away. The block list lays those rows out; it is not itself the scrollback.

mod block;
mod event;
mod id;
mod list;
mod state;
mod tool;
mod visibility;

#[cfg(test)]
mod tests;

pub use block::{Block, BlockMetadata};
pub use event::{BlockEvent, BlockLine};
pub use id::BlockId;
pub use list::{BlockList, PendingClaim, SessionInfo};
pub use state::{BlockKind, BlockOwner, BlockState, BlockView};
pub use tool::{ToolOutcome, ToolResult};
pub use visibility::BlockVisibility;
