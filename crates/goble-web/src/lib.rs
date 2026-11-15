//! Browser client for the Goble gateway.
//!
//! **Stale as of 2026-11-15.** The browser and desktop surface is built from
//! the Expo app in `apps/mobile`; this crate is kept for reference and receives
//! no new work. See the crate's README.
//!
//! The main area draws one of two panes: a terminal on the worker, or the
//! cloud agent viewer (a transcript and a composer). The pure half — wire
//! protocol, key encoding, the `vt100` wrapper, theme, tabs, the session state
//! machine and the viewer's transcript — compiles and is tested natively. The
//! `wasm32` target adds the canvas renderer and the DOM shell.
//!
//! The gateway serves the wasm-bindgen `--target web` glue from `/app/`; that
//! glue calls the exported [`start`] after `init()`.

pub mod input;
pub mod layout;
pub mod protocol;
pub mod session;
pub mod theme;
pub mod viewer;
pub mod vt;

#[cfg(target_arch = "wasm32")]
mod dom;
#[cfg(target_arch = "wasm32")]
mod render;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Install a panic hook that writes to the browser console, then let the JS
/// entry point call [`start`]. Keeping this separate from `start` means an
/// accidental double call does not build the UI twice.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&JsValue::from_str(&format!("goble-web panic: {info}")));
    }));
}

/// Build the client shell and boot the worker list.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn start() {
    dom::start();
}
