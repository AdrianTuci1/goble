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

use crate::image::{ImageCache, ImageChunk, ImageDelete, ImagePlacement, ImageTap};

use super::cell::{CellAttrs, ScreenCell, ScreenColor, ScreenLine, Underline};
use super::config::ScreenConfig;
use super::cursor::{CursorShape, CursorState};
use super::event::{ScreenEvent, ScreenEvents, ScreenQuery};
use super::search::{search_lines, SearchError, SearchMatch, SearchOptions};
use super::size::ScreenSize;

/// One terminal surface.
pub struct Screen {
    term: Term<ScreenEvents>,
    processor: Processor,
    listener: ScreenEvents,
    size: ScreenSize,
    /// The image sequences the VT parser does not understand, and the bounded
    /// cache of decoded placements this screen has seen.
    image_tap: ImageTap,
    images: ImageCache,
    /// Once frozen the screen is closed: its content is complete and further
    /// bytes are discarded instead of being interpreted.
    frozen: bool,
    dropped_bytes: usize,
    /// Bumped whenever the grid's content changes, so a search can tell whether
    /// the results it holds are still about the screen it searched.
    revision: u64,
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
            image_tap: ImageTap::new(),
            images: ImageCache::default(),
            frozen: false,
            dropped_bytes: 0,
            revision: 0,
        }
    }

    /// A counter that changes whenever the grid's content changes. A search
    /// caches its matches against it so new output invalidates them.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn size(&self) -> ScreenSize {
        self.size
    }

    /// Resize the grid. Content reflows, and because block screens keep
    /// scrollback nothing that was already printed is dropped.
    pub fn resize(&mut self, size: ScreenSize) {
        self.size = size;
        self.term.resize(size);
        self.revision = self.revision.wrapping_add(1);
    }

    /// Feed bytes from the PTY. A frozen screen ignores them: its content is
    /// complete and nothing the shell writes now belongs to it.
    pub fn feed(&mut self, bytes: &[u8]) {
        if self.frozen {
            self.dropped_bytes += bytes.len();
            return;
        }
        // Image sequences are lifted out in stream order. Each preceding byte
        // chunk is parsed first, so an image is placed at the cursor cell the
        // program had reached when it emitted the sequence.
        for chunk in self.image_tap.tap(bytes) {
            match chunk {
                ImageChunk::Bytes(bytes) => self.advance(&bytes),
                ImageChunk::Image(image) => {
                    let (row, column) = self.placement_cell();
                    self.images.insert(*image, row, column);
                }
                ImageChunk::Delete(delete) => self.delete_images(delete),
            }
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Advance the VT parser, clearing image placements when the program swaps
    /// the primary and alternate screens. The two grids are separate surfaces:
    /// an image placed on one must not be painted over the other when the
    /// program switches, or a TUI would draw the shell's images over itself.
    fn advance(&mut self, bytes: &[u8]) {
        let alt_screen = self.is_alt_screen();
        self.processor.advance(&mut self.term, bytes);
        if self.is_alt_screen() != alt_screen {
            self.images.clear();
        }
    }

    /// Apply a kitty delete to the placements this screen holds.
    fn delete_images(&mut self, delete: ImageDelete) {
        match delete {
            ImageDelete::Image(id) => self.images.remove(id),
            // The cache holds placements only; transmitted pixels live on the
            // tap, so both all-scopes delete the same entries here.
            ImageDelete::All | ImageDelete::AllPlacements => self.images.clear(),
        }
    }

    /// The image's anchor as an absolute content row and a column. The garbage
    /// collection of old history shifts a row's index only when it is dropped,
    /// so a placement that survives remains anchored to the text it was drawn
    /// beside.
    fn placement_cell(&self) -> (usize, usize) {
        let content = self.term.renderable_content();
        let line = content.cursor.point.line.0.max(0) as usize;
        (
            self.history_size().saturating_add(line),
            content.cursor.point.column.0,
        )
    }

    /// The image placements visible in the current viewport, top row first.
    pub fn images(&self) -> Vec<ImagePlacement> {
        self.images.visible(
            self.history_size(),
            self.display_offset(),
            self.size.screen_lines,
        )
    }

    /// The bounded image cache, including placements the viewport has scrolled
    /// past.
    pub fn image_cache(&self) -> &ImageCache {
        &self.images
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
        self.image_tap = ImageTap::new();
        self.images.clear();
        self.processor.advance(&mut self.term, b"\x1bc");
        self.revision = self.revision.wrapping_add(1);
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

    /// Rows of scrollback currently held. The name a view reads when it wants
    /// the length of the history rather than a grid detail.
    pub fn history_len(&self) -> usize {
        self.history_size()
    }

    /// Every row the grid holds: history followed by the visible screen.
    pub fn total_lines(&self) -> usize {
        self.term.grid().total_lines()
    }

    /// One row by absolute index, oldest first.
    ///
    /// `0` is the oldest row the grid still holds and [`Self::history_len`] is
    /// the first row of the live screen. `None` past the grid's end.
    pub fn line_at(&self, index: usize) -> Option<ScreenLine> {
        let grid = self.term.grid();
        if index >= grid.total_lines() {
            return None;
        }
        let history = grid.history_size();
        // History rows have negative line numbers and sit above the screen.
        let line = Line(index as i32 - history as i32);
        let row = &grid[line];
        let mut cells = Vec::with_capacity(self.size.columns);
        for column in 0..self.size.columns {
            cells.push(map_cell(&row[Column(column)]));
        }
        Some(ScreenLine { cells })
    }

    /// The row's text by absolute index, or `None` past the grid's end.
    pub fn line_text_at(&self, index: usize) -> Option<String> {
        self.line_at(index).map(|line| line.text())
    }

    /// Search history and the visible screen.
    ///
    /// Results are in absolute line order (see [`SearchMatch::line`]). An empty
    /// query matches nothing, and only regex mode can fail.
    pub fn search(
        &self,
        query: &str,
        options: SearchOptions,
    ) -> Result<Vec<SearchMatch>, SearchError> {
        let lines: Vec<ScreenLine> = (0..self.total_lines())
            .filter_map(|index| self.line_at(index))
            .collect();
        search_lines(&lines, query, options)
    }

    /// The visible row `line` is drawn on, if it is in the viewport.
    pub fn visible_row_of(&self, line: usize) -> Option<usize> {
        let history = self.history_size();
        let top = history.saturating_sub(self.display_offset());
        let row = line.checked_sub(top)?;
        (row < self.size.screen_lines).then_some(row)
    }

    /// Scroll the viewport so `line` is visible, leaving it alone when it
    /// already is. A line above the viewport lands in the top row; one below
    /// lands in the bottom row.
    pub fn scroll_to_line(&mut self, line: usize) {
        let history = self.history_size();
        let screen_lines = self.size.screen_lines.max(1);
        let offset = self.display_offset();
        let top = history.saturating_sub(offset);
        let bottom = top.saturating_add(screen_lines - 1);
        if line < top {
            let delta = top - line;
            self.scroll_display(i32::try_from(delta).unwrap_or(i32::MAX));
        } else if line > bottom {
            let delta = line.saturating_sub(bottom);
            self.scroll_display(-i32::try_from(delta).unwrap_or(i32::MAX));
        }
    }

    /// The cursor's absolute row over history and the visible screen.
    pub fn cursor_line(&self) -> usize {
        let history = self.history_size();
        let top = history.saturating_sub(self.display_offset());
        top.saturating_add(self.cursor().line)
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
