//! Terminal block: one command and its captured output, as the transcript draws it.
//!
//! One module per surface: the lines and runs a block is made of ([`line`]), the
//! block's data and status ([`data`]), its filter tray ([`filter`]), the plumbing
//! a surface hands a block ([`plumbing`]) and the [`block::TerminalBlock`]
//! element that lays them out. The re-exports below keep the module's public
//! paths (`elements::terminal_block::TerminalBlock`,
//! `elements::terminal_block::terminal_block`, ...) unchanged.

mod block;
mod data;
mod filter;
mod line;
mod plumbing;
#[cfg(test)]
mod tests;

pub use block::{terminal_block, TerminalBlock};
pub use data::{TerminalData, TerminalMeta, TerminalStatus};
pub use filter::{filter_option_labels, TerminalCopyHandler, TerminalFilter};
pub use line::{TerminalLine, TerminalLineKind, TerminalRun};
pub use plumbing::TerminalBlockPlumbing;

#[cfg(test)]
pub(crate) use block::{BUTTON_SIZE, HEADER_SPACING, PADDING_X, PADDING_Y};
