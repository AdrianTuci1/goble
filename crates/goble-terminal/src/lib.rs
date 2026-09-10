//! Terminal core for goble.
//!
//! Four layers, each independently testable:
//!
//! * [`keys`] — host → PTY encoding (key + modifiers + terminal mode → bytes).
//! * [`hooks`] — the shell-integration hook channel: a byte tap that lifts our
//!   own `DCS` envelope out of the PTY stream and leaves every other byte
//!   untouched for the VT parser.
//! * [`screen`] — a VT screen (grid, cursor, modes, alt screen) over the
//!   vendored `alacritty_terminal` crate.
//! * [`blocks`] — the command-block model, which owns one pair of screens per
//!   command.
//!
//! The VT parser itself is not ours: it is the vendored Alacritty crate, driven
//! through `alacritty_terminal::vte`. Nothing in this crate re-implements escape
//! parsing.

#![warn(missing_debug_implementations)]

pub mod blocks;
pub mod hooks;
pub mod keys;
pub mod screen;

pub use blocks::{
    Block, BlockEvent, BlockId, BlockLine, BlockList, BlockMetadata, BlockState, SessionInfo,
};
pub use hooks::{HookEvent, HookTap};
pub use keys::{Key, KeyEncoder, Modifiers, TermMode};
pub use screen::{
    CellAttrs, CursorState, Screen, ScreenCell, ScreenColor, ScreenConfig, ScreenEvent,
    ScreenEvents, ScreenLine, ScreenQuery, ScreenSize, Underline,
};
