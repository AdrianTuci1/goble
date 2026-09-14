//! The system clipboard, for the vim registers that reach outside the editor
//! (`"+`, `"*`, and the unnamed register when the user asked for it).
//!
//! macOS gets the real pasteboard through `pbcopy`/`pbpaste` — the same helper
//! the transcript's copy action uses, so no extra dependency is pulled in.
//! Other platforms keep a process-wide buffer, so a yank and a paste still
//! round-trip inside the session instead of silently doing nothing.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::vim::Clipboard;

pub struct SystemClipboard;

#[cfg(not(target_os = "macos"))]
thread_local! {
    /// The in-process buffer non-macOS builds fall back to.
    static FALLBACK: RefCell<String> = const { RefCell::new(String::new()) };
}

impl Clipboard for SystemClipboard {
    fn read(&mut self) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            let output = std::process::Command::new("pbpaste").output().ok()?;
            if !output.status.success() {
                return None;
            }
            String::from_utf8(output.stdout).ok()
        }
        #[cfg(not(target_os = "macos"))]
        {
            FALLBACK.with(|buffer| Some(buffer.borrow().clone()))
        }
    }

    fn write(&mut self, text: &str) {
        #[cfg(target_os = "macos")]
        {
            use std::io::Write;
            match std::process::Command::new("pbcopy")
                .stdin(std::process::Stdio::piped())
                .spawn()
            {
                Ok(mut child) => {
                    if let Some(stdin) = child.stdin.as_mut() {
                        let _ = stdin.write_all(text.as_bytes());
                    }
                    let _ = child.wait();
                }
                Err(e) => log::warn!("could not launch pbcopy to write the clipboard: {e}"),
            }
        }
        #[cfg(not(target_os = "macos"))]
        FALLBACK.with(|buffer| *buffer.borrow_mut() = text.to_string());
    }
}

/// A clipboard handle to hand the rich input.
pub fn system_clipboard() -> Rc<RefCell<dyn Clipboard>> {
    Rc::new(RefCell::new(SystemClipboard))
}
