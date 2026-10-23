use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::{
    AppContext, Element, LayoutContext, PaintContext, Point, ScrollState, SizeConstraint, TextSpan,
};
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::platform::text_atlas::{measure_text_family, FontWeight};
use crate::syntax::{HighlightedLine, HighlightedSpans};
use crate::theme::{ColorToken, FontFamily};
use super::model::{DiffLineKind, DiffRow, DiffStats, Hunk, build_rows, gap_text, header_text};

const FONT_SIZE: f32 = 12.0;

const LINE_HEIGHT: f32 = 1.35;

/// Width of the vertical rail drawn down the left of a changed row.
const RAIL_WIDTH: f32 = 2.0;

const GUTTER_GAP: f32 = 8.0;

const MARKER_GAP: f32 = 8.0;

const PAD_RIGHT: f32 = 8.0;

#[derive(Clone, Copy, Debug, Default)]
struct Metrics {
    number_x: f32,
    number_w: f32,
    marker_x: f32,
    text_x: f32,
    header_x: f32,
    row_h: f32,
}

/// The rows and syntax runs of a parsed diff, prepared once and shared by every
/// frame that draws it.
///
/// The element tree is rebuilt every frame, so re-deriving the rows and
/// re-highlighting every changed line there would make a large review panel
/// cost tens of milliseconds per frame. A host that keeps a parsed diff across
/// frames prepares one of these when it reads the diff and hands it to
/// [`Diff::with_prepared`].
#[derive(Clone, Default)]
pub struct PreparedDiff {
    rows: Rc<Vec<DiffRow>>,
    /// The widest row's text and the digits its gutter needs, measured once
    /// when the rows are built. A card inside a sideways scroll region reads
    /// these every frame; walking every line of every visible file to measure
    /// them again is what made a large review cost the frame.
    text_width: f32,
    gutter_digits: u32,
    /// `None` until a host asks for the colour, so a working-tree read can
    /// prepare every diff's rows while only the cards in view pay for syntax.
    /// A review of many hundred changed files would otherwise highlight every
    /// diff on the frame it reads them, freezing the window.
    highlights: Rc<RefCell<Option<Rc<Vec<Option<HighlightedLine>>>>>>,
}

/// The rows past which a diff stays in its flat change colours. A diff this
/// tall is opened by hand, and colouring both sides of it in one frame costs
/// more than the read that produced it.
const MAX_HIGHLIGHT_ROWS: usize = 900;

/// One side of a diff as a worker takes it: the lines to colour, joined as one
/// text so the parser keeps its state across them, and the row of this diff
/// each line belongs to.
#[derive(Clone, Debug)]
pub struct DiffHighlightSide {
    pub text: String,
    pub rows: Vec<usize>,
}

/// A diff's colouring work in a form a worker thread can take. A host asks for
/// it with [`PreparedDiff::highlight_job`] and hands the answers back with
/// [`PreparedDiff::install_highlights`], so a large card never colours itself
/// inside a frame.
#[derive(Clone, Debug)]
pub struct DiffHighlightJob {
    pub sides: Vec<DiffHighlightSide>,
}

impl std::fmt::Debug for PreparedDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The text spans carry no `Debug`; the counts are what a log needs to
        // tell one prepared diff from another.
        f.debug_struct("PreparedDiff")
            .field("rows", &self.rows.len())
            .field(
                "highlights",
                &self.highlights.borrow().as_ref().map(|runs| runs.len()),
            )
            .finish()
    }
}

impl PreparedDiff {
    /// Prepare `hunks` for drawing, highlighting the changed lines as
    /// `language` (a path, extension or language token; an unknown one leaves
    /// the flat change colour).
    pub fn new(hunks: &[Hunk], language: &str) -> Self {
        let prepared = Self::rows_only(hunks);
        prepared.ensure_highlighted(language);
        prepared
    }

    /// Prepare the rows only. The host asks for the colour with
    /// [`Self::ensure_highlighted`] when the diff is actually in view, so a
    /// read of a large working tree does not highlight every changed file.
    pub fn rows_only(hunks: &[Hunk]) -> Self {
        let rows = build_rows(hunks);
        let text_width = rows_text_width(&rows);
        let gutter_digits = gutter_digits(hunks);
        Self {
            rows: Rc::new(rows),
            text_width,
            gutter_digits,
            highlights: Rc::new(RefCell::new(None)),
        }
    }

    /// The work a worker has to do to colour this diff, or `None` when it is
    /// already coloured or too tall to colour. The sides are built here, on
    /// the frame, because they are cheap joins of rows already in memory; the
    /// parser, which is the expensive part, runs on the worker.
    pub fn highlight_job(&self) -> Option<DiffHighlightJob> {
        if self.highlights.borrow().is_some() || self.rows.len() > MAX_HIGHLIGHT_ROWS {
            return None;
        }
        Some(DiffHighlightJob {
            sides: diff_sides(&self.rows),
        })
    }

    /// Install the worker's answers, one per side of `job`, aligned with this
    /// diff's rows. A second install is ignored: the first answer wins, the
    /// same way [`Self::ensure_highlighted`] colours once.
    pub fn install_highlights(&self, job: &DiffHighlightJob, answers: &[Option<HighlightedSpans>]) {
        let mut slot = self.highlights.borrow_mut();
        if slot.is_some() {
            return;
        }
        let lines: Vec<Option<Vec<HighlightedLine>>> = answers
            .iter()
            .map(|answer| answer.as_ref().map(crate::syntax::spans_to_lines))
            .collect();
        let assembled = assemble_highlights(self.rows.len(), &job.sides, &lines);
        *slot = Some(Rc::new(assembled));
    }

    /// Colour this diff once, as `language`, and share the result with every
    /// later frame. A diff past [`MAX_HIGHLIGHT_ROWS`] keeps the flat change
    /// colours; the attempt is still recorded, so no frame asks twice.
    pub fn ensure_highlighted(&self, language: &str) {
        let mut slot = self.highlights.borrow_mut();
        if slot.is_some() {
            return;
        }
        let runs = if self.rows.len() <= MAX_HIGHLIGHT_ROWS {
            highlight_changed_rows(&self.rows, language)
        } else {
            Vec::new()
        };
        *slot = Some(Rc::new(runs));
    }

    /// The syntax runs, or an empty set when nobody has asked for them yet.
    fn highlight_runs(&self) -> Rc<Vec<Option<HighlightedLine>>> {
        self.highlights
            .borrow()
            .clone()
            .unwrap_or_else(|| Rc::new(Vec::new()))
    }

    /// How many rows the prepared diff draws. A host that stacks several diffs
    /// in one scroll region reads this to know where the next one starts.
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// The width of the widest row, measured when the rows were built.
    pub fn text_width(&self) -> f32 {
        self.text_width
    }

    /// The digits the widest line number needs, measured when the rows were
    /// built.
    pub fn gutter_digits(&self) -> u32 {
        self.gutter_digits
    }
}

/// The width of the widest drawn row, headers and gap notes included.
fn rows_text_width(rows: &[DiffRow]) -> f32 {
    rows.iter()
        .map(|row| match row {
            DiffRow::Line(line) => line_width(&line.text),
            DiffRow::HunkHeader { .. } => line_width(&header_text(row)),
            DiffRow::Gap { skipped } => line_width(&gap_text(*skipped)),
        })
        .fold(0.0_f32, f32::max)
}

/// The digits the line-number gutter needs for this diff.
fn gutter_digits(hunks: &[Hunk]) -> u32 {
    let mut max_digits = 1;
    for hunk in hunks.iter() {
        max_digits = max_digits.max(digit_count(hunk.old_start + hunk.old_count));
        max_digits = max_digits.max(digit_count(hunk.new_start + hunk.new_count));
        for line in &hunk.lines {
            if let Some(number) = line.old_line {
                max_digits = max_digits.max(digit_count(number));
            }
            if let Some(number) = line.new_line {
                max_digits = max_digits.max(digit_count(number));
            }
        }
    }
    max_digits
}

/// Unified-diff rows drawn inline: a line-number gutter, a change rail and a
/// full-width insert/delete background band from the theme's diff tokens, with
/// a separator row for the context skipped between hunks.
///
/// The changed lines are code: when a language is supplied ([`Diff::with_language`])
/// each one is run through the shared highlighter, so the code is coloured
/// inside the band; an unknown language leaves it in the flat change colour.
///
/// It is deliberately not a card: there is no border, no fill behind the whole
/// element and no rounded corner. The element is constructed from parsed
/// [`Hunk`]s (see [`parse_unified_diff`](super::model::parse_unified_diff)), never from a raw diff string.
pub struct Diff {
    hunks: Rc<Vec<Hunk>>,
    show_line_numbers: bool,
    /// Whether the element takes the whole width it is given instead of only
    /// the width its longest row needs. A diff inside a scrollable panel asks
    /// for this so the change bands run to the panel's edge, the way the
    /// reference draws them.
    fill_width: bool,
    rows: Rc<Vec<DiffRow>>,
    /// The measurements [`PreparedDiff`] took when it built the rows, reused
    /// instead of walking every row again on each layout.
    prepared_text_width: Option<f32>,
    prepared_gutter_digits: Option<u32>,
    /// Highlighted runs per row, aligned with [`Diff::rows`]. `None` on a row
    /// means it is drawn in its flat colour (context, header, gap, or a changed
    /// line whose language did not resolve).
    highlights: Rc<Vec<Option<HighlightedLine>>>,
    /// The scroll region this diff is drawn inside, when there is one. Its
    /// last painted viewport lets [`Diff::paint`] skip the rows that are
    /// clipped away instead of emitting a command for every line of a file the
    /// user has scrolled past.
    viewport: Option<Rc<RefCell<ScrollState>>>,
    metrics: Metrics,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Diff {
    pub fn new(hunks: Vec<Hunk>) -> Self {
        Self::new_shared(Rc::new(hunks))
    }

    /// The same, over hunks a host already shares (a cached review panel).
    pub fn new_shared(hunks: Rc<Vec<Hunk>>) -> Self {
        let rows = Rc::new(build_rows(&hunks));
        Self {
            hunks,
            show_line_numbers: true,
            fill_width: false,
            rows,
            prepared_text_width: None,
            prepared_gutter_digits: None,
            highlights: Rc::new(Vec::new()),
            viewport: None,
            metrics: Metrics::default(),
            size: None,
            origin: None,
        }
    }

    /// A diff whose rows and syntax runs were prepared once, at read time. This
    /// is the constructor a per-frame host wants: it derives nothing here.
    pub fn from_prepared(hunks: Rc<Vec<Hunk>>, prepared: PreparedDiff) -> Self {
        let highlights = prepared.highlight_runs();
        Self {
            hunks,
            show_line_numbers: true,
            fill_width: false,
            rows: prepared.rows,
            prepared_text_width: Some(prepared.text_width),
            prepared_gutter_digits: Some(prepared.gutter_digits),
            highlights,
            viewport: None,
            metrics: Metrics::default(),
            size: None,
            origin: None,
        }
    }

    /// Use rows and syntax runs that were prepared once, instead of deriving
    /// them here on every frame.
    pub fn with_prepared(mut self, prepared: PreparedDiff) -> Self {
        let highlights = prepared.highlight_runs();
        self.rows = prepared.rows;
        self.prepared_text_width = Some(prepared.text_width);
        self.prepared_gutter_digits = Some(prepared.gutter_digits);
        self.highlights = highlights;
        self
    }

    /// Highlight the changed lines as `language`.
    ///
    /// `language` may be the edited file's path, its extension or a language
    /// token; an empty or unknown value degrades to the flat change colour
    /// rather than drawing nothing.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        let language = language.into();
        self.highlights = Rc::new(highlight_changed_rows(&self.rows, &language));
        self
    }

    /// Skip rows outside the viewport of the scroll region `state` belongs to.
    /// The region clips the element anyway; this keeps a long diff from
    /// building a draw command for every line scrolled out of sight.
    pub fn with_viewport(mut self, state: Rc<RefCell<ScrollState>>) -> Self {
        self.viewport = Some(state);
        self
    }

    /// Hide the line-number gutter, leaving only the rail and the marker.
    pub fn with_line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }

    /// Take the whole width the layout offers. The change bands and the
    /// insert/delete rail then span the panel instead of stopping at the
    /// longest line.
    pub fn with_fill_width(mut self, fill: bool) -> Self {
        self.fill_width = fill;
        self
    }

    pub fn hunks(&self) -> &[Hunk] {
        self.hunks.as_slice()
    }

    pub fn stats(&self) -> DiffStats {
        let mut stats = DiffStats::default();
        for hunk in self.hunks.iter() {
            stats.added += hunk.added();
            stats.removed += hunk.removed();
        }
        stats
    }

    /// The rows the element will draw, derived from the parsed hunks.
    pub fn display_rows(&self) -> Vec<DiffRow> {
        (*self.rows).clone()
    }

    fn text_color(kind: DiffLineKind, app: &AppContext) -> ColorU {
        match kind {
            DiffLineKind::Context => app.theme.color(ColorToken::Text),
            DiffLineKind::Added => app.theme.color(ColorToken::Success),
            DiffLineKind::Removed => app.theme.color(ColorToken::Error),
        }
    }

    /// The full-width band behind a changed row: the theme's insert/delete
    /// background. Context rows are never banded, so they fall back to the
    /// surface.
    fn change_bg(kind: DiffLineKind, app: &AppContext) -> ColorU {
        match kind {
            DiffLineKind::Added => app.theme.color(ColorToken::DiffInsertBg),
            DiffLineKind::Removed => app.theme.color(ColorToken::DiffDeleteBg),
            DiffLineKind::Context => app.theme.color(ColorToken::Surface),
        }
    }

    fn marker(kind: DiffLineKind) -> &'static str {
        match kind {
            DiffLineKind::Context => " ",
            DiffLineKind::Added => "+",
            DiffLineKind::Removed => "-",
        }
    }
}

/// Highlight every code line of `rows` as `language`, aligned with those rows:
/// a context line is code too, and colouring only the changed half of a hunk
/// makes the review read as two kinds of text. A row that is not a body line,
/// or whose language did not resolve, is `None` — the painter reads that as the
/// flat line colour, the change's own for an added or removed line.
fn highlight_changed_rows(rows: &[DiffRow], language: &str) -> Vec<Option<HighlightedLine>> {
    let sides = diff_sides(rows);
    let answers: Vec<Option<Vec<HighlightedLine>>> = sides
        .iter()
        .map(|side| crate::syntax::highlight(&side.text, language))
        .collect();
    assemble_highlights(rows.len(), &sides, &answers)
}

/// The two sides of a diff as the texts a highlighter colours. A hunk carries
/// two versions of the file: the old side is its context and removed lines, the
/// new side its context and added ones. Highlighting each side as one text
/// keeps the parser's state across lines, so a block comment or a multi-line
/// string stays one colour instead of restarting at every row.
fn diff_sides(rows: &[DiffRow]) -> Vec<DiffHighlightSide> {
    let mut sides = Vec::new();
    for new_side in [true, false] {
        let rows_of_side: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| match row {
                DiffRow::Line(line) => match line.kind {
                    DiffLineKind::Context => Some(index),
                    DiffLineKind::Added => new_side.then_some(index),
                    DiffLineKind::Removed => (!new_side).then_some(index),
                },
                _ => None,
            })
            .collect();
        if rows_of_side.is_empty() {
            continue;
        }
        let text = rows_of_side
            .iter()
            .filter_map(|&index| match &rows[index] {
                DiffRow::Line(line) => Some(line.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        sides.push(DiffHighlightSide {
            text,
            rows: rows_of_side,
        });
    }
    sides
}

/// Put each side's coloured lines on the rows they came from. Context lines are
/// on both sides, and the first side that coloured them wins.
fn assemble_highlights(
    row_count: usize,
    sides: &[DiffHighlightSide],
    answers: &[Option<Vec<HighlightedLine>>],
) -> Vec<Option<HighlightedLine>> {
    let mut out: Vec<Option<HighlightedLine>> = vec![None; row_count];
    for (side, answer) in sides.iter().zip(answers) {
        let Some(lines) = answer else {
            continue;
        };
        for (line_index, &row_index) in side.rows.iter().enumerate() {
            if out[row_index].is_some() {
                continue;
            }
            if let Some(runs) = lines.get(line_index).filter(|runs| !runs.is_empty()) {
                out[row_index] = Some(runs.clone());
            }
        }
    }
    out
}

/// Draw one changed line's body: its highlighted runs when the language
/// resolved, otherwise the whole line in the change's flat colour. Either way
/// the line is mono and unwrapped, so the layout the gutter and rail were
/// measured against is unchanged.
fn draw_line_text(
    renderer: &mut crate::render::Renderer,
    x: f32,
    y: f32,
    text: &str,
    runs: Option<&[TextSpan]>,
    fallback: ColorU,
) {
    match runs {
        Some(runs) if !runs.is_empty() => {
            let mut x = x;
            for run in runs {
                let width = measure_text_family(
                    &run.text,
                    FONT_SIZE,
                    LINE_HEIGHT,
                    f32::INFINITY,
                    run.weight,
                    FontFamily::Mono,
                    run.italic,
                )
                .x;
                renderer.draw_text_with_font(
                    vec2f(x, y),
                    run.text.clone(),
                    FONT_SIZE,
                    run.color,
                    f32::INFINITY,
                    LINE_HEIGHT,
                    run.weight,
                    FontFamily::Mono,
                    run.italic,
                );
                x += width;
            }
        }
        _ => renderer.draw_text_with_font(
            vec2f(x, y),
            text.to_string(),
            FONT_SIZE,
            fallback,
            f32::INFINITY,
            LINE_HEIGHT,
            FontWeight::Regular,
            FontFamily::Mono,
            false,
        ),
    }
}

impl Default for Diff {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Element for Diff {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let rows = Rc::clone(&self.rows);
        if rows.is_empty() {
            self.size = Some(Vector2F::zero());
            return Vector2F::zero();
        }

        // A prepared diff carries the digit count its gutter needs, so the
        // layout does not walk every hunk of every visible file each frame.
        let max_digits = self
            .prepared_gutter_digits
            .unwrap_or_else(|| gutter_digits(&self.hunks));

        let mut metrics = Metrics {
            row_h: FONT_SIZE * LINE_HEIGHT,
            header_x: RAIL_WIDTH,
            ..Metrics::default()
        };
        let mut x = RAIL_WIDTH;
        if self.show_line_numbers {
            // One gutter, not one per side: an added or context row shows its
            // new number and a removed row the old one it left.
            let number_w = line_width(&"0".repeat(max_digits as usize));
            metrics.number_x = x;
            metrics.number_w = number_w;
            x += number_w + GUTTER_GAP;
        }
        metrics.marker_x = x;
        x += line_width("+") + MARKER_GAP;
        metrics.text_x = x;

        // A panel-width diff does not need its longest row: the width comes
        // from the constraint. Inside a sideways scroll region the width is
        // unbounded instead, so the card is as wide as its longest line, and
        // that is what the swipe travels over. A prepared diff measured that
        // row once when it built its rows.
        let max_text_w = if self.fill_width && constraint.max.x.is_finite() {
            0.0
        } else {
            self.prepared_text_width
                .unwrap_or_else(|| rows_text_width(&rows))
        };

        let content_w = metrics.text_x + max_text_w + PAD_RIGHT;
        let width = if self.fill_width && constraint.max.x.is_finite() {
            constraint.max.x.max(constraint.min.x)
        } else {
            content_w.max(constraint.min.x).min(constraint.max.x)
        };
        let height = rows.len() as f32 * metrics.row_h;
        let size = vec2f(width, height.max(constraint.min.y));

        self.metrics = metrics;
        self.rows = rows;
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };

        let metrics = self.metrics;
        let muted = app.theme.color(ColorToken::Muted);

        // Only the rows the scroll region can show are drawn. Everything else
        // would be clipped away after paying for its command.
        let window = self
            .viewport
            .as_ref()
            .and_then(|state| state.borrow().painted_viewport());
        let (first, last) = match window {
            Some(window) => {
                let top = (window.min_y() - origin.y) / metrics.row_h;
                let bottom = (window.max_y() - origin.y) / metrics.row_h;
                let first = top.floor().max(0.0) as usize;
                let last = (bottom.ceil().max(0.0) as usize + 1).min(self.rows.len());
                (first.min(last), last)
            }
            None => (0, self.rows.len()),
        };

        for index in first..last {
            let row = &self.rows[index];
            let y = origin.y + index as f32 * metrics.row_h;
            match row {
                DiffRow::Line(line) => {
                    let color = Self::text_color(line.kind, app);
                    if line.kind != DiffLineKind::Context {
                        renderer.fill_rect(
                            rectf(origin.x, y, size.x, metrics.row_h),
                            Self::change_bg(line.kind, app),
                        );
                        renderer.fill_rect(rectf(origin.x, y, RAIL_WIDTH, metrics.row_h), color);
                    }
                    if self.show_line_numbers {
                        if let Some(number) = line.new_line.or(line.old_line) {
                            draw_number(
                                renderer,
                                origin.x + metrics.number_x + metrics.number_w,
                                y,
                                number,
                                muted,
                            );
                        }
                    }
                    let marker = Self::marker(line.kind);
                    if marker != " " {
                        renderer.draw_text_with_font(
                            vec2f(origin.x + metrics.marker_x, y),
                            marker,
                            FONT_SIZE,
                            color,
                            f32::INFINITY,
                            LINE_HEIGHT,
                            FontWeight::Regular,
                            FontFamily::Mono,
                            false,
                        );
                    }
                    draw_line_text(
                        renderer,
                        origin.x + metrics.text_x,
                        y,
                        &line.text,
                        self.highlights.get(index).and_then(|runs| runs.as_deref()),
                        color,
                    );
                }
                DiffRow::HunkHeader { .. } => {
                    renderer.draw_text_with_font(
                        vec2f(origin.x + metrics.header_x, y),
                        header_text(row),
                        FONT_SIZE,
                        muted,
                        f32::INFINITY,
                        LINE_HEIGHT,
                        FontWeight::Regular,
                        FontFamily::Mono,
                        false,
                    );
                }
                DiffRow::Gap { skipped } => {
                    renderer.draw_text_with_font(
                        vec2f(origin.x + metrics.header_x, y),
                        gap_text(*skipped),
                        FONT_SIZE,
                        muted,
                        f32::INFINITY,
                        LINE_HEIGHT,
                        FontWeight::Regular,
                        FontFamily::Mono,
                        false,
                    );
                }
            }
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

fn draw_number(
    renderer: &mut crate::render::Renderer,
    right_x: f32,
    y: f32,
    number: u32,
    color: ColorU,
) {
    let text = number.to_string();
    let width = line_width(&text);
    renderer.draw_text_with_font(
        vec2f(right_x - width, y),
        text,
        FONT_SIZE,
        color,
        f32::INFINITY,
        LINE_HEIGHT,
        FontWeight::Regular,
        FontFamily::Mono,
        false,
    );
}

fn digit_count(mut n: u32) -> u32 {
    let mut digits = 1;
    while n >= 10 {
        n /= 10;
        digits += 1;
    }
    digits
}

fn line_width(text: &str) -> f32 {
    measure_text_family(
        text,
        FONT_SIZE,
        LINE_HEIGHT,
        f32::INFINITY,
        FontWeight::Regular,
        FontFamily::Mono,
        false,
    )
    .x
}
