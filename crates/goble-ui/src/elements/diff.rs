use crate::color::ColorU;
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::platform::text_atlas::{measure_text_family, FontWeight};
use crate::theme::{ColorToken, FontFamily};

const FONT_SIZE: f32 = 12.0;
const LINE_HEIGHT: f32 = 1.35;
/// Width of the vertical rail drawn down the left of a changed row.
const RAIL_WIDTH: f32 = 2.0;
const GUTTER_GAP: f32 = 8.0;
const MARKER_GAP: f32 = 8.0;
const PAD_RIGHT: f32 = 8.0;
/// How far a changed row's background is tinted toward its change colour.
const TINT: f32 = 0.10;

/// The three line kinds a unified diff can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
}

/// One body line of a hunk, with the line number each side of the diff shows.
///
/// A removed line has only an old number, an added line only a new one, and a
/// context line both — so which gutter column is populated *is* the change type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub text: String,
}

/// One `@@ -old,count +new,count @@ section` block of a unified diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    /// The function/context label the header carries after the closing `@@`.
    pub section: String,
    pub lines: Vec<DiffLine>,
}

impl Hunk {
    /// The hunk's `@@ … @@` header, exactly as it appears in a unified diff.
    pub fn header(&self) -> String {
        let mut text = format!(
            "@@ -{} +{} @@",
            format_range(self.old_start, self.old_count),
            format_range(self.new_start, self.new_count)
        );
        if !self.section.is_empty() {
            text.push(' ');
            text.push_str(&self.section);
        }
        text
    }

    pub fn added(&self) -> usize {
        self.count(DiffLineKind::Added)
    }

    pub fn removed(&self) -> usize {
        self.count(DiffLineKind::Removed)
    }

    pub fn context(&self) -> usize {
        self.count(DiffLineKind::Context)
    }

    fn count(&self, kind: DiffLineKind) -> usize {
        self.lines.iter().filter(|line| line.kind == kind).count()
    }
}

/// Added/removed totals across a whole diff.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiffStats {
    pub added: usize,
    pub removed: usize,
}

/// A row the diff element draws. Hunks contribute a header row and their body
/// lines; the context skipped between two hunks contributes a gap separator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffRow {
    HunkHeader {
        old_start: u32,
        old_count: u32,
        new_start: u32,
        new_count: u32,
        section: String,
    },
    /// A separator standing in for the unchanged lines neither hunk shows.
    Gap {
        skipped: usize,
    },
    Line(DiffLine),
}

/// Parse a unified diff into hunks.
///
/// `---`/`+++` file headers, `diff --git`/`index` lines and
/// `\ No newline at end of file` markers are skipped; a hunk ends once it has
/// consumed the old and new line counts its header declares.
pub fn parse_unified_diff(input: &str) -> Vec<Hunk> {
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current: Option<Hunk> = None;
    let mut consumed_old = 0u32;
    let mut consumed_new = 0u32;

    for raw in input.lines() {
        if raw.starts_with("@@") {
            if let Some(hunk) = current.take() {
                hunks.push(hunk);
            }
            if let Some(hunk) = parse_hunk_header(raw) {
                consumed_old = 0;
                consumed_new = 0;
                current = Some(hunk);
            }
            continue;
        }

        let Some(hunk) = current.as_mut() else {
            continue;
        };

        // The header declares how many lines the hunk covers; once both sides
        // are satisfied, a further line starts a new region.
        if consumed_old >= hunk.old_count && consumed_new >= hunk.new_count {
            let finished = current.take().expect("current is Some");
            hunks.push(finished);
            continue;
        }

        let (kind, text) = match raw.as_bytes().first() {
            Some(b' ') => (DiffLineKind::Context, &raw[1..]),
            Some(b'+') => (DiffLineKind::Added, &raw[1..]),
            Some(b'-') => (DiffLineKind::Removed, &raw[1..]),
            Some(b'\\') => continue,
            _ => continue,
        };

        let old_line = (kind != DiffLineKind::Added).then(|| hunk.old_start + consumed_old);
        let new_line = (kind != DiffLineKind::Removed).then(|| hunk.new_start + consumed_new);
        if kind != DiffLineKind::Added {
            consumed_old += 1;
        }
        if kind != DiffLineKind::Removed {
            consumed_new += 1;
        }
        hunk.lines.push(DiffLine {
            kind,
            old_line,
            new_line,
            text: text.to_string(),
        });
    }

    if let Some(hunk) = current {
        hunks.push(hunk);
    }
    hunks
}

fn parse_hunk_header(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@")?;
    let end = rest.find("@@")?;
    let ranges = &rest[..end];
    let section = rest[end + 2..].trim().to_string();
    let mut tokens = ranges.split_whitespace();
    let (old_start, old_count) = parse_range(tokens.next()?)?;
    let (new_start, new_count) = parse_range(tokens.next()?)?;
    Some(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        section,
        lines: Vec::new(),
    })
}

fn parse_range(token: &str) -> Option<(u32, u32)> {
    let token = token.trim_start_matches(['-', '+']);
    let mut parts = token.split(',');
    let start = parts.next()?.trim().parse().ok()?;
    let count = match parts.next() {
        Some(count) => count.trim().parse().ok()?,
        None => 1,
    };
    Some((start, count))
}

/// `count` of 1 is elided, matching git's `-1 +1` form.
fn format_range(start: u32, count: u32) -> String {
    if count == 1 {
        format!("{start}")
    } else {
        format!("{start},{count}")
    }
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

fn build_rows(hunks: &[Hunk]) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    for (index, hunk) in hunks.iter().enumerate() {
        if index > 0 {
            rows.push(DiffRow::Gap {
                skipped: skipped_between(&hunks[index - 1], hunk),
            });
        }
        rows.push(DiffRow::HunkHeader {
            old_start: hunk.old_start,
            old_count: hunk.old_count,
            new_start: hunk.new_start,
            new_count: hunk.new_count,
            section: hunk.section.clone(),
        });
        rows.extend(hunk.lines.iter().cloned().map(DiffRow::Line));
    }
    rows
}

/// Unchanged lines between the end of `prev` and the start of `next`.
fn skipped_between(prev: &Hunk, next: &Hunk) -> usize {
    let prev_end = prev.old_start.saturating_add(prev.old_count);
    next.old_start.saturating_sub(prev_end) as usize
}

fn header_text(row: &DiffRow) -> String {
    match row {
        DiffRow::HunkHeader {
            old_start,
            old_count,
            new_start,
            new_count,
            section,
        } => {
            let mut text = format!(
                "@@ -{} +{} @@",
                format_range(*old_start, *old_count),
                format_range(*new_start, *new_count)
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

fn gap_text(skipped: usize) -> String {
    if skipped == 1 {
        "... 1 unchanged line skipped".to_string()
    } else {
        format!("... {skipped} unchanged lines skipped")
    }
}

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

/// Unified-diff rows drawn inline: line-number gutters, a change rail and
/// add/remove colouring from the theme's success/error tokens, with a separator
/// row for the context skipped between hunks.
///
/// It is deliberately not a card: there is no border, no fill behind the whole
/// element and no rounded corner. The element is constructed from parsed
/// [`Hunk`]s (see [`parse_unified_diff`]), never from a raw diff string.
pub struct Diff {
    hunks: Vec<Hunk>,
    show_line_numbers: bool,
    rows: Vec<DiffRow>,
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
            metrics: Metrics::default(),
            size: None,
            origin: None,
        }
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

    fn marker(kind: DiffLineKind) -> &'static str {
        match kind {
            DiffLineKind::Context => " ",
            DiffLineKind::Added => "+",
            DiffLineKind::Removed => "-",
        }
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
        let surface = app.theme.color(ColorToken::Surface);

        for (index, row) in self.rows.iter().enumerate() {
            let y = origin.y + index as f32 * metrics.row_h;
            match row {
                DiffRow::Line(line) => {
                    let color = Self::text_color(line.kind, app);
                    if line.kind != DiffLineKind::Context {
                        renderer.fill_rect(
                            rectf(origin.x, y, size.x, metrics.row_h),
                            surface.mix(&color, TINT),
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
                    renderer.draw_text_with_font(
                        vec2f(origin.x + metrics.text_x, y),
                        line.text.clone(),
                        FONT_SIZE,
                        color,
                        f32::INFINITY,
                        LINE_HEIGHT,
                        FontWeight::Regular,
                        FontFamily::Mono,
                        false,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;
    use crate::test_util::{command_counts, render_element};

    fn sample_text() -> String {
        [
            "diff --git a/src/lib.rs b/src/lib.rs",
            "index 1111111..2222222 100644",
            "--- a/src/lib.rs",
            "+++ b/src/lib.rs",
            "@@ -1,3 +1,4 @@ fn main() {",
            " fn main() {",
            "-    let x = 1;",
            "+    let x = 2;",
            "+    let y = 3;",
            " }",
            "@@ -10,3 +11,2 @@ fn other() {",
            "     do_thing();",
            "-    removed();",
            " }",
        ]
        .join("\n")
    }

    fn painted_diff() -> Vec<(String, ColorU)> {
        let app = AppContext::default();
        let mut element: Box<dyn Element> = Box::new(Diff::new(parse_unified_diff(&sample_text())));
        let commands = render_element(&mut element, vec2f(600.0, 400.0), &app);
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, color, .. } => Some((text.clone(), *color)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn parses_unified_diff_into_hunks() {
        let hunks = parse_unified_diff(&sample_text());
        assert_eq!(hunks.len(), 2);
        assert_eq!(
            (
                hunks[0].old_start,
                hunks[0].old_count,
                hunks[0].new_start,
                hunks[0].new_count
            ),
            (1, 3, 1, 4)
        );
        assert_eq!(hunks[0].section, "fn main() {");
        assert_eq!(hunks[0].header(), "@@ -1,3 +1,4 @@ fn main() {");
        assert_eq!(hunks[1].header(), "@@ -10,3 +11,2 @@ fn other() {");
    }

    #[test]
    fn gutter_columns_encode_the_change_type() {
        let hunks = parse_unified_diff(&sample_text());
        let lines = &hunks[0].lines;
        // Context: both columns carry a number.
        assert_eq!(lines[0].kind, DiffLineKind::Context);
        assert_eq!((lines[0].old_line, lines[0].new_line), (Some(1), Some(1)));
        // Removed: only the old column is populated.
        assert_eq!(lines[1].kind, DiffLineKind::Removed);
        assert_eq!((lines[1].old_line, lines[1].new_line), (Some(2), None));
        // Added: only the new column is populated.
        assert_eq!(lines[2].kind, DiffLineKind::Added);
        assert_eq!((lines[2].old_line, lines[2].new_line), (None, Some(2)));
        assert_eq!((lines[3].old_line, lines[3].new_line), (None, Some(3)));
    }

    #[test]
    fn hunk_and_diff_line_counts() {
        let hunks = parse_unified_diff(&sample_text());
        assert_eq!(
            (hunks[0].added(), hunks[0].removed(), hunks[0].context()),
            (2, 1, 2)
        );
        assert_eq!(
            (hunks[1].added(), hunks[1].removed(), hunks[1].context()),
            (0, 1, 2)
        );
        let diff = Diff::new(hunks);
        assert_eq!(
            diff.stats(),
            DiffStats {
                added: 2,
                removed: 2
            }
        );
    }

    #[test]
    fn skipped_context_between_hunks_is_a_gap_row() {
        let diff = Diff::new(parse_unified_diff(&sample_text()));
        let rows = diff.display_rows();

        let gaps: Vec<usize> = rows
            .iter()
            .filter_map(|row| match row {
                DiffRow::Gap { skipped } => Some(*skipped),
                _ => None,
            })
            .collect();
        // Hunk 1 ends at old line 4; hunk 2 starts at old line 10.
        assert_eq!(gaps, vec![6]);

        let gap_at = rows
            .iter()
            .position(|row| matches!(row, DiffRow::Gap { .. }))
            .expect("a gap row");
        let second_header = rows
            .iter()
            .position(|row| matches!(row, DiffRow::HunkHeader { old_start: 10, .. }))
            .expect("the second hunk header");
        assert_eq!(gap_at + 1, second_header, "the gap sits between the hunks");
        assert!(
            matches!(rows[0], DiffRow::HunkHeader { old_start: 1, .. }),
            "the first row is the first hunk's header, not a gap"
        );
        // 2 headers + 5 body lines + 3 body lines + 1 gap.
        assert_eq!(rows.len(), 11);
    }

    #[test]
    fn renders_gutters_markers_and_theme_change_colours() {
        let app = AppContext::default();
        let runs = painted_diff();
        let success = app.theme.color(ColorToken::Success);
        let error = app.theme.color(ColorToken::Error);
        let text = app.theme.color(ColorToken::Text);
        let muted = app.theme.color(ColorToken::Muted);

        assert!(
            runs.iter()
                .any(|(run, color)| run == "+" && *color == success),
            "an added row draws a + marker in the success colour, got {runs:?}"
        );
        assert!(
            runs.iter()
                .any(|(run, color)| run == "-" && *color == error),
            "a removed row draws a - marker in the error colour, got {runs:?}"
        );
        assert!(
            runs.iter()
                .any(|(run, color)| run == "    let x = 2;" && *color == success),
            "added body text uses the success token"
        );
        assert!(
            runs.iter()
                .any(|(run, color)| run == "    let x = 1;" && *color == error),
            "removed body text uses the error token"
        );
        assert!(
            runs.iter()
                .any(|(run, color)| run == "fn main() {" && *color == text),
            "context body text stays in the text token"
        );
        // Gutter numbers are drawn in the muted token, both hunk starts included.
        assert!(runs
            .iter()
            .any(|(run, color)| run == "10" && *color == muted));
        assert!(runs
            .iter()
            .any(|(run, color)| run == "12" && *color == muted));
    }

    #[test]
    fn draws_the_hunk_header_and_the_skipped_context_separator() {
        let runs = painted_diff();
        assert!(
            runs.iter()
                .any(|(run, _)| run == "@@ -1,3 +1,4 @@ fn main() {"),
            "the first hunk header is drawn"
        );
        assert!(
            runs.iter()
                .any(|(run, _)| run == "@@ -10,3 +11,2 @@ fn other() {"),
            "the second hunk header is drawn"
        );
        assert!(
            runs.iter()
                .any(|(run, _)| run == "... 6 unchanged lines skipped"),
            "the gap separator names how much context was skipped, got {runs:?}"
        );
    }

    #[test]
    fn is_inline_and_draws_no_box() {
        let app = AppContext::default();
        let mut element: Box<dyn Element> = Box::new(Diff::new(parse_unified_diff(&sample_text())));
        let commands = render_element(&mut element, vec2f(600.0, 400.0), &app);
        let counts = command_counts(&commands);

        assert_eq!(
            counts.stroke_rect, 0,
            "a diff is inline: it draws no border"
        );
        for command in &commands {
            if let RenderCommand::FillRect { corner_radius, .. } = command {
                assert_eq!(
                    *corner_radius, 0.0,
                    "diff rows are square rails and tints, never a rounded card"
                );
            }
        }
        assert!(counts.fill_rect > 0, "changed rows paint a tint and a rail");
        assert!(counts.draw_text > 0);
    }

    #[test]
    fn empty_diff_lays_out_to_nothing() {
        let app = AppContext::default();
        let mut element: Box<dyn Element> = Box::new(Diff::new(Vec::new()));
        let commands = render_element(&mut element, vec2f(300.0, 200.0), &app);
        assert!(commands.is_empty(), "an empty diff paints nothing");
        assert_eq!(element.size(), Some(Vector2F::zero()));
    }

    #[test]
    fn hiding_line_numbers_drops_the_gutters() {
        let app = AppContext::default();
        let mut element: Box<dyn Element> =
            Box::new(Diff::new(parse_unified_diff(&sample_text())).with_line_numbers(false));
        let commands = render_element(&mut element, vec2f(600.0, 400.0), &app);
        let runs: Vec<String> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            !runs.iter().any(|run| run == "10" || run == "12"),
            "the old/new gutter numbers must not be drawn, got {runs:?}"
        );
        assert!(runs.iter().any(|run| run == "    let x = 2;"));
    }
}
