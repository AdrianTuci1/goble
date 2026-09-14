/// Rows of scrollback a block screen keeps.
///
/// A block draws its whole grid, not just the viewport, so this is what stops a
/// long command's output from being dropped: the rows a block scrolls through
/// are its own output, and they have to survive. The cap bounds the memory one
/// runaway command can take.
pub const BLOCK_HISTORY: usize = 50_000;

/// What a screen is for. The emulator's `Config` is fixed at construction, so
/// these choices have to be made up front.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenConfig {
    /// Rows of scrollback.
    pub history: usize,
    /// Let applications *read* the clipboard through `OSC 52`. Off by default:
    /// a program that can ask for the clipboard can exfiltrate it.
    pub allow_clipboard_read: bool,
}

impl ScreenConfig {
    /// A screen belonging to one command block.
    pub const fn block() -> Self {
        Self {
            history: BLOCK_HISTORY,
            allow_clipboard_read: false,
        }
    }

    /// A screen that scrolls, with `history` rows of scrollback.
    pub const fn scrolling(history: usize) -> Self {
        Self {
            history,
            allow_clipboard_read: false,
        }
    }

    pub const fn allow_clipboard_read(mut self) -> Self {
        self.allow_clipboard_read = true;
        self
    }
}

impl Default for ScreenConfig {
    fn default() -> Self {
        Self::block()
    }
}
