//! Goble native app library.
//!
//! The executable (`main.rs`) is a thin shell that opens the backend, mounts
//! [`root_view::RootView`] and runs the event loop. Everything else — the
//! state machinery, action wiring, UI builder and the runtime/orchestration
//! decision — lives here in the app crate (matching warp-new's thick-app
//! model), so the app owns both the state and the element tree without a
//! hot-reusable dylib or ABI boundary.

pub mod actions;
pub mod ai;
pub mod root_view;
pub mod runtime;
pub mod state;
pub mod ui;
