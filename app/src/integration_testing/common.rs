//! Helpers shared by the Goble app integration tests.
//!
//! Each test file declares `mod common;` and uses these to stand up a real
//! [`DesktopState`] backed by an in-memory sqlite store plus a temp thread
//! store, exactly the way the running app composes its backend.

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};

/// Build a `DesktopState` over an in-memory sqlite store and a temp thread
/// store directory.
///
/// The returned handle must be kept alive for the whole test: the state owns a
/// `PathBuf` that points into it. Bind it (e.g. `let (desktop, _dir) = ...`)
/// so it lives to the end of the test scope.
pub fn desktop_state() -> (std::sync::Arc<DesktopState>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("create temp thread store dir");
    let state = DesktopState::new(
        Store::open_in_memory().expect("open in-memory store"),
        ThreadStore::new(dir.path()).expect("open thread store"),
    );
    (state, dir)
}
