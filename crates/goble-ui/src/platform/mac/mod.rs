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
}
