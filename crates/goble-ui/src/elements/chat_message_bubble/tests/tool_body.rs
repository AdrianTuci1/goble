//! The body a tool result draws is bounded: what a frame lays out is what a pane
//! can show, never what the tool returned. A codebase search over a big tree
//! answers with tens of kilobytes on one line per match, and an unbounded body
//! is shaped and wrapped again on every frame.

use super::*;

/// The bytes one expanded body draws, written out rather than read from
/// `tool_body::MAX_BODY_BYTES`: raising that constant has to break these tests,
/// because raising it is what widens what a frame lays out.
const BODY_BYTES: usize = 16 * 1024;
/// The characters one drawn line carries before it is elided.
const LINE_CHARS: usize = 2_000;

/// A result of `lines` lines, each `line_chars` characters long, the shape a
/// search's matches take when a repository's files are one line each.
fn large_result(lines: usize, line_chars: usize) -> String {
    let long = "x".repeat(line_chars);
    let mut result = String::from("matches");
    for index in 0..lines {
        result.push('\n');
        result.push_str(&format!("src/generated.rs:{index}: {long}"));
    }
    result
}

/// The characters the frame draws, and the visual rows they land on, across
/// every run the bubble painted.
fn drawn(commands: &[RenderCommand]) -> (usize, usize) {
    let chars = drawn_texts(commands)
        .iter()
        .map(|text| text.chars().count())
        .sum();
    (chars, drawn_rows(commands))
}

/// An expanded body draws the byte cap's worth of result and one row naming what
/// it left: a megabyte on six-kilobyte lines costs a pane's body, not a
/// megabyte of shaping per frame.
#[test]
fn an_expanded_result_is_bounded_by_its_byte_cap() {
    let result = large_result(200, 6_000);
    assert!(
        result.len() > 1_000_000,
        "the fixture is a megabyte of result"
    );
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "codebase_search",
            r#"{"pattern":"goblin-worker"}"#,
            ToolCallStatus::Finished,
            Some(&result),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let (chars, rows) = drawn(&commands);
    assert!(
        chars <= BODY_BYTES + 128,
        "the expanded body draws at most the byte cap, drew {chars} characters"
    );
    assert!(
        rows <= 400,
        "the expanded body's rows follow the cap, not the result: {rows} rows"
    );
    // DSH caps an expanded body in a scrollable card, so the elision row it
    // names lives below the card's viewport; the cap is what the frame paints.
    let app = AppContext::default();
    let card = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::SurfaceRaised)
                && (*corner_radius - crate::elements::chat_typography::card_radius(&app)).abs()
                    < 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the expanded body is drawn on DSH's card");
    let cap = crate::elements::chat_typography::CARD_MAX_INNER_HEIGHT
        + 2.0 * crate::elements::chat_typography::CARD_PADDING.top
        + 1.0;
    assert!(
        card.height() <= cap,
        "the body card is capped at DSH's max height ({cap}), got {card:?}"
    );
}

/// A truncated body draws the head and tail of the result with each line elided
/// to the line cap: eight lines of six kilobytes are eight capped lines, not the
/// result's head spelled out.
#[test]
fn a_truncated_result_elides_its_long_lines() {
    let result = large_result(20, 6_000);
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "codebase_search",
            r#"{"pattern":"goblin-worker"}"#,
            ToolCallStatus::Finished,
            Some(&result),
        )],
        Some(ToolDisplayMode::Truncated),
    );

    let (chars, _) = drawn(&commands);
    let head_and_tail = 8 * LINE_CHARS + 128;
    assert!(
        chars <= head_and_tail,
        "truncated draws its lines elided to {LINE_CHARS}, drew {chars} characters"
    );
}

/// A read is bounded the same way: a one-line file of a hundred kilobytes is one
/// capped line of excerpt, and the row says the rest is not drawn.
#[test]
fn an_expanded_read_elides_a_line_past_its_cap() {
    let result = "x".repeat(100_000);
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "read_file",
            r#"{"path":"src/generated.rs"}"#,
            ToolCallStatus::Finished,
            Some(&result),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let (chars, _) = drawn(&commands);
    assert!(
        chars <= BODY_BYTES + 128,
        "the read's one line is elided, drew {chars} characters"
    );
}
