//! Goble native app library.
//!
//! The executable (`main.rs`) is a thin shell that opens the backend, mounts
//! [`root_view::RootView`] and runs the event loop. The state machinery and
//! action wiring live here so the hot-reloadable element tree can be rebuilt
//! every frame and so integration tests (`integration_testing/`) can drive the
//! real application flow against a live [`goble_desktop_service::DesktopState`].

pub mod actions;
pub mod ai;
pub mod hot_ui;
pub mod root_view;
pub mod state;
