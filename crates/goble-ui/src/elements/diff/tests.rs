use crate::color::ColorU;
use crate::elements::AppContext;
use crate::elements::Element;
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
    // Gutter numbers are drawn in the muted token, both hunk starts included.
    assert!(runs
        .iter()
        .any(|(run, color)| run == "10" && *color == muted));
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

    let highlighted = crate::syntax::highlight(line, "src/lib.rs").expect("rust resolves");
    assert!(
        highlighted[0].len() > 1,
        "the fixture line must highlight as more than one run"
    );
    let expected: std::collections::HashSet<ColorU> =
        highlighted[0].iter().map(|span| span.color).collect();
    assert!(expected.len() > 1, "the fixture must be multi-coloured");
    let drawn: std::collections::HashSet<ColorU> =
        runs.iter().map(|(_, _, color)| *color).collect();
    assert!(
        expected.is_subset(&drawn),
        "the code inside the band is painted in the highlighter's colours: \
         expected {expected:?}, drawn {drawn:?}"
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
        !runs.iter().any(|run| run == "10" || run == "12"),
        "the old/new gutter numbers must not be drawn, got {runs:?}"
    );
    assert!(runs.iter().any(|run| run == "    let x = 2;"));
}
