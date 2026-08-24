//! Worker-side helpers that bridge `goble-core` agent memory into harness runs.

pub mod injector;
pub mod loader;

pub use injector::{build_context, should_compact, transcript_tail};
pub use loader::{load_or_create, persist};

#[cfg(test)]
mod injector_tests;

#[cfg(test)]
mod loader_tests;
