use crate::color::ColorU;
use crate::elements::AppContext;
use crate::elements::Element;
use crate::elements::LayoutContext;
use crate::elements::SizeConstraint;
use crate::geometry::Vector2F;
use crate::geometry::vec2f;
use crate::render::RenderCommand;
use crate::test_util::{command_counts, render_element};
use crate::theme::ColorToken;
use super::*;

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
    // The one gutter draws a row's new number, or the old one for a removed
    // line, in the muted token.
    assert!(runs
        .iter()
        .any(|(run, color)| run == "11" && *color == muted));
    assert!(runs
        .iter()
        .any(|(run, color)| run == "12" && *color == muted));
}

#[test]
fn changed_rows_paint_the_theme_band_at_full_width() {
    let app = AppContext::default();
    let mut element: Box<dyn Element> = Box::new(Diff::new(parse_unified_diff(&sample_text())));
    let commands = render_element(&mut element, vec2f(600.0, 400.0), &app);

    let insert = app.theme.color(ColorToken::DiffInsertBg);
    let delete = app.theme.color(ColorToken::DiffDeleteBg);
    let width = element.size().expect("a laid out diff has a size").x;

    let bands: Vec<(ColorU, f32)> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == insert || *color == delete =>
            {
                Some((*color, rect.width()))
            }
            _ => None,
        })
        .collect();

    assert!(
        bands.iter().any(|(color, _)| *color == insert),
        "an added row paints the insert band, got {bands:?}"
    );
    assert!(
        bands.iter().any(|(color, _)| *color == delete),
        "a removed row paints the delete band, got {bands:?}"
    );
    assert!(
        bands.iter().all(|(_, band_width)| *band_width == width),
        "every band spans the full row width {width}, got {bands:?}"
    );

    // The weak 10% surface tint this used to be is gone.
    let surface = app.theme.color(ColorToken::Surface);
    let success = app.theme.color(ColorToken::Success);
    let error = app.theme.color(ColorToken::Error);
    let old_tints = [surface.mix(&success, 0.10), surface.mix(&error, 0.10)];
    assert!(
        !bands.iter().any(|(color, _)| old_tints.contains(color)),
        "changed rows no longer paint a 10% surface tint, got {bands:?}"
    );
}

#[test]
fn changed_lines_are_highlighted_inside_the_band() {
    let line = "    let x = 2;";
    let app = AppContext::default();
    let mut element: Box<dyn Element> =
        Box::new(Diff::new(parse_unified_diff(&sample_text())).with_language("src/lib.rs"));
    let commands = render_element(&mut element, vec2f(600.0, 400.0), &app);
    let insert = app.theme.color(ColorToken::DiffInsertBg);

    let band = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. } if *color == insert => Some(*rect),
            _ => None,
        })
        .expect("the first added row paints an insert band");

    let mut runs: Vec<(f32, String, ColorU)> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText {
                origin,
                text,
                color,
                ..
            } if origin.y >= band.min_y() && origin.y < band.max_y() => {
                Some((origin.x, text.clone(), *color))
            }
            _ => None,
        })
        .collect();
    runs.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("finite x"));

    let joined: String = runs.iter().map(|(_, text, _)| text.as_str()).collect();
    assert!(
        joined.ends_with(line),
        "the runs reconstruct the code line, got {joined:?}"
    );

    let drawn: std::collections::HashSet<ColorU> =
        runs.iter().map(|(_, _, color)| *color).collect();
    assert!(
        drawn.len() > 1,
        "the code inside the band is painted in the highlighter's colours: {drawn:?}"
    );
}

#[test]
fn an_unknown_language_keeps_the_flat_change_colour() {
    let app = AppContext::default();
    let mut element: Box<dyn Element> =
        Box::new(Diff::new(parse_unified_diff(&sample_text())).with_language("not-a-language"));
    let commands = render_element(&mut element, vec2f(600.0, 400.0), &app);
    let success = app.theme.color(ColorToken::Success);
    let runs: Vec<(String, ColorU)> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, color, .. } => Some((text.clone(), *color)),
            _ => None,
        })
        .collect();
    assert!(
        runs.iter()
            .any(|(run, color)| run == "    let x = 2;" && *color == success),
        "an unresolved language must fall back to the flat success colour, got {runs:?}"
    );
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
        !runs.iter().any(|run| run == "11" || run == "12"),
        "the gutter numbers must not be drawn, got {runs:?}"
    );
    assert!(runs.iter().any(|run| run == "    let x = 2;"));
}

// ---------------------------------------------------------------------------
// DiffSpec: DSH's measurements are declared once, in public constants.
// ---------------------------------------------------------------------------

#[test]
fn diff_spec_carries_dshs_review_changed_files_and_line_tokens() {
    use crate::elements::diff::DiffSpec;

    assert_eq!(DiffSpec::REVIEW_HEADER_HEIGHT, 38.0);
    assert_eq!(DiffSpec::REVIEW_SELECTOR_HEIGHT, 28.0);
    assert_eq!(DiffSpec::REVIEW_TOOL_SIZE, 28.0);

    assert_eq!(DiffSpec::CHANGED_FILES_HEADER_HEIGHT, 60.0);
    assert_eq!(DiffSpec::CHANGED_FILES_TILE, 40.0);
    assert_eq!(DiffSpec::CHANGED_FILES_TILE_RADIUS, 10.0);
    assert_eq!(DiffSpec::CHANGED_FILES_TITLE_SIZE, 13.0);
    assert_eq!(DiffSpec::CHANGED_FILES_TITLE_LINE_HEIGHT, 20.0);
    assert_eq!(DiffSpec::CHANGED_FILES_STAT_SIZE, 10.0);
    assert_eq!(DiffSpec::CHANGED_FILES_STAT_LINE_HEIGHT, 16.0);
    assert_eq!(
        (
            DiffSpec::CHANGED_FILES_ROW_PAD_TOP,
            DiffSpec::CHANGED_FILES_ROW_PAD_RIGHT,
            DiffSpec::CHANGED_FILES_ROW_PAD_BOTTOM,
            DiffSpec::CHANGED_FILES_ROW_PAD_LEFT,
        ),
        (7.0, 18.0, 7.0, 14.0)
    );
    assert_eq!(DiffSpec::CHANGED_FILES_PATH_SIZE, 12.0);
    assert_eq!(DiffSpec::CHANGED_FILES_COUNT_SIZE, 11.0);
    assert_eq!(DiffSpec::CHANGED_FILES_COUNT_LINE_HEIGHT, 18.0);
    assert_eq!(DiffSpec::CHANGED_FILES_FOLD, 4);

    assert_eq!(DiffSpec::DIFF_LINE_HEIGHT, 22.0);
    assert_eq!(DiffSpec::gutter_width(DiffSpec::DIFF_FONT_SIZE), 42.0);
    assert_eq!(DiffSpec::DIFF_MARKER_WIDTH, 3.0);
    assert!((DiffSpec::sign_width(DiffSpec::DIFF_FONT_SIZE) - 14.4).abs() < 0.001);
}

// ---------------------------------------------------------------------------
// FileDiffView: the reusable unified/split comparison.
// ---------------------------------------------------------------------------

fn commands_for(element: &mut Box<dyn Element>, width: f32) -> Vec<RenderCommand> {
    let app = AppContext::default();
    crate::test_util::render_element(element, vec2f(width, 2_000.0), &app)
}

fn text_runs(commands: &[RenderCommand]) -> Vec<(String, ColorU, f32)> {
    commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText {
                text,
                color,
                origin,
                ..
            } => Some((text.clone(), *color, origin.y)),
            _ => None,
        })
        .collect()
}

#[test]
fn file_diff_view_draws_the_unified_gutters_markers_and_bands() {
    let app = AppContext::default();
    let hunks = parse_unified_diff(&sample_text());
    let mut element: Box<dyn Element> = FileDiffView::new(hunks)
        .with_layout(DiffLayout::Unified)
        .with_language("src/lib.rs")
        .finish();
    let commands = commands_for(&mut element, 600.0);
    let size = element.size().expect("a laid out comparison has a size");

    // Two hunks: 2 headers (22 + 4 + 4), 8 body rows at 22, one 8 px hunk gap
    // and the body's 8 + 16 vertical padding.
    assert_eq!(
        size.y,
        8.0 + (30.0 * 2.0) + (22.0 * 8.0) + (8.0 * 2.0) + 16.0
    );

    let insert = app.theme.color(ColorToken::DiffInsertBg);
    let delete = app.theme.color(ColorToken::DiffDeleteBg);
    let bands: Vec<f32> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. } if *color == insert || *color == delete => {
                Some(rect.width())
            }
            _ => None,
        })
        .collect();
    assert!(
        bands.iter().any(|width| *width == size.x),
        "changed rows paint the full comparison width, got {bands:?}"
    );

    let markers: Vec<f32> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == app.theme.color(ColorToken::Success)
                    || *color == app.theme.color(ColorToken::Error) =>
            {
                Some(rect.width())
            }
            _ => None,
        })
        .collect();
    assert!(
        markers
            .iter()
            .any(|width| *width == DiffSpec::DIFF_MARKER_WIDTH),
        "a changed row paints DSH's 3 px marker, got {markers:?}"
    );

    let texts = text_runs(&commands);
    assert!(texts
        .iter()
        .any(|(text, _, _)| text == "@@ -1,3 +1,4 @@ fn main() {"));
    assert!(texts.iter().any(|(text, _, _)| text == "+"));
    assert!(texts.iter().any(|(text, _, _)| text == "-"));
    assert!(texts.iter().any(|(text, _, _)| text == "1"));
}

#[test]
fn file_diff_view_splits_into_two_cells_and_fills_empty_sides() {
    let app = AppContext::default();
    let hunks = parse_unified_diff(&sample_text());
    let mut element: Box<dyn Element> = FileDiffView::new(hunks)
        .with_layout(DiffLayout::Split)
        .finish();
    let commands = commands_for(&mut element, 600.0);

    let text = |needle: &str| {
        commands.iter().any(
            |command| matches!(command, RenderCommand::DrawText { text, .. } if text == needle),
        )
    };
    // The removed line has an old number and an empty new side; the added line
    // the reverse. Context appears on both sides.
    assert!(text("    let x = 1;"), "the removed line draws on the left");
    assert!(text("    let x = 2;"), "the added line draws on the right");
    assert!(text("fn main() {"));

    let empty = ColorU::new(
        app.theme.color(ColorToken::Hover).r,
        app.theme.color(ColorToken::Hover).g,
        app.theme.color(ColorToken::Hover).b,
        128,
    );
    let empty_fills = commands
        .iter()
        .filter(
            |command| matches!(command, RenderCommand::FillRect { color, .. } if *color == empty),
        )
        .count();
    assert!(empty_fills > 0, "paired rows paint an empty side fill");
}

#[test]
fn file_diff_view_wraps_and_doubles_the_row_height() {
    let mut long = String::from("+");
    for _ in 0..60 {
        long.push_str("word ");
    }
    let diff = format!("--- a/long.txt\n+++ b/long.txt\n@@ -1 +1 @@\n-old\n{long}\n");
    let hunks = parse_unified_diff(&diff);
    let mut plain: Box<dyn Element> = FileDiffView::new(hunks.clone()).finish();
    plain.layout(
        SizeConstraint::loose(vec2f(320.0, 2_000.0)),
        &mut LayoutContext::default(),
        &AppContext::default(),
    );
    let plain_height = plain.size().expect("laid out").y;

    let mut wrapped: Box<dyn Element> = FileDiffView::new(hunks).with_wrap(true).finish();
    let commands = commands_for(&mut wrapped, 320.0);
    let wrapped_height = wrapped.size().expect("laid out").y;
    assert!(
        wrapped_height > plain_height,
        "wrapping a long line makes its row taller: plain {plain_height}, wrapped {wrapped_height}"
    );
    let drawn = text_runs(&commands).len();
    assert!(
        drawn > 5,
        "the wrapped line is drawn as several visual lines, got {drawn} runs"
    );
}

#[test]
fn prepared_diff_prepares_split_rows_and_side_colours() {
    let hunks = parse_unified_diff(&sample_text());
    let prepared = PreparedDiff::new(&hunks, "src/lib.rs");
    let split = prepared.split_rows();
    assert!(
        split.iter().any(|row| matches!(row, super::SplitDiffRow::Row(row) if row.left.is_some() && row.right.is_none())),
        "a deletion pairs with an empty new side"
    );
    assert!(
        split.iter().any(|row| matches!(row, super::SplitDiffRow::Row(row) if row.left.is_none() && row.right.is_some())),
        "an addition pairs with an empty old side"
    );
    let (old, new) = prepared.highlighted_lines();
    assert!(
        !old.is_empty() || !new.is_empty(),
        "the prepared sides carry coloured lines"
    );
}

#[test]
fn file_diff_view_paints_nothing_for_an_empty_comparison() {
    let mut element: Box<dyn Element> = FileDiffView::new(Vec::new()).finish();
    let commands = commands_for(&mut element, 400.0);
    assert!(commands.is_empty());
    assert_eq!(element.size(), Some(Vector2F::zero()));
}

// ---------------------------------------------------------------------------
// Highlighted runs and wrapping: the drawn text must reconstruct the source.
// ---------------------------------------------------------------------------

fn context_hunk(text: &str) -> Vec<Hunk> {
    vec![Hunk {
        old_start: 1,
        old_count: 1,
        new_start: 1,
        new_count: 1,
        section: "fn build()".to_string(),
        lines: vec![DiffLine {
            kind: DiffLineKind::Context,
            old_line: Some(1),
            new_line: Some(1),
            text: text.to_string(),
        }],
    }]
}

/// The body text commands of a one-line comparison, in paint order.
fn body_text(commands: &[RenderCommand]) -> String {
    commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. } if origin.y >= 30.0 => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn the_highlighter_reconstructs_the_source_line_byte_for_byte() {
    let lines = [
        "    let panes = build_panes(state);",
        "        draw_new(pane);",
        "        mark_visible(&pane);",
        "    let value = compute(arg_one, arg_two);",
        "fn build_reviewer(state: &ReviewState) -> Reviewer {",
        "    for pane in panes.iter().filter(|pane| pane.visible) {",
    ];
    for line in lines {
        let highlighted = crate::syntax::highlight(line, "rust").expect("rust resolves");
        let joined: String = highlighted
            .iter()
            .flatten()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(joined, line, "highlight runs must spell the source line");
    }
}

#[test]
fn wrapped_highlighted_lines_reconstruct_the_source_exactly() {
    let lines = [
        "    let panes = build_panes(state);",
        "        draw_new(pane);",
        "    let value = compute(arg_one, arg_two);",
        "fn build_reviewer(state: &ReviewState) -> Reviewer {",
    ];
    for line in lines {
        for wrap in [false, true] {
            for width in [90.0, 120.0, 180.0, 260.0, 320.0] {
                let mut element: Box<dyn Element> = FileDiffView::new(context_hunk(line))
                    .with_language("src/panes.rs")
                    .with_wrap(wrap)
                    .with_line_numbers(false)
                    .finish();
                let commands = commands_for(&mut element, width);
                assert_eq!(
                    body_text(&commands),
                    line,
                    "wrap={wrap} width={width} must draw the source line exactly"
                );
            }
        }
    }
}

#[test]
fn wrapped_highlighted_runs_keep_the_space_at_a_run_boundary() {
    // `build_panes(state)` splits into several highlighted runs; at a width
    // that folds between two of them, every run must still land on the source.
    let line = "    let panes = build_panes(state);";
    let mut element: Box<dyn Element> = FileDiffView::new(context_hunk(line))
        .with_language("src/panes.rs")
        .with_wrap(true)
        .with_line_numbers(false)
        .finish();
    let commands = commands_for(&mut element, 160.0);
    let rows: Vec<String> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. } if origin.y >= 30.0 => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect();
    assert!(rows.len() > 1, "the line wraps into at least two rows: {rows:?}");
    assert_eq!(rows.concat(), line);
}

// ---------------------------------------------------------------------------
// One-sided comparisons: split collapses to one full-width column.
// ---------------------------------------------------------------------------

fn add_only_hunk() -> Vec<Hunk> {
    vec![Hunk {
        old_start: 0,
        old_count: 0,
        new_start: 1,
        new_count: 2,
        section: String::new(),
        lines: vec![
            DiffLine {
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: Some(1),
                text: "        draw_new(pane);".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: Some(2),
                text: "        mark_visible(&pane);".to_string(),
            },
        ],
    }]
}

fn del_only_hunk() -> Vec<Hunk> {
    vec![Hunk {
        old_start: 1,
        old_count: 2,
        new_start: 0,
        new_count: 0,
        section: String::new(),
        lines: vec![
            DiffLine {
                kind: DiffLineKind::Removed,
                old_line: Some(1),
                new_line: None,
                text: "        draw_old(pane);".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Removed,
                old_line: Some(2),
                new_line: None,
                text: "        legacy_footer();".to_string(),
            },
        ],
    }]
}

fn two_sided_hunk() -> Vec<Hunk> {
    vec![Hunk {
        old_start: 1,
        old_count: 1,
        new_start: 1,
        new_count: 1,
        section: String::new(),
        lines: vec![
            DiffLine {
                kind: DiffLineKind::Removed,
                old_line: Some(1),
                new_line: None,
                text: "        draw_old(pane);".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: Some(1),
                text: "        draw_new(pane);".to_string(),
            },
            DiffLine {
                kind: DiffLineKind::Added,
                old_line: None,
                new_line: Some(2),
                text: "        mark_visible(&pane);".to_string(),
            },
        ],
    }]
}

fn empty_fill(commands: &[RenderCommand], app: &AppContext) -> usize {
    let hover = app.theme.color(ColorToken::Hover);
    let empty = ColorU::new(hover.r, hover.g, hover.b, 128);
    commands
        .iter()
        .filter(|command| matches!(command, RenderCommand::FillRect { color, .. } if *color == empty))
        .count()
}

#[test]
fn a_one_sided_split_collapses_to_one_full_width_column() {
    let app = AppContext::default();
    for (label, hunks, band) in [
        (
            "add-only",
            add_only_hunk(),
            app.theme.color(ColorToken::DiffInsertBg),
        ),
        (
            "del-only",
            del_only_hunk(),
            app.theme.color(ColorToken::DiffDeleteBg),
        ),
    ] {
        let view = FileDiffView::new(hunks)
            .with_layout(DiffLayout::Split)
            .with_language("src/panes.rs");
        assert!(view.is_one_sided(), "{label} is one-sided");
        let mut element: Box<dyn Element> = view.finish();
        let commands = commands_for(&mut element, 500.0);
        let width = element.size().expect("laid out").x;
        assert_eq!(width, 500.0);
        assert_eq!(
            empty_fill(&commands, &app),
            0,
            "{label} must not paint an empty half: {commands:?}"
        );
        let bands: Vec<f32> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. } if *color == band => {
                    Some(rect.width())
                }
                _ => None,
            })
            .collect();
        assert!(
            bands.iter().any(|band_width| *band_width == width),
            "{label} paints its change band across the full width: {bands:?}"
        );
    }
}

#[test]
fn a_two_sided_split_keeps_its_two_half_columns() {
    let app = AppContext::default();
    let view = FileDiffView::new(two_sided_hunk())
        .with_layout(DiffLayout::Split)
        .with_language("src/panes.rs");
    assert!(!view.is_one_sided());
    assert_eq!(view.resolved_layout(), DiffLayout::Split);
    let mut element: Box<dyn Element> = view.finish();
    let commands = commands_for(&mut element, 500.0);
    let width = element.size().expect("laid out").x;
    let insert = app.theme.color(ColorToken::DiffInsertBg);
    let bands: Vec<f32> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. } if *color == insert => Some(rect.width()),
            _ => None,
        })
        .collect();
    assert!(
        bands.iter().any(|band_width| *band_width < width * 0.5),
        "a two-sided add fills only its cell, not the row: {bands:?}"
    );
    assert!(
        empty_fill(&commands, &app) > 0,
        "a two-sided split still fills the empty half of a paired row"
    );
}

/// The runs of one highlighted line must start at the exact pen position the
/// rasterizer will have reached, not at the whole-pixel layout budget: the old
/// `advance_width` rounded every glyph up, so each style boundary after the
/// first drifted right by a fraction of a pixel per glyph and the preview read
/// `build_panes (state );` instead of `build_panes(state);`. Concatenating the
/// run texts cannot see that drift, so this asserts the geometry.
#[test]
fn adjacent_highlight_runs_start_at_the_exact_pen_advance() {
    use crate::platform::text_atlas::{advance_width, advance_width_exact, FontWeight};
    use crate::theme::FontFamily;

    let line = "    let panes = build_panes(state);";
    let mut element: Box<dyn Element> = FileDiffView::new(context_hunk(line))
        .with_language("src/panes.rs")
        .with_wrap(false)
        .with_line_numbers(false)
        .finish();
    let commands = commands_for(&mut element, 420.0);
    let mut runs: Vec<(f32, String)> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. } if origin.y >= 30.0 => {
                Some((origin.x, text.clone()))
            }
            _ => None,
        })
        .collect();
    runs.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(runs.len() > 2, "the line is highlighted in several runs: {runs:?}");

    let mut rounded_differed = false;
    for pair in runs.windows(2) {
        let (x, text) = &pair[0];
        let (next_x, next_text) = &pair[1];
        let exact = advance_width_exact(
            text,
            super::DiffSpec::DIFF_FONT_SIZE,
            FontWeight::Regular,
            FontFamily::Mono,
            false,
        );
        let rounded = advance_width(
            text,
            super::DiffSpec::DIFF_FONT_SIZE,
            FontWeight::Regular,
            FontFamily::Mono,
            false,
        );
        assert!(
            (next_x - (x + exact)).abs() < 0.01,
            "run {next_text:?} starts at the exact pen position after {text:?}: \
             expected {}, got {next_x}",
            x + exact
        );
        rounded_differed |= rounded > exact;
    }
    assert!(
        rounded_differed,
        "the fixture must contain a boundary where the rounded budget \
         overshoots, or this test cannot catch the corruption"
    );
}
