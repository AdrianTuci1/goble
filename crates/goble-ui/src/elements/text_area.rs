use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::{
    caret, vec2f, AppContext, CaretShape, Code, ConstrainedBox, Container, CrossAxisAlignment,
    Element, Empty, EventContext, Fill, Flex, LayoutContext, MainAxisSize, PaintContext, Point,
    ScrollState, SizeConstraint, Text, TextSpan,
};
use crate::event::{DispatchedEvent, ModifiersState};
use crate::geometry::{PointF, Vector2F};
use crate::platform::text_atlas::{measure_text_family, FontWeight};
use crate::syntax::HighlightedLine;
use crate::theme::{ColorToken, FontFamily};
use crate::vim::{Clipboard, VimBuffer, VimOutcome, VimState};

/// The size the field's own text is drawn at, matching the `Text` elements the
/// row is built from: a click is turned into a character index by measuring
/// prefixes at the very size they are painted.
const TEXT_FONT_SIZE: f32 = 12.0;
const TEXT_LINE_HEIGHT: f32 = 1.2;

/// How many rows a windowed multi-line buffer draws before its region has
/// reported a viewport to size itself against. The first frame has no
/// measurement, so it draws a screenful for the tallest pane; the frame after
/// draws what fits.
const FIRST_FRAME_ROWS: usize = 64;
/// The rows a window keeps beyond the region's viewport, so a buffer scrolled to
/// a line that is not its first still fills, top and bottom.
const WINDOW_SLACK: usize = 4;

/// The syntax runs a multi-line buffer's lines are drawn with.
///
/// A run set belongs to the text it was highlighted from, so the host resolves
/// the mapping itself: per line of the buffer, the run set that still spells
/// that line. A line with none — one the buffer has changed, or a file whose
/// type resolved no language at all — is drawn in the plain text colour;
/// painting a stale set would colour characters the runs were never about.
#[derive(Clone)]
pub struct LineRuns {
    /// The run sets, one per line of the text they were highlighted from.
    pub lines: Rc<Vec<HighlightedLine>>,
    /// One entry per line of the buffer, in order: the index in [`Self::lines`]
    /// of the run set that spells that line, or `None` for a plain line.
    pub sources: Vec<Option<usize>>,
}

pub struct TextArea {
    value: String,
    placeholder: String,
    focused: bool,
    min_height: f32,
    masked: bool,
    /// Where the insertion beam sits, as a character index into the value.
    /// Shared with the host when the host owns the state (the composer does:
    /// the element is rebuilt every frame, so an index kept here would reset to
    /// the start on every one of them).
    caret: Option<Rc<RefCell<usize>>>,
    /// The index used when no shared one was handed over.
    local_caret: usize,
    /// Whether the field holds a multi-line buffer: the value is drawn one row
    /// per line, `Up`/`Down` walk lines and `Home`/`End` hold a line's own
    /// ends. Off by default, so every single-line field is unchanged.
    multiline: bool,
    /// How many monospace digits a multi-line row's line number is padded to,
    /// drawn in the muted colour in front of the text. `0` draws no gutter.
    line_numbers: usize,
    /// One line box's height, as a multiple of the font size.
    line_height: f32,
    /// The region the host scrolls this buffer in, when it put the field in one.
    /// The buffer then lays out only the rows that region's viewport can show,
    /// and stands the rest in as spacers (see [`TextArea::drawn_rows`]). `None`
    /// — every single-line field, and every multi-line field the host draws
    /// whole — lays out every row, as before.
    scroll: Option<Rc<RefCell<ScrollState>>>,
    /// The syntax runs the buffer's lines are drawn with, when the host
    /// resolved them — see [`LineRuns`].
    line_runs: Option<Rc<LineRuns>>,
    /// Where a shift-selection began, as a character index; `None` is the
    /// steady state. Shared with the host the way the caret is, so a selection
    /// survives the per-frame rebuild.
    anchor: Option<Rc<RefCell<Option<usize>>>>,
    /// Whether a press outside the field drops its focus. The terminal pane's
    /// rich input keeps it: it is the only place a command can be typed, so a
    /// click on the output above it must not take the keyboard away.
    blur_on_outside_click: bool,
    /// Whether the field claims the whole width it is given, so a press on the
    /// empty tail of its line still lands in it. Off by default: a field that
    /// shares a row with something else must not cover it.
    full_width: bool,
    /// Modal (vim) editing, when the host turned it on. The state lives beside
    /// the host's own, because this element is rebuilt every frame.
    vim: Option<Rc<RefCell<VimState>>>,
    /// The system clipboard the engine's `"+`/`"*` registers reach, when the
    /// host supplies one.
    clipboard: Option<Rc<RefCell<dyn Clipboard>>>,
    on_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_focus_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    on_submit: Option<Rc<RefCell<dyn FnMut(ModifiersState) + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    root: Option<Box<dyn Element>>,
}

impl TextArea {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            placeholder: String::new(),
            focused: false,
            min_height: 80.0,
            masked: false,
            caret: None,
            local_caret: 0,
            multiline: false,
            line_numbers: 0,
            line_height: TEXT_LINE_HEIGHT,
            scroll: None,
            line_runs: None,
            anchor: None,
            blur_on_outside_click: true,
            full_width: false,
            vim: None,
            clipboard: None,
            on_change: None,
            on_focus_change: None,
            on_submit: None,
            size: None,
            origin: None,
            root: None,
        }
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn with_min_height(mut self, height: f32) -> Self {
        self.min_height = height;
        self
    }

    /// Share the insertion point with the host, so the beam survives the
    /// per-frame rebuild of the element tree.
    pub fn with_caret(mut self, caret: Rc<RefCell<usize>>) -> Self {
        self.caret = Some(caret);
        self
    }

    /// Draw a multi-line buffer: one row per line, with the arrow keys walking
    /// lines, `Home`/`End` holding the caret's own line's ends, and `Shift`
    /// with any arrow extending a selection from where the first one started.
    /// A line longer than the field is not folded; the pane that scrolls it
    /// cuts it instead, so a line keeps its columns.
    pub fn with_multiline(mut self, multiline: bool) -> Self {
        self.multiline = multiline;
        self
    }

    /// Pad every multi-line row's line number to `digits` monospace digits and
    /// draw it, muted, in front of the text; `0` draws none.
    pub fn with_line_numbers(mut self, digits: usize) -> Self {
        self.line_numbers = digits;
        self
    }

    /// How tall one line box is, as a multiple of the font size. Every row and
    /// the caret are drawn at it, so a row's height never depends on whether
    /// the caret is in it.
    pub fn with_line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    /// The region the host scrolls this buffer in — the same [`ScrollState`] the
    /// [`Scrollable`](crate::elements::Scrollable) around the field was given.
    ///
    /// A multi-line buffer handed one draws only the rows that region's viewport
    /// can show and stands the rest in as spacers of the same height, so the
    /// text costs a screenful per frame instead of the whole buffer — the region
    /// keeps the whole buffer's scroll range, and the caret is brought into view
    /// when it moves off it. A buffer without one is drawn whole.
    pub fn with_scroll_state(mut self, scroll: Rc<RefCell<ScrollState>>) -> Self {
        self.scroll = Some(scroll);
        self
    }

    /// The syntax runs the buffer's lines are drawn with, one entry per line of
    /// the buffer — see [`LineRuns`].
    pub fn with_line_runs(mut self, runs: Option<Rc<LineRuns>>) -> Self {
        self.line_runs = runs;
        self
    }

    /// Share the shift-selection's anchor with the host, so a selection
    /// survives the per-frame rebuild of the element tree.
    pub fn with_anchor(mut self, anchor: Rc<RefCell<Option<usize>>>) -> Self {
        self.anchor = Some(anchor);
        self
    }

    /// Render the typed value as a masked (bullet) string while keeping the real
    /// value intact — used for credential/secret fields.
    pub fn with_masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    /// Claim the whole width the field is given, so a press to the right of the
    /// text is still a press in the field (the beam then lands at the end of the
    /// line). Leave it off when the field shares a row with a control beside it.
    pub fn with_full_width(mut self, full_width: bool) -> Self {
        self.full_width = full_width;
        self
    }

    pub fn with_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Turn on modal (vim) editing for this field: the host's [`VimState`] owns
    /// the mode, so it survives the per-frame rebuild.
    pub fn with_vim(mut self, vim: Rc<RefCell<VimState>>) -> Self {
        self.vim = Some(vim);
        self
    }

    /// The system clipboard the `"+`/`"*` registers read and write.
    pub fn with_clipboard(mut self, clipboard: Rc<RefCell<dyn Clipboard>>) -> Self {
        self.clipboard = Some(clipboard);
        self
    }

    /// [`Self::with_vim`] for a host that already holds an `Option`, so a
    /// composer can pass its own setting straight through.
    pub fn with_vim_opt(mut self, vim: Option<Rc<RefCell<VimState>>>) -> Self {
        self.vim = vim;
        self
    }

    /// [`Self::with_clipboard`] for a host that already holds an `Option`.
    pub fn with_clipboard_opt(mut self, clipboard: Option<Rc<RefCell<dyn Clipboard>>>) -> Self {
        self.clipboard = clipboard;
        self
    }

    /// Whether a press outside the field blurs it (the default). A host that
    /// owns the keyboard with this field — the terminal pane's rich input —
    /// turns it off, so a click elsewhere in the pane leaves the caret in place.
    pub fn with_blur_on_outside_click(mut self, blur: bool) -> Self {
        self.blur_on_outside_click = blur;
        self
    }

    pub fn with_on_focus_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_focus_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// When set, pressing Enter fires the callback instead of inserting a
    /// newline. The callback receives the modifiers of the Enter key event so
    /// the host can tell a plain Enter (terminal command) from Cmd/Ctrl+Enter
    /// (submit to agent).
    pub fn with_on_submit<F: FnMut(ModifiersState) + 'static>(mut self, callback: F) -> Self {
        self.on_submit = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    fn set_focused(&mut self, focused: bool) {
        if self.focused == focused {
            return;
        }
        self.focused = focused;
        if let Some(cb) = self.on_focus_change.as_ref() {
            (cb.borrow_mut())(focused);
        }
    }

    /// The string the field draws: the value itself, one bullet per character
    /// when the field is masked, and the placeholder when there is nothing of
    /// the user's own to draw.
    fn display(&self) -> String {
        if self.value.is_empty() {
            self.placeholder.clone()
        } else if self.masked {
            "•".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        }
    }

    fn rebuild(&mut self, app: &AppContext) {
        if self.multiline {
            self.rebuild_multiline(app);
            return;
        }
        let empty = self.value.is_empty();
        let display = self.display();
        // An empty field draws its placeholder in the muted colour, so a guide
        // never reads as text the user typed.
        let color = if empty {
            ColorToken::Muted
        } else {
            ColorToken::Text
        };

        // Transparent: no background/border or internal padding so the textarea
        // reads as part of the surrounding rich-input bar.
        let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
        if !self.focused {
            row = row.with_child(Text::new(display).with_theme_color(color, app).finish());
        } else {
            // The beam is its own slot in the row, with the text it precedes on
            // one side and the rest on the other, so it lands between the two
            // characters it sits between instead of always trailing the line.
            // A placeholder is only a guide, so the beam leads it: that is where
            // the user's own text starts.
            let at = if empty { 0 } else { self.caret_index() };
            let len = display.chars().count();
            // Modal editing draws the caret in the mode's own shape and paints
            // the character a visual selection covers. The placeholder is never
            // a character the caret covers.
            let (shape, selection) = match self.vim.as_ref() {
                Some(vim) => {
                    let vim = vim.borrow();
                    (caret_shape(vim.mode()), vim.selection(at, len))
                }
                None => (CaretShape::Bar, None),
            };
            let under = if empty { None } else { display.chars().nth(at) };
            match selection {
                Some(selected) => {
                    row = self.selection_row(app, row, &display, at, shape, under, color, selected)
                }
                None => {
                    let (before, after) = split_at_char(&display, at);
                    row = row
                        .with_child(text_run(app, before, color))
                        .with_child(caret(app, shape, under))
                        .with_child(
                            Text::new(after_the_caret(shape, under, &after))
                                .with_theme_color(color, app)
                                .finish(),
                        );
                }
            }
        }
        self.root = Some(Container::new(row.finish()).finish());
    }

    /// The multi-line buffer: one row per line, the line's gutter in front of
    /// its text and the caret inside the line it sits on.
    ///
    /// Every row is built from the same pieces — a run-painted [`Code`] per
    /// segment, and the caret as a beam exactly one line box tall — so all the
    /// rows are one height and the text below the caret never moves as the
    /// caret moves.
    ///
    /// Only the rows [`TextArea::drawn_rows`] gives are built; the rest of the
    /// buffer is two spacers, so the content the host's region measures is still
    /// the whole buffer's.
    fn rebuild_multiline(&mut self, app: &AppContext) {
        let chars: Vec<char> = self.value.chars().collect();
        let caret = self.caret_index();
        let selection = self.selection();
        let plain = app.theme.color(ColorToken::Text);
        let lines = self.value.matches('\n').count() + 1;
        let window = self.drawn_rows(lines);
        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start);
        if window.start > 0 {
            column = column.with_child(row_spacer(window.start, self.line_box()));
        }
        let mut start = 0usize;
        let mut index = 0usize;
        loop {
            let end = chars[start..]
                .iter()
                .position(|ch| *ch == '\n')
                .map(|at| start + at)
                .unwrap_or(chars.len());
            if index >= window.start && index < window.end {
                let line: String = chars[start..end].iter().collect();
                column = column.with_child(self.line_row(
                    app, index, &line, start, caret, &selection, plain,
                ));
            }
            index += 1;
            if end >= chars.len() {
                break;
            }
            start = end + 1;
        }
        if window.end < lines {
            column = column.with_child(row_spacer(lines - window.end, self.line_box()));
        }
        self.root = Some(Container::new(column.finish()).finish());
    }

    /// The rows of the buffer this frame lays out.
    ///
    /// Every line of a multi-line buffer is an element of the frame and the tree
    /// is rebuilt every frame, so a buffer that draws all of itself lays out
    /// every line of it on every frame, whatever changed — a two-thousand-line
    /// file costs a two-thousand-row layout per frame to show forty of them.
    /// When the host put the field in a scroll region ([`Self::with_scroll_state`]),
    /// the region's own viewport bounds the window; without one the whole buffer
    /// is the window.
    ///
    /// The rows outside it are not dropped: [`Self::rebuild_multiline`] stands
    /// them in as spacers of the same height, so the offset the host keeps still
    /// means the same line and the scroll range is still the whole buffer's.
    fn drawn_rows(&self, lines: usize) -> std::ops::Range<usize> {
        let Some(scroll) = self.scroll.as_ref() else {
            return 0..lines;
        };
        let (offset, viewport) = {
            let scroll = scroll.borrow();
            (scroll.offset(), scroll.viewport())
        };
        let line_box = self.line_box();
        let first = ((offset / line_box).floor().max(0.0) as usize).min(lines.saturating_sub(1));
        let rows = if viewport > 0.0 {
            (viewport / line_box).ceil() as usize
        } else {
            FIRST_FRAME_ROWS
        };
        first..(first + rows + WINDOW_SLACK).min(lines)
    }

    /// Bring the row the caret sits on into the field's own viewport, when the
    /// host put the field in a scroll region.
    ///
    /// Called when the caret moves rather than on every frame: a buffer scrolled
    /// by hand must not be pulled back to a caret the user left behind. A caret
    /// already inside the window is left where it is, so a move within the pane
    /// scrolls nothing.
    fn reveal_caret(&self) {
        let Some(scroll) = self.scroll.as_ref() else {
            return;
        };
        let (offset, viewport) = {
            let scroll = scroll.borrow();
            (scroll.offset(), scroll.viewport())
        };
        if viewport <= 0.0 {
            return;
        }
        let line_box = self.line_box();
        let caret = self.caret_index();
        let line = self
            .value
            .chars()
            .take(caret)
            .filter(|ch| *ch == '\n')
            .count();
        let first = (offset / line_box).floor().max(0.0) as usize;
        let rows = (viewport / line_box).ceil() as usize;
        let target = if line < first {
            line as f32 * line_box
        } else if line >= first + rows {
            (line + 1) as f32 * line_box - viewport
        } else {
            return;
        };
        scroll.borrow_mut().scroll_by(target - offset);
    }

    /// One line of the buffer: the padded line number, the line's runs cut at
    /// the caret and at the selection's edges, and the caret where it belongs.
    ///
    /// The line's text is the run set the host still maps to it — the syntax
    /// read's own — or one plain run when the buffer changed the line.
    #[allow(clippy::too_many_arguments)]
    fn line_row(
        &self,
        app: &AppContext,
        index: usize,
        line: &str,
        start: usize,
        caret: usize,
        selection: &Option<std::ops::Range<usize>>,
        plain: ColorU,
    ) -> Box<dyn Element> {
        let length = line.chars().count();
        let caret_column = (caret >= start && caret <= start + length).then(|| caret - start);
        // The columns of this line the selection covers, when it covers any.
        let selected = selection.as_ref().and_then(|selection| {
            let from = selection.start.max(start);
            let to = selection.end.min(start + length);
            (to > from).then(|| (from - start)..(to - start))
        });
        let mut cuts = vec![0usize, length];
        if let Some(column) = caret_column {
            cuts.push(column);
        }
        if let Some(range) = &selected {
            cuts.push(range.start);
            cuts.push(range.end);
        }
        cuts.sort_unstable();
        cuts.dedup();

        let runs = self.line_runs_for(index, line, plain);
        let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Start);
        if self.line_numbers > 0 {
            let number = TextSpan::plain(format!(
                "{:>width$} ",
                index + 1,
                width = self.line_numbers
            ))
            .with_color(app.theme.color(ColorToken::Muted));
            row = row.with_child(runs_code(self.line_height, vec![number]));
        }
        for pair in cuts.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            if caret_column == Some(from) {
                row = row.with_child(self.line_beam(app));
            }
            let piece = runs_code(self.line_height, slice_runs(&runs, from, to));
            let piece: Box<dyn Element> = match &selected {
                Some(range) if from >= range.start && to <= range.end => Container::new(piece)
                    .with_background(Fill::Solid(app.theme.color(ColorToken::Selected)))
                    .finish(),
                _ => piece,
            };
            row = row.with_child(piece);
        }
        if caret_column == Some(length) {
            row = row.with_child(self.line_beam(app));
        }
        row.finish()
    }

    /// The insertion point inside a multi-line row.
    ///
    /// The shared beam is a fixed [`caret::CARET_HEIGHT`], which is taller than
    /// this field's line box; a caret taller than its own row would make the
    /// caret's line taller than every other one and shift the text under it.
    /// The bar takes no advance, so the row's own text keeps its columns.
    fn line_beam(&self, app: &AppContext) -> Box<dyn Element> {
        caret::caret_bar(app, self.line_box())
    }

    /// One line's height: what every row of a multi-line buffer is drawn at,
    /// the caret's own row included.
    fn line_box(&self) -> f32 {
        (TEXT_FONT_SIZE * self.line_height).ceil()
    }

    /// The runs the line `index` of the buffer is drawn from: the run set the
    /// host still maps to it when that set spells the line, or one plain run.
    fn line_runs_for(&self, index: usize, line: &str, plain: ColorU) -> Vec<TextSpan> {
        let runs = self.line_runs.as_ref().and_then(|runs| {
            let source = runs.sources.get(index).copied().flatten()?;
            let runs = runs.lines.get(source)?;
            runs_spell(runs, line).then(|| runs.clone())
        });
        runs.unwrap_or_else(|| vec![TextSpan::plain(line.to_string()).with_color(plain)])
    }

    /// The selection the buffer holds: from the anchor to the caret, or `None`
    /// when nothing is selected.
    fn selection(&self) -> Option<std::ops::Range<usize>> {
        let anchor = (*self.anchor.as_ref()?.borrow())?;
        let caret = self.caret_index();
        let (start, end) = if anchor <= caret {
            (anchor, caret)
        } else {
            (caret, anchor)
        };
        (start != end).then_some(start..end)
    }

    /// Move the insertion point, starting a selection when `extend` is set —
    /// from where the caret was when the first `Shift`+arrow arrived — and
    /// dropping the selection when it is not.
    fn move_caret(&mut self, index: usize, extend: bool) {
        if let Some(anchor) = self.anchor.as_ref() {
            if extend {
                if anchor.borrow().is_none() {
                    *anchor.borrow_mut() = Some(self.caret_index());
                }
            } else {
                *anchor.borrow_mut() = None;
            }
        }
        self.set_caret_index(index);
    }

    /// The index one line up or down from the caret, keeping the column it sits
    /// in where the line it lands on is long enough.
    fn vertical_index(&self, chars: &[char], caret: usize, up: bool) -> usize {
        let (start, end) = line_bounds(chars, caret);
        let column = caret - start;
        if up {
            if start == 0 {
                return caret;
            }
            let (previous_start, previous_end) = line_bounds(chars, start - 1);
            return (previous_start + column).min(previous_end);
        }
        if end >= chars.len() {
            return caret;
        }
        let next_start = end + 1;
        let (_, next_end) = line_bounds(chars, next_start);
        (next_start + column).min(next_end)
    }

    /// One key against a multi-line buffer: `None` when the key is not the
    /// field's at all, otherwise whether it changed the text (a move reports
    /// `false`). A key the field does not model is left to the rest of the app,
    /// so a pane never swallows a shortcut it does not answer.
    fn multiline_key(&mut self, key: &str, modifiers: &ModifiersState) -> Option<bool> {
        let chars: Vec<char> = self.value.chars().collect();
        let caret = self.caret_index();
        let extend = modifiers.shift;
        match key {
            "ArrowLeft" => {
                let index = match self.selection() {
                    Some(range) if !extend => range.start,
                    _ => caret.saturating_sub(1),
                };
                self.move_caret(index, extend);
                Some(false)
            }
            "ArrowRight" => {
                let index = match self.selection() {
                    Some(range) if !extend => range.end,
                    _ => (caret + 1).min(chars.len()),
                };
                self.move_caret(index, extend);
                Some(false)
            }
            "ArrowUp" | "ArrowDown" => {
                let index = self.vertical_index(&chars, caret, key == "ArrowUp");
                self.move_caret(index, extend);
                Some(false)
            }
            "Home" => {
                let (start, _) = line_bounds(&chars, caret);
                self.move_caret(start, extend);
                Some(false)
            }
            "End" => {
                let (_, end) = line_bounds(&chars, caret);
                self.move_caret(end, extend);
                Some(false)
            }
            "Backspace" => {
                if self.replace_selection() {
                    return Some(true);
                }
                if caret == 0 {
                    return Some(false);
                }
                remove_char(&mut self.value, caret - 1);
                self.set_caret_index(caret - 1);
                Some(true)
            }
            "Delete" => {
                if self.replace_selection() {
                    return Some(true);
                }
                if caret >= chars.len() {
                    return Some(false);
                }
                remove_char(&mut self.value, caret);
                Some(true)
            }
            "Enter" => {
                if let Some(cb) = self.on_submit.as_ref() {
                    (cb.borrow_mut())(*modifiers);
                    return Some(false);
                }
                self.replace_selection();
                let caret = self.caret_index();
                insert_char(&mut self.value, caret, '\n');
                self.set_caret_index(caret + 1);
                Some(true)
            }
            _ => {
                if modifiers.alt || key.len() != 1 {
                    return None;
                }
                self.replace_selection();
                let caret = self.caret_index();
                insert_char(&mut self.value, caret, key.chars().next().unwrap());
                self.set_caret_index(caret + 1);
                Some(true)
            }
        }
    }

    /// Replace the selected characters with nothing, leaving the caret where
    /// the selection began — what typing over a selection and a Backspace or
    /// Delete on one both mean. Reports whether there was a selection.
    fn replace_selection(&mut self) -> bool {
        let Some(range) = self.selection() else {
            return false;
        };
        remove_range(&mut self.value, range.start, range.end);
        self.move_caret(range.start, false);
        true
    }

    /// The character index a press at `position` falls on in a multi-line
    /// buffer: the line the press is in, then the character boundary nearest it
    /// in that line. A press past the end of a line puts the caret at its end.
    fn index_at_point(&self, position: Vector2F, left: f32, top: f32) -> usize {
        let row = (((position.y - top) / self.line_box()).floor().max(0.0)) as usize;
        let chars: Vec<char> = self.value.chars().collect();
        let mut start = 0usize;
        for _ in 0..row {
            match chars[start..].iter().position(|ch| *ch == '\n') {
                Some(at) => start = start + at + 1,
                // Past the last line: the press stays on it.
                None => break,
            }
        }
        let end = chars[start..]
            .iter()
            .position(|ch| *ch == '\n')
            .map(|at| start + at)
            .unwrap_or(chars.len());
        let line: String = chars[start..end].iter().collect();
        let offset = position.x - left - self.gutter_width();
        start + index_in(&line, offset, FontFamily::Mono).min(end - start)
    }

    /// The width a multi-line row's line-number gutter takes, which is what a
    /// press on that row steps over before it reaches the text.
    fn gutter_width(&self) -> f32 {
        if self.line_numbers == 0 {
            return 0.0;
        }
        measure_text_family(
            &format!("{:>width$} ", 0, width = self.line_numbers),
            TEXT_FONT_SIZE,
            self.line_height,
            f32::INFINITY,
            FontWeight::Regular,
            FontFamily::Mono,
            false,
        )
        .x
    }

    /// The focused row while a visual selection is live: the text is cut at the
    /// selection's edges and the caret's own position, the covered segments get
    /// the selection background, and the caret keeps its place between them.
    #[allow(clippy::too_many_arguments)]
    fn selection_row(
        &self,
        app: &AppContext,
        mut row: Flex,
        display: &str,
        at: usize,
        shape: CaretShape,
        under: Option<char>,
        color: ColorToken,
        selected: std::ops::Range<usize>,
    ) -> Flex {
        let len = display.chars().count();
        let mut bounds = vec![0, selected.start.min(len), at.min(len), selected.end.min(len), len];
        bounds.sort_unstable();
        bounds.dedup();
        for pair in bounds.windows(2) {
            let (mut start, end) = (pair[0], pair[1]);
            if start == at {
                row = row.with_child(caret(app, shape, under));
                // A block or an underline caret carries the character it covers
                // (see `caret_covers_character`), and its cell takes that
                // character's own advance: the segment after the caret starts
                // past it, so the character is not drawn a second time and the
                // row is not a cell wider than the text.
                if caret::caret_covers_character(shape, under) {
                    start += 1;
                }
            }
            if end > start {
                let segment = split_at_char(display, start).1;
                let segment = split_at_char(&segment, end - start).0;
                // Every segment is a run of its own, so each one starts where
                // the pen of the one before it stopped: the caret takes no room
                // and a run measures its ink, which would pull the covered
                // characters out from under the selection background.
                let text = text_run(app, segment, color);
                let text = if start >= selected.start && end <= selected.end {
                    Container::new(text)
                        .with_background(crate::elements::Fill::Solid(app.theme.color(ColorToken::Selected)))
                        .finish()
                } else {
                    text
                };
                row = row.with_child(text);
            }
        }
        if bounds.last() == Some(&at) {
            row = row.with_child(caret(app, shape, under));
        }
        row
    }

    /// The insertion point, clamped to the value's length.
    fn caret_index(&self) -> usize {
        let index = match self.caret.as_ref() {
            Some(caret) => *caret.borrow(),
            None => self.local_caret,
        };
        index.min(self.value.chars().count())
    }

    fn set_caret_index(&mut self, index: usize) {
        let index = index.min(self.value.chars().count());
        match self.caret.as_ref() {
            Some(caret) => *caret.borrow_mut() = index,
            None => self.local_caret = index,
        }
    }

    /// The character index a press `offset` pixels from the field's left edge
    /// falls on: the character boundary nearest the press, measured at the very
    /// size the text is drawn at. A press past the end of the line puts the beam
    /// at the end, where the next character typed joins on.
    fn index_at_offset(&self, offset: f32) -> usize {
        if self.value.is_empty() {
            return 0;
        }
        index_in(&self.display(), offset, FontFamily::System).min(self.value.chars().count())
    }

    /// Hand one key to the modal engine, writing its buffer back when the engine
    /// moved the insertion point or edited the text. The engine owns the mode
    /// and the registers; the field owns the text and the caret.
    fn vim_key(
        &mut self,
        vim: &Rc<RefCell<VimState>>,
        key: &str,
        modifiers: &ModifiersState,
    ) -> VimOutcome {
        let mut buffer = VimBuffer::new(self.value.clone(), self.caret_index());
        // The clipboard handle is cloned out of `self` first: the engine holds
        // it across the call, and the field's own value/caret are written back
        // right after.
        let clipboard = self.clipboard.clone();
        let mut clipboard_borrow: Option<std::cell::RefMut<'_, dyn Clipboard>> = match clipboard.as_ref() {
            Some(cell) => Some(cell.borrow_mut()),
            None => None,
        };
        let outcome = match clipboard_borrow.as_mut() {
            Some(clip) => vim
                .borrow_mut()
                .handle(key, modifiers, &mut buffer, Some(&mut **clip)),
            None => vim.borrow_mut().handle(key, modifiers, &mut buffer, None),
        };
        match outcome {
            VimOutcome::Moved => self.set_caret_index(buffer.caret),
            VimOutcome::Edited => {
                self.value = buffer.text;
                self.set_caret_index(buffer.caret);
            }
            VimOutcome::Passthrough | VimOutcome::Consumed => {}
        }
        self.reveal_caret();
        outcome
    }
}

/// The room `rows` rows take in a windowed buffer without any of them being laid
/// out: what a line the buffer is not drawing stands in as, so the content its
/// scroll region measures is still the whole buffer's height.
fn row_spacer(rows: usize, line_box: f32) -> Box<dyn Element> {
    Empty::new()
        .with_size(vec2f(0.0, rows as f32 * line_box))
        .finish()
}

/// The caret shape a vim mode draws: a bar where typing inserts, a block where
/// a command reads the character under the caret, an underline for replace.
fn caret_shape(mode: crate::vim::VimMode) -> CaretShape {
    use crate::vim::VimMode;
    match mode {
        VimMode::Insert => CaretShape::Bar,
        VimMode::Normal | VimMode::Visual(_) => CaretShape::Block,
        VimMode::Replace => CaretShape::Underline,
    }
}

/// The byte offset of the `index`-th character, or the end of the string.
fn char_offset(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(offset, _)| offset)
}

/// Split `text` around its `index`-th character, for the beam to sit between.
fn split_at_char(text: &str, index: usize) -> (String, String) {
    let offset = char_offset(text, index);
    (text[..offset].to_string(), text[offset..].to_string())
}

/// The text the row draws after the caret. A bar takes no room and covers
/// nothing, so it is the text from the caret on; a block or an underline caret
/// draws the character it covers inside its own cell, which takes that
/// character's advance, so the run after it starts past that character and the
/// character is drawn exactly once.
fn after_the_caret(shape: CaretShape, under: Option<char>, after: &str) -> String {
    if caret::caret_covers_character(shape, under) {
        split_at_char(after, 1).1
    } else {
        after.to_string()
    }
}

/// A probe glyph wide enough that a run measured with it spans the pen.
const PEN_PROBE: &str = "W";

/// Where the pen is after `text`: how far past its own origin the character
/// drawn next starts.
///
/// A run reports the extent of its ink, which is a side bearing short of the
/// pen — and a whole space short when the run ends in whitespace, which has no
/// ink at all. The field cuts the line into runs around the caret, so each cut
/// has to be placed at the pen: measuring the distance the probe glyph moves
/// when it is drawn after `text` gives it, whatever the text ends with.
fn pen_width(text: &str) -> f32 {
    let measured = |value: &str| {
        measure_text_family(
            value,
            TEXT_FONT_SIZE,
            TEXT_LINE_HEIGHT,
            f32::INFINITY,
            FontWeight::Regular,
            FontFamily::System,
            false,
        )
        .x
    };
    (measured(&format!("{text}{PEN_PROBE}")) - measured(PEN_PROBE)).max(0.0)
}

/// One run of the field's text, laid out at the width its pen reaches rather
/// than at what it measures: a row that is cut into runs around the caret has
/// to place each one after a cut where the text's own columns put it.
fn text_run(app: &AppContext, text: String, color: ColorToken) -> Box<dyn Element> {
    let width = pen_width(&text);
    ConstrainedBox::new(Text::new(text).with_theme_color(color, app).finish())
        .with_min_width(width)
        .finish()
}

fn insert_char(text: &mut String, index: usize, ch: char) {
    let offset = char_offset(text, index);
    text.insert(offset, ch);
}

/// Remove the character that begins at `index`, if there is one.
fn remove_char(text: &mut String, index: usize) {
    let offset = char_offset(text, index);
    if let Some(ch) = text[offset..].chars().next() {
        text.replace_range(offset..offset + ch.len_utf8(), "");
    }
}

/// Remove the characters `start..end` from `text`.
fn remove_range(text: &mut String, start: usize, end: usize) {
    if end <= start {
        return;
    }
    let from = char_offset(text, start);
    let to = char_offset(text, end);
    text.replace_range(from..to, "");
}

/// The characters of the line the `index`-th character sits in: where the line
/// starts, and the index of the `\n` that ends it (or the end of the buffer).
fn line_bounds(chars: &[char], index: usize) -> (usize, usize) {
    let index = index.min(chars.len());
    let start = chars[..index]
        .iter()
        .rposition(|ch| *ch == '\n')
        .map(|at| at + 1)
        .unwrap_or(0);
    let end = chars[index..]
        .iter()
        .position(|ch| *ch == '\n')
        .map(|at| index + at)
        .unwrap_or(chars.len());
    (start, end)
}

/// The character index in `line` that a pointer `offset` pixels from its start
/// is nearest to. A press past the end of the line puts the index at the end,
/// where the next character typed joins on.
fn index_in(line: &str, offset: f32, family: FontFamily) -> usize {
    if line.is_empty() || offset <= 0.0 {
        return 0;
    }
    let mut nearest = (0usize, offset.abs());
    let mut boundary = String::new();
    for (index, ch) in line.chars().enumerate() {
        boundary.push(ch);
        let end = measure_text_family(
            &boundary,
            TEXT_FONT_SIZE,
            TEXT_LINE_HEIGHT,
            f32::INFINITY,
            FontWeight::Regular,
            family,
            false,
        )
        .x;
        let distance = (end - offset).abs();
        if distance < nearest.1 {
            nearest = (index + 1, distance);
        }
        if end >= offset {
            break;
        }
    }
    nearest.0
}

/// Whether the runs spell exactly `line`. A run set belongs to the text it was
/// highlighted from, so a line the buffer has changed must not be drawn in the
/// colours of the line that used to be there.
fn runs_spell(runs: &[TextSpan], line: &str) -> bool {
    let mut rest = line;
    for run in runs {
        match rest.strip_prefix(run.text.as_str()) {
            Some(tail) => rest = tail,
            None => return false,
        }
    }
    rest.is_empty()
}

/// The runs that cover the characters `start..end` of the line they spell.
fn slice_runs(runs: &[TextSpan], start: usize, end: usize) -> Vec<TextSpan> {
    let mut pieces = Vec::new();
    let mut at = 0usize;
    for run in runs {
        let length = run.text.chars().count();
        let from = start.max(at);
        let to = end.min(at + length);
        if to > from {
            let mut piece = run.clone();
            piece.text = run.text.chars().skip(from - at).take(to - from).collect();
            pieces.push(piece);
        }
        at += length;
        if at >= end {
            break;
        }
    }
    pieces
}

/// One row's piece of text, painted from the runs it is made of by the same
/// [`Code`] element the file pane draws its read-only lines with. Every row of
/// a multi-line buffer is built this way — the gutter, the plain lines and the
/// caret's own line — so all of them are exactly one line box tall.
fn runs_code(line_height: f32, runs: Vec<TextSpan>) -> Box<dyn Element> {
    Code::new("")
        .with_font_size(TEXT_FONT_SIZE)
        .with_line_height(line_height)
        .with_highlighted_lines(vec![runs])
        .finish()
}

impl Default for TextArea {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for TextArea {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // Rebuild every layout so externally-driven value/focus changes render.
        self.rebuild(app);
        let mut inner_constraint = constraint;
        inner_constraint.min.y = inner_constraint.min.y.max(self.min_height);
        let mut size = self
            .root
            .as_mut()
            .unwrap()
            .layout(inner_constraint, ctx, app);
        // The root is transparent, so keep the reported/hit area at least the
        // requested minimum: the whole composer bar row stays clickable even
        // when the displayed text is empty.
        size.x = size.x.max(inner_constraint.min.x);
        size.y = size.y.max(inner_constraint.min.y);
        // When the field claims its line, the hit area is the whole bounded
        // width rather than the text extent, so a press to the right of the
        // text reaches the field and moves the beam to the end.
        if self.full_width && inner_constraint.max.x.is_finite() {
            size.x = size.x.max(inner_constraint.max.x);
        }
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.as_mut().unwrap().paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        match event {
            DispatchedEvent::MouseDown { position, .. } => {
                if let Some(bounds) = self.bounds() {
                    if bounds.contains(PointF::new(position.x, position.y)) {
                        self.set_focused(true);
                        // The press moves the beam as well as the focus: the
                        // press is turned into the character boundary nearest
                        // it, so the beam lands under the pointer instead of
                        // staying where the last keystroke left it. A press
                        // drops whatever was selected.
                        let index = if self.multiline {
                            self.index_at_point(*position, bounds.min_x(), bounds.min_y())
                        } else {
                            self.index_at_offset(position.x - bounds.min_x())
                        };
                        self.move_caret(index, false);
                        self.reveal_caret();
                        return true;
                    }
                    if self.blur_on_outside_click {
                        self.set_focused(false);
                    }
                }
                false
            }
            DispatchedEvent::KeyDown { key, modifiers } => {
                if !self.focused {
                    return false;
                }
                if let Some(vim) = self.vim.clone() {
                    match self.vim_key(&vim, key, modifiers) {
                        VimOutcome::Passthrough => {}
                        VimOutcome::Consumed => return true,
                        VimOutcome::Moved => return true,
                        VimOutcome::Edited => {
                            if let Some(cb) = self.on_change.as_ref() {
                                (cb.borrow_mut())(self.value.clone());
                            }
                            return true;
                        }
                    }
                }
                // A chorded key is a shortcut the field does not model: the
                // buffer's own handling stops here, so the "s" of Cmd+S saves
                // the file instead of being typed into it as well.
                if self.multiline && (modifiers.command || modifiers.ctrl) {
                    return false;
                }
                if self.multiline {
                    return match self.multiline_key(key, modifiers) {
                        Some(changed) => {
                            self.reveal_caret();
                            if changed {
                                if let Some(cb) = self.on_change.as_ref() {
                                    (cb.borrow_mut())(self.value.clone());
                                }
                            }
                            true
                        }
                        None => false,
                    };
                }
                let caret = self.caret_index();
                let length = self.value.chars().count();
                match key.as_str() {
                    // Moving the beam changes nothing about the value, so it is
                    // consumed without reporting a change.
                    "ArrowLeft" => {
                        self.set_caret_index(caret.saturating_sub(1));
                        return true;
                    }
                    "ArrowRight" => {
                        self.set_caret_index(caret + 1);
                        return true;
                    }
                    "Home" => {
                        self.set_caret_index(0);
                        return true;
                    }
                    "End" => {
                        self.set_caret_index(length);
                        return true;
                    }
                    "Backspace" => {
                        if caret > 0 {
                            remove_char(&mut self.value, caret - 1);
                            self.set_caret_index(caret - 1);
                        }
                    }
                    "Delete" => {
                        if caret < length {
                            remove_char(&mut self.value, caret);
                        }
                    }
                    "Enter" => {
                        if let Some(cb) = self.on_submit.as_ref() {
                            (cb.borrow_mut())(*modifiers);
                            return true;
                        }
                        insert_char(&mut self.value, caret, '\n');
                        self.set_caret_index(caret + 1);
                    }
                    // One typed character is inserted where the beam is, not
                    // appended: a named key the editor does not model is left
                    // to the rest of the app.
                    _ => {
                        if key.len() != 1 {
                            return false;
                        }
                        insert_char(&mut self.value, caret, key.chars().next().unwrap());
                        self.set_caret_index(caret + 1);
                    }
                }
                if let Some(cb) = self.on_change.as_ref() {
                    (cb.borrow_mut())(self.value.clone());
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;

    fn app() -> AppContext {
        AppContext::default()
    }

    #[test]
    fn enter_fires_submit_instead_of_newline() {
        let app = app();
        let submitted = Rc::new(RefCell::new(0));
        let submitted_clone = submitted.clone();
        let mut area = TextArea::new()
            .with_value("hello")
            .with_focused(true)
            .with_on_submit(move |_mods| *submitted_clone.borrow_mut() += 1);
        area.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let handled = area.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Enter".to_string(),
                modifiers: Default::default(),
            },
            &mut event_ctx,
            &app,
        );
        assert!(handled, "Enter should be handled when focused");
        assert_eq!(*submitted.borrow(), 1, "submit callback should fire");
        assert_eq!(
            area.value(),
            "hello",
            "Enter should not insert a newline when submitting"
        );
    }

    /// A placeholder is a guide, not text: the beam leads it, and the first
    /// character the user types opens the line at the start.
    #[test]
    fn the_beam_leads_a_placeholder_and_typing_starts_the_line() {
        let app = app();
        let mut area = TextArea::new()
            .with_placeholder("Ask anything...")
            .with_focused(true);
        area.layout(
            SizeConstraint::loose(Vector2F::new(300.0, 40.0)),
            &mut LayoutContext::default(),
            &app,
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        for key in ["l", "s"] {
            area.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: key.to_string(),
                    modifiers: Default::default(),
                },
                &mut event_ctx,
                &app,
            );
        }
        assert_eq!(
            area.value(),
            "ls",
            "the guide never becomes part of the value the user typed"
        );
    }

    /// The beam is a position in the value, not a mark at its end: the arrow
    /// keys move it and the next character lands where it sits.
    #[test]
    fn the_beam_moves_with_the_arrows_and_typing_lands_where_it_sits() {
        let app = app();
        let caret = Rc::new(RefCell::new(0));
        let mut area = TextArea::new()
            .with_value("abcd")
            .with_focused(true)
            .with_caret(caret.clone());
        let mut event_ctx = EventContext::default();
        let mut press = |area: &mut TextArea, key: &str| {
            area.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: key.to_string(),
                    modifiers: Default::default(),
                },
                &mut event_ctx,
                &app,
            )
        };

        // The beam starts where the host left it, then walks left twice.
        press(&mut area, "End");
        assert_eq!(*caret.borrow(), 4, "End puts the beam after the value");
        press(&mut area, "ArrowLeft");
        press(&mut area, "ArrowLeft");
        assert_eq!(*caret.borrow(), 2, "two lefts move the beam two places");
        press(&mut area, "X");
        assert_eq!(
            area.value(),
            "abXcd",
            "the character lands where the beam sits, not at the end"
        );
        assert_eq!(*caret.borrow(), 3, "the beam advances over what it inserted");

        // Backspace takes the character before the beam, not the last one.
        press(&mut area, "Home");
        press(&mut area, "ArrowRight");
        press(&mut area, "Backspace");
        assert_eq!(area.value(), "bXcd", "Backspace deletes behind the beam");
        assert_eq!(*caret.borrow(), 0, "and the beam stays at the start");
    }

    #[test]
    fn mouse_down_fires_focus_change() {
        let app = app();
        let focus_changes = Rc::new(RefCell::new(Vec::new()));
        let changes_clone = focus_changes.clone();
        // The textarea is transparent, so the clickable area is the text
        // extent; a placeholder gives it a real hit box at (10, 10).
        let mut area = TextArea::new()
            .with_placeholder("Type a message")
            .with_on_focus_change(move |focused| {
                changes_clone.borrow_mut().push(focused);
            });
        area.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        area.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(10.0, 10.0),
                button: 0,
            },
            &mut event_ctx,
            &app,
        );
        assert_eq!(
            *focus_changes.borrow(),
            vec![true],
            "clicking inside should report gaining focus"
        );

        area.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(500.0, 500.0),
                button: 0,
            },
            &mut event_ctx,
            &app,
        );
        assert_eq!(
            *focus_changes.borrow(),
            vec![true, false],
            "clicking outside should report losing focus"
        );
    }

    /// With vim on, Escape leaves insert mode and the next key is a command the
    /// engine runs on the field's own text; `u` puts the edit back.
    #[test]
    fn vim_commands_edit_the_field_and_undo_restores_it() {
        let app = app();
        let vim = Rc::new(RefCell::new(VimState::new()));
        let mut area = TextArea::new()
            .with_value("abc")
            .with_focused(true)
            .with_vim(vim.clone());
        area.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
        let mut event_ctx = EventContext::default();
        let mut press = |area: &mut TextArea, key: &str| {
            area.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: key.to_string(),
                    modifiers: Default::default(),
                },
                &mut event_ctx,
                &app,
            )
        };

        press(&mut area, "Escape");
        assert_eq!(
            vim.borrow().mode(),
            crate::vim::VimMode::Normal,
            "Escape leaves insert mode"
        );
        assert_eq!(vim.borrow().selection(0, 3), None);

        press(&mut area, "x");
        assert_eq!(area.value(), "bc", "x deletes the character under the caret");
        press(&mut area, "u");
        assert_eq!(area.value(), "abc", "u puts the deleted character back");
    }

    /// The field's own keys still work with vim on: an arrow in insert mode is
    /// the engine's passthrough, and the plain caret handling moves the beam.
    #[test]
    fn arrows_still_move_the_caret_with_vim_on() {
        let app = app();
        let vim = Rc::new(RefCell::new(VimState::new()));
        let caret = Rc::new(RefCell::new(3));
        let mut area = TextArea::new()
            .with_value("abc")
            .with_focused(true)
            .with_caret(caret.clone())
            .with_vim(vim);
        area.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
        let mut event_ctx = EventContext::default();
        let handled = area.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "ArrowLeft".to_string(),
                modifiers: Default::default(),
            },
            &mut event_ctx,
            &app,
        );
        assert!(handled, "the arrow is still the field's key");
        assert_eq!(*caret.borrow(), 2, "insert mode keeps the plain arrow move");
    }

    /// A visual selection is the engine's, and the field renders it: the layout
    /// must accept the highlighted run without dropping a character.
    #[test]
    fn a_visual_selection_renders_without_losing_text() {
        let app = app();
        let vim = Rc::new(RefCell::new(VimState::new()));
        let mut area = TextArea::new()
            .with_value("hello")
            .with_focused(true)
            .with_vim(vim.clone());
        let mut event_ctx = EventContext::default();
        let mut press = |area: &mut TextArea, key: &str| {
            area.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: key.to_string(),
                    modifiers: Default::default(),
                },
                &mut event_ctx,
                &app,
            )
        };
        press(&mut area, "Escape");
        press(&mut area, "v");
        press(&mut area, "l");
        press(&mut area, "l");
        assert!(
            vim.borrow().selection(2, 5).is_some(),
            "the selection covers the beam's run"
        );
        area.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);
        assert_eq!(area.value(), "hello", "selection changes no text");
    }

    /// A press in the field moves the beam: it lands on the character boundary
    /// nearest the press, so a click between two letters puts it between them
    /// rather than at the end of the line.
    #[test]
    fn a_press_puts_the_beam_where_the_pointer_is() {
        let app = app();
        let caret = Rc::new(RefCell::new(0));
        let mut area = TextArea::new()
            .with_value("abcd")
            .with_focused(true)
            .with_full_width(true)
            .with_caret(caret.clone());
        area.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 40.0)),
            &mut LayoutContext::default(),
            &app,
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let after_two = measure_text_family(
            "ab",
            TEXT_FONT_SIZE,
            TEXT_LINE_HEIGHT,
            f32::INFINITY,
            FontWeight::Regular,
            FontFamily::System,
            false,
        )
        .x;
        let mut event_ctx = EventContext::default();
        let mut click = |area: &mut TextArea, x: f32| {
            area.dispatch_event(
                &DispatchedEvent::MouseDown {
                    position: vec2f(x, 10.0),
                    button: 0,
                },
                &mut event_ctx,
                &app,
            )
        };

        assert!(click(&mut area, after_two), "the press is the field's");
        assert_eq!(*caret.borrow(), 2, "the beam lands between b and c");

        // Past the end of the line the beam clamps to the end, where the next
        // character typed joins on.
        assert!(click(&mut area, 190.0));
        assert_eq!(*caret.borrow(), 4, "a press in the empty tail ends the line");

        assert!(click(&mut area, 0.0));
        assert_eq!(*caret.borrow(), 0, "a press at the left edge leads the line");

        assert!(
            !click(&mut area, 400.0),
            "a press outside the field is not its own"
        );
        assert_eq!(*caret.borrow(), 0, "and it leaves the beam where it was");
    }

    /// A field that does not claim its line keeps its hit box at the text, so a
    /// control sharing the row keeps its own clicks.
    #[test]
    fn a_field_that_does_not_claim_its_line_keeps_its_hit_box_at_the_text() {
        let app = app();
        let mut area = TextArea::new().with_value("a");
        let size = area.layout(
            SizeConstraint::loose(Vector2F::new(200.0, 40.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(
            size.x < 200.0,
            "the hit box is the text extent, not the whole line, got {}",
            size.x
        );
        area.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        assert!(
            !area.dispatch_event(
                &DispatchedEvent::MouseDown {
                    position: vec2f(150.0, 10.0),
                    button: 0,
                },
                &mut event_ctx,
                &app,
            ),
            "a press right of the text belongs to whatever sits beside the field"
        );
    }

    /// The block caret is a filled cell with the letter it covers drawn on top
    /// of it in the cell's background colour, so a letter under the beam is
    /// still readable instead of disappearing behind the block.
    #[test]
    fn the_block_caret_draws_the_letter_it_covers() {
        use crate::render::RenderCommand;
        use crate::test_util::render_element;

        let app = app();
        let vim = Rc::new(RefCell::new(VimState::new()));
        let mut area: Box<dyn Element> = Box::new(
            TextArea::new()
                .with_value("hello")
                .with_focused(true)
                .with_vim(vim.clone()),
        );
        let mut event_ctx = EventContext::default();
        area.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Escape".to_string(),
                modifiers: Default::default(),
            },
            &mut event_ctx,
            &app,
        );
        assert_eq!(vim.borrow().mode(), crate::vim::VimMode::Normal);

        let commands = render_element(&mut area, vec2f(200.0, 40.0), &app);
        let covered = commands.iter().find_map(|command| match command {
            RenderCommand::DrawText { text, color, .. } if text == "h" => Some(*color),
            _ => None,
        });
        assert_eq!(
            covered,
            Some(app.theme.color(ColorToken::Bg)),
            "the letter under the block is drawn in the cell's background colour"
        );
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, RenderCommand::FillRect { .. })),
            "the block itself is a filled cell under the letter"
        );
    }

    /// The beam adds nothing to the line it sits in: the text after it is drawn
    /// where the pen of the text before it stops, so the letters keep their
    /// places as the caret moves along the line — and the bar is painted over
    /// the character it reaches, before that character is drawn, so what it
    /// covers stays legible on the focus fill.
    #[test]
    fn the_beam_takes_no_room_and_stays_under_the_letter_it_reaches() {
        use crate::elements::caret::CARET_WIDTH;
        use crate::render::RenderCommand;
        use crate::test_util::render_element;

        let app = app();
        let focus = app.theme.color(ColorToken::Focus);
        let caret = Rc::new(RefCell::new(3));
        let mut area: Box<dyn Element> = Box::new(
            TextArea::new()
                .with_value("abcdef")
                .with_focused(true)
                .with_caret(Rc::clone(&caret)),
        );
        let commands = render_element(&mut area, vec2f(200.0, 40.0), &app);

        let before = pen_width("abc");
        let beam_at = commands
            .iter()
            .position(|command| {
                matches!(command, RenderCommand::FillRect { color, .. } if *color == focus)
            })
            .expect("the focused field paints its beam");
        let beam = match &commands[beam_at] {
            RenderCommand::FillRect { rect, .. } => *rect,
            _ => unreachable!(),
        };
        let (tail_at, tail) = commands
            .iter()
            .enumerate()
            .find_map(|(index, command)| match command {
                RenderCommand::DrawText { origin, text, .. } if text == "def" => {
                    Some((index, *origin))
                }
                _ => None,
            })
            .expect("the text after the beam is drawn");
        assert!(
            (beam.min_x() - before).abs() < 1.0,
            "the beam starts where the characters before it end: {before} against {}",
            beam.min_x()
        );
        assert!(
            (tail.x - beam.min_x()).abs() < 1.0,
            "the text after the beam starts on the beam: {} against {}",
            tail.x,
            beam.min_x()
        );
        // The bar is thicker than the beam it replaces, reaches over the
        // character after it, and is painted before that character is drawn.
        assert!(
            (beam.width() - CARET_WIDTH).abs() < 0.5 && CARET_WIDTH > 3.0,
            "the bar is a thick one: {beam:?}"
        );
        assert!(
            tail.x < beam.max_x(),
            "the bar reaches over the character after it: {beam:?} against {tail:?}"
        );
        assert!(
            beam_at < tail_at,
            "the bar is painted before the character it covers, so the letter stays drawn"
        );
    }

    /// Where the caret is does not change where the letters are: the run after
    /// it starts at the pen of the run before it at every index, a prefix that
    /// ends in a space included — a space has no ink, so a row that placed the
    /// run after the caret by what the prefix measures would pull the rest of
    /// the line a space to the left whenever the caret sat after one.
    #[test]
    fn the_text_keeps_its_columns_wherever_the_caret_sits() {
        use crate::render::RenderCommand;
        use crate::test_util::render_element;

        let app = app();
        // (caret, prefix, the run the row draws after the caret)
        let cases = [(0usize, "", "ab cd"), (1, "a", "b cd"), (2, "ab", " cd"), (3, "ab ", "cd")];
        for (at, prefix, tail) in cases {
            let caret = Rc::new(RefCell::new(at));
            let mut area: Box<dyn Element> = Box::new(
                TextArea::new()
                    .with_value("ab cd")
                    .with_focused(true)
                    .with_caret(Rc::clone(&caret)),
            );
            let commands = render_element(&mut area, vec2f(200.0, 40.0), &app);
            let (origin, drawn) = commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawText { origin, text, .. } if text == tail => {
                        Some((*origin, text.clone()))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the run after the caret ({tail:?}) is drawn"));
            assert_eq!(drawn, tail);
            assert!(
                (origin.x - pen_width(prefix)).abs() < 0.5,
                "the run after the caret at {at} starts where the pen is: {} against {}",
                origin.x,
                pen_width(prefix)
            );
            // The caret cuts the line into runs without changing what it says.
            let said: String = commands
                .iter()
                .filter_map(|command| match command {
                    RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(said, "ab cd", "the caret at {at} leaves the text alone");
        }
    }

    /// A block caret on a space is a cell the space's own advance wide: a space
    /// has no ink to measure, so a cell sized by what the character draws would
    /// vanish and pull the characters after it across the line.
    #[test]
    fn the_block_caret_on_a_space_covers_it_and_keeps_the_columns() {
        use crate::render::RenderCommand;
        use crate::test_util::render_element;

        let app = app();
        let focus = app.theme.color(ColorToken::Focus);
        let caret = Rc::new(RefCell::new(0));
        let vim = Rc::new(RefCell::new(VimState::new()));
        let mut area: Box<dyn Element> = Box::new(
            TextArea::new()
                .with_value("a b")
                .with_focused(true)
                .with_caret(Rc::clone(&caret))
                .with_vim(vim.clone()),
        );
        area.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Escape".to_string(),
                modifiers: Default::default(),
            },
            &mut EventContext::default(),
            &app,
        );
        assert_eq!(vim.borrow().mode(), crate::vim::VimMode::Normal);
        // Normal mode holds the block over the character under the caret; the
        // space between the two letters is the one that has no ink.
        *caret.borrow_mut() = 1;

        // Where the pen stops, measured on the font: the distance the probe
        // glyph moves when it is drawn after the text.
        let pen = |text: &str| {
            measure_text_family(
                &format!("{text}W"),
                TEXT_FONT_SIZE,
                TEXT_LINE_HEIGHT,
                f32::INFINITY,
                FontWeight::Regular,
                FontFamily::System,
                false,
            )
            .x - measure_text_family(
                "W",
                TEXT_FONT_SIZE,
                TEXT_LINE_HEIGHT,
                f32::INFINITY,
                FontWeight::Regular,
                FontFamily::System,
                false,
            )
            .x
        };
        let space = pen("a ") - pen("a");
        assert!(space > 0.0, "a space advances the pen: {space}");

        let commands = render_element(&mut area, vec2f(200.0, 40.0), &app);
        let cell = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. } if *color == focus => Some(*rect),
                _ => None,
            })
            .expect("the block caret is a filled cell");
        assert!(
            (cell.min_x() - pen("a")).abs() < 1.0,
            "the cell starts where the space does: {} against {}",
            cell.min_x(),
            pen("a")
        );
        assert!(
            (cell.width() - space).abs() < 1.0,
            "the cell is the space's own advance ({space}): {cell:?}"
        );
        let tail = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. } if text == "b" => Some(*origin),
                _ => None,
            })
            .expect("the character after the space is drawn");
        assert!(
            (tail.x - cell.max_x()).abs() < 1.0,
            "the text after the caret starts where its cell ends: {} against {}",
            tail.x,
            cell.max_x()
        );
    }

    /// The beam is the focus blue — the colour the reference's editor cursor
    /// carries — and a blurred field draws no beam at all.
    #[test]
    fn the_focused_field_paints_its_beam_in_the_focus_blue() {
        use crate::elements::caret::{CARET_HEIGHT, CARET_WIDTH};
        use crate::render::RenderCommand;
        use crate::test_util::render_element;

        let app = app();
        let focus = app.theme.color(ColorToken::Focus);

        let mut area: Box<dyn Element> =
            Box::new(TextArea::new().with_value("hi").with_focused(true));
        let commands = render_element(&mut area, vec2f(200.0, 40.0), &app);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { rect, color, .. }
                    if *color == focus
                        && (rect.width() - CARET_WIDTH).abs() < 0.5
                        && (rect.height() - CARET_HEIGHT).abs() < 0.5
            )),
            "the focused field draws its beam in the focus blue: {commands:?}"
        );

        let mut blurred: Box<dyn Element> = Box::new(TextArea::new().with_value("hi"));
        let commands = render_element(&mut blurred, vec2f(200.0, 40.0), &app);
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { color, .. } if *color == focus
            )),
            "a blurred field draws no beam: {commands:?}"
        );
    }

    /// A multi-line field is one row per line of its buffer, with a padded line
    /// number in front of each and a beam exactly one line box tall, so the rows
    /// below the caret never move as it moves.
    #[test]
    fn a_multiline_field_draws_one_row_per_line_with_its_own_beam() {
        use crate::elements::caret::CARET_WIDTH;
        use crate::render::RenderCommand;
        use crate::test_util::render_element;

        let app = app();
        let focus = app.theme.color(ColorToken::Focus);
        let caret = Rc::new(RefCell::new(0));
        let mut area: Box<dyn Element> = Box::new(
            TextArea::new()
                .with_value("fn main() {\n    run();\n}")
                .with_multiline(true)
                .with_line_numbers(5)
                .with_line_height(1.35)
                .with_caret(Rc::clone(&caret))
                .with_focused(true),
        );
        let commands = render_element(&mut area, vec2f(400.0, 200.0), &app);
        let line_box = (TEXT_FONT_SIZE * 1.35).ceil();
        let beam = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. } if *color == focus => Some(*rect),
                _ => None,
            })
            .expect("the focused field paints its beam");
        assert!(
            (beam.width() - CARET_WIDTH).abs() < 0.5 && (beam.height() - line_box).abs() < 0.5,
            "the beam is one line box tall: {beam:?} against {line_box}"
        );
        for (line, number) in [
            ("fn main() {", "    1 "),
            ("    run();", "    2 "),
            ("}", "    3 "),
        ] {
            assert!(
                commands.iter().any(
                    |command| matches!(command, RenderCommand::DrawText { text, .. } if text == line)
                ),
                "the line {line:?} is drawn"
            );
            assert!(
                commands.iter().any(
                    |command| matches!(command, RenderCommand::DrawText { text, .. } if text == number)
                ),
                "the line's number {number:?} is drawn"
            );
        }
    }

    /// The keys a multi-line buffer answers: a character lands where the beam
    /// is, Enter opens a line, Backspace and Delete join one, and the arrows
    /// walk lines rather than the whole buffer.
    #[test]
    fn a_multiline_buffer_takes_characters_enter_and_the_editing_keys() {
        let app = app();
        let value = Rc::new(RefCell::new(String::new()));
        let reported = Rc::clone(&value);
        let caret = Rc::new(RefCell::new(5));
        let anchor = Rc::new(RefCell::new(None));
        let mut area = TextArea::new()
            .with_value("ab\ncd")
            .with_multiline(true)
            .with_focused(true)
            .with_caret(Rc::clone(&caret))
            .with_anchor(Rc::clone(&anchor))
            .with_on_change(move |text| *reported.borrow_mut() = text);
        let mut event_ctx = EventContext::default();
        let mut press = |area: &mut TextArea, key: &str| {
            area.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: key.to_string(),
                    modifiers: Default::default(),
                },
                &mut event_ctx,
                &app,
            )
        };

        press(&mut area, "End");
        assert_eq!(*caret.borrow(), 5, "End is the end of the caret's own line");
        press(&mut area, "X");
        assert_eq!(area.value(), "ab\ncdX", "the character lands at the beam");
        assert_eq!(*value.borrow(), "ab\ncdX", "and the host is told the text");

        press(&mut area, "Home");
        assert_eq!(*caret.borrow(), 3, "Home is the start of the caret's own line");
        press(&mut area, "Enter");
        assert_eq!(area.value(), "ab\n\ncdX", "Enter opens a line at the beam");
        assert_eq!(*caret.borrow(), 4, "and the beam leads the new line");

        press(&mut area, "Backspace");
        assert_eq!(area.value(), "ab\ncdX", "Backspace closes the line again");
        press(&mut area, "Delete");
        assert_eq!(area.value(), "ab\ndX", "Delete takes the character after the beam");
        assert_eq!(*caret.borrow(), 3, "and leaves the beam where it was");

        press(&mut area, "ArrowRight");
        press(&mut area, "ArrowRight");
        assert_eq!(*caret.borrow(), 5, "the beam walks to the end of the last line");
        press(&mut area, "ArrowUp");
        assert_eq!(*caret.borrow(), 2, "Up lands on the line above, in the same column");
        press(&mut area, "ArrowDown");
        assert_eq!(*caret.borrow(), 5, "and Down comes back to the same column");
    }

    /// Shift+arrows select: the field remembers where the selection began, and
    /// a key that replaces the selection takes all of it at once.
    #[test]
    fn shift_arrows_select_and_a_key_replaces_the_selection() {
        use crate::render::RenderCommand;
        use crate::test_util::render_element;

        let app = app();
        let caret = Rc::new(RefCell::new(4));
        let anchor = Rc::new(RefCell::new(None));
        let value = Rc::new(RefCell::new(String::new()));
        let reported = Rc::clone(&value);
        let mut area: Box<dyn Element> = Box::new(
            TextArea::new()
                .with_value("ab\ncd")
                .with_multiline(true)
                .with_focused(true)
                .with_caret(Rc::clone(&caret))
                .with_anchor(Rc::clone(&anchor))
                .with_on_change(move |text| *reported.borrow_mut() = text),
        );
        let mut event_ctx = EventContext::default();
        let shift = ModifiersState {
            shift: true,
            ..Default::default()
        };
        area.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "ArrowRight".to_string(),
                modifiers: shift,
            },
            &mut event_ctx,
            &app,
        );
        assert_eq!(*caret.borrow(), 5, "the shift-extended beam moves");
        assert_eq!(*anchor.borrow(), Some(4), "and the selection began where it was");

        let commands = render_element(&mut area, vec2f(400.0, 200.0), &app);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { color, .. }
                    if *color == app.theme.color(ColorToken::Selected)
            )),
            "the selected character is drawn over its band: {commands:?}"
        );

        // A character typed over a selection replaces all of it.
        area.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Z".to_string(),
                modifiers: Default::default(),
            },
            &mut event_ctx,
            &app,
        );
        assert_eq!(*value.borrow(), "ab\ncZ", "the selection was replaced");
        assert_eq!(*anchor.borrow(), None, "and the selection is over");
    }

    /// A chord is a shortcut, not a character: a key that arrives with the
    /// command or control key held is left to the app, so Cmd+S saves the file
    /// instead of typing an `s` into it.
    #[test]
    fn a_chorded_key_is_never_typed_into_a_multiline_buffer() {
        let app = app();
        let caret = Rc::new(RefCell::new(5));
        let mut area = TextArea::new()
            .with_value("ab\ncd")
            .with_multiline(true)
            .with_focused(true)
            .with_caret(caret);
        let mut event_ctx = EventContext::default();
        for modifiers in [
            ModifiersState {
                command: true,
                ..Default::default()
            },
            ModifiersState {
                ctrl: true,
                ..Default::default()
            },
        ] {
            let handled = area.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: "s".to_string(),
                    modifiers,
                },
                &mut event_ctx,
                &app,
            );
            assert!(!handled, "the field leaves the chord to the app");
            assert_eq!(area.value(), "ab\ncd", "and types nothing");
        }
    }

    /// A blurred field is nobody's: it takes no key at all, so a pane that is
    /// not the focused one never swallows what its neighbour needs.
    #[test]
    fn a_blurred_multiline_field_takes_no_key() {
        let app = app();
        let mut area = TextArea::new()
            .with_value("ab\ncd")
            .with_multiline(true)
            .with_focused(false);
        let mut event_ctx = EventContext::default();
        for key in ["x", "Backspace", "Delete", "Enter", "ArrowLeft", "Home", "End"] {
            assert!(
                !area.dispatch_event(
                    &DispatchedEvent::KeyDown {
                        key: key.to_string(),
                        modifiers: Default::default(),
                    },
                    &mut event_ctx,
                    &app,
                ),
                "{key} is left to the app"
            );
        }
        assert_eq!(area.value(), "ab\ncd", "and nothing was typed");
    }
    /// A multi-line buffer the host put in a scroll region, as a file pane does:
    /// the field and the region share one `ScrollState`, and the buffer is
    /// `lines` lines long.
    fn windowed_field(
        lines: usize,
        caret: Rc<RefCell<usize>>,
    ) -> (Box<dyn Element>, Rc<RefCell<ScrollState>>) {
        use crate::elements::{Axis, Scrollable};

        let value: String = (0..lines).map(|line| format!("line {line}\n")).collect();
        let scroll = Rc::new(RefCell::new(ScrollState::default()));
        let field = TextArea::new()
            .with_value(value)
            .with_multiline(true)
            .with_line_numbers(5)
            .with_line_height(1.35)
            .with_min_height(0.0)
            .with_focused(true)
            .with_caret(caret)
            .with_scroll_state(Rc::clone(&scroll));
        (
            Box::new(
                Scrollable::new(field.finish(), Axis::Vertical).with_state(Rc::clone(&scroll)),
            ),
            scroll,
        )
    }

    /// One frame of `root`, as the commands it drew.
    fn frame_of(
        root: &mut Box<dyn Element>,
        app: &AppContext,
        size: Vector2F,
    ) -> Vec<crate::render::RenderCommand> {
        let _ = root.layout(SizeConstraint::loose(size), &mut LayoutContext::default(), app);
        let mut ctx = PaintContext::new(crate::render::Renderer::new());
        root.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    /// The line numbers the frame drew in the gutter, in order.
    fn drawn_line_numbers(commands: &[crate::render::RenderCommand]) -> Vec<usize> {
        commands
            .iter()
            .filter_map(|command| match command {
                crate::render::RenderCommand::DrawText { text, .. } => {
                    let padded = text.strip_suffix(' ')?;
                    // The gutter is right-aligned, so a small number carries
                    // leading spaces that `parse` would reject.
                    (padded.len() == 5 && padded.trim().chars().all(|ch| ch.is_ascii_digit()))
                        .then(|| padded.trim().parse().ok())?
                }
                _ => None,
            })
            .collect()
    }

    /// A multi-line buffer is laid out at most a screenful at a time: a buffer of
    /// two thousand lines costs the rows the field's own region can show, not the
    /// whole buffer, while the region still measures the whole buffer's height —
    /// the lines outside the window stand in as spacers.
    #[test]
    fn a_windowed_buffer_lays_out_only_the_rows_its_viewport_shows() {
        let app = app();
        let lines = 2_000usize;
        let caret = Rc::new(RefCell::new(0));
        let (mut root, scroll) = windowed_field(lines, caret);
        let size = vec2f(600.0, 2_000.0);
        let line_box = (TEXT_FONT_SIZE * 1.35).ceil();

        // The first frame has no viewport measured yet, so it draws a screenful.
        let first = frame_of(&mut root, &app, size);
        assert_eq!(
            drawn_line_numbers(&first).len(),
            FIRST_FRAME_ROWS + WINDOW_SLACK,
            "the frame with no viewport yet draws a screenful, not the buffer"
        );

        let second = frame_of(&mut root, &app, size);
        let drawn = drawn_line_numbers(&second);
        let (viewport, max_offset) = {
            let scroll = scroll.borrow();
            (scroll.viewport(), scroll.max_offset())
        };
        assert!(viewport > 0.0, "the region measured a viewport");
        let rows = (viewport / line_box).ceil() as usize;
        assert_eq!(
            drawn.len(),
            rows + WINDOW_SLACK,
            "the frame with a viewport draws its own rows and the window's slack, \
             {rows} rows in {viewport} points"
        );
        assert_eq!(drawn[0], 1, "and starts at the top of the buffer");
        // The buffer's value ends in a newline, so it has one more line than it
        // has newlines: a last, empty one.
        let scroll_range = (lines + 1) as f32 * line_box - viewport;
        assert_eq!(
            max_offset,
            scroll_range,
            "the rows outside the window stand in at their own height, so the region \
             still scrolls through the whole buffer"
        );
    }

    /// The caret is brought into view when it moves off the window — and only
    /// then: a buffer scrolled by hand keeps the place the user put it, even
    /// though the caret is left outside the window.
    #[test]
    fn the_caret_is_brought_into_view_only_when_it_moves_off_the_window() {
        let app = app();
        let lines = 2_000usize;
        let value: String = (0..lines).map(|line| format!("line {line}\n")).collect();
        // The first character of the line 1_500 of the buffer.
        let far = value
            .match_indices('\n')
            .nth(1_499)
            .map(|(offset, _)| offset + 1)
            .expect("the buffer has that many lines");
        let caret = Rc::new(RefCell::new(far));
        let (mut root, scroll) = windowed_field(lines, Rc::clone(&caret));
        let size = vec2f(600.0, 2_000.0);
        let line_box = (TEXT_FONT_SIZE * 1.35).ceil();

        let _ = frame_of(&mut root, &app, size);
        let commands = frame_of(&mut root, &app, size);
        let drawn = drawn_line_numbers(&commands);
        assert_eq!(drawn[0], 1, "the buffer opens at its first line");
        assert!(
            !drawn.contains(&1_501),
            "and the caret's own row is outside the window: {drawn:?}"
        );
        assert_eq!(scroll.borrow().offset(), 0.0, "nothing scrolled it");

        // A key that moves the caret brings its row into the window.
        let mut ctx = EventContext::default();
        assert!(root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "ArrowUp".to_string(),
                modifiers: Default::default(),
            },
            &mut ctx,
            &app,
        ));
        let commands = frame_of(&mut root, &app, size);
        let drawn = drawn_line_numbers(&commands);
        assert!(
            drawn.contains(&1_501),
            "the caret's row is in the window after the move: {drawn:?}"
        );
        let (offset, viewport) = {
            let scroll = scroll.borrow();
            (scroll.offset(), scroll.viewport())
        };
        let rows = (viewport / line_box).ceil() as usize;
        let first = (offset / line_box).floor() as usize;
        assert!(
            (1_499..1_500).all(|line| line >= first && line < first + rows),
            "the caret's own row 1500 sits inside the rows the pane shows: \
             {first}..{} against the caret's line",
            first + rows
        );
        assert!(
            drawn[0] > 1,
            "the window followed the caret down the buffer: {drawn:?}"
        );
        let moved = caret.borrow().to_owned();
        assert!(
            value[moved..].starts_with("line 1499\n"),
            "and the key moved the caret to the head of the line above"
        );

        // A move inside the window scrolls nothing.
        let before = scroll.borrow().offset();
        assert!(root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "ArrowUp".to_string(),
                modifiers: Default::default(),
            },
            &mut ctx,
            &app,
        ));
        assert_eq!(
            scroll.borrow().offset(),
            before,
            "a move within the window leaves the region where it was"
        );

        // Scrolling the buffer by hand is not undone by the caret it leaves
        // outside the window.
        scroll.borrow_mut().reset();
        let _ = frame_of(&mut root, &app, size);
        let commands = frame_of(&mut root, &app, size);
        assert_eq!(scroll.borrow().offset(), 0.0, "the buffer is back at its top");
        assert_eq!(
            drawn_line_numbers(&commands)[0],
            1,
            "and the window is drawn from the top again"
        );
    }
}
