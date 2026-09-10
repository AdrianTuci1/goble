//! Screen domain: live broadcast + computer-use controls.
//!
//! Product logic lives in the executable (`app`), split into surface-level
//! domain directories. This directory owns the state and callbacks for the
//! screen panel; the element tree itself lives in [`crate::ui`].

pub mod actions;
pub mod state;

pub use actions::make_screen_actions;
pub use state::ScreenState;
