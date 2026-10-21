//! The text a surface drew, recorded so a pointer can select and copy it.
//!
//! The tree is rebuilt every frame and its text elements draw whole runs at
//! once, so nothing in the tree knows where a character landed. A surface that
//! wants grok-build's drag-to-copy records what each element painted here: one
//! entry per visual line, with the run's text and the line's box. A pointer is
//! then a `(line, column)` pair in that recording, which is also what the
//! selection and the clipboard read.

use crate::geometry::{rectf, RectF, Vector2F};
use crate::platform::text_atlas::{advance_width, mono_advance, FontWeight};
use crate::theme::FontFamily;

/// A character position in the recorded text: the visual line in paint order
/// and the character column in that line's own text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SelectionPoint {
    pub line: usize,
    pub column: usize,
}

impl SelectionPoint {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// The face a run is measured with, so a column is the character the renderer
/// actually drew there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunMetrics {
    pub font_size: f32,
    pub line_height: f32,
    pub family: FontFamily,
    pub weight: FontWeight,
    pub italic: bool,
}

impl RunMetrics {
    pub const fn new(
        font_size: f32,
        line_height: f32,
        family: FontFamily,
        weight: FontWeight,
        italic: bool,
    ) -> Self {
        Self {
            font_size,
            line_height,
            family,
            weight,
            italic,
        }
    }

    /// The metrics a monospaced terminal/code run is drawn with.
    pub const fn mono(font_size: f32, line_height: f32) -> Self {
        Self::new(font_size, line_height, FontFamily::Mono, FontWeight::Regular, false)
    }

    fn advance(&self, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        advance_width(
            text,
            self.font_size,
            self.weight,
            self.family,
            self.italic,
        )
    }

    /// The width one monospaced cell is drawn at. Drawing rounds every glyph
    /// advance up to a whole pixel, so the cell is the rounded advance.
    fn mono_width(&self) -> f32 {
        mono_advance(self.font_size, self.weight).ceil().max(0.1)
    }
}

/// One drawn run: text placed at `left` on the line it belongs to.
#[derive(Clone, Debug)]
pub struct SelectionRun {
    /// The run's left edge, in window coordinates.
    pub left: f32,
    /// The top of the row the run is drawn on, in window coordinates.
    pub top: f32,
    pub height: f32,
    pub text: String,
    pub metrics: RunMetrics,
}

impl SelectionRun {
    pub fn new(
        left: f32,
        top: f32,
        height: f32,
        text: impl Into<String>,
        metrics: RunMetrics,
    ) -> Self {
        Self {
            left,
            top,
            height,
            text: text.into(),
            metrics,
        }
    }

    /// The x the run's `index`th character is drawn at, relative to the run's
    /// left edge.
    fn char_advance(&self, index: usize) -> f32 {
        let prefix: String = self.text.chars().take(index).collect();
        self.metrics.advance(&prefix)
    }

    /// The character under `offset_x` (distance from the run's left edge).
    fn char_at(&self, offset_x: f32) -> usize {
        let count = self.text.chars().count();
        if count == 0 {
            return 0;
        }
        if self.metrics.family == FontFamily::Mono {
            let width = self.metrics.mono_width();
            return ((offset_x / width).floor().max(0.0) as usize).min(count - 1);
        }
        // The last character whose left edge is at or before the pointer.
        let mut low = 0usize;
        let mut high = count;
        while low < high {
            let mid = (low + high).div_ceil(2);
            if self.char_advance(mid) <= offset_x {
                low = mid;
            } else {
                high = mid - 1;
            }
        }
        low.saturating_sub(1).min(count - 1)
    }
}

/// One visual line: the runs drawn on the same row, in reading order.
#[derive(Clone, Debug)]
pub struct SelectableLine {
    pub left: f32,
    pub top: f32,
    pub height: f32,
    pub runs: Vec<SelectionRun>,
}

impl SelectableLine {
    /// The line's text as it was drawn: the runs concatenated.
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }

    pub fn char_count(&self) -> usize {
        self.runs.iter().map(|run| run.text.chars().count()).sum()
    }

    /// The x of `column`, relative to the line's left edge. A column past the
    /// line's end is its end.
    pub fn x_for_column(&self, column: usize) -> f32 {
        let mut seen = 0usize;
        for run in &self.runs {
            let count = run.text.chars().count();
            if column < seen + count {
                return run.left - self.left + run.char_advance(column - seen);
            }
            seen += count;
        }
        self.runs
            .last()
            .map(|run| run.left - self.left + run.metrics.advance(&run.text))
            .unwrap_or(0.0)
    }

    /// The x where `column` ends: the right edge of the character itself.
    pub fn x_after_column(&self, column: usize) -> f32 {
        self.x_for_column(column) + self.column_width(column)
    }

    fn column_width(&self, column: usize) -> f32 {
        let mut seen = 0usize;
        for run in &self.runs {
            let count = run.text.chars().count();
            if column < seen + count {
                let index = column - seen;
                let ch: String = run.text.chars().skip(index).take(1).collect();
                return run.metrics.advance(&ch);
            }
            seen += count;
        }
        0.0
    }

    /// The column under `x` (window coordinates).
    pub fn column_at(&self, x: f32) -> usize {
        let mut seen = 0usize;
        let last = self.runs.len().saturating_sub(1);
        for (index, run) in self.runs.iter().enumerate() {
            let count = run.text.chars().count();
            let end = run.left + run.metrics.advance(&run.text);
            // A pointer past the last run's ink belongs to its last character.
            if x < end || index == last {
                return seen + run.char_at((x - run.left).max(0.0));
            }
            seen += count;
        }
        0
    }
}

/// A drag selection over a surface's recorded text.
///
/// The surface records the lines it painted every frame; a press freezes that
/// recording here, so the pointer is read against the text the user pressed on
/// even while the transcript keeps streaming underneath. The selection stays
/// visible after the button comes up (the way grok-build's does) until the next
/// press outside the text clears it.
#[derive(Clone, Debug, Default)]
pub struct TranscriptSelection {
    text: SelectableText,
    anchor: Option<SelectionPoint>,
    cursor: Option<SelectionPoint>,
    dragging: bool,
}

impl TranscriptSelection {
    /// Start a selection over `text` at `point`.
    pub fn begin(&mut self, text: SelectableText, point: SelectionPoint) {
        self.text = text;
        self.anchor = Some(point);
        self.cursor = Some(point);
        self.dragging = true;
    }

    /// Move the end of the live drag to `point`. A selection that is not being
    /// dragged is left alone.
    pub fn extend(&mut self, point: SelectionPoint) {
        if self.dragging {
            self.cursor = Some(point);
        }
    }

    /// The position under `position` in the frozen recording.
    pub fn point_at(&self, position: Vector2F) -> Option<SelectionPoint> {
        self.text.point_at(position)
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn has_selection(&self) -> bool {
        self.anchor.is_some() && self.cursor.is_some()
    }

    /// Finish the drag. Returns the text to copy when the gesture covered more
    /// than the one character a plain click lands on; a click copies nothing, so
    /// it never replaces the clipboard. The highlight stays until a later press
    /// clears it.
    pub fn finish(&mut self) -> Option<String> {
        self.dragging = false;
        let (anchor, cursor) = (self.anchor?, self.cursor?);
        if anchor == cursor {
            // A click selects one character but copies nothing; it must not
            // leave that character highlighted either.
            self.cancel();
            return None;
        }
        let text = self.text.text_between(anchor, cursor);
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    /// Forget the selection and its highlight.
    pub fn cancel(&mut self) {
        self.text.clear();
        self.anchor = None;
        self.cursor = None;
        self.dragging = false;
    }

    /// The boxes the live selection covers, for the surface to paint.
    pub fn highlight_rects(&self) -> Vec<RectF> {
        match (self.anchor, self.cursor) {
            (Some(anchor), Some(cursor)) => self.text.highlight_rects(anchor, cursor),
            _ => Vec::new(),
        }
    }
}

/// Every visual line a surface drew this frame, in paint order.
#[derive(Clone, Debug, Default)]
pub struct SelectableText {
    pub lines: Vec<SelectableLine>,
}

impl SelectableText {
    pub fn clear(&mut self) {
        self.lines.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Record one run. Runs drawn on the same row join one line; a run on a new
    /// row starts the next one. Elements paint top to bottom, so the row order
    /// is the order the pointer reads.
    pub fn push_run(&mut self, run: SelectionRun) {
        if let Some(line) = self.lines.last_mut() {
            if (line.top - run.top).abs() < 0.5 {
                line.left = line.left.min(run.left);
                line.height = line.height.max(run.height);
                line.runs.push(run);
                return;
            }
        }
        self.lines.push(SelectableLine {
            left: run.left,
            top: run.top,
            height: run.height,
            runs: vec![run],
        });
    }

    /// The position under a pointer. A pointer below the last line belongs to
    /// that line's end, and one above the first to its start, so a drag can
    /// run past the text and still select all of it.
    pub fn point_at(&self, position: Vector2F) -> Option<SelectionPoint> {
        let index = self.line_at(position.y)?;
        let line = &self.lines[index];
        Some(SelectionPoint::new(index, line.column_at(position.x)))
    }

    fn line_at(&self, y: f32) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        // The row the pointer is on, or the nearest row when it is in the gap
        // between two rows.
        let mut best = 0usize;
        let mut best_distance = f32::INFINITY;
        for (index, line) in self.lines.iter().enumerate() {
            let bottom = line.top + line.height;
            let distance = if y < line.top {
                line.top - y
            } else if y > bottom {
                y - bottom
            } else {
                0.0
            };
            if distance < best_distance {
                best_distance = distance;
                best = index;
            }
            if distance == 0.0 {
                return Some(index);
            }
        }
        Some(best)
    }

    /// The text between two positions, in reading order. Each line's own text
    /// is copied, so a wrapped paragraph comes out as the lines the user saw.
    pub fn text_between(&self, anchor: SelectionPoint, cursor: SelectionPoint) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let (start, end) = if anchor <= cursor {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        };
        let last_line = self.lines.len() - 1;
        let end_line = end.line.min(last_line);
        let start_line = start.line.min(end_line);
        let mut out = Vec::with_capacity(end_line - start_line + 1);
        for index in start_line..=end_line {
            let line = &self.lines[index];
            let chars: Vec<char> = line.text().chars().collect();
            if chars.is_empty() {
                out.push(String::new());
                continue;
            }
            let from = if index == start_line {
                start.column.min(chars.len() - 1)
            } else {
                0
            };
            let to = if index == end_line {
                end.column.min(chars.len() - 1)
            } else {
                chars.len() - 1
            };
            if from > to {
                out.push(String::new());
                continue;
            }
            let text: String = chars[from..=to].iter().collect();
            out.push(text.trim_end().to_string());
        }
        out.join("\n")
    }

    /// The boxes a selection covers, in window coordinates, one per line.
    pub fn highlight_rects(
        &self,
        anchor: SelectionPoint,
        cursor: SelectionPoint,
    ) -> Vec<RectF> {
        if self.lines.is_empty() {
            return Vec::new();
        }
        let (start, end) = if anchor <= cursor {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        };
        let last_line = self.lines.len() - 1;
        let end_line = end.line.min(last_line);
        let start_line = start.line.min(end_line);
        let mut out = Vec::with_capacity(end_line - start_line + 1);
        for index in start_line..=end_line {
            let line = &self.lines[index];
            let count = line.char_count();
            if count == 0 {
                continue;
            }
            let from = if index == start_line {
                start.column.min(count - 1)
            } else {
                0
            };
            let to = if index == end_line {
                end.column.min(count - 1)
            } else {
                count - 1
            };
            if from > to {
                continue;
            }
            let x = line.left + line.x_for_column(from);
            let right = line.left + line.x_after_column(to);
            out.push(rectf(x, line.top, (right - x).max(1.0), line.height));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> RunMetrics {
        RunMetrics::mono(13.0, 1.4)
    }

    fn line(top: f32, text: &str) -> SelectableLine {
        SelectableLine {
            left: 10.0,
            top,
            height: 18.2,
            runs: vec![SelectionRun::new(10.0, top, 18.2, text, metrics())],
        }
    }

    #[test]
    fn a_pointer_selects_the_character_it_is_over() {
        let mut text = SelectableText::default();
        text.push_run(SelectionRun::new(10.0, 0.0, 18.2, "hello", metrics()));
        let width = metrics().mono_width();
        let point = text
            .point_at(Vector2F::new(10.0 + width * 2.5, 5.0))
            .expect("the line is recorded");
        assert_eq!(point, SelectionPoint::new(0, 2));
        let past = text
            .point_at(Vector2F::new(10.0 + width * 40.0, 5.0))
            .expect("past the text is the line's end");
        assert_eq!(past.column, 4);
    }

    #[test]
    fn a_drag_copies_every_line_it_covers() {
        let mut text = SelectableText::default();
        text.lines.push(line(0.0, "one"));
        text.lines.push(line(20.0, "two"));
        text.lines.push(line(40.0, "three"));
        let copied = text.text_between(SelectionPoint::new(0, 1), SelectionPoint::new(1, 2));
        assert_eq!(copied, "ne\ntwo");
        let all = text.text_between(SelectionPoint::new(0, 0), SelectionPoint::new(2, 4));
        assert_eq!(all, "one\ntwo\nthree");
    }

    #[test]
    fn the_highlight_runs_from_the_first_character_to_the_last() {
        let mut text = SelectableText::default();
        text.lines.push(line(0.0, "hello"));
        let rects = text.highlight_rects(SelectionPoint::new(0, 1), SelectionPoint::new(0, 3));
        assert_eq!(rects.len(), 1);
        let width = metrics().mono_width();
        assert!((rects[0].min().x - (10.0 + width)).abs() < 0.01);
        assert!((rects[0].width() - width * 3.0).abs() < 0.01);
    }

    #[test]
    fn a_click_copies_nothing_and_leaves_no_highlight() {
        let mut text = SelectableText::default();
        text.lines.push(line(0.0, "hello"));
        let mut selection = TranscriptSelection::default();
        selection.begin(text, SelectionPoint::new(0, 2));
        assert_eq!(selection.finish(), None, "a click never copies");
        assert!(!selection.has_selection());
        assert!(selection.highlight_rects().is_empty());
    }

    #[test]
    fn runs_on_one_row_join_the_same_line() {
        let mut text = SelectableText::default();
        text.push_run(SelectionRun::new(10.0, 0.0, 18.2, "bold ", metrics()));
        text.push_run(SelectionRun::new(50.0, 0.0, 18.2, "plain", metrics()));
        assert_eq!(text.lines.len(), 1);
        assert_eq!(text.lines[0].text(), "bold plain");
        assert_eq!(text.lines[0].char_count(), 10);
    }
}
