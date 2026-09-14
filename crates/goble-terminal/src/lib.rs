//! Terminal core for goble.
//!
//! Layers, each independently testable:
//!
//! * [`keys`] — host → PTY encoding (key + modifiers + terminal mode → bytes).
//! * [`mouse`] — host → PTY mouse reports, gated by the negotiated mode.
//! * [`hooks`] — the shell-integration hook channel: a byte tap that lifts our
//!   own `DCS` envelope out of the PTY stream and leaves every other byte
//!   untouched for the VT parser.
//! * [`osc`] — an observer for the `OSC` numbers the parser discards: the
//!   working directory (`7`) and the prompt markers (`133`).
//! * [`palette`] — resolving a cell's colour to RGB.
//! * [`screen`] — a VT screen (grid, cursor, modes, alt screen) over the
//!   vendored `alacritty_terminal` crate.
//! * [`blocks`] — the command-block model, which owns one pair of screens per
//!   command.
//!
//! The VT parser itself is not ours: it is the vendored Alacritty crate, driven
//! through `alacritty_terminal::vte`. Nothing in this crate re-implements escape
//! parsing.

#![warn(missing_debug_implementations)]

pub mod ansi;
pub mod blocks;
pub mod hooks;
/// The shell-integration scripts that emit the hook channel. The pane's PTY
/// spawn writes the matching one out and points the shell at it (bash
/// `--rcfile`, zsh `ZDOTDIR`).
pub mod integration {
    /// Shell integration for bash.
    pub const BASH: &str = include_str!("../assets/shell/bash.sh");
    /// Shell integration for zsh.
    pub const ZSH: &str = include_str!("../assets/shell/zsh.sh");
    /// The `.zshenv` shim written alongside [`ZSH`]. zsh reads `.zshenv` from
    /// `$ZDOTDIR` alone, so the redirect has to restore the user's own file.
    pub const ZSH_ENV: &str = include_str!("../assets/shell/zsh_env.sh");
}
pub mod keys;
pub mod mouse;
pub mod osc;
pub mod palette;
pub mod screen;

pub use blocks::{
    Block, BlockEvent, BlockId, BlockLine, BlockList, BlockMetadata, BlockOwner, BlockState,
    PendingClaim, SessionInfo, ToolOutcome, ToolResult,
};
pub use hooks::{HookEvent, HookTap};
pub use keys::{Key, KeyEncoder, Modifiers, TermMode};
pub use mouse::{encode_mouse, MouseAction, MouseButton};
pub use osc::{OscEvent, OscTap, PromptMarker};
pub use palette::Palette;
pub use screen::{
    CellAttrs, CursorShape, CursorState, Screen, ScreenCell, ScreenColor, ScreenConfig,
    ScreenEvent, ScreenEvents, ScreenLine, ScreenQuery, ScreenSize, Underline,
};
