//! A VT screen: grid, cursor, modes and alternate screen.
//!
//! One [`Screen`] is one terminal surface. The block model (see
//! [`crate::blocks`]) gives every command block its own screen, which is why
//! scrollback history is configured per screen and can be zero.
//!
//! The emulator itself is the vendored `alacritty_terminal` crate, driven
//! through its re-exported `vte` parser. This module is the seam: it owns the
//! parser, hands the renderer a plain row/cell snapshot, and keeps the
//! crate's types out of the rest of the codebase.

use std::fmt;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{
    Event as AlacrittyEvent, EventListener, WindowSize as AlacrittyWindowSize,
};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{
    point_to_viewport, Config, Osc52, Term, TermMode as AlacrittyTermMode,
};
use alacritty_terminal::vte::ansi::{
    Color as AlacrittyColor, CursorShape, NamedColor, Processor, Rgb,
};

/// Terminal size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct ScreenSize {
    pub columns: usize,
    pub screen_lines: usize,
}

impl ScreenSize {
    pub const fn new(columns: usize, screen_lines: usize) -> Self {
        Self {
            columns,
            screen_lines,
        }
    }
}

impl Dimensions for ScreenSize {
    fn total_lines(&self) -> usize {
        self.screen_lines
    }

    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

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

/// A colour a cell can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenColor {
    Named(NamedColor),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Underline style, as set by `SGR 4:n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Underline {
    #[default]
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

/// Cell attributes the renderer needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellAttrs {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: Underline,
    pub underline_color: Option<ScreenColor>,
    pub inverse: bool,
    pub hidden: bool,
    pub strikeout: bool,
    /// The cell holds the first half of a double-width glyph.
    pub wide: bool,
    /// The cell is the right half of a double-width glyph.
    pub wide_spacer: bool,
    /// A double-width glyph did not fit at the end of the row.
    pub leading_wide_spacer: bool,
    /// The row continues on the next one (soft wrap).
    pub wrapline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenCell {
    pub ch: char,
    pub fg: ScreenColor,
    pub bg: ScreenColor,
    pub attrs: CellAttrs,
    /// Combining marks attached to this cell.
    pub zerowidth: Vec<char>,
    pub hyperlink: Option<String>,
}

impl Default for ScreenCell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: ScreenColor::Named(NamedColor::Foreground),
            bg: ScreenColor::Named(NamedColor::Background),
            attrs: CellAttrs::default(),
            zerowidth: Vec::new(),
            hyperlink: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScreenLine {
    pub cells: Vec<ScreenCell>,
}

impl ScreenLine {
    /// The row's text with trailing blanks removed.
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(self.cells.len());
        for cell in &self.cells {
            let skip = cell.attrs.wide_spacer || cell.attrs.leading_wide_spacer;
            if !skip {
                out.push(cell.ch);
            }
            for mark in &cell.zerowidth {
                out.push(*mark);
            }
        }
        out.truncate(out.trim_end().len());
        out
    }

    pub fn is_blank(&self) -> bool {
        self.cells
            .iter()
            .all(|c| c.ch == ' ' && c.zerowidth.is_empty())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorState {
    /// Row within the viewport.
    pub line: usize,
    pub column: usize,
    pub visible: bool,
    pub shape: CursorShape,
}

/// Events the emulator surfaces to the application. Anything the grid cannot
/// act on itself travels through here instead.
///
/// Three of the emulator's requests — clipboard read, colour report, text-area
/// size — arrive with a formatter closure rather than a value, so they cannot
/// live in this `PartialEq` enum. They are queued as [`ScreenQuery`] and
/// announced by [`ScreenEvent::QueryPending`]; take them with
/// [`Screen::drain_queries`].
#[derive(Debug, Clone, PartialEq)]
pub enum ScreenEvent {
    Title(String),
    ResetTitle,
    Bell,
    CursorBlinkingChange,
    ClipboardStore(String),
    /// Bytes the emulator wants written back to the PTY (query replies).
    PtyWrite(String),
    /// The emulator is waiting for an answer; see [`Screen::drain_queries`].
    QueryPending,
    MouseCursorDirty,
    Wakeup,
    Exit,
    ChildExit(i32),
}

/// A question from the emulator that only the application can answer.
///
/// The answer goes back to the PTY in the escape-sequence form the emulator
/// expects, which is what the formatter produces.
#[derive(Clone)]
pub enum ScreenQuery {
    /// Write the current clipboard contents back, in the requested encoding.
    ClipboardLoad {
        format: Arc<dyn Fn(&str) -> String + Send + Sync>,
    },
    /// Report a palette colour (index, usually an OSC 4 index) as an RGB triple.
    ColorRequest {
        index: usize,
        format: Arc<dyn Fn(Rgb) -> String + Send + Sync>,
    },
    /// Report the text area size.
    TextAreaSize {
        format: Arc<dyn Fn(AlacrittyWindowSize) -> String + Send + Sync>,
    },
}

impl fmt::Debug for ScreenQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScreenQuery::ClipboardLoad { .. } => f.write_str("ClipboardLoad"),
            ScreenQuery::ColorRequest { index, .. } => f
                .debug_struct("ColorRequest")
                .field("index", index)
                .finish(),
            ScreenQuery::TextAreaSize { .. } => f.write_str("TextAreaSize"),
        }
    }
}

impl ScreenQuery {
    /// The bytes to write back for a clipboard read, or `None` if this is a
    /// different kind of query.
    pub fn clipboard_reply(&self, text: &str) -> Option<String> {
        match self {
            ScreenQuery::ClipboardLoad { format } => Some(format(text)),
            _ => None,
        }
    }

    /// The bytes to write back for a colour report.
    pub fn color_reply(&self, index: usize, rgb: (u8, u8, u8)) -> Option<String> {
        match self {
            ScreenQuery::ColorRequest {
                index: wanted,
                format,
            } if *wanted == index => {
                let (r, g, b) = rgb;
                Some(format(Rgb { r, g, b }))
            }
            _ => None,
        }
    }

    /// The bytes to write back for a text-area size request. Cell metrics are
    /// ours, not the emulator's, so they are passed in.
    pub fn text_area_reply(
        &self,
        columns: u16,
        screen_lines: u16,
        cell_width: u16,
        cell_height: u16,
    ) -> Option<String> {
        match self {
            ScreenQuery::TextAreaSize { format } => Some(format(AlacrittyWindowSize {
                num_cols: columns,
                num_lines: screen_lines,
                cell_width,
                cell_height,
            })),
            _ => None,
        }
    }

    /// The palette index a colour report is about.
    pub fn color_index(&self) -> Option<usize> {
        match self {
            ScreenQuery::ColorRequest { index, .. } => Some(*index),
            _ => None,
        }
    }
}

/// Collects [`ScreenEvent`]s from the emulator.
///
/// The listener is handed to the emulator by value and must be `&self`-callable,
/// so the queue lives behind a shared handle that the caller also holds.
#[derive(Debug, Clone, Default)]
pub struct ScreenEvents {
    queue: Arc<Mutex<Vec<ScreenEvent>>>,
    queries: Arc<Mutex<Vec<ScreenQuery>>>,
}

impl ScreenEvents {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take everything collected so far.
    pub fn drain(&self) -> Vec<ScreenEvent> {
        let mut queue = self.queue.lock().expect("screen event queue poisoned");
        std::mem::take(&mut *queue)
    }

    /// Take the pending questions, oldest first.
    pub fn drain_queries(&self) -> Vec<ScreenQuery> {
        let mut queries = self.queries.lock().expect("screen query queue poisoned");
        std::mem::take(&mut *queries)
    }

    pub fn query_count(&self) -> usize {
        self.queries.lock().map(|q| q.len()).unwrap_or(0)
    }

    fn push(&self, event: ScreenEvent) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.push(event);
        }
    }

    fn push_query(&self, query: ScreenQuery) {
        if let Ok(mut queries) = self.queries.lock() {
            queries.push(query);
        }
        self.push(ScreenEvent::QueryPending);
    }
}

impl EventListener for ScreenEvents {
    fn send_event(&self, event: AlacrittyEvent) {
        let mapped = match event {
            AlacrittyEvent::Title(title) => ScreenEvent::Title(title),
            AlacrittyEvent::ResetTitle => ScreenEvent::ResetTitle,
            AlacrittyEvent::ClipboardStore(_, text) => ScreenEvent::ClipboardStore(text),
            AlacrittyEvent::ClipboardLoad(_, format) => {
                self.push_query(ScreenQuery::ClipboardLoad { format });
                return;
            }
            AlacrittyEvent::PtyWrite(text) => ScreenEvent::PtyWrite(text),
            AlacrittyEvent::TextAreaSizeRequest(format) => {
                self.push_query(ScreenQuery::TextAreaSize { format });
                return;
            }
            AlacrittyEvent::ColorRequest(index, format) => {
                self.push_query(ScreenQuery::ColorRequest { index, format });
                return;
            }
            AlacrittyEvent::MouseCursorDirty => ScreenEvent::MouseCursorDirty,
            AlacrittyEvent::Wakeup => ScreenEvent::Wakeup,
            AlacrittyEvent::Bell => ScreenEvent::Bell,
            AlacrittyEvent::CursorBlinkingChange => ScreenEvent::CursorBlinkingChange,
            AlacrittyEvent::Exit => ScreenEvent::Exit,
            AlacrittyEvent::ChildExit(status) => {
                ScreenEvent::ChildExit(status.code().unwrap_or(-1))
            }
        };
        self.push(mapped);
    }
}

/// One terminal surface.
pub struct Screen {
    term: Term<ScreenEvents>,
    processor: Processor,
    listener: ScreenEvents,
    size: ScreenSize,
    /// Once frozen the screen is closed: its content is complete and further
    /// bytes are discarded instead of being interpreted.
    frozen: bool,
    dropped_bytes: usize,
}

impl fmt::Debug for Screen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Screen")
            .field("size", &self.size)
            .field("frozen", &self.frozen)
            .field("alt_screen", &self.is_alt_screen())
            .finish_non_exhaustive()
    }
}

impl Screen {
    /// A screen with the given size and policy.
    pub fn new(size: ScreenSize, config: ScreenConfig) -> Self {
        let listener = ScreenEvents::new();
        let config = Config {
            scrolling_history: config.history,
            osc52: if config.allow_clipboard_read {
                Osc52::CopyPaste
            } else {
                Osc52::OnlyCopy
            },
            ..Config::default()
        };
        let term = Term::new(config, &size, listener.clone());
        Self {
            term,
            processor: Processor::new(),
            listener,
            size,
            frozen: false,
            dropped_bytes: 0,
        }
    }

    pub fn size(&self) -> ScreenSize {
        self.size
    }

    /// Resize the grid. Content reflows, and because block screens keep
    /// scrollback nothing that was already printed is dropped.
    pub fn resize(&mut self, size: ScreenSize) {
        self.size = size;
        self.term.resize(size);
    }

    /// Feed bytes from the PTY. A frozen screen ignores them: its content is
    /// complete and nothing the shell writes now belongs to it.
    pub fn feed(&mut self, bytes: &[u8]) {
        if self.frozen {
            self.dropped_bytes += bytes.len();
            return;
        }
        self.processor.advance(&mut self.term, bytes);
    }

    /// Take the events the emulator produced while processing input.
    pub fn drain_events(&self) -> Vec<ScreenEvent> {
        self.listener.drain()
    }

    /// Take the questions the emulator is waiting on, oldest first.
    pub fn drain_queries(&self) -> Vec<ScreenQuery> {
        self.listener.drain_queries()
    }

    pub fn query_count(&self) -> usize {
        self.listener.query_count()
    }

    /// Reset like `RIS` (`ESC c`): blank grid, default modes, default cursor
    /// style, tabs back to their defaults. This is what the shell's `clear`
    /// builtin resolves to.
    pub fn reset(&mut self) {
        self.frozen = false;
        self.processor.advance(&mut self.term, b"\x1bc");
    }

    pub fn mode(&self) -> AlacrittyTermMode {
        *self.term.mode()
    }

    /// The subset of the terminal mode that changes key encoding.
    pub fn key_mode(&self) -> crate::keys::TermMode {
        crate::keys::TermMode {
            app_cursor: self.mode().contains(AlacrittyTermMode::APP_CURSOR),
        }
    }

    pub fn is_alt_screen(&self) -> bool {
        self.mode().contains(AlacrittyTermMode::ALT_SCREEN)
    }

    pub fn is_bracketed_paste(&self) -> bool {
        self.mode().contains(AlacrittyTermMode::BRACKETED_PASTE)
    }

    /// Rows scrolled back from the live screen.
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// Rows of scrollback currently held.
    pub fn history_size(&self) -> usize {
        let grid = self.term.grid();
        grid.total_lines().saturating_sub(grid.screen_lines())
    }

    /// Scroll the viewport; positive `lines` scrolls up into history.
    pub fn scroll_display(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// Cursor position within the viewport.
    pub fn cursor(&self) -> CursorState {
        let content = self.term.renderable_content();
        let point = point_to_viewport(content.display_offset, content.cursor.point);
        let (line, column) = match point {
            Some(point) => (point.line, point.column.0),
            None => (0, 0),
        };
        CursorState {
            line,
            column,
            visible: content.cursor.shape != CursorShape::Hidden,
            shape: content.cursor.shape,
        }
    }

    /// Every visible row, as the renderer wants it.
    pub fn lines(&self) -> Vec<ScreenLine> {
        let content = self.term.renderable_content();
        let mut lines: Vec<ScreenLine> = Vec::with_capacity(self.size.screen_lines);
        let mut current: Option<i32> = None;

        // The iterator walks visible cells in reading order, so a change of
        // line means the next row starts.
        for indexed in content.display_iter {
            let line = indexed.point.line.0;
            if current != Some(line) {
                lines.push(ScreenLine {
                    cells: Vec::with_capacity(self.size.columns),
                });
                current = Some(line);
            }
            if let Some(row) = lines.last_mut() {
                row.cells.push(map_cell(indexed.cell));
            }
        }

        lines
    }

    /// Every row the grid holds, oldest first: scrollback history and then the
    /// visible rows.
    ///
    /// This is what a block draws. A block shows everything its command printed,
    /// including the part that scrolled past the viewport, so the viewport is
    /// not the right thing to render from.
    pub fn content_lines(&self) -> Vec<ScreenLine> {
        let grid = self.term.grid();
        let history = grid.history_size();
        let mut lines = Vec::with_capacity(grid.total_lines());

        for row_index in 0..grid.total_lines() {
            // History rows have negative line numbers and sit above the screen.
            let line = Line(row_index as i32 - history as i32);
            let row = &grid[line];
            let mut cells = Vec::with_capacity(self.size.columns);
            for column in 0..self.size.columns {
                cells.push(map_cell(&row[Column(column)]));
            }
            lines.push(ScreenLine { cells });
        }

        lines
    }

    /// One visible row.
    pub fn line(&self, row: usize) -> Option<ScreenLine> {
        self.lines().into_iter().nth(row)
    }

    /// The visible screen as text, with trailing blank rows removed.
    pub fn text(&self) -> String {
        let mut lines: Vec<String> = self.lines().iter().map(|l| l.text()).collect();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }

    /// Close the screen: its content is complete.
    ///
    /// A frozen screen ignores further PTY bytes, so the next command's prompt
    /// cannot leak into this command's block. Nothing is truncated: the block
    /// was already showing its whole content.
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// Bytes discarded because the screen was already frozen.
    pub fn dropped_bytes(&self) -> usize {
        self.dropped_bytes
    }
}

fn map_cell(cell: &Cell) -> ScreenCell {
    ScreenCell {
        ch: cell.c,
        fg: map_color(cell.fg),
        bg: map_color(cell.bg),
        attrs: CellAttrs {
            bold: cell.flags.contains(Flags::BOLD),
            dim: cell.flags.contains(Flags::DIM),
            italic: cell.flags.contains(Flags::ITALIC),
            underline: underline_of(cell.flags),
            underline_color: cell.underline_color().map(map_color),
            inverse: cell.flags.contains(Flags::INVERSE),
            hidden: cell.flags.contains(Flags::HIDDEN),
            strikeout: cell.flags.contains(Flags::STRIKEOUT),
            wide: cell.flags.contains(Flags::WIDE_CHAR),
            wide_spacer: cell.flags.contains(Flags::WIDE_CHAR_SPACER),
            leading_wide_spacer: cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER),
            wrapline: cell.flags.contains(Flags::WRAPLINE),
        },
        zerowidth: cell.zerowidth().map(<[char]>::to_vec).unwrap_or_default(),
        hyperlink: cell.hyperlink().map(|link| link.uri().to_owned()),
    }
}

fn underline_of(flags: Flags) -> Underline {
    if flags.contains(Flags::UNDERCURL) {
        Underline::Curly
    } else if flags.contains(Flags::DOTTED_UNDERLINE) {
        Underline::Dotted
    } else if flags.contains(Flags::DASHED_UNDERLINE) {
        Underline::Dashed
    } else if flags.contains(Flags::DOUBLE_UNDERLINE) {
        Underline::Double
    } else if flags.contains(Flags::UNDERLINE) {
        Underline::Single
    } else {
        Underline::None
    }
}

fn map_color(color: AlacrittyColor) -> ScreenColor {
    match color {
        AlacrittyColor::Named(named) => ScreenColor::Named(named),
        AlacrittyColor::Indexed(index) => ScreenColor::Indexed(index),
        AlacrittyColor::Spec(Rgb { r, g, b }) => ScreenColor::Rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(columns: usize, lines: usize, history: usize) -> Screen {
        Screen::new(
            ScreenSize::new(columns, lines),
            ScreenConfig::scrolling(history),
        )
    }

    #[test]
    fn prints_text() {
        let mut s = screen(20, 3, 0);
        s.feed(b"hello");
        assert_eq!(s.text(), "hello");
        assert_eq!(s.cursor().column, 5);
        assert_eq!(s.cursor().line, 0);
        assert!(s.cursor().visible);
    }

    #[test]
    fn interprets_cursor_addressing() {
        let mut s = screen(20, 5, 0);
        s.feed(b"one\x1b[3;5Htwo");
        let lines: Vec<String> = s.lines().iter().map(|l| l.text()).collect();
        assert_eq!(lines[0], "one");
        assert_eq!(lines[2], "    two");
    }

    #[test]
    fn interprets_sgr_colour_and_attributes() {
        let mut s = screen(20, 2, 0);
        s.feed(b"\x1b[1;31mred\x1b[0m");
        let line = s.line(0).unwrap();
        assert_eq!(line.cells[0].ch, 'r');
        assert!(line.cells[0].attrs.bold);
        // `SGR 31` is the palette's red (index 1), which the palette names.
        assert_eq!(line.cells[0].fg, ScreenColor::Named(NamedColor::Red));
        // After the reset the attributes are gone.
        s.feed(b"plain");
        let line = s.line(0).unwrap();
        assert_eq!(line.cells[3].ch, 'p');
        assert!(!line.cells[3].attrs.bold);
        assert_eq!(line.cells[3].fg, ScreenColor::Named(NamedColor::Foreground));
    }

    #[test]
    fn indexed_colour_survives_as_an_index() {
        let mut s = screen(20, 2, 0);
        s.feed(b"\x1b[38;5;208mx");
        assert_eq!(s.line(0).unwrap().cells[0].fg, ScreenColor::Indexed(208));
    }

    #[test]
    fn truecolor_is_reported_verbatim() {
        let mut s = screen(10, 2, 0);
        s.feed(b"\x1b[38;2;18;52;86mx");
        assert_eq!(s.line(0).unwrap().cells[0].fg, ScreenColor::Rgb(18, 52, 86));
    }

    #[test]
    fn wide_characters_occupy_two_cells() {
        let mut s = screen(10, 2, 0);
        s.feed("日本".as_bytes());
        let line = s.line(0).unwrap();
        assert_eq!(line.cells[0].ch, '日');
        assert!(line.cells[0].attrs.wide);
        assert!(line.cells[1].attrs.wide_spacer);
        assert_eq!(line.cells[2].ch, '本');
        // The spacer is not part of the text.
        assert_eq!(line.text(), "日本");
    }

    #[test]
    fn combining_marks_stay_with_their_cell() {
        let mut s = screen(10, 2, 0);
        s.feed("e\u{0301}".as_bytes());
        let line = s.line(0).unwrap();
        assert_eq!(line.cells[0].ch, 'e');
        assert_eq!(line.cells[0].zerowidth, vec!['\u{0301}']);
        assert_eq!(line.text(), "e\u{0301}");
    }

    #[test]
    fn alt_screen_swaps_and_restores() {
        let mut s = screen(20, 3, 0);
        s.feed(b"primary");
        s.feed(b"\x1b[?1049h");
        assert!(s.is_alt_screen());
        assert_eq!(s.text(), "", "the alt screen starts blank");
        // Full-screen apps clear and home the cursor themselves, as vim does.
        s.feed(b"\x1b[2J\x1b[Hfullscreen");
        assert_eq!(s.text(), "fullscreen");
        s.feed(b"\x1b[?1049l");
        assert!(!s.is_alt_screen());
        assert_eq!(s.text(), "primary");
    }

    #[test]
    fn alt_screen_has_no_scrollback() {
        let mut s = screen(10, 2, 0);
        s.feed(b"\x1b[?1049h");
        for i in 0..10 {
            s.feed(format!("line{i}\r\n").as_bytes());
        }
        assert_eq!(s.history_size(), 0);
    }

    #[test]
    fn scrollback_grows_when_enabled() {
        let mut s = screen(20, 2, 100);
        for i in 0..10 {
            s.feed(format!("line{i}\r\n").as_bytes());
        }
        assert!(s.history_size() > 0);
        s.scroll_display(1);
        assert_eq!(s.display_offset(), 1);
        s.scroll_to_bottom();
        assert_eq!(s.display_offset(), 0);
    }

    #[test]
    fn bracketed_paste_mode_is_visible_to_the_input_layer() {
        let mut s = screen(10, 2, 0);
        assert!(!s.is_bracketed_paste());
        s.feed(b"\x1b[?2004h");
        assert!(s.is_bracketed_paste());
    }

    #[test]
    fn application_cursor_mode_is_reported_for_key_encoding() {
        let mut s = screen(10, 2, 0);
        assert!(!s.key_mode().app_cursor);
        s.feed(b"\x1b[?1h");
        assert!(s.key_mode().app_cursor);
    }

    #[test]
    fn resize_reflows_and_keeps_content() {
        let mut s = screen(20, 6, 100);
        s.feed(b"the quick brown fox jumps");
        assert_eq!(s.text(), "the quick brown fox\njumps");
        s.resize(ScreenSize::new(10, 6));
        let text = s.text();
        assert!(text.contains("brown fox"), "{text:?}");
        assert!(text.contains("jumps"), "{text:?}");
    }

    #[test]
    fn osc_8_hyperlinks_are_exposed() {
        let mut s = screen(20, 2, 0);
        s.feed(b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\");
        let cell = &s.line(0).unwrap().cells[0];
        assert_eq!(cell.ch, 'l');
        assert_eq!(cell.hyperlink.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn events_reach_the_listener() {
        let mut s = screen(20, 2, 0);
        s.feed(b"\x1b]0;a title\x07");
        s.feed(b"\x07");
        let events = s.drain_events();
        assert!(
            events.contains(&ScreenEvent::Title("a title".into())),
            "{events:?}"
        );
        assert!(events.contains(&ScreenEvent::Bell), "{events:?}");
        assert!(s.drain_events().is_empty(), "events are drained once");
    }

    #[test]
    fn a_frozen_screen_ignores_further_output() {
        let mut s = screen(20, 5, 0);
        s.feed(b"result");
        s.freeze();
        s.feed(b"\r\nnext prompt");
        assert_eq!(s.text(), "result");
        assert!(s.is_frozen());
        assert!(s.dropped_bytes() > 0);
    }

    #[test]
    fn freezing_twice_is_harmless() {
        let mut s = screen(20, 5, 0);
        s.feed(b"a\r\nb");
        s.freeze();
        s.freeze();
        assert_eq!(s.text(), "a\nb");
    }

    #[test]
    fn a_block_screen_keeps_output_that_scrolled_past_the_viewport() {
        let mut s = Screen::new(ScreenSize::new(20, 3), ScreenConfig::block());
        for index in 0..10 {
            s.feed(format!("line{index}\r\n").as_bytes());
        }
        // The viewport shows the tail...
        assert!(!s.text().contains("line0"), "{:?}", s.text());
        // ...but the block draws its whole grid.
        let content: Vec<String> = s.content_lines().iter().map(ScreenLine::text).collect();
        assert_eq!(content[0], "line0");
        assert_eq!(content[9], "line9");
        assert_eq!(
            s.history_size(),
            content.len() - 3,
            "history and screen are contiguous"
        );
    }

    #[test]
    fn content_survives_a_shrink_that_needs_more_rows() {
        let mut s = Screen::new(ScreenSize::new(20, 3), ScreenConfig::block());
        s.feed(b"a rather long line that has to wrap once the screen is narrow");
        s.resize(ScreenSize::new(10, 3));
        // Rows are hard-wrapped, so joining them back recovers the whole line.
        let joined: String = s.content_lines().iter().map(ScreenLine::text).collect();
        assert_eq!(
            joined,
            "a rather long line that has to wrap once the screen is narrow"
        );
    }

    #[test]
    fn clipboard_read_keeps_the_formatter() {
        let mut s = Screen::new(
            ScreenSize::new(20, 2),
            ScreenConfig::scrolling(0).allow_clipboard_read(),
        );
        // OSC 52 with an empty payload and `?` asks for the clipboard.
        s.feed(b"\x1b]52;c;?\x07");
        let queries = s.drain_queries();
        assert_eq!(queries.len(), 1, "{queries:?}");
        let reply = queries[0]
            .clipboard_reply("copied")
            .expect("a clipboard query");
        assert!(
            reply.contains("Y29waWVk"),
            "base64 of the selection: {reply:?}"
        );
        assert_eq!(s.query_count(), 0, "queries are drained once");
    }

    #[test]
    fn colour_report_keeps_the_formatter_and_the_index() {
        let mut s = screen(20, 2, 0);
        s.feed(b"\x1b]4;1;?\x07");
        let queries = s.drain_queries();
        let query = queries.first().expect("a colour query");
        assert_eq!(query.color_index(), Some(1));
        assert!(
            query.color_reply(2, (1, 2, 3)).is_none(),
            "the wrong index is not answered"
        );
        let reply = query
            .color_reply(1, (255, 0, 0))
            .expect("the requested index");
        assert!(reply.contains("ffff/0000/0000"), "{reply:?}");
    }

    #[test]
    fn query_pending_is_announced_as_an_event() {
        let mut s = screen(20, 2, 0);
        s.feed(b"\x1b]4;1;?\x07");
        assert!(s.drain_events().contains(&ScreenEvent::QueryPending));
    }

    #[test]
    fn clipboard_read_is_denied_unless_enabled() {
        let mut s = screen(20, 2, 0);
        s.feed(b"\x1b]52;c;?\x07");
        assert!(s.drain_queries().is_empty());
        assert!(!s.drain_events().contains(&ScreenEvent::QueryPending));
    }

    #[test]
    fn reset_blanks_the_grid_and_restores_modes() {
        let mut s = screen(20, 3, 0);
        s.feed(b"\x1b[?1h\x1b[31mred text");
        assert!(s.key_mode().app_cursor);
        s.reset();
        assert_eq!(s.text(), "");
        assert!(!s.key_mode().app_cursor);
        assert_eq!(s.cursor().line, 0);
        assert_eq!(s.cursor().column, 0);
    }
}
