//! The reusable file comparison: DSH's unified and side-by-side layouts over
//! the hunks the transcript's review surfaces already parse.
//!
//! [`FileDiffView`] draws the body of a comparison, not a card and not the
//! review tab's toolbar: a caller owns the surrounding surface and hands the
//! view its parsed [`Hunk`]s (or a [`PreparedDiff`] shared across frames), its
//! layout and wrap choices, and optionally the scroll region whose viewport it
//! should paint. The measurements come from [`DiffSpec`], the same numbers the
//! changed-files card uses.
//!
//! The unified layout draws DSH's four columns (`old gutter | new gutter |
//! sign | text`); the split layout draws two equal cells, each with its own
//! gutter. A changed line paints the theme's insert/delete band across the row,
//! a 3 px marker at the left of its gutter, and the gutter fill behind its
//! line number. A split cell with no line on that side paints the empty fill.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use super::model::{header_text, DiffLine, DiffLineKind, DiffRow, Hunk, SplitDiffRow, SplitRow};
use super::paint::{highlight_changed_rows, DiffHighlightSets, DiffSide, PreparedDiff};
use super::spec::DiffSpec;
use crate::color::ColorU;
use crate::elements::{
    AppContext, Element, LayoutContext, PaintContext, Point, ScrollState, SizeConstraint, TextSpan,
};
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::platform::text_atlas::{
    advance_width_exact, measure_text_family, wrapped_lines, FontWeight,
};
use crate::syntax::HighlightedLine;
use crate::theme::{ColorToken, FontFamily};

/// Which of DSH's two comparison layouts to draw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiffLayout {
    /// Two line-number columns, a sign column and the text.
    #[default]
    Unified,
    /// Two equal cells, the old version on the left and the new on the right.
    Split,
}

const FONT_SIZE: f32 = DiffSpec::DIFF_FONT_SIZE;
const LINE_HEIGHT: f32 = DiffSpec::DIFF_LINE_HEIGHT;
const LINE_FACTOR: f32 = DiffSpec::DIFF_LINE_HEIGHT_FACTOR;
const GUTTER_W: f32 = SyncGutter::WIDTH;
const SIGN_W: f32 = SyncSign::WIDTH;

/// `3.5em` at the comparison's 12 px mono size, kept const-foldable.
struct SyncGutter;
impl SyncGutter {
    const WIDTH: f32 = 3.5 * FONT_SIZE;
}
/// `1.2em` at the comparison's 12 px mono size.
struct SyncSign;
impl SyncSign {
    const WIDTH: f32 = 1.2 * FONT_SIZE;
}

/// One painted row's offset and height inside the comparison body.
#[derive(Clone, Copy, Debug, Default)]
struct RowBox {
    top: f32,
    height: f32,
}

/// The reusable unified/side-by-side comparison element.
pub struct FileDiffView {
    hunks: Rc<Vec<Hunk>>,
    layout: DiffLayout,
    wrap: bool,
    show_line_numbers: bool,
    hunk_headers: bool,
    fill_width: bool,
    rows: Rc<Vec<DiffRow>>,
    split_rows: Rc<Vec<SplitDiffRow>>,
    /// Whether the comparison has only additions or only deletions. Such a
    /// file is drawn as one full-width column even when the caller asked for
    /// the side-by-side layout: the absent version would otherwise be an empty
    /// half of the pane (DSH falls back the same way).
    one_sided: bool,
    highlights: Rc<Vec<Option<HighlightedLine>>>,
    old_highlights: Rc<HashMap<u32, HighlightedLine>>,
    new_highlights: Rc<HashMap<u32, HighlightedLine>>,
    viewport: Option<Rc<RefCell<ScrollState>>>,
    row_boxes: Vec<RowBox>,
    body_height: f32,
    content_width: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Default for FileDiffView {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl FileDiffView {
    /// A comparison over parsed hunks, with no syntax colours yet.
    pub fn new(hunks: Vec<Hunk>) -> Self {
        Self::new_shared(Rc::new(hunks))
    }

    /// The same over hunks a host already shares.
    pub fn new_shared(hunks: Rc<Vec<Hunk>>) -> Self {
        Self::from_rows(
            Rc::clone(&hunks),
            Rc::new(super::model::build_rows(&hunks)),
            Rc::new(super::model::build_split_rows(&hunks)),
            DiffHighlightSets::default(),
        )
    }

    /// A comparison whose rows and syntax runs were prepared once, at read
    /// time. This is the constructor a per-frame review wants: it derives no
    /// rows and colours no text here.
    pub fn from_prepared(hunks: Rc<Vec<Hunk>>, prepared: PreparedDiff) -> Self {
        let rows = prepared.rows_shared();
        let split_rows = prepared.split_rows();
        let (old, new) = prepared.highlighted_lines();
        Self::from_rows(
            hunks,
            rows,
            split_rows,
            DiffHighlightSets {
                unified: prepared.highlight_runs(),
                old,
                new,
            },
        )
    }

    fn from_rows(
        hunks: Rc<Vec<Hunk>>,
        rows: Rc<Vec<DiffRow>>,
        split_rows: Rc<Vec<SplitDiffRow>>,
        highlights: DiffHighlightSets,
    ) -> Self {
        Self {
            hunks,
            layout: DiffLayout::Unified,
            wrap: false,
            show_line_numbers: true,
            hunk_headers: true,
            fill_width: true,
            one_sided: rows_are_one_sided(&rows),
            rows,
            split_rows,
            highlights: highlights.unified,
            old_highlights: highlights.old,
            new_highlights: highlights.new,
            viewport: None,
            row_boxes: Vec::new(),
            body_height: 0.0,
            content_width: 0.0,
            size: None,
            origin: None,
        }
    }

    /// Reuse rows and colours prepared once, instead of deriving them here.
    pub fn with_prepared(mut self, prepared: PreparedDiff) -> Self {
        self.rows = prepared.rows_shared();
        self.split_rows = prepared.split_rows();
        self.one_sided = rows_are_one_sided(&self.rows);
        self.highlights = prepared.highlight_runs();
        let (old, new) = prepared.highlighted_lines();
        self.old_highlights = old;
        self.new_highlights = new;
        self
    }

    /// Highlight the changed and context lines as `language`.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        let sets = highlight_changed_rows(&self.rows, &language.into());
        self.highlights = sets.unified;
        self.old_highlights = sets.old;
        self.new_highlights = sets.new;
        self
    }

    /// Draw the side-by-side layout instead of the unified one.
    pub fn with_layout(mut self, layout: DiffLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Draw side by side when `split` is true, unified otherwise.
    pub fn with_split(mut self, split: bool) -> Self {
        self.layout = if split {
            DiffLayout::Split
        } else {
            DiffLayout::Unified
        };
        self
    }

    /// Wrap long lines inside the available text column instead of clipping
    /// them at the element's edge.
    pub fn with_wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Y: the review's inline diff starts directly with the changed lines; it
    /// has no second header and no `@@` band.
    pub fn with_hunk_headers(mut self, show: bool) -> Self {
        self.hunk_headers = show;
        self
    }

    /// Show or hide the line-number gutters.
    pub fn with_line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }

    /// Take the width the layout offers rather than shrinking to the longest
    /// row. Defaults to true: a comparison inside a review pane spans it.
    pub fn with_fill_width(mut self, fill: bool) -> Self {
        self.fill_width = fill;
        self
    }

    /// Skip rows outside the viewport of the scroll region `state` belongs to.
    pub fn with_viewport(mut self, state: Rc<RefCell<ScrollState>>) -> Self {
        self.viewport = Some(state);
        self
    }

    pub fn hunks(&self) -> &[Hunk] {
        self.hunks.as_slice()
    }

    /// The layout the caller asked for, before a one-sided comparison falls
    /// back to a single column.
    pub fn layout_mode(&self) -> DiffLayout {
        self.layout
    }

    /// The layout actually drawn. A one-sided split collapses to
    /// [`DiffLayout::Unified`]; every two-sided comparison keeps its request.
    pub fn resolved_layout(&self) -> DiffLayout {
        if self.layout == DiffLayout::Split && self.one_sided {
            DiffLayout::Unified
        } else {
            self.layout
        }
    }

    /// Whether the comparison has additions on only one version (or deletions
    /// on only one), which is what collapses a requested split to one column.
    pub fn is_one_sided(&self) -> bool {
        self.one_sided
    }

    pub fn wraps(&self) -> bool {
        self.wrap
    }

    /// The unified rows the view draws, gap separators included.
    pub fn display_rows(&self) -> Vec<DiffRow> {
        (*self.rows).clone()
    }

    /// The side-by-side rows the view draws, gap separators included.
    pub fn display_split_rows(&self) -> Vec<SplitDiffRow> {
        (*self.split_rows).clone()
    }

    /// The rows that carry a line of either version, in drawing order.
    fn layout_rows(&self, width: f32) -> (Vec<RowBox>, f32) {
        let mut boxes = Vec::with_capacity(match self.layout {
            DiffLayout::Unified => self.rows.len(),
            DiffLayout::Split => self.split_rows.len(),
        });
        let mut top = DiffSpec::DIFF_BODY_PAD_TOP;
        let mut first_hunk = true;
        let layout = self.resolved_layout();
        match layout {
            DiffLayout::Unified => {
                let wrap_width = self.unified_wrap_width(width);
                for row in self.rows.iter() {
                    match row {
                        DiffRow::Gap { .. } => {}
                        DiffRow::HunkHeader { .. } if !self.hunk_headers => {}
                        DiffRow::HunkHeader { .. } => {
                            if !first_hunk {
                                top += DiffSpec::DIFF_HUNK_GAP;
                            }
                            first_hunk = false;
                            let height = LINE_HEIGHT + DiffSpec::DIFF_HUNK_HEADER_PAD_Y * 2.0;
                            boxes.push(RowBox { top, height });
                            top += height;
                        }
                        DiffRow::Line(line) => {
                            let height =
                                self.wrapped_row_height([Some(&line.text), None], wrap_width);
                            boxes.push(RowBox { top, height });
                            top += height;
                        }
                    }
                }
            }
            DiffLayout::Split => {
                let wrap_width = self.split_wrap_width(width);
                for row in self.split_rows.iter() {
                    match row {
                        SplitDiffRow::Gap { .. } => {}
                        SplitDiffRow::HunkHeader { .. } if !self.hunk_headers => {}
                        SplitDiffRow::HunkHeader { .. } => {
                            if !first_hunk {
                                top += DiffSpec::DIFF_HUNK_GAP;
                            }
                            first_hunk = false;
                            let height = LINE_HEIGHT + DiffSpec::DIFF_HUNK_HEADER_PAD_Y * 2.0;
                            boxes.push(RowBox { top, height });
                            top += height;
                        }
                        SplitDiffRow::Row(row) => {
                            let height = self.wrapped_row_height(
                                [
                                    row.left.as_ref().map(|line| &line.text),
                                    row.right.as_ref().map(|line| &line.text),
                                ],
                                wrap_width,
                            );
                            boxes.push(RowBox { top, height });
                            top += height;
                        }
                    }
                }
            }
        }
        // DSH puts an 8 px margin under every hunk, then the body's 16 px foot.
        let height = if boxes.is_empty() {
            0.0
        } else {
            top + DiffSpec::DIFF_HUNK_GAP + DiffSpec::DIFF_BODY_PAD_BOTTOM
        };
        (boxes, height)
    }

    fn unified_wrap_width(&self, width: f32) -> f32 {
        if !self.wrap {
            return f32::INFINITY;
        }
        let columns = if self.show_line_numbers {
            GUTTER_W * 2.0 + SIGN_W
        } else {
            0.0
        };
        (width - columns - DiffSpec::DIFF_TEXT_PAD_RIGHT).max(1.0)
    }

    fn split_wrap_width(&self, width: f32) -> f32 {
        if !self.wrap {
            return f32::INFINITY;
        }
        let cell = (width - DiffSpec::DIFF_SPLIT_DIVIDER) * 0.5;
        let gutter = if self.show_line_numbers {
            GUTTER_W
        } else {
            0.0
        };
        (cell - gutter - DiffSpec::DIFF_TEXT_PAD_RIGHT).max(1.0)
    }

    fn wrapped_row_height(&self, texts: [Option<&String>; 2], wrap_width: f32) -> f32 {
        if !self.wrap || !wrap_width.is_finite() {
            return LINE_HEIGHT;
        }
        let lines = texts
            .into_iter()
            .flatten()
            .map(|text| {
                let measured = measure_text_family(
                    text,
                    FONT_SIZE,
                    LINE_FACTOR,
                    wrap_width,
                    FontWeight::Regular,
                    FontFamily::Mono,
                    false,
                );
                (measured.y / (FONT_SIZE * LINE_FACTOR)).round().max(1.0) as usize
            })
            .max()
            .unwrap_or(1);
        LINE_HEIGHT * lines as f32
    }

    /// The widest comparison line, used when the view may not fill its parent.
    fn measure_content_width(&self) -> f32 {
        match self.resolved_layout() {
            DiffLayout::Unified => {
                let text = self
                    .rows
                    .iter()
                    .map(|row| match row {
                        DiffRow::Line(line) => line_width(&line.text),
                        DiffRow::HunkHeader { .. } => {
                            DiffSpec::DIFF_HUNK_HEADER_PAD_X * 2.0 + line_width(&header_text(row))
                        }
                        DiffRow::Gap { skipped } => {
                            line_width(&format!("... {skipped} unchanged lines skipped"))
                        }
                    })
                    .fold(0.0_f32, f32::max);
                let columns = if self.show_line_numbers {
                    GUTTER_W * 2.0 + SIGN_W
                } else {
                    0.0
                };
                columns + text + DiffSpec::DIFF_TEXT_PAD_RIGHT
            }
            DiffLayout::Split => {
                let text = self
                    .split_rows
                    .iter()
                    .map(|row| match row {
                        SplitDiffRow::Row(row) => widest_split_cell(row),
                        SplitDiffRow::HunkHeader { .. } => line_width(&split_header_text(row)),
                        SplitDiffRow::Gap { skipped } => {
                            line_width(&format!("... {skipped} unchanged lines skipped"))
                        }
                    })
                    .fold(0.0_f32, f32::max);
                let gutter = if self.show_line_numbers {
                    GUTTER_W
                } else {
                    0.0
                };
                let cell = gutter + text + DiffSpec::DIFF_TEXT_PAD_RIGHT;
                cell * 2.0 + DiffSpec::DIFF_SPLIT_DIVIDER
            }
        }
    }

    fn change_bg(kind: DiffLineKind, app: &AppContext) -> ColorU {
        match kind {
            DiffLineKind::Added => app.theme.color(ColorToken::DiffInsertBg),
            DiffLineKind::Removed => app.theme.color(ColorToken::DiffDeleteBg),
            DiffLineKind::Context => app.theme.color(ColorToken::Surface),
        }
    }

    fn gutter_fill(kind: DiffLineKind, app: &AppContext) -> ColorU {
        let band = Self::change_bg(kind, app);
        let dark = luminance(app.theme.color(ColorToken::Bg)) < 0.5;
        let toward = if dark {
            app.theme.color(ColorToken::Bg)
        } else {
            app.theme.color(ColorToken::Surface)
        };
        band.mix(&toward, 0.45)
    }

    fn marker_color(kind: DiffLineKind, app: &AppContext) -> ColorU {
        match kind {
            DiffLineKind::Added => app.theme.color(ColorToken::Success),
            DiffLineKind::Removed => app.theme.color(ColorToken::Error),
            DiffLineKind::Context => app.theme.color(ColorToken::Muted),
        }
    }

    fn text_color(kind: DiffLineKind, app: &AppContext) -> ColorU {
        match kind {
            DiffLineKind::Context => app.theme.color(ColorToken::Muted),
            _ => app.theme.color(ColorToken::Text),
        }
    }

    fn empty_fill(app: &AppContext) -> ColorU {
        let hover = app.theme.color(ColorToken::Hover);
        ColorU::new(hover.r, hover.g, hover.b, 128)
    }

    fn draw_hunk_header(
        renderer: &mut crate::render::Renderer,
        origin: Vector2F,
        app: &AppContext,
        text: &str,
    ) {
        renderer.draw_text_with_font(
            vec2f(
                origin.x + DiffSpec::DIFF_HUNK_HEADER_PAD_X,
                origin.y + DiffSpec::DIFF_HUNK_HEADER_PAD_Y,
            ),
            text.to_string(),
            FONT_SIZE,
            app.theme.color(ColorToken::Muted),
            f32::INFINITY,
            LINE_FACTOR,
            FontWeight::Regular,
            FontFamily::Mono,
            false,
        );
    }

    fn draw_number(
        renderer: &mut crate::render::Renderer,
        cell_x: f32,
        y: f32,
        number: u32,
        color: ColorU,
    ) {
        let text = number.to_string();
        let width = line_width(&text);
        renderer.draw_text_with_font(
            vec2f(
                cell_x + GUTTER_W - DiffSpec::DIFF_NUMBER_PAD_RIGHT - width,
                y,
            ),
            text,
            FONT_SIZE,
            color,
            f32::INFINITY,
            LINE_FACTOR,
            FontWeight::Regular,
            FontFamily::Mono,
            false,
        );
    }

    /// Paint one line's body: highlighted runs when they exist, otherwise the
    /// flat kind colour. `wrap_width` is infinite for the clipping layout.
    ///
    /// The visual rows are byte ranges that partition the source exactly:
    /// whitespace the renderer folds at a soft break belongs to the preceding
    /// row, and the last row carries any trailing whitespace the wrap pass
    /// trimmed. Drawing a row as a range, rather than searching the line for
    /// the wrapped text, is what keeps highlighted runs and the source in
    /// step at a run boundary.
    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        renderer: &mut crate::render::Renderer,
        x: f32,
        y: f32,
        text: &str,
        runs: Option<&[TextSpan]>,
        fallback: ColorU,
        wrap_width: f32,
        line_height: f32,
    ) {
        if !wrap_width.is_finite() || line_width(text) <= wrap_width {
            draw_runs(renderer, x, y, text, runs, fallback);
            return;
        }
        for (index, range) in wrapped_ranges(text, wrap_width).iter().enumerate() {
            if range.is_empty() {
                continue;
            }
            let line_y = y + index as f32 * line_height;
            match runs {
                Some(runs) => {
                    draw_sliced_runs(renderer, x, line_y, runs, range.start, range.end, text)
                }
                None => {
                    renderer.draw_text_with_font(
                        vec2f(x, line_y),
                        text[range.clone()].to_string(),
                        FONT_SIZE,
                        fallback,
                        f32::INFINITY,
                        LINE_FACTOR,
                        FontWeight::Regular,
                        FontFamily::Mono,
                        false,
                    );
                }
            }
        }
    }

    fn draw_unified_line(
        &self,
        renderer: &mut crate::render::Renderer,
        origin: Vector2F,
        width: f32,
        row: &DiffLine,
        runs: Option<&[TextSpan]>,
        wrap_width: f32,
        row_height: f32,
        app: &AppContext,
    ) {
        let kind = row.kind;
        if kind != DiffLineKind::Context {
            renderer.fill_rect(
                rectf(origin.x, origin.y, width, row_height),
                Self::change_bg(kind, app),
            );
        }
        let marker = Self::marker_color(kind, app);
        if self.show_line_numbers {
            let gutter = Self::gutter_fill(kind, app);
            renderer.fill_rect(rectf(origin.x, origin.y, GUTTER_W, row_height), gutter);
            renderer.fill_rect(
                rectf(origin.x + GUTTER_W, origin.y, GUTTER_W, row_height),
                gutter,
            );
            if kind != DiffLineKind::Context {
                renderer.fill_rect(
                    rectf(origin.x, origin.y, DiffSpec::DIFF_MARKER_WIDTH, row_height),
                    marker,
                );
            }
            if let Some(number) = row.old_line {
                Self::draw_number(renderer, origin.x, origin.y, number, marker);
            }
            if let Some(number) = row.new_line {
                Self::draw_number(renderer, origin.x + GUTTER_W, origin.y, number, marker);
            }
        } else if kind != DiffLineKind::Context {
            renderer.fill_rect(
                rectf(origin.x, origin.y, DiffSpec::DIFF_MARKER_WIDTH, row_height),
                marker,
            );
        }
        let sign_x = origin.x
            + if self.show_line_numbers {
                GUTTER_W * 2.0
            } else {
                0.0
            };
        let sign = match kind {
            DiffLineKind::Added => "+",
            DiffLineKind::Removed => "-",
            DiffLineKind::Context => "",
        };
        if !sign.is_empty() {
            let sign_width = line_width(sign);
            renderer.draw_text_with_font(
                vec2f(sign_x + (SIGN_W - sign_width) * 0.5, origin.y),
                sign,
                FONT_SIZE,
                marker,
                f32::INFINITY,
                LINE_FACTOR,
                FontWeight::Regular,
                FontFamily::Mono,
                false,
            );
        }
        let text_x = origin.x
            + if self.show_line_numbers {
                GUTTER_W * 2.0 + SIGN_W
            } else {
                0.0
            };
        Self::draw_text(
            renderer,
            text_x,
            origin.y,
            &row.text,
            runs,
            Self::text_color(kind, app),
            wrap_width,
            LINE_HEIGHT,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_split_cell(
        &self,
        renderer: &mut crate::render::Renderer,
        cell_x: f32,
        y: f32,
        cell_width: f32,
        row: Option<&DiffLine>,
        side: DiffSide,
        runs: Option<&[TextSpan]>,
        wrap_width: f32,
        row_height: f32,
        app: &AppContext,
    ) {
        let Some(line) = row else {
            renderer.fill_rect(
                rectf(cell_x, y, cell_width, row_height),
                Self::empty_fill(app),
            );
            return;
        };
        let kind = line.kind;
        if kind != DiffLineKind::Context {
            renderer.fill_rect(
                rectf(cell_x, y, cell_width, row_height),
                Self::change_bg(kind, app),
            );
        }
        let marker = Self::marker_color(kind, app);
        let text_x = if self.show_line_numbers {
            let gutter = Self::gutter_fill(kind, app);
            renderer.fill_rect(rectf(cell_x, y, GUTTER_W, row_height), gutter);
            if kind != DiffLineKind::Context {
                renderer.fill_rect(
                    rectf(cell_x, y, DiffSpec::DIFF_MARKER_WIDTH, row_height),
                    marker,
                );
            }
            let number = match side {
                DiffSide::Old => line.old_line,
                DiffSide::New => line.new_line,
            };
            if let Some(number) = number {
                Self::draw_number(renderer, cell_x, y, number, marker);
            }
            cell_x + GUTTER_W
        } else {
            if kind != DiffLineKind::Context {
                renderer.fill_rect(
                    rectf(cell_x, y, DiffSpec::DIFF_MARKER_WIDTH, row_height),
                    marker,
                );
            }
            cell_x
        };
        Self::draw_text(
            renderer,
            text_x,
            y,
            &line.text,
            runs,
            Self::text_color(kind, app),
            wrap_width,
            LINE_HEIGHT,
        );
    }

    fn row_window(&self, origin: Vector2F) -> (usize, usize) {
        let Some(state) = self.viewport.as_ref() else {
            return (0, self.row_boxes.len());
        };
        let Some(view) = state.borrow().painted_viewport() else {
            return (0, self.row_boxes.len());
        };
        let mut first = self.row_boxes.len();
        let mut last = 0;
        for (index, row) in self.row_boxes.iter().enumerate() {
            let top = origin.y + row.top;
            let bottom = top + row.height;
            if bottom >= view.min_y() && top <= view.max_y() {
                first = first.min(index);
                last = last.max(index + 1);
            }
        }
        if first >= last {
            (0, 0)
        } else {
            (first, last)
        }
    }
}

impl Element for FileDiffView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        if self.rows.is_empty() {
            self.size = Some(Vector2F::zero());
            return Vector2F::zero();
        }
        let content_width = self.measure_content_width();
        let width = if self.fill_width && constraint.max.x.is_finite() {
            constraint.max.x.max(constraint.min.x)
        } else {
            content_width.max(constraint.min.x).min(constraint.max.x)
        };
        let (boxes, body_height) = self.layout_rows(width);
        let height = body_height.max(constraint.min.y);
        let size = vec2f(width, height);
        self.row_boxes = boxes;
        self.body_height = body_height;
        self.content_width = content_width;
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let (first, last) = self.row_window(origin);
        let mut box_index = 0usize;
        match self.resolved_layout() {
            DiffLayout::Unified => {
                let wrap_width = self.unified_wrap_width(size.x);
                for (index, row) in self.rows.iter().enumerate() {
                    if matches!(row, DiffRow::Gap { .. })
                        || (!self.hunk_headers && matches!(row, DiffRow::HunkHeader { .. }))
                    {
                        continue;
                    }
                    if box_index >= self.row_boxes.len() {
                        break;
                    }
                    let bounds = self.row_boxes[box_index];
                    let paint = box_index >= first && box_index < last;
                    let y = origin.y + bounds.top;
                    if paint {
                        match row {
                            DiffRow::HunkHeader { .. } => {
                                Self::draw_hunk_header(
                                    renderer,
                                    vec2f(origin.x, y),
                                    app,
                                    &header_text(row),
                                );
                            }
                            DiffRow::Line(line) => {
                                let runs =
                                    self.highlights.get(index).and_then(|runs| runs.as_deref());
                                self.draw_unified_line(
                                    renderer,
                                    vec2f(origin.x, y),
                                    size.x,
                                    line,
                                    runs,
                                    wrap_width,
                                    bounds.height,
                                    app,
                                );
                            }
                            DiffRow::Gap { .. } => {}
                        }
                    }
                    box_index += 1;
                }
            }
            DiffLayout::Split => {
                let wrap_width = self.split_wrap_width(size.x);
                let divider = DiffSpec::DIFF_SPLIT_DIVIDER;
                let cell_width = (size.x - divider) * 0.5;
                for row in self.split_rows.iter() {
                    if matches!(row, SplitDiffRow::Gap { .. })
                        || (!self.hunk_headers && matches!(row, SplitDiffRow::HunkHeader { .. }))
                    {
                        continue;
                    }
                    if box_index >= self.row_boxes.len() {
                        break;
                    }
                    let bounds = self.row_boxes[box_index];
                    let paint = box_index >= first && box_index < last;
                    let y = origin.y + bounds.top;
                    if paint {
                        match row {
                            SplitDiffRow::HunkHeader { .. } => {
                                Self::draw_hunk_header(
                                    renderer,
                                    vec2f(origin.x, y),
                                    app,
                                    &split_header_text(row),
                                );
                            }
                            SplitDiffRow::Row(row) => {
                                let left_runs = row
                                    .left
                                    .as_ref()
                                    .and_then(|line| line.old_line)
                                    .and_then(|number| self.old_highlights.get(&number))
                                    .map(|runs| runs.as_slice());
                                let right_runs = row
                                    .right
                                    .as_ref()
                                    .and_then(|line| line.new_line)
                                    .and_then(|number| self.new_highlights.get(&number))
                                    .map(|runs| runs.as_slice());
                                self.draw_split_cell(
                                    renderer,
                                    origin.x,
                                    y,
                                    cell_width,
                                    row.left.as_ref(),
                                    DiffSide::Old,
                                    left_runs,
                                    wrap_width,
                                    bounds.height,
                                    app,
                                );
                                renderer.fill_rect(
                                    rectf(origin.x + cell_width, y, divider, bounds.height),
                                    app.theme.color(ColorToken::Border),
                                );
                                self.draw_split_cell(
                                    renderer,
                                    origin.x + cell_width + divider,
                                    y,
                                    cell_width,
                                    row.right.as_ref(),
                                    DiffSide::New,
                                    right_runs,
                                    wrap_width,
                                    bounds.height,
                                    app,
                                );
                            }
                            SplitDiffRow::Gap { .. } => {}
                        }
                    }
                    box_index += 1;
                }
            }
        }
        // Keep the prepared measurements part of the view's state: a host that
        // stacks several diffs reads them through the element's size.
        let _ = (self.body_height, self.content_width);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

fn draw_runs(
    renderer: &mut crate::render::Renderer,
    x: f32,
    y: f32,
    text: &str,
    runs: Option<&[TextSpan]>,
    fallback: ColorU,
) {
    match runs {
        Some(runs) if !runs.is_empty() => {
            draw_sliced_runs(renderer, x, y, runs, 0, text.len(), text)
        }
        _ => {
            renderer.draw_text_with_font(
                vec2f(x, y),
                text.to_string(),
                FONT_SIZE,
                fallback,
                f32::INFINITY,
                LINE_FACTOR,
                FontWeight::Regular,
                FontFamily::Mono,
                false,
            );
        }
    }
}

/// The exact source byte ranges one line wraps into at `wrap_width`.
///
/// [`wrapped_lines`] reports where each visual row starts and ends, but it
/// trims a row's trailing whitespace and consumes the whitespace it folds on.
/// A styled line cannot be drawn from those strings alone: a run boundary on
/// the fold would either lose the space or draw it twice. These ranges instead
/// partition `text` with no gap, so concatenating what each row draws is the
/// source line byte for byte.
fn wrapped_ranges(text: &str, wrap_width: f32) -> Vec<Range<usize>> {
    let visual = wrapped_lines(
        text,
        FONT_SIZE,
        LINE_FACTOR,
        wrap_width,
        FontWeight::Regular,
        FontFamily::Mono,
        false,
    );
    if visual.is_empty() {
        return vec![0..text.len()];
    }
    let mut ranges: Vec<Range<usize>> = Vec::with_capacity(visual.len());
    let mut cursor = 0usize;
    for line in &visual {
        let (start, end) = if line.text.is_empty() {
            (cursor, cursor)
        } else if let Some(offset) = text[cursor..].find(&line.text) {
            let start = cursor + offset;
            (start, start + line.text.len())
        } else {
            // The visual text normally comes straight out of the source. Keep
            // the row partition intact even if a font fallback reports a row
            // the source does not spell verbatim.
            (cursor, advance_boundary(text, cursor, line.text.len()))
        };
        let start = start.max(cursor).min(text.len());
        let end = end.max(start).min(text.len());
        // Whitespace at a soft break is not on either visual row. It stays in
        // the range set, at the end of the row before it, so a highlighter run
        // that begins the next row never gains a leading space on screen.
        if let Some(previous) = ranges.last_mut() {
            previous.end = previous.end.max(start);
        }
        ranges.push(start..end);
        cursor = end.max(cursor);
    }
    if let Some(last) = ranges.last_mut() {
        last.end = text.len();
    } else {
        ranges.push(0..text.len());
    }
    ranges
}

/// The first character boundary at or after `cursor + len`, clamped to `text`.
fn advance_boundary(text: &str, cursor: usize, len: usize) -> usize {
    let mut end = (cursor + len).min(text.len());
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }
    end
}

/// Draw the intersection of `runs` with the byte range `start..end`, advancing
/// `x` by each run's advance width.
///
/// The advance, not the ink extent, is what places the next run: a run that
/// ends in a space paints no ink for it, but the space still occupies a cell,
/// so measuring only its ink would pull the following token one column left.
fn draw_sliced_runs(
    renderer: &mut crate::render::Renderer,
    x: f32,
    y: f32,
    runs: &[TextSpan],
    start: usize,
    end: usize,
    full_text: &str,
) {
    let mut offset = 0usize;
    for run in runs {
        let run_start = offset;
        let run_end = run_start + run.text.len();
        offset = run_end;
        let mut lo = start.max(run_start);
        let mut hi = end.min(run_end);
        if lo >= hi {
            continue;
        }
        // A span set from a highlighter normally reconstructs the source line
        // byte for byte; an unusual span still must not slice a glyph in half.
        while lo < run_end && !run.text.is_char_boundary(lo - run_start) {
            lo += 1;
        }
        while hi > run_start && !run.text.is_char_boundary(hi - run_start) {
            hi -= 1;
        }
        if lo >= hi {
            continue;
        }
        let text = run.text[lo - run_start..hi - run_start].to_string();
        if text.is_empty() {
            continue;
        }
        // Place the run from the exact source prefix with the exact pen
        // advance, not the whole-pixel layout budget: the renderer draws each
        // glyph at fontdue's fractional advance, so the rounded budget would
        // overshoot at every style boundary and read as an inserted space.
        let run_x = x + advance_width_exact(
            &full_text[..lo],
            FONT_SIZE,
            FontWeight::Regular,
            FontFamily::Mono,
            false,
        );
        renderer.draw_text_with_font(
            vec2f(run_x, y),
            text,
            FONT_SIZE,
            run.color,
            f32::INFINITY,
            LINE_FACTOR,
            run.weight,
            FontFamily::Mono,
            run.italic,
        );
    }
}

/// Whether `rows` carry only additions or only deletions (or neither). A
/// comparison with both kinds keeps the side-by-side layout; a comparison with
/// one kind is drawn as a single full-width column.
fn rows_are_one_sided(rows: &[DiffRow]) -> bool {
    let mut added = false;
    let mut removed = false;
    for row in rows {
        if let DiffRow::Line(line) = row {
            match line.kind {
                DiffLineKind::Added => added = true,
                DiffLineKind::Removed => removed = true,
                DiffLineKind::Context => {}
            }
        }
        if added && removed {
            return false;
        }
    }
    added != removed
}

fn line_width(text: &str) -> f32 {
    measure_text_family(
        text,
        FONT_SIZE,
        LINE_FACTOR,
        f32::INFINITY,
        FontWeight::Regular,
        FontFamily::Mono,
        false,
    )
    .x
}

fn widest_split_cell(row: &SplitRow) -> f32 {
    row.left
        .iter()
        .chain(row.right.iter())
        .map(|line| line_width(&line.text))
        .fold(0.0_f32, f32::max)
}

fn split_header_text(row: &SplitDiffRow) -> String {
    match row {
        SplitDiffRow::HunkHeader {
            old_start,
            old_count,
            new_start,
            new_count,
            section,
        } => {
            let mut text = format!(
                "@@ -{} +{} @@",
                range_text(*old_start, *old_count),
                range_text(*new_start, *new_count)
            );
            if !section.is_empty() {
                text.push(' ');
                text.push_str(section);
            }
            text
        }
        _ => String::new(),
    }
}

fn range_text(start: u32, count: u32) -> String {
    if count == 1 {
        start.to_string()
    } else {
        format!("{start},{count}")
    }
}

fn luminance(color: ColorU) -> f32 {
    (0.2126 * color.r as f32 + 0.7152 * color.g as f32 + 0.0722 * color.b as f32) / 255.0
}
