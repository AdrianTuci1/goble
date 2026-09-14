use std::fmt;

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{
    point_to_viewport, Config, Osc52, Term, TermMode as AlacrittyTermMode,
};
use alacritty_terminal::vte::ansi::{
    Color as AlacrittyColor, CursorShape as AlacrittyCursorShape, Processor, Rgb,
};

use super::cell::{CellAttrs, ScreenCell, ScreenColor, ScreenLine, Underline};
use super::config::ScreenConfig;
use super::cursor::{CursorShape, CursorState};
use super::event::{ScreenEvent, ScreenEvents, ScreenQuery};
use super::size::ScreenSize;

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

    /// The parts of the terminal mode the input layer reads: how a key is
    /// encoded, whether focus changes are reported, and whether the mouse is
    /// reported at all. See [`crate::keys::TermMode`].
    pub fn input_mode(&self) -> crate::keys::TermMode {
        let mode = self.mode();
        crate::keys::TermMode {
            app_cursor: mode.contains(AlacrittyTermMode::APP_CURSOR),
            bracketed_paste: mode.contains(AlacrittyTermMode::BRACKETED_PASTE),
            focus_reporting: mode.contains(AlacrittyTermMode::FOCUS_IN_OUT),
            mouse_click: mode.contains(AlacrittyTermMode::MOUSE_REPORT_CLICK),
            mouse_drag: mode.contains(AlacrittyTermMode::MOUSE_DRAG),
            mouse_motion: mode.contains(AlacrittyTermMode::MOUSE_MOTION),
            sgr_mouse: mode.contains(AlacrittyTermMode::SGR_MOUSE),
        }
    }

    /// The deferred-wrap flag: the last column was written, so the next
    /// character starts a new row instead of overwriting the cell.
    ///
    /// This is not visible in the grid — the cursor still sits in the last
    /// column — which is exactly why the conformance harness asserts it.
    pub fn wrap_pending(&self) -> bool {
        self.term.grid().cursor.input_needs_wrap
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
            visible: content.cursor.shape != AlacrittyCursorShape::Hidden,
            shape: map_cursor_shape(content.cursor.shape),
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

fn map_cursor_shape(shape: AlacrittyCursorShape) -> CursorShape {
    match shape {
        AlacrittyCursorShape::Block => CursorShape::Block,
        AlacrittyCursorShape::HollowBlock => CursorShape::HollowBlock,
        AlacrittyCursorShape::Beam => CursorShape::Beam,
        AlacrittyCursorShape::Underline => CursorShape::Underline,
        AlacrittyCursorShape::Hidden => CursorShape::Hidden,
    }
}
