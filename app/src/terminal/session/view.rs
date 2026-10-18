use goble_terminal::{
    CursorState, ImagePlacement, Palette, ScreenLine, SearchError, SearchMatch, SearchOptions,
    TermMode,
};

use crate::terminal::copy::{selection_text, Cell, Selection};
use crate::terminal::TerminalSnapshot;

use super::TerminalSession;

/// Everything the pane needs to paint one frame of a session.
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalViewState {
    /// The visible rows, top row first.
    pub rows: Vec<ScreenLine>,
    /// Image placements in the same viewport, anchored to cells.
    pub images: Vec<ImagePlacement>,
    pub cursor: CursorState,
    /// How the program wants keys and pointer reports encoded.
    pub mode: TermMode,
    pub alt_screen: bool,
    /// Lines scrolled back from the bottom; 0 is the live screen.
    pub display_offset: usize,
    pub scrollback: usize,
    pub title: Option<String>,
    /// Whether any cell holds something. A session that has not drawn yet gets
    /// the pane's own hint line instead of a black rectangle.
    pub has_content: bool,
}

impl TerminalSession {
    /// This session's grid size, in cells.
    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// Reshape the screen and the pty, and remember the font metrics the
    /// pixel-sized reports need.
    pub fn set_size(&mut self, rows: u16, cols: u16, cell_width: u16, cell_height: u16) {
        self.cell = (cell_width, cell_height);
        if (rows, cols) == self.size || rows == 0 || cols == 0 {
            return;
        }
        self.size = (rows, cols);

        if let Ok(mut state) = self.state.lock() {
            state.resize(cols as usize, rows as usize);
        }
        #[cfg(unix)]
        self.resize_daemon(rows, cols, cell_width, cell_height);
    }

    /// Reshape the grid to `cols` × `rows`, keeping the font cell metrics the
    /// pane last reported. The control surface resizes by cells only.
    pub fn set_grid_size(&mut self, rows: u16, cols: u16) {
        let (cell_width, cell_height) = self.cell;
        self.set_size(rows, cols, cell_width, cell_height);
    }

    /// The palette the pane paints with, which is also what colour queries are
    /// answered from.
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    /// How the running program wants keys and pointer reports encoded.
    pub fn input_mode(&self) -> TermMode {
        self.state
            .lock()
            .map(|state| state.input_mode())
            .unwrap_or(TermMode::NONE)
    }

    /// Clone what the next frame paints: the rows, the cursor and the modes.
    pub fn view(&self) -> TerminalViewState {
        let Ok(state) = self.state.lock() else {
            return TerminalViewState::empty();
        };
        let rows = state.rows();
        let has_content = rows.iter().any(|row| !row.is_blank());
        TerminalViewState {
            rows,
            images: state.images(),
            cursor: state.cursor(),
            mode: state.input_mode(),
            alt_screen: state.is_alt_screen(),
            display_offset: state.display_offset(),
            scrollback: state.history_size(),
            title: self.title.clone(),
            has_content,
        }
    }

    /// Scroll the display through the scrollback (`0` is the live bottom).
    pub fn scroll(&mut self, lines: i32) {
        if let Ok(mut state) = self.state.lock() {
            state.scroll(lines);
        }
    }

    /// Rows of scrollback held above the visible screen.
    pub fn history_len(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.history_len())
            .unwrap_or(0)
    }

    /// Every row the screen holds: history followed by the visible screen.
    pub fn total_lines(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.total_lines())
            .unwrap_or(0)
    }

    /// Changes whenever the grid's content changes.
    pub fn revision(&self) -> u64 {
        self.state.lock().map(|state| state.revision()).unwrap_or(0)
    }

    /// One row by absolute index, oldest first.
    pub fn line_at(&self, index: usize) -> Option<ScreenLine> {
        self.state.lock().ok().and_then(|state| state.line_at(index))
    }

    /// The last occupied column count of a row.
    pub fn line_len(&self, index: usize) -> usize {
        self.state
            .lock()
            .map(|state| state.line_len(index))
            .unwrap_or(0)
    }

    /// Search history and the visible screen.
    pub fn search(
        &self,
        query: &str,
        options: SearchOptions,
    ) -> Result<Vec<SearchMatch>, SearchError> {
        self.state
            .lock()
            .map(|state| state.search(query, options))
            .unwrap_or(Ok(Vec::new()))
    }

    /// Where copy mode starts: the shell's cursor, clamped to the content.
    pub fn copy_start_cell(&self) -> Cell {
        self.state
            .lock()
            .map(|state| {
                let (line, column) = state.copy_start_cell();
                Cell::new(line, column)
            })
            .unwrap_or_default()
    }

    /// Convert a viewport row into an absolute content row.
    pub fn absolute_line(&self, row: usize) -> Option<usize> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.absolute_line(row))
    }

    /// Scroll until `line` is visible.
    pub fn scroll_to_line(&mut self, line: usize) {
        if let Ok(mut state) = self.state.lock() {
            state.scroll_to_line(line);
        }
    }

    /// The text a selection covers.
    pub fn selection_text(&self, selection: Selection) -> String {
        let Ok(state) = self.state.lock() else {
            return String::new();
        };
        selection_text(selection, |index| state.line_at(index))
    }

    /// Coalesce the newest output into a snapshot for painting.
    ///
    /// The last element is the bottom visible row, as a line buffer's would be;
    /// trailing blank rows are dropped so an idle shell contributes no block.
    pub fn snapshot(&self, max_lines: usize) -> TerminalSnapshot {
        let Ok(state) = self.state.lock() else {
            return TerminalSnapshot { lines: Vec::new() };
        };
        let mut lines: Vec<String> = state.rows().iter().map(|line| line.text()).collect();
        while lines.last().is_some_and(|line| line.is_empty()) {
            lines.pop();
        }
        if lines.len() > max_lines {
            lines.drain(..lines.len() - max_lines);
        }
        TerminalSnapshot { lines }
    }
}

impl TerminalViewState {
    /// The state of a session whose lock could not be taken: nothing to paint.
    pub fn empty() -> Self {
        Self {
            rows: Vec::new(),
            images: Vec::new(),
            cursor: CursorState {
                line: 0,
                column: 0,
                visible: false,
                shape: goble_terminal::CursorShape::Block,
            },
            mode: TermMode::NONE,
            alt_screen: false,
            display_offset: 0,
            scrollback: 0,
            title: None,
            has_content: false,
        }
    }
}
