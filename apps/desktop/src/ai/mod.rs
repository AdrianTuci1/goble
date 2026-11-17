//! AI domain: vault secrets + MCP connectors.
//!
//! Product logic lives in the executable (`app`), split into surface-level
//! domain directories. This directory owns the state and callbacks for the
//! AI auxiliary panels; the element tree itself lives in [`crate::ui`].

pub mod actions;
pub mod state;

pub use actions::make_ai_actions;
pub use state::AiState;
