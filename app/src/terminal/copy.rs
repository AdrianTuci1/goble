//! Copy mode, selection and the scrollback find bar.
//!
//! The state here is app-owned and keyed by pane in [`TerminalRegistry`], for
//! the same reason the scroll offsets are: the element tree is rebuilt every
//! frame. The maths — a range between two cells, a word under a column, a line
//! range, the copy-mode motions and the key classifiers — is pure and testable
//! without a pty or an element tree.

use std::time::{Duration, Instant};

use goble_terminal::blocks::BlockView;
use goble_terminal::{ScreenCell, ScreenLine, SearchError, SearchMatch, SearchOptions};
use goble_ui::event::ModifiersState;

use super::TerminalRegistry;

/// How long two presses on the same cell are one multi-click gesture.
pub const MULTI_CLICK_INTERVAL: Duration = Duration::from_millis(400);

/// A cell in a screen's content: an absolute row over history and the visible
/// screen, and a column in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct Cell {
    pub line: usize,
    pub column: usize,
}

impl Cell {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// A selection from an anchor to the moving end. The range is inclusive, and
/// the two ends are ordered on demand, so a drag in either direction is the
/// same range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Cell,
    pub cursor: Cell,
}

impl Selection {
    pub const fn new(anchor: Cell, cursor: Cell) -> Self {
        Self { anchor, cursor }
    }

    /// The end earlier in reading order and the end later. Both are included.
    pub fn ordered(&self) -> (Cell, Cell) {
        if (self.anchor.line, self.anchor.column) <= (self.cursor.line, self.cursor.column) {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }

    pub fn contains(&self, cell: Cell) -> bool {
        let (start, end) = self.ordered();
        (start.line, start.column) <= (cell.line, cell.column)
            && (cell.line, cell.column) <= (end.line, end.column)
    }

    /// The first and last cell, in reading order.
    pub fn range(&self) -> (Cell, Cell) {
        self.ordered()
    }

    /// Whether the selection covers more than the one cell a plain click
    /// touches. A drag, a double-clicked word and a triple-clicked line all
    /// do; a click is not a copy.
    pub fn spans_more_than_one_cell(&self) -> bool {
        let (start, end) = self.ordered();
        start != end
    }
}

/// A character cell's place on a line: its column and how many columns it
/// occupies. Wide characters take two; combining marks take none.
struct LineCell {
    column: usize,
    width: usize,
    ch: char,
}

/// A text line as the screen-line shape the word and line helpers read.
fn text_line(text: &str) -> ScreenLine {
    ScreenLine {
        cells: text
            .chars()
            .map(|ch| ScreenCell {
                ch,
                ..Default::default()
            })
            .collect(),
    }
}

fn line_cells(line: &ScreenLine) -> Vec<LineCell> {
    let mut cells = Vec::new();
    for (column, cell) in line.cells.iter().enumerate() {
        if cell.attrs.wide_spacer || cell.attrs.leading_wide_spacer {
            continue;
        }
        cells.push(LineCell {
            column,
            width: if cell.attrs.wide { 2 } else { 1 },
            ch: cell.ch,
        });
    }
    cells
}

/// How a word character is grouped: whitespace separates everything, word
/// characters run together, and a run of punctuation is its own word.
fn word_class(ch: char) -> u8 {
    if ch.is_whitespace() {
        0
    } else if ch.is_alphanumeric() || ch == '_' {
        1
    } else {
        2
    }
}

/// The word under `column`, as an inclusive selection on `line`.
pub fn word_selection(line_index: usize, line: &ScreenLine, column: usize) -> Option<Selection> {
    let cells = line_cells(line);
    let index = cells
        .iter()
        .position(|cell| cell.column <= column && column < cell.column + cell.width)?;
    let class = word_class(cells[index].ch);
    if class == 0 {
        return None;
    }
    let mut first = index;
    while first > 0 && word_class(cells[first - 1].ch) == class {
        first -= 1;
    }
    let mut last = index;
    while last + 1 < cells.len() && word_class(cells[last + 1].ch) == class {
        last += 1;
    }
    let start = cells[first].column;
    let end = cells[last].column + cells[last].width - 1;
    Some(Selection::new(
        Cell::new(line_index, start),
        Cell::new(line_index, end),
    ))
}

/// The whole line, with trailing blanks left out.
pub fn line_selection(line_index: usize, line: &ScreenLine) -> Selection {
    let end = line.cells.iter().enumerate().fold(0, |end, (column, cell)| {
        if cell.ch == ' ' && cell.zerowidth.is_empty() {
            end
        } else {
            column + if cell.attrs.wide { 2 } else { 1 }
        }
    });
    Selection::new(
        Cell::new(line_index, 0),
        Cell::new(line_index, end.saturating_sub(1)),
    )
}

/// The text a selection covers, one row per line.
///
/// Trailing blanks on each line are dropped, which is what a yank or a copy of
/// a triple-clicked line should produce.
pub fn selection_text(
    selection: Selection,
    mut line_at: impl FnMut(usize) -> Option<ScreenLine>,
) -> String {
    let (start, end) = selection.ordered();
    let mut out = Vec::new();
    for line_index in start.line..=end.line {
        let Some(line) = line_at(line_index) else {
            break;
        };
        let from = if line_index == start.line {
            start.column
        } else {
            0
        };
        let to = if line_index == end.line {
            end.column
        } else {
            usize::MAX
        };
        let mut text = String::new();
        for (column, cell) in line.cells.iter().enumerate() {
            if cell.attrs.wide_spacer || cell.attrs.leading_wide_spacer {
                continue;
            }
            if column < from || column > to {
                continue;
            }
            text.push(cell.ch);
            for mark in &cell.zerowidth {
                text.push(*mark);
            }
        }
        out.push(text.trim_end().to_string());
    }
    out.join("\n")
}

/// A copy-mode motion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyMotion {
    Up,
    Down,
    Left,
    Right,
    Top,
    Bottom,
    LineStart,
    LineEnd,
}

/// What a key means while copy mode is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyAction {
    Move(CopyMotion),
    /// Start or end a visual selection at the cursor.
    Visual,
    /// Copy the visual selection (or the cursor's line) and leave copy mode.
    Yank,
    /// Open the find bar from copy mode.
    Search,
    /// Leave copy mode.
    Exit,
    Ignore,
}

/// A pane's copy-mode cursor and visual anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CopyMode {
    pub active: bool,
    pub cursor: Cell,
    pub visual: Option<Cell>,
}

impl CopyMode {
    pub fn enter(cursor: Cell) -> Self {
        Self {
            active: true,
            cursor,
            visual: None,
        }
    }

    pub fn selection(&self) -> Option<Selection> {
        self.visual.map(|anchor| Selection::new(anchor, self.cursor))
    }

    fn clamp_column(&mut self, line_len: usize) {
        self.cursor.column = if line_len == 0 {
            0
        } else {
            self.cursor.column.min(line_len - 1)
        };
    }

    /// Apply a motion within `total_lines` rows, asking `line_len` for a row's
    /// last occupied column count.
    pub fn apply(
        &mut self,
        motion: CopyMotion,
        total_lines: usize,
        mut line_len: impl FnMut(usize) -> usize,
    ) {
        let last_line = total_lines.saturating_sub(1);
        match motion {
            CopyMotion::Up => {
                self.cursor.line = self.cursor.line.saturating_sub(1);
                self.clamp_column(line_len(self.cursor.line));
            }
            CopyMotion::Down => {
                self.cursor.line = self.cursor.line.saturating_add(1).min(last_line);
                self.clamp_column(line_len(self.cursor.line));
            }
            CopyMotion::Left => {
                if self.cursor.column > 0 {
                    self.cursor.column -= 1;
                } else if self.cursor.line > 0 {
                    self.cursor.line -= 1;
                    self.cursor.column = line_len(self.cursor.line).saturating_sub(1);
                }
            }
            CopyMotion::Right => {
                let len = line_len(self.cursor.line);
                if self.cursor.column + 1 < len {
                    self.cursor.column += 1;
                } else if self.cursor.line < last_line {
                    self.cursor.line += 1;
                    self.cursor.column = 0;
                }
            }
            CopyMotion::Top => {
                self.cursor.line = 0;
                self.clamp_column(line_len(0));
            }
            CopyMotion::Bottom => {
                self.cursor.line = last_line;
                self.clamp_column(line_len(last_line));
            }
            CopyMotion::LineStart => {
                self.cursor.column = 0;
            }
            CopyMotion::LineEnd => {
                self.cursor.column = line_len(self.cursor.line).saturating_sub(1);
            }
        }
    }
}

/// What a key means while copy mode is on. A command/control/alt chord is left
/// for the pane's own chords; the bare keys are copy mode's.
pub fn classify_copy_key(key: &str, modifiers: ModifiersState) -> CopyAction {
    if modifiers.command || modifiers.ctrl || modifiers.alt {
        return CopyAction::Ignore;
    }
    match key {
        "h" | "ArrowLeft" => CopyAction::Move(CopyMotion::Left),
        "j" | "ArrowDown" => CopyAction::Move(CopyMotion::Down),
        "k" | "ArrowUp" => CopyAction::Move(CopyMotion::Up),
        "l" | "ArrowRight" => CopyAction::Move(CopyMotion::Right),
        "g" => {
            if modifiers.shift {
                CopyAction::Move(CopyMotion::Bottom)
            } else {
                CopyAction::Move(CopyMotion::Top)
            }
        }
        "G" => CopyAction::Move(CopyMotion::Bottom),
        "0" => CopyAction::Move(CopyMotion::LineStart),
        "$" => CopyAction::Move(CopyMotion::LineEnd),
        "v" => CopyAction::Visual,
        "y" => CopyAction::Yank,
        "/" => CopyAction::Search,
        "q" | "Escape" => CopyAction::Exit,
        _ => CopyAction::Ignore,
    }
}

/// Whether a chord enters or leaves copy mode: Cmd/Ctrl+Shift+Space, or
/// Cmd/Ctrl+Shift+C.
pub fn is_copy_mode_chord(key: &str, modifiers: ModifiersState) -> bool {
    (modifiers.command || modifiers.ctrl)
        && modifiers.shift
        && !modifiers.alt
        && (key == " " || key.eq_ignore_ascii_case("c"))
}

/// What a key means while the find bar is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindAction {
    Close,
    Next,
    Previous,
    ToggleRegex,
    /// Type into the bar's field: the key belongs to the search input.
    Input,
    Ignore,
}

/// Classify a key against the find bar. `field_focused` is true while the
/// query field holds the caret, where printable keys and Backspace are typed
/// rather than read as navigation.
pub fn classify_find_key(
    key: &str,
    modifiers: ModifiersState,
    field_focused: bool,
) -> FindAction {
    if key == "Escape" {
        return FindAction::Close;
    }
    if modifiers.alt && key.eq_ignore_ascii_case("r") {
        return FindAction::ToggleRegex;
    }
    if (modifiers.command || modifiers.ctrl) && !modifiers.shift && key.eq_ignore_ascii_case("f") {
        return FindAction::Close;
    }
    let enter = key.eq_ignore_ascii_case("enter") || key.eq_ignore_ascii_case("return");
    if enter {
        return if modifiers.shift {
            FindAction::Previous
        } else {
            FindAction::Next
        };
    }
    if !field_focused {
        if key == "n" || (modifiers.shift && key.eq_ignore_ascii_case("n")) {
            return if modifiers.shift || key == "N" {
                FindAction::Previous
            } else {
                FindAction::Next
            };
        }
        if key == "N" {
            return FindAction::Previous;
        }
    }
    if field_focused && (key.chars().count() == 1 || key == "Backspace") {
        return FindAction::Input;
    }
    FindAction::Ignore
}

/// The find bar of one pane: its query, the matches it found and which one is
/// current.
#[derive(Debug, Clone, Default)]
pub struct FindBar {
    pub open: bool,
    pub query: String,
    pub regex: bool,
    pub focused: bool,
    pub matches: Vec<SearchMatch>,
    pub current: Option<usize>,
    pub error: Option<String>,
    searched_query: String,
    searched_regex: bool,
    searched_revision: u64,
}

impl FindBar {
    /// The state of a freshly opened bar. The query starts empty so the bar
    /// never searches old text the user has forgotten about.
    pub fn opened() -> Self {
        Self {
            open: true,
            focused: true,
            ..Self::default()
        }
    }

    pub fn current_match(&self) -> Option<&SearchMatch> {
        self.current.and_then(|index| self.matches.get(index))
    }

    /// The bar's count as `current/total`. An empty result is `0/0`.
    pub fn count_label(&self) -> String {
        match self.current_match() {
            Some(_) => format!("{}/{}", self.current.unwrap_or(0) + 1, self.matches.len()),
            None => format!("0/{}", self.matches.len()),
        }
    }

    pub fn set_results(&mut self, result: Result<Vec<SearchMatch>, SearchError>, reset: bool) {
        match result {
            Ok(matches) => {
                self.error = None;
                self.matches = matches;
                if reset || self.current.is_none_or(|index| index >= self.matches.len()) {
                    self.current = (!self.matches.is_empty()).then_some(0);
                }
            }
            Err(error) => {
                self.matches.clear();
                self.current = None;
                self.error = Some(error.to_string());
            }
        }
    }

    pub fn next(&mut self) {
        if self.matches.is_empty() {
            self.current = None;
            return;
        }
        self.current = Some(match self.current {
            Some(index) => (index + 1) % self.matches.len(),
            None => 0,
        });
    }

    pub fn previous(&mut self) {
        if self.matches.is_empty() {
            self.current = None;
            return;
        }
        self.current = Some(match self.current {
            Some(0) | None => self.matches.len() - 1,
            Some(index) => index - 1,
        });
    }
}

/// A multi-click run on one cell.
#[derive(Debug, Clone, Copy)]
pub struct Click {
    pub at: Instant,
    pub cell: Cell,
    pub count: u32,
}

/// One pane's copy/selection/find state.
#[derive(Debug, Clone, Default)]
pub struct PaneTerminalState {
    pub copy: CopyMode,
    pub selection: Option<Selection>,
    /// A selection over the pane's block text (the normal shell surface). Its
    /// cells are lines of the flattened block text built by
    /// [`TerminalRegistry::block_text_lines`] and character columns in them.
    pub block_selection: Option<Selection>,
    pub find: FindBar,
    /// Set while a mouse drag is extending a cell selection.
    pub drag_anchor: Option<Cell>,
    /// Set while a mouse drag is extending a block-text selection.
    pub block_drag_anchor: Option<Cell>,
    pub last_click: Option<Click>,
    /// The part of a trackpad wheel gesture that has not added up to a whole
    /// row yet, in rows. The scrollback moves in rows; a gesture reports
    /// pixels, and the leftover would otherwise be rounded away every event.
    pub wheel_remainder: f32,
}

impl TerminalRegistry {
    /// The pane's copy, selection and find state.
    pub fn pane_ui(&self, pane_id: u64) -> Option<&PaneTerminalState> {
        self.ui.get(&pane_id)
    }

    pub fn pane_ui_mut(&mut self, pane_id: u64) -> &mut PaneTerminalState {
        self.ui.entry(pane_id).or_default()
    }

    pub fn copy_active(&self, pane_id: u64) -> bool {
        self.pane_ui(pane_id)
            .is_some_and(|state| state.copy.active)
    }

    /// Enter copy mode at the shell's cursor, leaving any visual selection
    /// behind.
    pub fn enter_copy_mode(&mut self, pane_id: u64) {
        let cursor = self
            .sessions
            .get(&pane_id)
            .map(|session| session.copy_start_cell())
            .unwrap_or_default();
        let ui = self.pane_ui_mut(pane_id);
        ui.copy = CopyMode::enter(cursor);
        ui.selection = None;
        ui.block_selection = None;
        ui.drag_anchor = None;
        ui.block_drag_anchor = None;
    }

    pub fn exit_copy_mode(&mut self, pane_id: u64) {
        let ui = self.pane_ui_mut(pane_id);
        ui.copy = CopyMode::default();
        ui.drag_anchor = None;
    }

    pub fn toggle_copy_mode(&mut self, pane_id: u64) {
        if self.copy_active(pane_id) {
            self.exit_copy_mode(pane_id);
        } else {
            self.enter_copy_mode(pane_id);
        }
    }

    /// Apply a copy-mode motion and keep the cursor row on screen.
    pub fn move_copy(&mut self, pane_id: u64, motion: CopyMotion) {
        let Some(session) = self.sessions.get(&pane_id) else {
            return;
        };
        let total = session.total_lines().max(1);
        let mut cursor = self
            .pane_ui(pane_id)
            .map(|state| state.copy.cursor)
            .unwrap_or_default();
        cursor.line = cursor.line.min(total - 1);
        let mut mode = CopyMode {
            active: true,
            cursor,
            visual: self
                .pane_ui(pane_id)
                .and_then(|state| state.copy.visual),
        };
        mode.apply(motion, total, |line| session.line_len(line));
        let cursor = mode.cursor;
        let visual = mode.visual;
        let ui = self.pane_ui_mut(pane_id);
        ui.copy.cursor = cursor;
        ui.copy.visual = visual;
        if let Some(session) = self.sessions.get_mut(&pane_id) {
            session.scroll_to_line(cursor.line);
        }
    }

    /// Toggle the visual anchor at the cursor.
    pub fn toggle_copy_visual(&mut self, pane_id: u64) {
        let ui = self.pane_ui_mut(pane_id);
        if !ui.copy.active {
            return;
        }
        ui.copy.visual = match ui.copy.visual {
            Some(_) => None,
            None => Some(ui.copy.cursor),
        };
    }

    /// The text `y` would copy: the visual selection, or the cursor's line.
    pub fn copy_mode_yank_text(&self, pane_id: u64) -> Option<String> {
        let state = self.pane_ui(pane_id)?;
        if !state.copy.active {
            return None;
        }
        let session = self.sessions.get(&pane_id)?;
        let selection = match state.copy.selection() {
            Some(selection) => selection,
            None => line_selection(
                state.copy.cursor.line,
                &session.line_at(state.copy.cursor.line)?,
            ),
        };
        Some(session.selection_text(selection))
    }

    /// Leave copy mode after a yank.
    pub fn finish_copy_yank(&mut self, pane_id: u64) {
        let ui = self.pane_ui_mut(pane_id);
        ui.copy = CopyMode::default();
        ui.drag_anchor = None;
        ui.block_drag_anchor = None;
        ui.selection = None;
        ui.block_selection = None;
    }

    pub fn find_open(&self, pane_id: u64) -> bool {
        self.pane_ui(pane_id).is_some_and(|state| state.find.open)
    }

    pub fn open_find(&mut self, pane_id: u64) {
        self.pane_ui_mut(pane_id).find = FindBar::opened();
        self.exit_copy_mode(pane_id);
        self.refresh_find(pane_id);
    }

    pub fn close_find(&mut self, pane_id: u64) {
        self.pane_ui_mut(pane_id).find = FindBar::default();
    }

    pub fn toggle_find(&mut self, pane_id: u64) {
        if self.find_open(pane_id) {
            self.close_find(pane_id);
        } else {
            self.open_find(pane_id);
        }
    }

    pub fn set_find_query(&mut self, pane_id: u64, query: String) {
        let ui = self.pane_ui_mut(pane_id);
        ui.find.query = query;
        ui.find.error = None;
        self.refresh_find(pane_id);
    }

    pub fn set_find_focused(&mut self, pane_id: u64, focused: bool) {
        self.pane_ui_mut(pane_id).find.focused = focused;
    }

    pub fn toggle_find_regex(&mut self, pane_id: u64) {
        let ui = self.pane_ui_mut(pane_id);
        ui.find.regex = !ui.find.regex;
        // Force the next refresh to run even when the revision has not moved.
        ui.find.searched_regex = !ui.find.regex;
        self.refresh_find(pane_id);
    }

    /// Recompute the matches when the query, the regex mode or the screen has
    /// changed since the last search.
    pub fn refresh_find(&mut self, pane_id: u64) {
        let Some(state) = self.pane_ui(pane_id) else {
            return;
        };
        if !state.find.open {
            return;
        }
        let query = state.find.query.clone();
        let regex = state.find.regex;
        let revision = self
            .sessions
            .get(&pane_id)
            .map(|session| session.revision())
            .unwrap_or(0);
        let state = self.pane_ui(pane_id).expect("the pane state was just read");
        let stale = query != state.find.searched_query
            || regex != state.find.searched_regex
            || revision != state.find.searched_revision;
        if !stale {
            return;
        }
        let reset = query != state.find.searched_query || regex != state.find.searched_regex;
        let result = self
            .sessions
            .get(&pane_id)
            .map(|session| {
                session.search(
                    &query,
                    SearchOptions {
                        regex,
                        case_sensitive: None,
                    },
                )
            })
            .unwrap_or(Ok(Vec::new()));
        let line = {
            let ui = self.pane_ui_mut(pane_id);
            ui.find.set_results(result, reset);
            ui.find.searched_query = query;
            ui.find.searched_regex = regex;
            ui.find.searched_revision = revision;
            ui.find.current_match().map(|found| found.line)
        };
        if let (Some(line), Some(session)) = (line, self.sessions.get_mut(&pane_id)) {
            session.scroll_to_line(line);
        }
    }

    pub fn find_next(&mut self, pane_id: u64) {
        let line = {
            let ui = self.pane_ui_mut(pane_id);
            ui.find.next();
            // Enter is also the way out of the field: `n` then navigates
            // instead of typing another query character.
            ui.find.focused = false;
            ui.find.current_match().map(|found| found.line)
        };
        if let (Some(line), Some(session)) = (line, self.sessions.get_mut(&pane_id)) {
            session.scroll_to_line(line);
        }
    }

    pub fn find_previous(&mut self, pane_id: u64) {
        let line = {
            let ui = self.pane_ui_mut(pane_id);
            ui.find.previous();
            ui.find.focused = false;
            ui.find.current_match().map(|found| found.line)
        };
        if let (Some(line), Some(session)) = (line, self.sessions.get_mut(&pane_id)) {
            session.scroll_to_line(line);
        }
    }

    /// Convert a visible grid row and column into an absolute content cell.
    pub fn screen_cell(&self, pane_id: u64, row: usize, column: usize) -> Option<Cell> {
        let session = self.sessions.get(&pane_id)?;
        Some(Cell::new(session.absolute_line(row)?, column))
    }

    /// Begin a selection at `cell`, resolving double- and triple-clicks into a
    /// word or a line. `now` is the press time, so the interval is testable.
    pub fn begin_selection(&mut self, pane_id: u64, cell: Cell, now: Instant) {
        let count = self
            .pane_ui(pane_id)
            .and_then(|state| state.last_click)
            .filter(|click| {
                click.cell == cell && now.saturating_duration_since(click.at) <= MULTI_CLICK_INTERVAL
            })
            .map_or(1, |click| (click.count + 1).min(3));
        let line = self
            .sessions
            .get(&pane_id)
            .and_then(|session| session.line_at(cell.line));
        let selection = match (count, line) {
            (2, Some(line)) => word_selection(cell.line, &line, cell.column)
                .unwrap_or_else(|| Selection::new(cell, cell)),
            (3, Some(line)) => line_selection(cell.line, &line),
            _ => Selection::new(cell, cell),
        };
        let anchor = selection.ordered().0;
        let ui = self.pane_ui_mut(pane_id);
        ui.selection = Some(selection);
        ui.block_selection = None;
        ui.drag_anchor = Some(anchor);
        ui.block_drag_anchor = None;
        ui.last_click = Some(Click {
            at: now,
            cell,
            count,
        });
    }

    /// The text lines a normal shell pane draws as its command sections, in
    /// the block view's order: each command followed by its output, blank
    /// output lines kept as the single space the block draws for them.
    pub fn block_text_lines(&self, pane_id: u64) -> Vec<String> {
        let Some(session) = self.sessions.get(&pane_id) else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        for block in session.visible_blocks(&BlockView::Terminal) {
            if block.card.is_some() {
                continue;
            }
            let command = block.command.trim_end();
            if command.is_empty() {
                continue;
            }
            lines.push(command.to_string());
            for line in goble_terminal::ansi::render_lines(block.output.as_bytes()) {
                let text = line.text();
                lines.push(if text.is_empty() {
                    " ".to_string()
                } else {
                    text
                });
            }
        }
        lines
    }

    /// Begin a selection over the pane's block text at `cell`, resolving
    /// double- and triple-clicks into a word or a line the way the grid path
    /// does. `now` is the press time, so the interval is testable.
    pub fn begin_block_selection(&mut self, pane_id: u64, cell: Cell, now: Instant) {
        let lines = self.block_text_lines(pane_id);
        if lines.is_empty() {
            return;
        }
        let cell = Cell::new(cell.line.min(lines.len() - 1), cell.column);
        let count = self
            .pane_ui(pane_id)
            .and_then(|state| state.last_click)
            .filter(|click| {
                click.cell == cell
                    && now.saturating_duration_since(click.at) <= MULTI_CLICK_INTERVAL
            })
            .map_or(1, |click| (click.count + 1).min(3));
        let line = text_line(&lines[cell.line]);
        let selection = match count {
            2 => word_selection(cell.line, &line, cell.column)
                .unwrap_or_else(|| Selection::new(cell, cell)),
            3 => line_selection(cell.line, &line),
            _ => Selection::new(cell, cell),
        };
        let anchor = selection.ordered().0;
        let ui = self.pane_ui_mut(pane_id);
        ui.selection = None;
        ui.drag_anchor = None;
        ui.block_selection = Some(selection);
        ui.block_drag_anchor = Some(anchor);
        ui.last_click = Some(Click {
            at: now,
            cell,
            count,
        });
    }

    /// Extend the block-text selection to `cell`.
    pub fn extend_block_selection(&mut self, pane_id: u64, cell: Cell) {
        let anchor = self
            .pane_ui(pane_id)
            .and_then(|state| state.block_drag_anchor);
        if let Some(anchor) = anchor {
            self.pane_ui_mut(pane_id).block_selection = Some(Selection::new(anchor, cell));
        }
    }

    /// End a block-text drag. Returns whether one was in progress.
    pub fn end_block_selection(&mut self, pane_id: u64) -> bool {
        self.pane_ui_mut(pane_id).block_drag_anchor.take().is_some()
    }

    pub fn block_selection(&self, pane_id: u64) -> Option<Selection> {
        self.pane_ui(pane_id).and_then(|state| state.block_selection)
    }

    /// The text a block-text selection covers.
    pub fn block_selection_text(&self, pane_id: u64) -> Option<String> {
        let selection = self.block_selection(pane_id)?;
        self.block_text_of(pane_id, selection)
    }

    fn block_text_of(&self, pane_id: u64, selection: Selection) -> Option<String> {
        let lines = self.block_text_lines(pane_id);
        if lines.is_empty() {
            return None;
        }
        Some(selection_text(selection, |index| {
            lines.get(index).map(|text| text_line(text.as_str()))
        }))
    }

    /// Extend the current selection to `cell`.
    pub fn extend_selection(&mut self, pane_id: u64, cell: Cell) {
        let anchor = self.pane_ui(pane_id).and_then(|state| state.drag_anchor);
        if let Some(anchor) = anchor {
            self.pane_ui_mut(pane_id).selection = Some(Selection::new(anchor, cell));
        }
    }

    /// End a mouse drag. Returns whether one was in progress.
    pub fn end_selection(&mut self, pane_id: u64) -> bool {
        let ui = self.pane_ui_mut(pane_id);
        ui.drag_anchor.take().is_some()
    }

    pub fn selection(&self, pane_id: u64) -> Option<Selection> {
        self.pane_ui(pane_id).and_then(|state| state.selection)
    }

    /// The text of the pane's current mouse selection.
    pub fn selection_text(&self, pane_id: u64) -> Option<String> {
        let selection = self.selection(pane_id)?;
        let session = self.sessions.get(&pane_id)?;
        Some(session.selection_text(selection))
    }

    /// The text a finished mouse drag selected, for copy-on-release. A gesture
    /// that covers a single cell is a click and copies nothing, so clicking in
    /// the output never overwrites the clipboard; a drag, and the word or line
    /// a double/triple click took, is copied.
    pub fn dragged_selection_text(&self, pane_id: u64) -> Option<String> {
        let state = self.pane_ui(pane_id)?;
        if let Some(selection) = state
            .selection
            .filter(Selection::spans_more_than_one_cell)
        {
            if let Some(session) = self.sessions.get(&pane_id) {
                let text = session.selection_text(selection);
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
        let selection = state
            .block_selection
            .filter(Selection::spans_more_than_one_cell)?;
        self.block_text_of(pane_id, selection)
    }

    /// The text `Cmd+C` should copy: the grid selection, the block-text
    /// selection, or copy mode's.
    pub fn copy_text(&self, pane_id: u64) -> Option<String> {
        if let Some(text) = self.selection_text(pane_id) {
            if !text.is_empty() {
                return Some(text);
            }
        }
        if let Some(text) = self.block_selection_text(pane_id) {
            if !text.is_empty() {
                return Some(text);
            }
        }
        self.copy_mode_yank_text(pane_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> ScreenLine {
        ScreenLine {
            cells: text
                .chars()
                .map(|ch| goble_terminal::ScreenCell {
                    ch,
                    ..Default::default()
                })
                .collect(),
        }
    }

    fn lines(screen: &mut Vec<ScreenLine>, text: &str) {
        for part in text.split('\n') {
            screen.push(line(part));
        }
    }

    #[test]
    fn a_reverse_drag_is_the_same_range() {
        let selection = Selection::new(Cell::new(4, 9), Cell::new(2, 1));
        assert_eq!(
            selection.ordered(),
            (Cell::new(2, 1), Cell::new(4, 9)),
            "reading order, whichever end moved"
        );
        assert!(selection.contains(Cell::new(3, 0)));
        assert!(selection.contains(Cell::new(2, 1)));
        assert!(selection.contains(Cell::new(4, 9)));
        assert!(!selection.contains(Cell::new(5, 0)));
    }

    #[test]
    fn a_double_click_takes_the_word_under_the_column() {
        let text = line("run cargo test --all");
        let selection = word_selection(3, &text, 6).expect("the word under 'cargo'");
        let (start, end) = selection.ordered();
        assert_eq!((start.line, start.column), (3, 4));
        assert_eq!((end.line, end.column), (3, 8));
        // Punctuation is its own word, and whitespace is no word at all.
        let punctuation = word_selection(3, &text, 15).expect("the run of dashes");
        assert_eq!(punctuation.ordered(), (Cell::new(3, 15), Cell::new(3, 16)));
        assert!(word_selection(3, &text, 3).is_none(), "a space");
    }

    #[test]
    fn a_triple_click_takes_the_line_without_its_trailing_blanks() {
        let text = line("hello world   ");
        let selection = line_selection(1, &text);
        assert_eq!(
            selection.ordered(),
            (Cell::new(1, 0), Cell::new(1, 10)),
            "the row is selected through its last non-blank cell"
        );
        let blank = line("     ");
        assert_eq!(line_selection(1, &blank).ordered().1.column, 0);
    }

    #[test]
    fn selection_text_covers_both_lines_and_trims_the_edges() {
        let mut screen = Vec::new();
        lines(&mut screen, "one two\nthree four\nfive");
        let selection = Selection::new(Cell::new(0, 4), Cell::new(1, 4));
        let text = selection_text(selection, |index| screen.get(index).cloned());
        assert_eq!(text, "two\nthree");
    }

    #[test]
    fn copy_motions_stay_inside_the_content_and_walk_lines() {
        let lengths = [5usize, 3, 0, 7];
        let mut mode = CopyMode::enter(Cell::new(0, 4));
        mode.apply(CopyMotion::Right, lengths.len(), |line| lengths[line]);
        assert_eq!(mode.cursor, Cell::new(1, 0), "right at the end wraps");
        mode.apply(CopyMotion::LineEnd, lengths.len(), |line| lengths[line]);
        assert_eq!(mode.cursor, Cell::new(1, 2));
        mode.apply(CopyMotion::Down, lengths.len(), |line| lengths[line]);
        assert_eq!(mode.cursor, Cell::new(2, 0), "an empty line clamps to 0");
        mode.apply(CopyMotion::Bottom, lengths.len(), |line| lengths[line]);
        assert_eq!(mode.cursor.line, 3);
        assert_eq!(mode.cursor.column, 0);
        mode.apply(CopyMotion::Top, lengths.len(), |line| lengths[line]);
        assert_eq!(mode.cursor, Cell::new(0, 0));
        mode.apply(CopyMotion::Left, lengths.len(), |line| lengths[line]);
        assert_eq!(mode.cursor, Cell::new(0, 0), "the top-left does not wrap");
    }

    #[test]
    fn copy_mode_keys_are_classified_and_chords_are_left_alone() {
        assert_eq!(
            classify_copy_key("j", ModifiersState::default()),
            CopyAction::Move(CopyMotion::Down)
        );
        assert_eq!(
            classify_copy_key("ArrowUp", ModifiersState::default()),
            CopyAction::Move(CopyMotion::Up)
        );
        assert_eq!(
            classify_copy_key("G", ModifiersState::default()),
            CopyAction::Move(CopyMotion::Bottom)
        );
        assert_eq!(
            classify_copy_key("g", ModifiersState::default()),
            CopyAction::Move(CopyMotion::Top)
        );
        assert_eq!(
            classify_copy_key("$", ModifiersState::default()),
            CopyAction::Move(CopyMotion::LineEnd)
        );
        assert_eq!(classify_copy_key("v", ModifiersState::default()), CopyAction::Visual);
        assert_eq!(classify_copy_key("y", ModifiersState::default()), CopyAction::Yank);
        assert_eq!(classify_copy_key("/", ModifiersState::default()), CopyAction::Search);
        assert_eq!(classify_copy_key("q", ModifiersState::default()), CopyAction::Exit);
        assert_eq!(
            classify_copy_key("j", ModifiersState::default()),
            CopyAction::Move(CopyMotion::Down)
        );
        let command = ModifiersState {
            command: true,
            ..ModifiersState::default()
        };
        assert_eq!(classify_copy_key("c", command), CopyAction::Ignore);
        let copy_chord = ModifiersState {
            command: true,
            shift: true,
            ..ModifiersState::default()
        };
        assert!(is_copy_mode_chord(" ", copy_chord));
        assert!(is_copy_mode_chord("c", copy_chord));
        assert!(!is_copy_mode_chord("c", command));
    }

    #[test]
    fn find_keys_navigate_and_the_field_keeps_what_is_typed() {
        let none = ModifiersState::default();
        assert_eq!(classify_find_key("Escape", none, true), FindAction::Close);
        assert_eq!(classify_find_key("Enter", none, true), FindAction::Next);
        let shift = ModifiersState {
            shift: true,
            ..ModifiersState::default()
        };
        assert_eq!(classify_find_key("Enter", shift, true), FindAction::Previous);
        assert_eq!(classify_find_key("n", none, true), FindAction::Input);
        assert_eq!(classify_find_key("n", none, false), FindAction::Next);
        assert_eq!(classify_find_key("N", none, false), FindAction::Previous);
        assert_eq!(classify_find_key("N", none, true), FindAction::Input);
        let alt = ModifiersState {
            alt: true,
            ..ModifiersState::default()
        };
        assert_eq!(classify_find_key("r", alt, false), FindAction::ToggleRegex);
        assert_eq!(classify_find_key("x", none, true), FindAction::Input);
        assert_eq!(classify_find_key("ArrowLeft", none, true), FindAction::Ignore);
    }

    #[test]
    fn find_bar_counts_and_wraps_in_both_directions() {
        let mut bar = FindBar::opened();
        assert_eq!(bar.count_label(), "0/0");
        let found = vec![
            SearchMatch {
                line: 1,
                col: 0,
                len: 3,
                text: "one".to_string(),
            },
            SearchMatch {
                line: 4,
                col: 2,
                len: 3,
                text: "two".to_string(),
            },
        ];
        bar.set_results(Ok(found), true);
        assert_eq!(bar.count_label(), "1/2");
        bar.next();
        assert_eq!(bar.count_label(), "2/2");
        bar.next();
        assert_eq!(bar.count_label(), "1/2", "next wraps");
        bar.previous();
        assert_eq!(bar.count_label(), "2/2", "previous wraps");
        assert_eq!(bar.current_match().map(|found| found.line), Some(4));
    }

    #[test]
    fn find_bar_reports_no_match_and_an_invalid_regex() {
        let mut bar = FindBar::opened();
        bar.set_results(Ok(Vec::new()), true);
        assert_eq!(bar.count_label(), "0/0");
        assert!(bar.current_match().is_none());

        bar.set_results(
            Err(SearchError::InvalidRegex("unclosed group".to_string())),
            true,
        );
        assert_eq!(bar.count_label(), "0/0");
        assert!(
            bar.error.as_deref().is_some_and(|error| error.contains("invalid regex")),
            "{:?}",
            bar.error
        );
        // A later good query clears the error.
        bar.set_results(Ok(Vec::new()), true);
        assert!(bar.error.is_none());
    }
}
