use crate::color::ColorU;
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint, TextSpan};
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::platform::text_atlas::{measure_text_family, FontWeight};
use crate::syntax::HighlightedLine;
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
    old_x: f32,
    new_x: f32,
    old_w: f32,
    new_w: f32,
    marker_x: f32,
    text_x: f32,
    header_x: f32,
    row_h: f32,
}

/// Unified-diff rows drawn inline: line-number gutters, a change rail and a
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
    hunks: Vec<Hunk>,
    show_line_numbers: bool,
    rows: Vec<DiffRow>,
    /// Highlighted runs per row, aligned with [`Diff::rows`]. `None` on a row
    /// means it is drawn in its flat colour (context, header, gap, or a changed
    /// line whose language did not resolve).
    highlights: Vec<Option<HighlightedLine>>,
    metrics: Metrics,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Diff {
    pub fn new(hunks: Vec<Hunk>) -> Self {
        Self {
            hunks,
            show_line_numbers: true,
            rows: Vec::new(),
            highlights: Vec::new(),
            metrics: Metrics::default(),
            size: None,
            origin: None,
        }
    }

    /// Highlight the changed lines as `language`.
    ///
    /// `language` may be the edited file's path, its extension or a language
    /// token; an empty or unknown value degrades to the flat change colour
    /// rather than drawing nothing.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        let language = language.into();
        self.highlights = highlight_changed_rows(&self.hunks, &language);
        self
    }

    /// Hide the two line-number gutters, leaving only the rail and the marker.
    pub fn with_line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }

    pub fn hunks(&self) -> &[Hunk] {
        &self.hunks
    }

    pub fn stats(&self) -> DiffStats {
        let mut stats = DiffStats::default();
        for hunk in &self.hunks {
            stats.added += hunk.added();
            stats.removed += hunk.removed();
        }
        stats
    }

    /// The rows the element will draw, derived from the parsed hunks.
    pub fn display_rows(&self) -> Vec<DiffRow> {
        build_rows(&self.hunks)
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

/// Highlight every changed line of `hunks` as `language`, aligned with
/// [`build_rows`]. A row that is not a changed body line, or whose language did
/// not resolve, is `None` — the painter reads that as the flat change colour.
fn highlight_changed_rows(hunks: &[Hunk], language: &str) -> Vec<Option<HighlightedLine>> {
    build_rows(hunks)
        .into_iter()
        .map(|row| match row {
            DiffRow::Line(line) if line.kind != DiffLineKind::Context => {
                crate::syntax::highlight(&line.text, language)
                    .and_then(|lines| lines.into_iter().next())
                    .filter(|runs| !runs.is_empty())
            }
            _ => None,
        })
        .collect()
}

/// Draw one changed line's body: its highlighted runs when the language
/// resolved, otherwise the whole line in the change's flat colour. Either way
/// the line is mono and unwrapped, so the layout the gutters and rail were
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
        let rows = build_rows(&self.hunks);
        if rows.is_empty() {
            self.rows.clear();
            self.size = Some(Vector2F::zero());
            return Vector2F::zero();
        }

        let mut max_digits = 1;
        for hunk in &self.hunks {
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

        let mut metrics = Metrics {
            row_h: FONT_SIZE * LINE_HEIGHT,
            header_x: RAIL_WIDTH,
            ..Metrics::default()
        };
        let mut x = RAIL_WIDTH;
        if self.show_line_numbers {
            let number_w = line_width(&"0".repeat(max_digits as usize));
            metrics.old_x = x;
            metrics.old_w = number_w;
            x += number_w + GUTTER_GAP;
            metrics.new_x = x;
            metrics.new_w = number_w;
            x += number_w + GUTTER_GAP;
        }
        metrics.marker_x = x;
        x += line_width("+") + MARKER_GAP;
        metrics.text_x = x;

        let max_text_w = rows
            .iter()
            .map(|row| match row {
                DiffRow::Line(line) => line_width(&line.text),
                DiffRow::HunkHeader { .. } => line_width(&header_text(row)),
                DiffRow::Gap { skipped } => line_width(&gap_text(*skipped)),
            })
            .fold(0.0_f32, f32::max);

        let content_w = metrics.text_x + max_text_w + PAD_RIGHT;
        let width = content_w.max(constraint.min.x).min(constraint.max.x);
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

        for (index, row) in self.rows.iter().enumerate() {
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
                        if let Some(number) = line.old_line {
                            draw_number(renderer, metrics.old_x + metrics.old_w, y, number, muted);
                        }
                        if let Some(number) = line.new_line {
                            draw_number(renderer, metrics.new_x + metrics.new_w, y, number, muted);
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
