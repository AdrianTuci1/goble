//! Callback wiring: turns app state + backend into [`UiActions`](crate::ui::UiActions)
//! closures.
//!
//! The view tree built by [`crate::ui`] only knows about these callbacks; the
//! actual behavior (mutating [`UiState`](crate::state::UiState), persisting
//! through [`DesktopState`](goble_desktop_service::DesktopState)) lives here in
//! the executable.
//!
//! One module per surface: the composer's two input paths ([`prompt`]), the
//! pane-tree helpers the wiring calls ([`pane_ops`]) and the closure wiring
//! itself (`make_actions`).

mod make_actions;
mod pane_ops;
mod prompt;

pub use make_actions::make_actions;

#[cfg(test)]
mod pane_independence_tests;
