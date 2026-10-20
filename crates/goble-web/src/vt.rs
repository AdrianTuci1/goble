//! Thin, native-testable wrapper around the `vt100` parser.
//!
//! The renderer only ever reads through this type. Keeping the parser and its
//! callbacks here means scrollback, selection, titles and the bell can be
//! tested without a browser.

use std::num::NonZeroU16;

/// Rows of history retained behind the visible screen.
pub const SCROLLBACK_LINES: usize = 2000;

/// Parser callbacks the client cares about. Everything else (`DCS`, clipboard
/// requests, resize requests) is intentionally ignored.
#[derive(Debug, Default)]
pub struct Callbacks {
    pub title: String,
    pub bell: bool,
}

impl vt100::Callbacks for Callbacks {
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.bell = true;
    }

    fn visual_bell(&mut self, _: &mut vt100::Screen) {
        self.bell = true;
    }

    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = String::from_utf8_lossy(title).into_owned();
    }

    fn set_window_icon_name(&mut self, _: &mut vt100::Screen, name: &[u8]) {
        if self.title.is_empty() {
            self.title = String::from_utf8_lossy(name).into_owned();
        }
    }
}

/// Cursor position and visibility, in zero-based cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
}

/// A selection in screen cells: `(row, col)`.
pub type CellPos = (u16, u16);

/// Mouse selection range, in document order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub start: CellPos,
    pub end: CellPos,
}

impl Selection {
    #[must_use]
    pub fn new(a: CellPos, b: CellPos) -> Self {
        if (a.0, a.1) <= (b.0, b.1) {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    /// The `(start_row, start_col, end_row, end_col)` tuple `vt100` wants.
    #[must_use]
    pub fn bounds(self) -> (u16, u16, u16, u16) {
        (self.start.0, self.start.1, self.end.0, self.end.1)
    }

    #[must_use]
    pub fn contains(self, row: u16, col: u16) -> bool {
        (row, col) >= (self.start.0, self.start.1) && (row, col) <= (self.end.0, self.end.1)
    }
}

/// One terminal screen plus its scrollback.
pub struct Terminal {
    parser: vt100::Parser<Callbacks>,
    cols: u16,
    rows: u16,
}

impl Terminal {
    #[must_use]
    pub fn new(cols: u16, rows: u16) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self {
            parser: vt100::Parser::new_with_callbacks(
                non_zero(rows),
                non_zero(cols),
                SCROLLBACK_LINES,
                Callbacks::default(),
            ),
            cols,
            rows,
        }
    }

    /// Feed raw pty bytes into the parser.
    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// Resize the visible grid. History survives a height change.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if (cols, rows) == (self.cols, self.rows) {
            return;
        }
        self.parser.set_size(non_zero(rows), non_zero(cols));
        self.cols = cols;
        self.rows = rows;
    }

    #[must_use]
    pub fn cols(&self) -> u16 {
        self.cols
    }

    #[must_use]
    pub fn rows(&self) -> u16 {
        self.rows
    }

    #[must_use]
    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    #[must_use]
    pub fn cell_at(&self, row: u16, col: u16) -> Option<&vt100::Cell> {
        if row >= self.rows || col >= self.cols {
            return None;
        }
        self.parser.screen().cell(row, col)
    }

    #[must_use]
    pub fn cursor(&self) -> Cursor {
        let (row, col) = self.parser.screen().cursor_position();
        Cursor {
            row,
            col,
            visible: !self.parser.screen().hide_cursor(),
        }
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.parser.callbacks().title
    }

    /// Read and clear the pending bell flag.
    pub fn take_bell(&mut self) -> bool {
        let bell = self.parser.callbacks_mut().bell;
        self.parser.callbacks_mut().bell = false;
        bell
    }

    #[must_use]
    pub fn alternate_screen(&self) -> bool {
        self.parser.screen().alternate_screen()
    }

    #[must_use]
    pub fn bracketed_paste(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    /// Lines currently held above the visible screen.
    #[must_use]
    pub fn scrollback_len(&self) -> usize {
        self.parser.screen().scrollback_len()
    }

    /// How many lines the view is scrolled back; `0` is the live screen.
    #[must_use]
    pub fn scroll_offset(&self) -> usize {
        self.parser.screen().scrollback()
    }

    /// Scroll by `delta` lines, positive into history. Returns the new offset.
    pub fn scroll(&mut self, delta: i32) -> usize {
        if self.alternate_screen() {
            return 0;
        }
        let max = self.scrollback_len() as i32;
        let next = (self.scroll_offset() as i32 + delta).clamp(0, max) as usize;
        self.parser.screen_mut().set_scrollback(next);
        next
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_to(self.scrollback_len());
    }

    pub fn scroll_to_bottom(&mut self) {
        self.parser.screen_mut().set_scrollback(0);
    }

    fn scroll_to(&mut self, offset: usize) {
        if self.alternate_screen() {
            return;
        }
        let offset = offset.min(self.scrollback_len());
        self.parser.screen_mut().set_scrollback(offset);
    }

    /// Text covered by a selection. Rows are joined with `\n`; wrapped rows are
    /// joined without one, matching the terminal's visual lines. Both endpoints
    /// are inclusive.
    #[must_use]
    pub fn selection_text(&self, selection: Selection) -> String {
        let (start_row, start_col, end_row, end_col) = selection.bounds();
        if start_row >= self.rows || end_row >= self.rows || start_col >= self.cols {
            return String::new();
        }
        let end_col = (end_col + 1).min(self.cols);
        self.parser
            .screen()
            .contents_between(start_row, start_col, end_row, end_col)
    }

    /// Plain text of one visible row, trailing blanks removed.
    #[must_use]
    pub fn row_text(&self, row: u16) -> String {
        if row >= self.rows {
            return String::new();
        }
        let mut text = String::new();
        for col in 0..self.cols {
            if let Some(cell) = self.cell_at(row, col) {
                text.push_str(cell.contents());
            }
        }
        text.trim_end().to_string()
    }

    /// Best-effort URL under `col` on `row`.
    ///
    /// A real terminal delegates this to the shell's link hints; here a simple
    /// whitespace token scan is enough to make `Ctrl`-click useful without
    /// pulling a regex engine into the wasm bundle.
    #[must_use]
    pub fn link_at(&self, row: u16, col: u16) -> Option<String> {
        if row >= self.rows || col >= self.cols {
            return None;
        }
        let mut token_start: Option<u16> = None;
        let mut token = String::new();
        let mut tokens: Vec<(u16, u16, String)> = Vec::new();
        for current in 0..self.cols {
            let text = self
                .cell_at(row, current)
                .map(vt100::Cell::contents)
                .unwrap_or("");
            if text.is_empty() || text.chars().all(char::is_whitespace) {
                if let Some(start) = token_start.take() {
                    tokens.push((start, current, std::mem::take(&mut token)));
                }
            } else {
                token_start.get_or_insert(current);
                token.push_str(text);
            }
        }
        if let Some(start) = token_start {
            tokens.push((start, self.cols, token));
        }

        let (_, _, raw) = tokens
            .into_iter()
            .find(|(start, end, _)| col >= *start && col < *end)?;
        let trimmed = raw
            .trim_end_matches(['.', ',', ';', ':', ')', ']', '}', '"', '\'', '>'])
            .trim_start_matches(['(', '[', '{', '"', '\'', '<']);
        if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
            Some(trimmed.to_string())
        } else if trimmed.starts_with("www.") {
            Some(format!("https://{trimmed}"))
        } else {
            None
        }
    }
}

fn non_zero(value: u16) -> NonZeroU16 {
    NonZeroU16::new(value).unwrap_or(NonZeroU16::MIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal() -> Terminal {
        Terminal::new(40, 6)
    }

    fn line(terminal: &mut Terminal, text: &str) {
        terminal.process(text.as_bytes());
        terminal.process(b"\r\n");
    }

    #[test]
    fn sgr_color_and_bold_are_parsed() {
        let mut term = terminal();
        term.process(b"\x1b[1;31mred\x1b[0m plain");
        let cell = term.cell_at(0, 0).unwrap();
        assert_eq!(cell.contents(), "r");
        assert_eq!(cell.fgcolor(), vt100::Color::Idx(1));
        assert!(cell.bold());
        let plain = term.cell_at(0, 4).unwrap();
        assert_eq!(plain.fgcolor(), vt100::Color::Default);
        assert!(!plain.bold());
    }

    #[test]
    fn cursor_movement_is_tracked() {
        let mut term = terminal();
        term.process(b"\x1b[3;5H");
        assert_eq!(
            term.cursor(),
            Cursor {
                row: 2,
                col: 4,
                visible: true
            }
        );
        term.process(b"ab");
        assert_eq!(term.cursor().col, 6);
        term.process(b"\x1b[?25l");
        assert!(!term.cursor().visible);
    }

    #[test]
    fn erase_clears_cells() {
        let mut term = terminal();
        term.process(b"abc\x1b[2J");
        assert_eq!(term.row_text(0), "");
        term.process(b"\x1b[Hxyz");
        assert_eq!(term.row_text(0), "xyz");
        term.process(b"\x1b[K");
        assert_eq!(term.row_text(0), "xyz");
        term.process(b"\x1b[2K");
        assert_eq!(term.row_text(0), "");
    }

    #[test]
    fn scrollback_accumulates_and_scrolls() {
        let mut term = Terminal::new(20, 3);
        for text in ["one", "two", "three", "four", "five"] {
            line(&mut term, text);
        }
        assert_eq!(term.row_text(0), "four");
        assert_eq!(term.row_text(1), "five");
        assert!(term.scrollback_len() >= 2);
        assert_eq!(term.scroll(1), 1);
        assert_eq!(term.row_text(0), "three");
        assert_eq!(term.row_text(1), "four");
        term.scroll_to_bottom();
        assert_eq!(term.scroll_offset(), 0);
        assert_eq!(term.row_text(0), "four");
    }

    #[test]
    fn scroll_clamps_at_both_ends() {
        let mut term = Terminal::new(20, 3);
        for text in ["a", "b", "c", "d", "e", "f"] {
            line(&mut term, text);
        }
        assert_eq!(term.scroll(10_000), term.scrollback_len());
        assert_eq!(term.scroll(-10_000), 0);
    }

    #[test]
    fn resize_keeps_content_and_reports_size() {
        let mut term = terminal();
        term.process(b"hello");
        term.resize(10, 30);
        assert_eq!(term.size(), (10, 30));
        assert_eq!(term.row_text(0), "hello");
        assert!(term.cell_at(0, 9).is_some());
        assert!(term.cell_at(0, 10).is_none());
        assert!(term.cell_at(30, 0).is_none());
    }

    #[test]
    fn wide_characters_use_two_cells() {
        let mut term = terminal();
        term.process("界a".as_bytes());
        let wide = term.cell_at(0, 0).unwrap();
        assert!(wide.is_wide());
        assert_eq!(wide.contents(), "界");
        assert!(term.cell_at(0, 1).unwrap().is_wide_continuation());
        assert_eq!(term.cell_at(0, 2).unwrap().contents(), "a");
        assert_eq!(term.row_text(0), "界a");
    }

    #[test]
    fn title_comes_from_osc_2() {
        let mut term = terminal();
        term.process(b"\x1b]2;build shell\x07");
        assert_eq!(term.title(), "build shell");
        term.process(b"\x1b]2;second\x1b\\");
        assert_eq!(term.title(), "second");
    }

    #[test]
    fn bell_is_reported_once() {
        let mut term = terminal();
        term.process(b"\x07");
        assert!(term.take_bell());
        assert!(!term.take_bell());
    }

    #[test]
    fn alternate_screen_is_tracked() {
        let mut term = Terminal::new(20, 3);
        line(&mut term, "shell");
        term.process(b"\x1b[?1049h");
        assert!(term.alternate_screen());
        term.process(b"fullscreen");
        assert_eq!(term.row_text(0), "fullscreen");
        term.process(b"\x1b[?1049l");
        assert!(!term.alternate_screen());
        assert_eq!(term.row_text(0), "shell");
    }

    #[test]
    fn bracketed_paste_mode_is_visible() {
        let mut term = terminal();
        assert!(!term.bracketed_paste());
        term.process(b"\x1b[?2004h");
        assert!(term.bracketed_paste());
        term.process(b"\x1b[?2004l");
        assert!(!term.bracketed_paste());
    }

    #[test]
    fn selection_text_covers_rows() {
        let mut term = terminal();
        term.process(b"hello\r\nworld");
        let selection = Selection::new((0, 0), (1, 3));
        assert_eq!(term.selection_text(selection), "hello\nworl");
    }

    #[test]
    fn selection_is_normalized_when_dragged_backwards() {
        let mut term = terminal();
        term.process(b"hello\r\nworld");
        let selection = Selection::new((1, 3), (0, 0));
        assert_eq!(selection.start, (0, 0));
        assert_eq!(term.selection_text(selection), "hello\nworl");
    }

    #[test]
    fn single_cell_selection_copies_one_character() {
        let mut term = terminal();
        term.process(b"hello");
        assert_eq!(term.selection_text(Selection::new((0, 2), (0, 2))), "l");
    }

    #[test]
    fn selection_clamps_to_the_last_column() {
        let mut term = terminal();
        term.process(b"hello");
        assert_eq!(
            term.selection_text(Selection::new((0, 0), (0, 999))),
            "hello"
        );
    }

    #[test]
    fn selection_clamps_to_visible_rows() {
        let mut term = terminal();
        term.process(b"hello");
        assert_eq!(term.selection_text(Selection::new((0, 0), (99, 3))), "");
    }

    #[test]
    fn selection_membership() {
        let selection = Selection::new((1, 2), (3, 4));
        assert!(selection.contains(2, 0));
        assert!(!selection.contains(0, 0));
        assert!(!selection.contains(4, 0));
    }

    #[test]
    fn links_are_found_in_a_row() {
        let mut term = terminal();
        term.process(b"see https://example.com/x for details");
        assert_eq!(
            term.link_at(0, 8),
            Some("https://example.com/x".to_string())
        );
        assert_eq!(term.link_at(0, 1), None);
        term.process(b"\r\nwww.example.org");
        assert_eq!(
            term.link_at(1, 1),
            Some("https://www.example.org".to_string())
        );
    }

    #[test]
    fn links_trim_trailing_punctuation() {
        let mut term = terminal();
        term.process(b"(https://example.com).");
        assert_eq!(term.link_at(0, 5), Some("https://example.com".to_string()));
    }

    #[test]
    fn row_text_trims_only_the_end() {
        let mut term = terminal();
        term.process(b"  padded  ");
        assert_eq!(term.row_text(0), "  padded");
        assert_eq!(term.row_text(99), "");
    }
}
