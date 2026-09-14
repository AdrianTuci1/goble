//! A VT screen: grid, cursor, modes and alternate screen.
//!
//! One [`Screen`] is one terminal surface. The block model (see
//! [`crate::blocks`]) gives every command block its own screen, which is why
//! scrollback history is configured per screen and can be zero.
//!
//! One module per surface of a screen: the size it is built with ([`size`]) and
//! the policy it is built under ([`config`]), the cell vocabulary the renderer
//! reads ([`cell`]), the cursor ([`cursor`]), the events and questions the
//! emulator raises ([`event`]) and [`Screen`] itself, which owns the parser and
//! the grid. Every surface is re-exported here, so `goble_terminal::screen::…`
//! names what it always did.
//!
//! The emulator itself is the vendored `alacritty_terminal` crate, driven
//! through its re-exported `vte` parser. This module is the seam: it owns the
//! parser, hands the renderer a plain row/cell snapshot, and keeps the
//! crate's types out of the rest of the codebase.

mod cell;
mod config;
mod cursor;
mod event;
mod screen;
mod size;

#[cfg(test)]
mod tests;

pub use cell::{CellAttrs, ScreenCell, ScreenColor, ScreenLine, Underline};
pub use config::{ScreenConfig, BLOCK_HISTORY};
pub use cursor::{CursorShape, CursorState};
pub use event::{ScreenEvent, ScreenEvents, ScreenQuery};
pub use screen::Screen;
pub use size::ScreenSize;
