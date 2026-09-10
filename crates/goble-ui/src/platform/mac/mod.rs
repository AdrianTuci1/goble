//! macOS-specific platform hooks for goble-ui.
//!
//! Currently the crate does not use native font APIs, so these re-export the
//! fallback text metrics. In the future this module can switch to
//! core-text / core-graphics / font-kit based measurement.

pub use super::fallback::*;

/// Native macOS application menu bar (`NSMenu`), following the warp-new
/// approach. Only meaningful on macOS; Windows/Linux rely on in-app keybindings
/// + the command palette instead of a native OS menu bar.
pub mod menus;
