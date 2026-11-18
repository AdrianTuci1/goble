//! Projects domain: per-project observability.
//!
//! Product logic lives in the executable (`app`), split into surface-level
//! domain directories. This directory owns the state and callbacks for the
//! Projects auxiliary panel; the element tree itself lives in [`crate::ui`].

pub mod actions;
pub mod state;

pub use actions::make_projects_actions;
pub use state::ProjectsState;
