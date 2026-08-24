#[cfg(feature = "hot-reload")]
#[hot_lib_reloader::hot_module(dylib = "goble_ui_hot")]
pub mod ui_hot {
    pub use goble_ui_hot::*;

    // Path is resolved relative to the cargo working directory. Run cargo
    // from the workspace root (as scripts/dev-ui.sh does).
    hot_functions_from_file!("crates/goble-ui-hot/src/lib.rs");

    /// Returns an observer that can block library reloads. Used by the root
    /// view to drop the old element tree before the dylib is swapped.
    #[lib_change_subscription]
    pub fn subscribe() -> hot_lib_reloader::LibReloadObserver {}
}

#[cfg(not(feature = "hot-reload"))]
pub mod ui_hot {
    pub use goble_ui_hot::*;
}

pub use ui_hot::{build_ui, AppTab, UiActions, UiSnapshot};
