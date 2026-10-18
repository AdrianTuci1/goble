//! The system clipboard, for the vim registers that reach outside the editor
//! (`"+`, `"*`, and the unnamed register when the user asked for it) and for
//! copying a terminal block.
//!
//! Each platform is driven through its usual helper:
//!
//! * macOS: `pbcopy` / `pbpaste`.
//! * Linux: `wl-copy` / `wl-paste`, then `xclip -selection clipboard`, then
//!   `xsel --clipboard`. The order prefers Wayland and falls back to X11.
//! * Windows: `cmd /C clip` and `powershell -NoProfile -Command Get-Clipboard`.
//!
//! A helper that is not installed (or fails) is skipped, and when none of them
//! runs the text is kept in a process-wide buffer, so a yank and a paste still
//! round-trip inside the session instead of silently doing nothing.

use std::cell::RefCell;
use std::io::Write;
use std::process::{Command, Stdio};
use std::rc::Rc;

use goble_ui::vim::Clipboard;

/// The platform whose clipboard helper list to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipboardPlatform {
    Macos,
    Linux,
    Windows,
    Other,
}

/// Which direction the clipboard is being used in. The read and write command
/// lines differ (`xclip` needs `-o` to read, and `xsel` needs `--output`), so
/// the candidates are chosen per direction rather than per program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipboardOp {
    Copy,
    Paste,
}

/// One helper invocation: a program and its non-stdin arguments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClipboardCommand {
    pub program: &'static str,
    pub args: &'static [&'static str],
}

const MACOS_COPY: &[ClipboardCommand] = &[ClipboardCommand {
    program: "pbcopy",
    args: &[],
}];
const MACOS_PASTE: &[ClipboardCommand] = &[ClipboardCommand {
    program: "pbpaste",
    args: &[],
}];
const LINUX_COPY: &[ClipboardCommand] = &[
    ClipboardCommand {
        program: "wl-copy",
        args: &[],
    },
    ClipboardCommand {
        program: "xclip",
        args: &["-selection", "clipboard"],
    },
    ClipboardCommand {
        program: "xsel",
        args: &["--clipboard", "--input"],
    },
];
const LINUX_PASTE: &[ClipboardCommand] = &[
    ClipboardCommand {
        program: "wl-paste",
        args: &[],
    },
    ClipboardCommand {
        program: "xclip",
        args: &["-selection", "clipboard", "-o"],
    },
    ClipboardCommand {
        program: "xsel",
        args: &["--clipboard", "--output"],
    },
];
const WINDOWS_COPY: &[ClipboardCommand] = &[ClipboardCommand {
    program: "cmd",
    args: &["/C", "clip"],
}];
const WINDOWS_PASTE: &[ClipboardCommand] = &[ClipboardCommand {
    program: "powershell",
    args: &["-NoProfile", "-Command", "Get-Clipboard"],
}];
const NO_COMMANDS: &[ClipboardCommand] = &[];

/// The ordered helpers for `platform` and `op`. Pure, so the choice can be
/// tested for every platform without running a clipboard tool.
pub(crate) fn clipboard_candidates(
    platform: ClipboardPlatform,
    op: ClipboardOp,
) -> &'static [ClipboardCommand] {
    match (platform, op) {
        (ClipboardPlatform::Macos, ClipboardOp::Copy) => MACOS_COPY,
        (ClipboardPlatform::Macos, ClipboardOp::Paste) => MACOS_PASTE,
        (ClipboardPlatform::Linux, ClipboardOp::Copy) => LINUX_COPY,
        (ClipboardPlatform::Linux, ClipboardOp::Paste) => LINUX_PASTE,
        (ClipboardPlatform::Windows, ClipboardOp::Copy) => WINDOWS_COPY,
        (ClipboardPlatform::Windows, ClipboardOp::Paste) => WINDOWS_PASTE,
        (ClipboardPlatform::Other, _) => NO_COMMANDS,
    }
}

const HOST_PLATFORM: ClipboardPlatform = if cfg!(target_os = "macos") {
    ClipboardPlatform::Macos
} else if cfg!(target_os = "linux") {
    ClipboardPlatform::Linux
} else if cfg!(target_os = "windows") {
    ClipboardPlatform::Windows
} else {
    ClipboardPlatform::Other
};

thread_local! {
    /// The in-process buffer used when no helper can run.
    static FALLBACK: RefCell<String> = const { RefCell::new(String::new()) };
}

fn store_fallback(text: &str) {
    FALLBACK.with(|buffer| *buffer.borrow_mut() = text.to_string());
}

fn fallback_text() -> String {
    FALLBACK.with(|buffer| buffer.borrow().clone())
}

fn run_write(command: &ClipboardCommand, text: &str) -> Result<(), String> {
    let mut child = Command::new(command.program)
        .args(command.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("{}: {error}", command.program))?;

    // Taking stdin and dropping it at the end of this block gives the helper
    // EOF before it is waited for.
    let write_result = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(text.as_bytes()),
        None => Ok(()),
    };
    let status = child
        .wait()
        .map_err(|error| format!("{}: {error}", command.program))?;

    match (write_result, status.success()) {
        (Ok(()), true) => Ok(()),
        (Err(error), _) => Err(format!("{}: {error}", command.program)),
        (Ok(()), false) => Err(format!("{} exited with {status}", command.program)),
    }
}

fn run_read(command: &ClipboardCommand) -> Result<String, String> {
    let output = Command::new(command.program)
        .args(command.args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("{}: {error}", command.program))?;
    if !output.status.success() {
        return Err(format!("{} exited with {}", command.program, output.status));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("{}: output is not UTF-8: {error}", command.program))
}

fn write_text(candidates: &[ClipboardCommand], text: &str) -> Result<(), String> {
    let mut failures = Vec::new();
    for command in candidates {
        match run_write(command, text) {
            Ok(()) => return Ok(()),
            Err(error) => failures.push(error),
        }
    }
    if failures.is_empty() {
        Err("no clipboard helper is available".to_string())
    } else {
        Err(failures.join("; "))
    }
}

fn read_text(candidates: &[ClipboardCommand]) -> Result<String, String> {
    let mut failures = Vec::new();
    for command in candidates {
        match run_read(command) {
            Ok(text) => return Ok(text),
            Err(error) => failures.push(error),
        }
    }
    if failures.is_empty() {
        Err("no clipboard helper is available".to_string())
    } else {
        Err(failures.join("; "))
    }
}

fn copy_text_with(candidates: &[ClipboardCommand], text: &str) {
    if let Err(error) = write_text(candidates, text) {
        log::warn!(
            "no system clipboard helper wrote the text ({error}); keeping an in-process copy"
        );
        store_fallback(text);
    }
}

fn paste_text_with(candidates: &[ClipboardCommand]) -> Option<String> {
    match read_text(candidates) {
        Ok(text) => Some(text),
        Err(error) => {
            log::warn!(
                "no system clipboard helper read the clipboard ({error}); using the in-process copy"
            );
            Some(fallback_text())
        }
    }
}

/// Copy `text` to the system clipboard, keeping an in-process copy when no
/// helper runs. Shared by the vim registers and the terminal block copy.
pub fn copy_text(text: &str) {
    copy_text_with(clipboard_candidates(HOST_PLATFORM, ClipboardOp::Copy), text);
}

pub struct SystemClipboard;

impl Clipboard for SystemClipboard {
    fn read(&mut self) -> Option<String> {
        paste_text_with(clipboard_candidates(HOST_PLATFORM, ClipboardOp::Paste))
    }

    fn write(&mut self, text: &str) {
        copy_text(text);
    }
}

/// A clipboard handle to hand the rich input.
pub fn system_clipboard() -> Rc<RefCell<dyn Clipboard>> {
    Rc::new(RefCell::new(SystemClipboard))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(candidates: &[ClipboardCommand]) -> Vec<Vec<&'static str>> {
        candidates
            .iter()
            .map(|command| {
                let mut argv = vec![command.program];
                argv.extend_from_slice(command.args);
                argv
            })
            .collect()
    }

    #[test]
    fn macos_uses_pbcopy_and_pbpaste() {
        assert_eq!(
            argv(clipboard_candidates(
                ClipboardPlatform::Macos,
                ClipboardOp::Copy
            )),
            vec![vec!["pbcopy"]]
        );
        assert_eq!(
            argv(clipboard_candidates(
                ClipboardPlatform::Macos,
                ClipboardOp::Paste
            )),
            vec![vec!["pbpaste"]]
        );
    }

    #[test]
    fn linux_prefers_wayland_then_xclip_then_xsel() {
        assert_eq!(
            argv(clipboard_candidates(
                ClipboardPlatform::Linux,
                ClipboardOp::Copy
            )),
            vec![
                vec!["wl-copy"],
                vec!["xclip", "-selection", "clipboard"],
                vec!["xsel", "--clipboard", "--input"],
            ]
        );
        assert_eq!(
            argv(clipboard_candidates(
                ClipboardPlatform::Linux,
                ClipboardOp::Paste
            )),
            vec![
                vec!["wl-paste"],
                vec!["xclip", "-selection", "clipboard", "-o"],
                vec!["xsel", "--clipboard", "--output"],
            ]
        );
    }

    #[test]
    fn windows_uses_clip_and_powershell_get_clipboard() {
        assert_eq!(
            argv(clipboard_candidates(
                ClipboardPlatform::Windows,
                ClipboardOp::Copy
            )),
            vec![vec!["cmd", "/C", "clip"]]
        );
        assert_eq!(
            argv(clipboard_candidates(
                ClipboardPlatform::Windows,
                ClipboardOp::Paste
            )),
            vec![vec![
                "powershell",
                "-NoProfile",
                "-Command",
                "Get-Clipboard"
            ]]
        );
    }

    #[test]
    fn every_helper_failing_keeps_an_in_process_copy() {
        store_fallback("");
        let missing = [ClipboardCommand {
            program: "goble-test-missing-clipboard-helper",
            args: &[],
        }];

        // The terminal block copy and the vim registers both go through this
        // writer; when every candidate fails the text must still round-trip.
        copy_text_with(&missing, "terminal block text");
        assert_eq!(fallback_text(), "terminal block text");
        assert_eq!(
            paste_text_with(&missing).as_deref(),
            Some("terminal block text")
        );
    }

    #[test]
    fn a_platform_without_helpers_uses_the_in_process_copy() {
        store_fallback("");
        copy_text_with(
            clipboard_candidates(ClipboardPlatform::Other, ClipboardOp::Copy),
            "kept",
        );
        assert_eq!(fallback_text(), "kept");
    }
}
