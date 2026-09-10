//! Medium domain: where a conversation's work runs.
//!
//! Product logic lives in the executable (`app`), split into surface-level
//! domain directories. This directory owns the state and callbacks for the
//! medium selector; the element tree itself lives in [`crate::ui`].

pub mod actions;
pub mod state;

pub use actions::make_media_actions;
pub use state::{medium_routing, MediaState};
