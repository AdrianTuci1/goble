//! macOS-specific platform hooks for goble-ui.
//!
//! The text metrics are the fallback heuristics. Everything native here is
//! AppKit, and every entry point is main-thread only, like AppKit itself.

pub use super::fallback::*;

/// Native macOS application menu bar (`NSMenu`), following the warp-new
/// approach. Only meaningful on macOS; Windows/Linux rely on in-app keybindings
/// + the command palette instead of a native OS menu bar.
pub mod menus;

/// Open the OS file picker — AppKit's `NSOpenPanel` in open mode — and return
/// the absolute path of the file the user chose, or `None` when they cancelled
/// or there is no panel to open.
///
/// `None` is also the answer off the main thread: AppKit panels are main-thread
/// only, and a caller that is not the UI thread gets no panel rather than a
/// crash. The UI dispatches events on the main thread (the menu handler in
/// [`menus`] is called the same way), so a click on a control lands here.
pub fn pick_file() -> Option<String> {
    let mtm = objc2::MainThreadMarker::new()?;
    let panel = objc2_app_kit::NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(false);
    panel.setAllowsMultipleSelection(false);
    // A modal panel: the user's answer is this call's return, which is why the
    // caller can add the path to its own state in the same click.
    if panel.runModal() != objc2_app_kit::NSModalResponseOK {
        return None;
    }
    panel
        .URLs()
        .firstObject()
        .and_then(|url| url.path())
        .map(|path| path.to_string())
}

/// Open the OS directory picker — AppKit's `NSOpenPanel` in directory mode —
/// and return the absolute path of the folder the user chose, or `None` when
/// they cancelled or there is no panel to open.
///
/// The add-workspace flow calls this from the "+" button's handler, which runs
/// on the UI thread, so the modal answer is available before the next frame.
/// Off the main thread it answers `None` for the same reason as
/// [`pick_file`]: AppKit panels are main-thread only.
pub fn pick_directory() -> Option<String> {
    let mtm = objc2::MainThreadMarker::new()?;
    let panel = objc2_app_kit::NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(false);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(false);
    // Like the file panel: modal, so the chosen folder is this call's return.
    if panel.runModal() != objc2_app_kit::NSModalResponseOK {
        return None;
    }
    panel
        .URLs()
        .firstObject()
        .and_then(|url| url.path())
        .map(|path| path.to_string())
}

/// Reveal `path` in Finder, selecting it in its enclosing folder — DSH's
/// "Show in Finder" workspace action (`NSWorkspace`'s
/// `selectFile:inFileViewerRootedAtPath:`).
///
/// `path` is the directory a Workspace row stands for: Finder opens the folder
/// around it with the item selected, which is exactly what the menu row's label
/// promises. It answers whether AppKit accepted the request; like the pickers,
/// it answers `false` off the main thread rather than driving AppKit from it.
pub fn reveal_in_file_manager(path: &str) -> bool {
    if objc2::MainThreadMarker::new().is_none() {
        return false;
    }
    let full_path = objc2_foundation::NSString::from_str(path);
    // An empty root asks Finder to reveal the item in its own enclosing folder
    // (the same call `open -R` makes) instead of opening a new rooted window.
    let root = objc2_foundation::NSString::from_str("");
    objc2_app_kit::NSWorkspace::sharedWorkspace()
        .selectFile_inFileViewerRootedAtPath(Some(&full_path), &root)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The picker answers off the main thread with no panel: a caller that is
    /// not the UI thread gets `None` instead of AppKit being driven from it.
    #[test]
    fn pick_file_returns_none_off_the_main_thread() {
        assert!(
            objc2::MainThreadMarker::new().is_none(),
            "the test runs on a worker thread, so the panel is never opened here"
        );
        assert_eq!(pick_file(), None);
    }

    /// The workspace flow's directory picker follows the same rule: a test
    /// thread gets no panel and no path.
    #[test]
    fn pick_directory_returns_none_off_the_main_thread() {
        assert!(
            objc2::MainThreadMarker::new().is_none(),
            "the test runs on a worker thread, so the panel is never opened here"
        );
        assert_eq!(pick_directory(), None);
    }

    /// "Show in Finder" is a real AppKit call, so a test thread does not drive
    /// it: it answers `false` and the caller reports the failure instead of
    /// pretending the reveal happened.
    #[test]
    fn reveal_in_file_manager_returns_false_off_the_main_thread() {
        assert!(
            objc2::MainThreadMarker::new().is_none(),
            "the test runs on a worker thread, so Finder is never driven here"
        );
        assert!(!reveal_in_file_manager("/tmp"));
    }
}
