use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::chat_content::{tool_fold_key, ChatRole, ToolDisplayMode};
use crate::elements::TerminalFilter;
use crate::elements::{terminal_block, AppContext, TerminalData, TerminalStatus};
use crate::geometry::{vec2f, Vector2F};
use crate::render::RenderCommand;
use crate::test_util::render_element;
use crate::platform::text_atlas::FontWeight;
use crate::theme::ColorToken;
use goble_core::harness::ToolCallStatus;
use goble_core::harness::{tool_kind_for, tool_row, ToolKind};
use super::*;
use super::super::{ChatMessageBubble, read_excerpt};
use super::super::read_excerpt::{drawn_lines, EXPANDED_LINE_CAP};
use super::super::tool_body::READ_TRUNCATION;

/// A tool call is inline rows: no border stroke and no raised `surface_2`
/// card background.
#[test]
fn tool_call_draws_no_border_or_card() {
    let (commands, _) =
        paint_tool_calls(vec![call("ls", "{}", ToolCallStatus::Finished, None)]);

    let borders = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
        .count();
    assert_eq!(borders, 0, "a tool call must not draw a border container");

    let app = AppContext::default();
    let card_bg = app.theme.color(ColorToken::SurfaceRaised);
    let card_fills = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == card_bg))
        .count();
    assert_eq!(
        card_fills, 0,
        "a tool call must not paint a raised card background"
    );
}

/// DSH's tool row: the whole row reads in one tertiary ink at the row's
/// regular weight. The verb is no longer bold and the operand no longer takes
/// grok-build's per-kind colour — `tool_subject_color` stays as the revert
/// seam, but the owner asked for DSH's single ink.
#[test]
fn a_tool_row_reads_in_one_quiet_ink_at_regular_weight() {
    let (commands, _) = paint_tool_calls(vec![call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some("1: fn main() {}"),
    )]);
    let app = AppContext::default();
    let words = commands.iter().find_map(|command| match command {
        RenderCommand::DrawText {
            text,
            color,
            font_weight,
            font_family,
            font_size,
            line_height,
            ..
        } if text.starts_with("Read") => {
            Some((*color, *font_weight, *font_family, *font_size, *line_height))
        }
        _ => None,
    });
    let (color, weight, family, size, line_height) = words.expect("the row's words are drawn");
    assert_eq!(weight, FontWeight::Regular, "DSH's tool row is regular weight");
    assert_eq!(
        color,
        app.theme.color(ColorToken::Muted),
        "the row reads in the quiet ink"
    );
    assert_eq!(family, crate::theme::FontFamily::System, "the row is prose, not mono");
    assert_eq!(size, 13.0, "DSH's process row is 13 px");
    assert!(
        (line_height - 24.0 / 13.0).abs() < 1e-5,
        "the row sits on a 24 px line, got {line_height}"
    );
    // No run in the row takes grok-build's per-kind colours any more.
    for per_kind in [ColorToken::Path, ColorToken::Command, ColorToken::Success] {
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { color, text, .. }
                    if *color == app.theme.color(per_kind) && !text.trim().is_empty()
            )),
            "the row must not use the per-kind {per_kind:?} ink"
        );
    }
}

/// A call with nothing to show collapses to a single row — the tool's own
/// name, on one baseline, with no argument or result row. `ls` carries no path
/// here, so the parse has no operand to name and falls back to the tool,
/// exactly as it does for a call no family claims.
///
/// The row wears the diamond whatever its status: the state is its words and
/// its colour, and the pointer swaps the mark for the arrow.
#[test]
fn collapsed_tool_call_renders_in_one_row() {
    let app = AppContext::default();
    let (commands, _) = paint_tool_calls(vec![call("ls", "{}", ToolCallStatus::Finished, None)]);

    assert_eq!(
        drawn_rows(&commands),
        1,
        "a collapsed tool call must occupy one row"
    );
    assert_eq!(
        drawn_row_texts(&commands),
        vec!["◆ ls".to_string()],
        "the diamond and the tool name are drawn"
    );
    assert_eq!(
        text_runs(&commands)[0].1,
        app.theme.color(ColorToken::Muted),
        "a resting row reads quiet"
    );
}

/// A tool whose definition declares no shape (here `list_entities`) expands in
/// place: the row names the tool and the one argument it can name, and the body
/// opens on the result. The argument payload is never printed as JSON.
#[test]
fn expanded_generic_tool_call_draws_its_result_and_never_prints_json() {
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "list_entities",
            r#"{"kind":"agent"}"#,
            ToolCallStatus::Running,
            Some("2 entities"),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t.contains("List Entities")),
        "the tool name is drawn as words, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "2 entities"),
        "the result row is drawn, got {texts:?}"
    );
    assert_eq!(
        drawn_row_texts(&commands),
        vec!["❯ List Entities agent".to_string(), "2 entities".to_string()],
        "the row and its result are the whole body, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains(['{', '}'])),
        "the argument payload is never printed as JSON, got {texts:?}"
    );
}

/// An integration call (`use_tool`) names each argument on its own row — the
/// shape the reference draws an integration tool's arguments in — rather than
/// printing the payload it was called with.
#[test]
fn an_integration_tools_arguments_are_named_not_printed() {
    let arguments = r#"{"name":"save_issue","input":{"title":"Bug"}}"#;
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "use_tool",
            arguments,
            ToolCallStatus::Finished,
            Some("saved"),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let rows = drawn_row_texts(&commands);
    assert!(
        rows.iter().any(|row| row.contains("name: save_issue")),
        "each argument is a `key: value` row, got {rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.contains("input: 1 field")),
        "a nested argument is named by how much it holds, got {rows:?}"
    );
    assert!(
        !rows
            .iter()
            .any(|row| row.contains('{') || row.contains('[')),
        "no row prints a payload in braces, got {rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row == arguments),
        "the payload is never printed as it was received, got {rows:?}"
    );
    assert_eq!(
        rows.last().map(String::as_str),
        Some("saved"),
        "the result closes the body, got {rows:?}"
    );
}

/// Each call is drawn by the family its name parses into; the row itself is
/// the parse's ([`goble_core::harness::tool_row`]), not the renderer's.
#[test]
fn tool_shapes_are_read_from_the_parsed_family() {
    assert_eq!(tool_kind_for("run_command"), ToolKind::Execute);
    assert_eq!(tool_kind_for("read_file"), ToolKind::Read);
    assert_eq!(tool_kind_for("write_file"), ToolKind::Create);
    assert_eq!(tool_kind_for("edit_file"), ToolKind::Edit);
    assert_eq!(tool_kind_for("web_search"), ToolKind::WebSearch);
    assert_eq!(tool_kind_for("run_agent"), ToolKind::SubAgent);
    assert_eq!(tool_kind_for("credentials"), ToolKind::Other);

    // The row the renderer draws is the parse's own: verb, operand, detail.
    let row = tool_row(
        "read_file",
        &serde_json::json!({ "path": "src/lib.rs" }),
        None,
    );
    assert_eq!(row.verb, "Read ");
    assert_eq!(row.subject, "src/lib.rs");
}

/// A command's call names the command on its own row and draws what it printed
/// beneath it, never the raw argument JSON.
#[test]
fn command_call_shows_the_command_and_its_output() {
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "run_command",
            r#"{"command":"cargo test -p goble-ui"}"#,
            ToolCallStatus::Finished,
            Some("test result: ok. 225 passed"),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t.contains("cargo test -p goble-ui")),
        "the command is the block's title and its command line, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "test result: ok. 225 passed"),
        "the output is shown, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains(r#"{"command""#)),
        "the raw argument JSON is not shown, got {texts:?}"
    );

    // DSH's `Run` row: one quiet ink, the command included, at the row's
    // regular weight.
    let app = AppContext::default();
    let row = commands.iter().find_map(|command| match command {
        RenderCommand::DrawText {
            text,
            color,
            font_weight,
            ..
        } if text.contains("Run") && text.contains("cargo test -p goble-ui") => {
            Some((*color, *font_weight))
        }
        _ => None,
    });
    assert_eq!(
        row,
        Some((app.theme.color(ColorToken::Text), FontWeight::Regular)),
        "the opened Run row is one primary, regular-weight run"
    );
}

/// An agent's command is drawn continuously in its reply, never as the
/// terminal block the pane draws. A block is a segment: it carries its own
/// header, its copy button and its filter, and those belong to a command the
/// *user* ran. An agent's call has none of them, and its output is unbounded
/// by a viewer chrome it did not ask for.
///
/// The build gate is the strongest form of this: the module that draws a tool
/// call's body does not reference the plumbing a block is built with, so a
/// block cannot be constructed on this path at all. This asserts the drawn
/// result of that — no block title, no block controls, no border.
#[test]
fn an_agents_command_is_not_a_terminal_block() {
    let command = "cargo test -p goble-ui";
    let output = "test result: ok. 231 passed";
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "run_command",
            r#"{"command":"cargo test -p goble-ui"}"#,
            ToolCallStatus::Finished,
            Some(output),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    // The block's own chrome: a copy or filter control is an icon, and the
    // block's status label is its header's trailing text. Neither is drawn.
    let icons = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::DrawIcon { .. }))
        .count();
    assert_eq!(icons, 0, "an agent's call draws no block controls");
    // The one frame is DSH's expanded-body card (a rounded fill under a
    // hairline), not the terminal block's header/chrome.
    let borders = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
        .count();
    assert_eq!(borders, 1, "an agent's call draws its body card and no block frame");

    // What it does draw: the row that names the command, then the output.
    let runs = text_runs(&commands);
    assert!(
        runs.iter().any(|(text, _, _, _)| text.contains(command)),
        "the row names the command it ran, got {runs:?}"
    );
    assert!(
        runs.iter().any(|(text, _, _, _)| text == output),
        "the output is drawn continuously under it, got {runs:?}"
    );
    // DSH's tool row is 13 px; the body is the 11 px code tier.
    let row_size = runs
        .iter()
        .find(|(text, _, _, _)| text.contains(command))
        .map(|(_, _, _, size)| *size)
        .expect("the row's size");
    assert_eq!(row_size, 13.0, "the row is DSH's 13 px process tier");
    assert!(
        runs.iter()
            .filter(|(text, _, _, _)| text == output)
            .all(|(_, _, _, size)| *size == 11.0),
        "the body is the 11 px code tier, got {runs:?}"
    );

    // The block's own title is nowhere: the command is drawn once, by the row
    // that names it, not a second time as a block header.
    let titles = runs
        .iter()
        .filter(|(text, _, _, _)| text.contains(command))
        .count();
    assert_eq!(titles, 1, "the command is drawn once, got {runs:?}");
}

/// The block the pane draws for a user's command is unchanged: one command is
/// still one block wherever the *user* ran it. The agent's path and the pane's
/// are the two sides of the same distinction, so both are pinned.
#[test]
fn a_users_command_is_still_the_terminal_block() {
    let app = AppContext::default();
    let data = TerminalData::for_command(
        "cargo test -p goble-ui",
        "test result: ok. 231 passed",
        TerminalStatus::Success,
    );
    let mut block = terminal_block(&data, TerminalFilter::default(), None, None);
    let block_commands = render_element(&mut block, vec2f(600.0, 200.0), &app);
    let block_runs = text_runs(&block_commands);
    assert!(
        block_runs.iter().any(|(text, _, _, _)| text == "❯ "),
        "the pane's block draws its own prompt line, got {block_runs:?}"
    );
    let block_rows = drawn_row_texts(&block_commands);
    assert!(
        block_rows.iter().any(|row| row == "cargo test -p goble-ui"),
        "the pane's block heads the command with its own title row, got {block_rows:?}"
    );

    // An agent's call to the same command carries no block chrome: it draws its
    // tool row, with no header repeating the command.
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "run_command",
            r#"{"command":"cargo test -p goble-ui"}"#,
            ToolCallStatus::Finished,
            Some("test result: ok. 231 passed"),
        )],
        Some(ToolDisplayMode::Expanded),
    );
    let transcript_rows = drawn_row_texts(&commands);
    assert!(
        transcript_rows
            .iter()
            .any(|row| row == "❯ Run cargo test -p goble-ui"),
        "an agent's call draws its own tool row, got {transcript_rows:?}"
    );
    assert_eq!(
        transcript_rows
            .iter()
            .filter(|row| row.as_str() == "cargo test -p goble-ui")
            .count(),
        0,
        "an agent's call draws no terminal header repeating the command, got \
         {transcript_rows:?}"
    );
}

/// A read shows a path rather than the arguments blob. Folded, the path is one
/// step away with its run: the aggregated header names the bucket, and opening
/// the run draws the member row that carries the path.
#[test]
fn read_call_shows_the_path() {
    let calls = vec![call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some("fn main() {}"),
    )];
    let (commands, _) = paint_tool_calls(calls.clone());

    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t == "Read 1 file"),
        "the folded run names its bucket, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains(r#"{"path""#)),
        "the raw argument JSON is not shown, got {texts:?}"
    );

    let (commands, _) = paint_tool_calls_with_run_open(calls, 0);
    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t.contains("src/lib.rs")),
        "the opened run's member row carries the path, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains(r#"{"path""#)),
        "the raw argument JSON is still not shown, got {texts:?}"
    );
}

/// A read starts folded: one aggregated header row, nothing else. Opening the
/// run shows the member row with the path; the file's body stays hidden until
/// that member is opened itself.
#[test]
fn folded_read_draws_one_line() {
    let calls = vec![call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some(&(1..=12).map(|i| format!("line {i}\n")).collect::<String>()),
    )];
    let (commands, _) = paint_tool_calls(calls.clone());

    assert_eq!(
        drawn_rows(&commands),
        1,
        "a folded read must occupy one header row"
    );
    assert_eq!(
        drawn_texts(&commands),
        vec!["◈".to_string(), "Read 1 file".to_string()],
        "the folded run draws only its aggregated label behind the dotted diamond"
    );

    let (commands, _) = paint_tool_calls_with_run_open(calls, 0);
    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t.contains("src/lib.rs")),
        "the opened run's member row carries its path, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("line 1")),
        "the file body is hidden while the member is folded, got {texts:?}"
    );

    // A member of a chain the user opened reads white whole — the verb and its
    // path — while a folded row is muted.
    let app = AppContext::default();
    let run = |needle: &str| {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawText {
                text,
                color,
                font_weight,
                ..
            } if text == needle => Some((*color, *font_weight)),
            _ => None,
        })
    };
    assert_eq!(
        run("Read src/lib.rs"),
        Some((app.theme.color(ColorToken::Text), FontWeight::Regular)),
        "a member of an open chain reads white at the row's regular weight"
    );
}

/// A command starts folded into its chain: the header states how many commands
/// ran, and neither the command line nor its output is drawn. Opening the chain
/// brings the command row in.
#[test]
fn folded_command_hides_its_output() {
    let (commands, _) = paint_tool_calls(vec![call(
        "run_command",
        r#"{"command":"cargo test -p goble-ui"}"#,
        ToolCallStatus::Finished,
        Some("test result: ok. 225 passed"),
    )]);

    assert_eq!(
        drawn_rows(&commands),
        1,
        "a folded command must occupy one header row"
    );
    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t == "Ran 1 command"),
        "the folded command's header counts the command, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("cargo test")),
        "the command line is hidden while folded, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("test result")),
        "the command output is hidden while folded, got {texts:?}"
    );
}

/// An edit starts folded carrying its `+N/-M` diffstat on the aggregated
/// label, and its hunks are not drawn until it is opened.
#[test]
fn folded_edit_shows_its_diffstat() {
    let (commands, _) = paint_tool_calls(vec![call(
        "edit_file",
        r#"{"path":"src/lib.rs","old_text":"let x = 1;","new_text":"let x = 2;"}"#,
        ToolCallStatus::Finished,
        Some(r#"edited "src/lib.rs""#),
    )]);

    let rows = drawn_row_texts(&commands);
    let header = rows
        .iter()
        .find(|row| row.contains("Edited"))
        .unwrap_or_else(|| panic!("the folded edit draws its row, got {rows:?}"));
    assert!(
        header.contains("Edited 1 file"),
        "the folded edit's header counts the file, got {header:?}"
    );
    assert!(
        header.ends_with("+1/-1"),
        "the folded edit's header carries its diffstat, got {header:?}"
    );
    assert!(
        !rows.iter().any(|row| row.contains("let x = 1;")),
        "the hunks are hidden while folded, got {rows:?}"
    );
}

/// A truncated read draws the first 5 and last 3 lines with the middle
/// elided — grok-build's read truncation — as a numbered excerpt.
#[test]
fn truncated_read_draws_first_five_and_last_three_lines() {
    let result: String = (1..=12).map(|i| format!("line {i}\n")).collect();
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "read_file",
            r#"{"path":"src/lib.rs"}"#,
            ToolCallStatus::Finished,
            Some(&result),
        )],
        Some(ToolDisplayMode::Truncated),
    );

    let rows = drawn_row_texts(&commands);
    for i in 1..=5 {
        assert!(
            rows.iter().any(|r| r.contains(&format!("line {i}"))),
            "line {i} is in the head, got {rows:?}"
        );
    }
    for i in 10..=12 {
        assert!(
            rows.iter().any(|r| r.contains(&format!("line {i}"))),
            "line {i} is in the tail, got {rows:?}"
        );
    }
    assert!(
        rows.iter().any(|r| r.contains('…')),
        "the elided middle is marked, got {rows:?}"
    );
    for i in 6..=9 {
        assert!(
            !rows.iter().any(|r| r.contains(&format!("line {i}"))),
            "line {i} is in the elided middle, got {rows:?}"
        );
    }
}

/// An expanded read is an editor excerpt: a right-aligned line-number
/// gutter (its width taken from the largest number), the file's lines
/// through H1's highlighter, each line on its own background band.
#[test]
fn expanded_read_draws_a_gutter_and_highlighted_lines_on_a_band() {
    let source: String = (1..=10).map(|i| format!("fn f{i}() {{}}\n")).collect();
    let app = AppContext::default();
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "read_file",
            r#"{"path":"src/lib.rs"}"#,
            ToolCallStatus::Finished,
            Some(&source),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t == " 1  "),
        "line 1's number is right-aligned in the two-wide gutter, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "10  "),
        "line 10's number fills the gutter, got {texts:?}"
    );

    // The body is highlighted, not one muted blob: drop the gutter and the
    // header's own runs and require more than one code colour.
    let header = ["◆", "Read src/lib.rs"];
    let body_colours: std::collections::HashSet<ColorU> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { text, color, .. }
                if !header.contains(&text.as_str())
                    && !(text.trim().len() < text.len()
                        && text.trim().chars().all(|ch| ch.is_ascii_digit())) =>
            {
                Some(*color)
            }
            _ => None,
        })
        .collect();
    assert!(
        body_colours.len() > 1,
        "the read's lines are highlighted, got {body_colours:?}"
    );

    // The body's lines sit on the one card DSH draws them in, so the excerpt
    // no longer bands each line separately: a rounded card fill and its 0.5 px
    // hairline are what the body is drawn on.
    let card = crate::elements::chat_typography::card_radius(&app);
    let card_fills = commands
        .iter()
        .filter(|c| {
            matches!(c, RenderCommand::FillRect { color, corner_radius, .. }
                if *color == app.theme.color(ColorToken::SurfaceRaised)
                    && (*corner_radius - card).abs() < 0.01)
        })
        .count();
    assert_eq!(card_fills, 1, "the excerpt sits on one card, got {card_fills}");
    assert!(
        commands.iter().any(|c| matches!(
            c,
            RenderCommand::StrokeRect { width, .. } if (*width - 0.5).abs() < 1e-5
        )),
        "the card carries DSH's hairline, got {commands:?}"
    );
    let per_line_bands = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::FillRect { color, corner_radius, .. }
            if *color == app.theme.color(ColorToken::Bg) && *corner_radius == 0.0))
        .count();
    assert_eq!(per_line_bands, 0, "the card owns the surface, not each line");
}

/// `count` numbered source lines, the shape the harness's `read_file` returns.
fn source_lines(count: usize) -> String {
    (1..=count).map(|i| format!("fn f{i}() {{}}\n")).collect()
}

/// A large read is bounded by what is on screen: expanded draws the file's head
/// only, inside DSH's capped scrollable body card, so a frame's work no longer
/// follows the file's length.
#[test]
fn an_expanded_read_is_capped_by_its_body_card() {
    let result = source_lines(2000);
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "read_file",
            r#"{"path":"src/lib.rs"}"#,
            ToolCallStatus::Finished,
            Some(&result),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    // The excerpt is drawn inside DSH's capped, scrollable card: only the rows
    // that fit the card's viewport are painted, so a frame's work follows the
    // cap and not the file.
    let app = AppContext::default();
    let card = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::FillRect { rect, color, corner_radius }
                if *color == app.theme.color(ColorToken::SurfaceRaised)
                    && (*corner_radius - crate::elements::chat_typography::card_radius(&app)).abs() < 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the excerpt is drawn on its card");
    assert!(
        card.height() <= crate::elements::chat_typography::CARD_MAX_INNER_HEIGHT
            + 2.0 * crate::elements::chat_typography::CARD_PADDING.top
            + 1.0,
        "the card is capped at DSH's max height, got {card:?}"
    );

    let rows = drawn_row_texts(&commands);
    assert!(
        rows.len() <= 20,
        "only the rows that fit the card's viewport are painted, got {}",
        rows.len()
    );
    assert!(
        rows.iter().any(|row| row.contains("f1()")),
        "the excerpt opens on the file's first line, got {rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row.contains("fn f200()")),
        "a line past the viewport is not painted, got {rows:?}"
    );
}

/// The drawn lines stay numbered as the file numbers them: the gutter is as
/// wide as the largest number the excerpt carries, and each band carries its
/// own line's number.
#[test]
fn a_capped_read_keeps_the_gutter_numbers_of_the_lines_it_draws() {
    let result = source_lines(2000);
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "read_file",
            r#"{"path":"src/lib.rs"}"#,
            ToolCallStatus::Finished,
            Some(&result),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t == "   1  "),
        "line 1 is right-aligned in the four-wide gutter, got {texts:?}"
    );
    // The card's viewport paints only the lines that fit it, numbered as the
    // file numbers them.
    let numbers: Vec<usize> = texts
        .iter()
        .filter_map(|text| text.trim().parse::<usize>().ok())
        .collect();
    assert!(
        numbers.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "the painted gutter is consecutive, got {numbers:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "2000  "),
        "the file's last number is not drawn, got {texts:?}"
    );
}

/// The helper the column draws from hands the highlighter only the drawn
/// blocks: the expanded head up to the cap, and truncated's head and tail,
/// with nothing from the elided middle joined or tokenised.
#[test]
fn drawn_lines_hands_the_highlighter_only_the_text_it_draws() {
    let result = source_lines(2000);
    let read = call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some(&result),
    );
    let arguments = serde_json::json!({ "path": "src/lib.rs" });
    let excerpt = read_excerpt(&arguments, &read).expect("content addresses");

    let open = drawn_lines(&excerpt, ToolDisplayMode::Expanded);
    assert_eq!(open.head, 0..EXPANDED_LINE_CAP);
    assert_eq!(
        open.head_text.lines().count(),
        EXPANDED_LINE_CAP,
        "the highlighter's input is the drawn head only"
    );
    assert!(
        !open.head_text.contains("fn f2000()"),
        "the elided tail is never joined for the highlighter"
    );
    assert!(open.tail.is_none() && open.tail_text.is_none());
    assert_eq!(
        open.residue,
        Some((
            2000 - EXPANDED_LINE_CAP,
            format!("… {} more lines", 2000 - EXPANDED_LINE_CAP)
        ))
    );

    let (first, last) = READ_TRUNCATION;
    let truncated = drawn_lines(&excerpt, ToolDisplayMode::Truncated);
    assert_eq!(truncated.head, 0..first);
    assert_eq!(truncated.tail, Some((2000 - last)..2000));
    assert_eq!(
        truncated.head_text.lines().count(),
        first,
        "truncated still draws grok-build's head"
    );
    let tail_text = truncated
        .tail_text
        .as_deref()
        .expect("the tail block is drawn");
    assert_eq!(tail_text.lines().count(), last, "truncated still draws grok-build's tail");
    assert_eq!(
        truncated.residue,
        Some((2000 - first - last, "…".to_string()))
    );
    assert!(
        !truncated.head_text.contains("fn f1000()") && !tail_text.contains("fn f1000()"),
        "the elided middle is in neither block"
    );
}

/// A read with no usable line information — here a failed read — degrades
/// to today's plain rows rather than drawing numbers it cannot justify.
#[test]
fn a_read_without_line_information_degrades_to_plain_rows() {
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "read_file",
            r#"{"path":"src/lib.rs"}"#,
            ToolCallStatus::Error,
            Some("failed to read \"src/lib.rs\": no such file"),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let texts = drawn_texts(&commands);
    assert!(
        texts
            .iter()
            .any(|t| t == "failed to read \"src/lib.rs\": no such file"),
        "the failed read's message is drawn as a plain row, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "1  "),
        "a failed read is not numbered, got {texts:?}"
    );

    let app = AppContext::default();
    let band = app.theme.color(ColorToken::Bg);
    let bands = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == band))
        .count();
    assert_eq!(bands, 0, "a plain row draws no read band, got {bands}");
}

/// A write is a Path tool, not a read: its confirmation is drawn plain, not
/// numbered as if it were a file's body.
#[test]
fn a_write_is_not_drawn_as_a_numbered_read() {
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "write_file",
            r#"{"path":"src/lib.rs","content":"fn main() {}"}"#,
            ToolCallStatus::Finished,
            Some(r#"wrote "src/lib.rs""#),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t == r#"wrote "src/lib.rs""#),
        "the write confirmation is drawn, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "1  "),
        "a write result is not line-numbered, got {texts:?}"
    );
}

/// A write whose harness recorded a diff draws the change: the added lines as
/// diff rows, and the diffstat on the row that folds them. The tool's own
/// confirmation only repeats the path, so it is not drawn beside the change.
#[test]
fn a_recorded_write_diff_draws_the_change() {
    let mut write = call(
        "write_file",
        r#"{"path":"src/lib.rs","content":"fn main() {}\nlet x = 1;\n"}"#,
        ToolCallStatus::Finished,
        Some(r#"wrote "src/lib.rs""#),
    );
    write.diff = Some("@@ -0,0 +1,2 @@\n+fn main() {}\n+let x = 1;\n".to_string());
    let (commands, _) =
        paint_tool_calls_with_fold(vec![write], Some(ToolDisplayMode::Expanded));

    let rows = drawn_row_texts(&commands);
    let header = rows
        .iter()
        .find(|row| row.contains("Creating"))
        .unwrap_or_else(|| panic!("the write draws its row, got {rows:?}"));
    assert!(
        header.ends_with("+2/-0"),
        "the row states the change it made, got {header:?}"
    );
    assert!(
        rows.iter().any(|row| row.ends_with("fn main() {}")),
        "the added lines are diff rows, got {rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.ends_with("let x = 1;")),
        "every added line is drawn, got {rows:?}"
    );
    let texts = drawn_texts(&commands);
    assert!(
        !texts.iter().any(|text| text.contains("wrote")),
        "the confirmation is not drawn beside the change, got {texts:?}"
    );
}

/// A recorded edit is drawn from the tool's own diff, so its hunks carry the
/// file's line numbers and the unchanged lines around the replacement.
#[test]
fn a_recorded_edit_diff_carries_the_files_context() {
    let mut edit = call(
        "edit_file",
        r#"{"path":"src/lib.rs","old_text":"let x = 1;","new_text":"let x = 2;"}"#,
        ToolCallStatus::Finished,
        Some(r#"edited "src/lib.rs""#),
    );
    edit.diff = Some(
        "@@ -2,3 +2,3 @@\n let x = 1;\n-let x = 2;\n+let x = 3;\n let x = 4;\n".to_string(),
    );
    let (commands, _) =
        paint_tool_calls_with_fold(vec![edit], Some(ToolDisplayMode::Expanded));

    let rows = drawn_row_texts(&commands);
    let header = rows
        .iter()
        .find(|row| row.contains("Edit"))
        .unwrap_or_else(|| panic!("the edit draws its row, got {rows:?}"));
    assert!(
        header.ends_with("+1/-1"),
        "the row's diffstat comes from the recorded diff, got {header:?}"
    );
    assert!(
        rows.iter().any(|row| row.ends_with("let x = 1;")),
        "the context before the change is drawn, got {rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.ends_with("let x = 3;")),
        "the added line is drawn, got {rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.ends_with("let x = 4;")),
        "the context after the change is drawn, got {rows:?}"
    );
    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|text| text == "3"),
        "the hunk's own line numbers are drawn, got {texts:?}"
    );
}

/// The base line comes from the call's own data: a `cat -n`-style gutter
/// already in the result is kept, so it is never re-numbered.
#[test]
fn a_pre_numbered_result_keeps_its_own_line_numbers() {
    let numbered = call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some("    7\tfn main() {}\n    8\t}\n"),
    );
    let arguments = serde_json::json!({ "path": "src/lib.rs" });
    let excerpt = read_excerpt(&arguments, &numbered).expect("a numbered body addresses");
    assert_eq!(excerpt.base_line, 7, "the gutter's own numbers are kept");
    assert_eq!(excerpt.lines, vec!["fn main() {}", "}"]);
}

/// An explicit offset is the base when the result carries no gutter; the
/// harness's `read_file` declares none, so the base falls to 1.
#[test]
fn an_offset_argument_sets_the_base_line() {
    let read = call(
        "read_file",
        r#"{"path":"src/lib.rs","offset":50}"#,
        ToolCallStatus::Finished,
        Some("fn foo() {}\nfn bar() {}\n"),
    );
    let arguments = serde_json::json!({ "path": "src/lib.rs", "offset": 50 });
    let excerpt = read_excerpt(&arguments, &read).expect("content addresses");
    assert_eq!(excerpt.base_line, 50);

    let no_offset = call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some("fn foo() {}\n"),
    );
    let arguments = serde_json::json!({ "path": "src/lib.rs" });
    assert_eq!(read_excerpt(&arguments, &no_offset).unwrap().base_line, 1);
}

/// A body that is only partly numbered has no trustworthy line numbers, so
/// it is not addressable.
#[test]
fn a_partially_numbered_result_is_not_addressable() {
    let read = call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some("    1\tfn main() {}\nnot numbered\n"),
    );
    let arguments = serde_json::json!({ "path": "src/lib.rs" });
    assert!(read_excerpt(&arguments, &read).is_none());
}

/// A click on a folded run's header opens the run; a click on a member's header
/// then advances that call's own fold `Collapsed -> Truncated -> Expanded ->
/// Collapsed`. Both states live in the app-owned map.
#[test]
fn clicking_a_tool_call_header_cycles_its_fold() {
    let app = AppContext::default();
    let fold = Rc::new(RefCell::new(HashMap::new()));
    let calls = vec![call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some("line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9"),
    )];
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(calls.clone())
        .with_tool_fold(fold.clone());
    let key = tool_fold_key(&calls[0], 0);
    let group = run_key(&calls, 0);

    // The run starts folded: its header is what a click reaches first.
    let commands = paint_bubble(&mut bubble, &app);
    let header = run_origin(&commands, "Read 1 file");
    click_at(&mut bubble, &app, header + vec2f(2.0, 4.0));
    assert_eq!(
        fold.borrow().get(&group),
        Some(&ToolDisplayMode::Expanded),
        "the first click opens the run"
    );
    assert_eq!(
        fold.borrow().get(&key),
        None,
        "and leaves the member's own fold alone"
    );
    assert!(
        drawn_row_texts(&paint_bubble(&mut bubble, &app))
            .iter()
            .any(|row| row.contains("src/lib.rs")),
        "the opened run draws its member row, path included"
    );

    // The member row is on screen now, and its header cycles the call. The
    // header's own run shifts right once the open row draws its rail, so each
    // click reads the run's position from the frame it lands on.
    let click_member = |bubble: &mut ChatMessageBubble| {
        let commands = paint_bubble(bubble, &app);
        let header = run_origin(&commands, "Read src/lib.rs");
        click_at(bubble, &app, header + vec2f(2.0, 4.0));
    };
    click_member(&mut bubble);
    assert_eq!(
        fold.borrow().get(&key),
        Some(&ToolDisplayMode::Truncated),
        "the next click truncates the call"
    );

    click_member(&mut bubble);
    assert_eq!(
        fold.borrow().get(&key),
        Some(&ToolDisplayMode::Expanded),
        "the next click expands the call"
    );

    click_member(&mut bubble);
    assert_eq!(
        fold.borrow().get(&key),
        Some(&ToolDisplayMode::Collapsed),
        "the last click folds the call back"
    );
}

/// An edit shows diff rows: the removed text, the added text and the Q9
/// element's `-`/`+` markers. The changed line is highlighted, so its code
/// is several runs; the row's runs concatenated are the line.
#[test]
fn edit_call_shows_diff_rows() {
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "edit_file",
            r#"{"path":"src/lib.rs","old_text":"let x = 1;","new_text":"let x = 2;"}"#,
            ToolCallStatus::Finished,
            Some(r#"edited "src/lib.rs""#),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t.contains("src/lib.rs")),
        "the edited path is shown, got {texts:?}"
    );
    let rows = drawn_row_texts(&commands);
    assert!(
        rows.iter().any(|row| row.ends_with("let x = 1;")),
        "the removed text is a diff row, got {rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.ends_with("let x = 2;")),
        "the added text is a diff row, got {rows:?}"
    );
    assert!(
        texts.iter().any(|t| t == "-"),
        "the removed row's marker is drawn, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "+"),
        "the added row's marker is drawn, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("old_text")),
        "the raw argument JSON is not shown, got {texts:?}"
    );
}

/// The edit's changed rows carry the theme's insert/delete band, and the
/// code inside them is highlighted as the edited file's language.
#[test]
fn edit_call_paints_the_diff_band_and_highlights_the_code() {
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "edit_file",
            r#"{"path":"src/lib.rs","old_text":"let x = 1;","new_text":"let x = 2;"}"#,
            ToolCallStatus::Finished,
            Some(r#"edited "src/lib.rs""#),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let app = AppContext::default();
    let insert = app.theme.color(ColorToken::DiffInsertBg);
    let delete = app.theme.color(ColorToken::DiffDeleteBg);
    let bands: Vec<ColorU> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { color, .. } if *color == insert || *color == delete => {
                Some(*color)
            }
            _ => None,
        })
        .collect();
    assert!(
        bands.contains(&insert),
        "the edit draws the insert band, got {bands:?}"
    );
    assert!(
        bands.contains(&delete),
        "the edit draws the delete band, got {bands:?}"
    );

    let highlighted =
        crate::syntax::highlight("let x = 2;", "src/lib.rs").expect("rust resolves");
    assert!(
        highlighted[0].len() > 1,
        "the edited line is multi-coloured"
    );
    let expected: std::collections::HashSet<ColorU> =
        highlighted[0].iter().map(|span| span.color).collect();
    let drawn: std::collections::HashSet<ColorU> = text_runs(&commands)
        .into_iter()
        .map(|(_, color, _, _)| color)
        .collect();
    assert!(
        expected.is_subset(&drawn),
        "the edited code is painted in the highlighter's colours: \
         expected {expected:?}, drawn {drawn:?}"
    );
}

/// A web search shows the query and the sources its result names.
#[test]
fn web_search_call_shows_the_query_and_its_sources() {
    let result = "2 results\nTITLE: Async Rust\nURL: https://example.com/async\nSNIPPET: a\n\nTITLE: Tokio\nURL: https://example.com/tokio\nSNIPPET: b\n";
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "web_search",
            r#"{"query":"rust async"}"#,
            ToolCallStatus::Finished,
            Some(result),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t.contains("rust async")),
        "the query is shown, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "https://example.com/async"),
        "the first source is shown, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "https://example.com/tokio"),
        "the second source is shown, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("TITLE:")),
        "the raw result is not shown, got {texts:?}"
    );
}

/// A tool block is bracketed down both sides, in the free margin beside the
/// text, while the pointer is on it or the group is open. The strokes take no
/// layout space, so the words keep the prose column's alignment.
#[test]
fn a_tool_block_is_bracketed_without_moving_its_words() {
    let app = AppContext::default();
    let read = || {
        call(
            "read_file",
            r#"{"path":"src/lib.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn main() {}\n"),
        )
    };
    for (label, fold, pointer) in [
        ("folded and resting", HashMap::new(), false),
        ("folded under the pointer", HashMap::new(), true),
        (
            "opened",
            HashMap::from([(
                format!("verb-group:{}", tool_fold_key(&read(), 0)),
                ToolDisplayMode::Expanded,
            )]),
            false,
        ),
    ] {
        let mut bubble = ChatMessageBubble::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Prose starts here.")],
        )
        .with_tool_calls(vec![read()])
        .with_tool_fold(Rc::new(RefCell::new(fold)))
        .with_tool_hover(Rc::new(RefCell::new(None)));
        let commands = if pointer {
            paint_with_pointer_over(&mut bubble, &app, "◈")
        } else {
            bubble.layout(
                SizeConstraint::loose(vec2f(400.0, 400.0)),
                &mut LayoutContext::default(),
                &app,
            );
            let mut paint_ctx = PaintContext::new(Renderer::new());
            bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
            paint_ctx
                .renderer
                .take()
                .map(|renderer| renderer.commands().to_vec())
                .unwrap_or_default()
        };

        let origin_x = |prefix: &str| {
            commands.iter().find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text.starts_with(prefix) => {
                    Some(origin.x)
                }
                _ => None,
            })
        };
        let prose = origin_x("Prose starts here.").expect("the prose is drawn");
        // DSH's row geometry: the 16 px leading box starts where prose starts
        // and the words begin one box and one 6 px gap past it.
        let leading = origin_x("❯")
            .or_else(|| origin_x("◈"))
            .or_else(|| origin_x("◆"))
            .unwrap_or_else(|| {
                panic!("{label}: the run's own header is drawn: {:?}", drawn_texts(&commands))
            });
        assert!(
            (leading - prose).abs() < 0.5,
            "{label}: the leading box starts in the prose column, got {leading} against {prose}"
        );
        let row_start = origin_x("Read").unwrap_or(leading);
        let expected = prose
            + crate::elements::chat_typography::LEADING_BOX
            + crate::elements::chat_typography::GLYPH_GAP;
        assert!(
            (row_start - expected).abs() < 0.5,
            "{label}: the words start past the leading box, got {row_start} against {expected}"
        );

        let brackets: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == app.theme.color(ColorToken::Muted)
                        && rect.size.width <= 2.5
                        && rect.size.height > 0.0 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .collect();
        let bracketed = label != "folded and resting";
        assert_eq!(
            brackets.len(),
            if bracketed { 2 } else { 0 },
            "{label}: both sides are bracketed, got {brackets:?}"
        );
        if bracketed {
            let (left, right) = (brackets[0], brackets[1]);
            assert!(
                left.min_x() < row_start && right.max_x() > row_start,
                "{label}: the strokes sit outside the words: {brackets:?} against {row_start}"
            );
            assert_eq!(
                left.min_y(),
                right.min_y(),
                "{label}: both strokes start on the block's first line"
            );
            assert!(
                right.max_x() - left.min_x() > 300.0,
                "{label}: the pair frames the block's width, not just its words: {brackets:?}"
            );
        }
    }
}

/// A call that stands on its own is framed while its body is open; a collapsed
/// one is framed only under the pointer, so the rail follows the selection.
#[test]
fn a_standalone_row_is_bracketed_while_open_or_under_the_pointer() {
    let app = AppContext::default();
    let brackets = |commands: &[RenderCommand]| {
        commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    RenderCommand::FillRect { rect, color, .. }
                        if *color == app.theme.color(ColorToken::Muted)
                            && rect.size.width <= 2.5
                            && rect.size.height > 0.0
                )
            })
            .count()
    };
    let paint = |bubble: &mut ChatMessageBubble, pointer: bool, prefix: &str| {
        if pointer {
            return paint_with_pointer_over(bubble, &app, prefix);
        }
        bubble.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        paint_ctx
            .renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    };
    let bubble = |call: ToolCall, mode: ToolDisplayMode| {
        let key = tool_fold_key(&call, 0);
        ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
            .with_tool_calls(vec![call])
            .with_tool_fold(Rc::new(RefCell::new(HashMap::from([(key, mode)]))))
            .with_tool_hover(Rc::new(RefCell::new(None)))
    };

    let read = call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some("fn main() {}"),
    );
    for (label, pointer) in [("away", false), ("under the pointer", true)] {
        let mut open = bubble(read.clone(), ToolDisplayMode::Expanded);
        let commands = paint(&mut open, pointer, "❯");
        assert_eq!(
            brackets(&commands),
            2,
            "an opened row keeps its rail with the pointer {label}"
        );
    }

    // A spawn ends a chain and stays on its own row, so it can be collapsed.
    let spawn = call(
        "run_agent",
        r#"{"description":"map the call sites"}"#,
        ToolCallStatus::Finished,
        Some("done"),
    );
    for (label, pointer, expected) in [
        ("away", false, 0),
        ("under the pointer", true, 2),
    ] {
        let mut collapsed = bubble(spawn.clone(), ToolDisplayMode::Collapsed);
        let commands = paint(&mut collapsed, pointer, "◆");
        assert_eq!(
            brackets(&commands),
            expected,
            "a collapsed stand-alone row is framed only under the pointer ({label})"
        );
    }
}

/// An opened row reads as selected: its mark stays the arrow and reads white
/// while the pointer is elsewhere, in the partial and the full open state.
#[test]
fn an_open_row_keeps_the_arrow_in_place_of_the_diamond() {
    let app = AppContext::default();
    for mode in [ToolDisplayMode::Truncated, ToolDisplayMode::Expanded] {
        let (commands, _) = paint_tool_calls_with_fold(
            vec![call(
                "read_file",
                r#"{"path":"src/lib.rs"}"#,
                ToolCallStatus::Finished,
                Some("fn main() {}"),
            )],
            Some(mode),
        );
        let runs = text_runs(&commands);
        let mark = runs
            .iter()
            .find(|(text, ..)| text == "❯" || text == "◆")
            .expect("the row draws its mark");
        assert_eq!(
            mark.0, "❯",
            "an opened row swaps the diamond for the arrow ({mode:?})"
        );
        assert_eq!(
            mark.1,
            app.theme.color(ColorToken::Text),
            "an opened row reads white ({mode:?})"
        );
    }
}

/// The rail is decided while the block paints: a hover cell still naming the
/// row from an earlier frame cannot keep the strokes on screen once the pointer
/// has left.
#[test]
fn a_closed_rows_rail_leaves_with_the_pointer() {
    let app = AppContext::default();
    let spawn = call(
        "run_agent",
        r#"{"description":"map the call sites"}"#,
        ToolCallStatus::Finished,
        Some("done"),
    );
    let key = tool_fold_key(&spawn, 0);

    let mut probe = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(vec![spawn.clone()])
        .with_tool_fold(Rc::new(RefCell::new(HashMap::from([(
            key.clone(),
            ToolDisplayMode::Collapsed,
        )]))))
        .with_tool_hover(Rc::new(RefCell::new(None)));
    probe.layout(
        SizeConstraint::loose(vec2f(400.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut ctx = PaintContext::new(Renderer::new());
    probe.paint(vec2f(0.0, 0.0), &mut ctx, &app);
    let origin = ctx
        .renderer
        .take()
        .and_then(|renderer| {
            renderer.commands().iter().find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text.starts_with('◆') => {
                    Some(*origin)
                }
                _ => None,
            })
        })
        .expect("the collapsed row draws its diamond");

    let strokes = |cursor: Vector2F| {
        let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
            .with_tool_calls(vec![spawn.clone()])
            .with_tool_fold(Rc::new(RefCell::new(HashMap::from([(
                key.clone(),
                ToolDisplayMode::Collapsed,
            )]))))
            .with_tool_hover(Rc::new(RefCell::new(Some(key.clone()))));
        bubble.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = cursor;
        bubble.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        ctx.renderer
            .take()
            .map(|renderer| {
                renderer
                    .commands()
                    .iter()
                    .filter(|command| {
                        matches!(
                            command,
                            RenderCommand::FillRect { rect, color, .. }
                                if *color == app.theme.color(ColorToken::Muted)
                                    && rect.size.width <= 2.5
                                    && rect.size.height > 0.0
                        )
                    })
                    .count()
            })
            .unwrap_or_default()
    };

    assert_eq!(
        strokes(vec2f(origin.x + 4.0, origin.y + 4.0)),
        2,
        "the row is framed while the pointer is on it"
    );
    assert_eq!(
        strokes(vec2f(origin.x + 4.0, origin.y + 220.0)),
        0,
        "the rail cannot outlive the pointer"
    );
}

/// A call under the pointer swaps its diamond for the arrow and reads white,
/// in the diamond's two columns, so its words stay where they were.
#[test]
fn a_tool_row_under_the_pointer_shows_the_arrow() {
    let app = AppContext::default();
    let call = call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some("fn main() {}"),
    );
    let key = tool_fold_key(&call, 0);
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(vec![call])
        .with_tool_fold(Rc::new(RefCell::new(HashMap::from([(
            key.clone(),
            ToolDisplayMode::Expanded,
        )]))))
        .with_tool_hover(Rc::new(RefCell::new(Some(key))));
    bubble.layout(
        SizeConstraint::loose(vec2f(400.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default();
    let runs = text_runs(&commands);
    let mark = runs
        .iter()
        .find(|(text, _, _, _)| text == "❯" || text == "◆")
        .expect("the row draws its mark");
    assert_eq!(mark.0, "❯", "the pointer swaps the diamond for the arrow");
    assert_eq!(
        mark.1,
        app.theme.color(ColorToken::Text),
        "the row under the pointer reads white"
    );
    assert!(
        runs.iter().any(|(text, ..)| text.starts_with("Read")),
        "the words are still drawn beside the arrow, got {runs:?}"
    );
}

/// A chain header reads quiet while it rests and white while it is selected.
/// Selecting it — by opening the chain or by putting the pointer over it —
/// brings the arrow in and brightens the label, with no band behind the row.
#[test]
fn the_chain_label_reads_quiet_until_selected() {
    let app = AppContext::default();
    let calls = || {
        vec![call(
            "read_file",
            r#"{"path":"src/lib.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn main() {}\n"),
        )]
    };
    let label_color = |commands: &[RenderCommand]| -> crate::color::ColorU {
        text_runs(commands)
            .into_iter()
            .find(|(text, _, _, _)| text == "Read 1 file")
            .expect("the chain's label is drawn")
            .1
    };
    let hover_bands = |commands: &[RenderCommand]| -> usize {
        commands
            .iter()
            .filter(|command| {
                matches!(command, RenderCommand::FillRect { color, .. } if *color == app.theme.color(ColorToken::Hover))
            })
            .count()
    };

    let (resting, _) = paint_tool_calls(calls());
    assert_eq!(
        label_color(&resting),
        app.theme.color(ColorToken::Muted),
        "a resting chain reads quiet"
    );
    let resting_texts = drawn_texts(&resting);
    assert!(
        resting_texts.iter().any(|text| text == "◈"),
        "a resting chain wears the dotted diamond, got {resting_texts:?}"
    );
    assert!(
        !resting_texts.iter().any(|text| text.contains('❯')),
        "the arrow appears only once the row is selected, got {resting_texts:?}"
    );
    assert_eq!(hover_bands(&resting), 0, "a chain row paints no band");

    let (open, _) = paint_tool_calls_with_run_open(calls(), 0);
    assert_eq!(
        label_color(&open),
        app.theme.color(ColorToken::Text),
        "an open chain reads white"
    );
    let open_texts = drawn_texts(&open);
    assert!(
        open_texts.iter().any(|text| text == "❯"),
        "an open chain swaps its mark for the arrow, got {open_texts:?}"
    );

    // The pointer brightens a closed chain the same way, through the app's
    // own hover cell: the row's paint reports it and the next frame builds
    // the arrow.
    let hovered: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    *hovered.borrow_mut() = Some(run_key(&calls(), 0));
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(calls())
        .with_tool_hover(Rc::clone(&hovered));
    bubble.layout(
        SizeConstraint::loose(vec2f(400.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let hovered_commands = paint_ctx
        .renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default();
    assert_eq!(
        label_color(&hovered_commands),
        app.theme.color(ColorToken::Text),
        "the chain under the pointer reads white while closed"
    );
    assert!(
        drawn_texts(&hovered_commands)
            .iter()
            .any(|text| text == "❯"),
        "the chain under the pointer draws its arrow"
    );
    assert!(
        !drawn_texts(&hovered_commands)
            .iter()
            .any(|text| text.contains("src/lib.rs")),
        "a bright chain stays closed until it is clicked"
    );
}

/// The card reads as one 13 / 24 process line whatever the fold map says, and
/// it always carries the control that enters the child.
#[test]
fn a_sub_agent_card_is_thirteen_px_in_both_folds() {
    let call = spawn_call(ToolCallStatus::Running);
    let record = record(
        "conv-child-1",
        crate::elements::chat_content::SubAgentRowStatus::Running,
        "reading",
        std::time::Duration::from_secs(4),
        None,
    );
    for mode in [None, Some(ToolDisplayMode::Expanded)] {
        let (commands, _) = paint_sub_agent(
            vec![call.clone()],
            HashMap::from([(call.id.clone(), record.clone())]),
            mode,
        );
        let sizes: Vec<f32> = text_runs(&commands)
            .into_iter()
            .map(|(_, _, _, size)| size)
            .collect();
        assert!(
            sizes.iter().all(|size| (*size - 13.0).abs() < 1e-5),
            "the card reads at DSH's 13 px in every fold, got {sizes:?}"
        );
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "arrow-up"
            )),
            "the card draws its open control"
        );
    }
}

/// Consecutive folded calls of every groupable family stack into one
/// aggregated header, in first-appearance order, with the counts and plural
/// nouns grok-build uses. Commands fold in with the rest.
#[test]
fn consecutive_groupable_calls_stack_into_one_run_header() {
    let mut calls = vec![
        call(
            "read_file",
            r#"{"path":"src/a.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn a() {}"),
        ),
        call(
            "grep",
            r#"{"pattern":"TODO"}"#,
            ToolCallStatus::Finished,
            Some("src/b.rs:1: TODO"),
        ),
        call(
            "list_dir",
            r#"{"path":"src"}"#,
            ToolCallStatus::Finished,
            Some("a.rs\nb.rs"),
        ),
        call(
            "run_command",
            r#"{"command":"cargo test"}"#,
            ToolCallStatus::Finished,
            Some("ok"),
        ),
        call(
            "read_file",
            r#"{"path":"src/c.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn c() {}"),
        ),
    ];
    // Two reads share a tool name; real calls carry distinct ids, and the fold
    // map is keyed by them.
    for (index, call) in calls.iter_mut().enumerate() {
        call.id = format!("call-{index}");
    }
    let (commands, _) = paint_tool_calls(calls);
    let rows = drawn_row_texts(&commands);
    // The aggregated label is longer than the test's pane, so it wraps; joining
    // the wrapped rows reads the whole header.
    assert_eq!(
        rows.join(" "),
        "◈ Read 2 files, Searched 1 pattern, Listed 1 dir, Ran 1 command",
        "the run folds every family in first-appearance order, got {rows:?}"
    );
    assert!(
        !drawn_texts(&commands).iter().any(|text| text == "src/a.rs"),
        "the folded members' paths are not drawn, got {rows:?}"
    );
}

/// The run's header states the work in the present tense while any member is
/// live, and appends grok-build's failed count in the error colour.
#[test]
fn a_running_run_uses_the_present_tense_and_counts_failures() {
    let app = AppContext::default();
    let calls = vec![
        call(
            "read_file",
            r#"{"path":"src/a.rs"}"#,
            ToolCallStatus::Running,
            None,
        ),
        call(
            "read_file",
            r#"{"path":"src/b.rs"}"#,
            ToolCallStatus::Error,
            Some("no such file"),
        ),
        call(
            "read_file",
            r#"{"path":"src/c.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn c() {}"),
        ),
    ];
    let (commands, _) = paint_tool_calls(calls);
    let rows = drawn_row_texts(&commands);
    assert_eq!(
        rows,
        vec!["◈ Reading 3 files · 1 failed".to_string()],
        "one header states the run's tense and failures"
    );
    let failed = text_runs(&commands)
        .into_iter()
        .find(|(text, _, _, _)| text.starts_with(" · "))
        .expect("the failure suffix is drawn");
    assert_eq!(
        failed.1,
        app.theme.color(ColorToken::Error),
        "a run with a failure carries the error accent on its count"
    );
}

/// A member the user opened keeps its own body without splitting the run: the
/// header still counts the folded members around it.
#[test]
fn an_opened_member_keeps_its_body_inside_the_folded_run() {
    let mut calls = vec![
        call(
            "read_file",
            r#"{"path":"src/a.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn a() {}"),
        ),
        call(
            "read_file",
            r#"{"path":"src/b.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn b() {}"),
        ),
        call(
            "read_file",
            r#"{"path":"src/c.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn c() {}"),
        ),
    ];
    for (index, call) in calls.iter_mut().enumerate() {
        call.id = format!("call-{index}");
    }
    let (commands, _) = paint_tool_calls_with_modes(
        calls,
        Some(&[
            ToolDisplayMode::Collapsed,
            ToolDisplayMode::Expanded,
            ToolDisplayMode::Collapsed,
        ]),
    );
    let rows = drawn_row_texts(&commands);
    assert!(
        rows.iter().any(|row| row.contains("Read 2 files")),
        "the header counts the folded members around the opened one, got {rows:?}"
    );
    let member = rows
        .iter()
        .find(|row| row.contains("src/b.rs"))
        .unwrap_or_else(|| panic!("the opened member draws its row, got {rows:?}"));
    assert!(
        member.starts_with("❯ Read "),
        "the opened member draws its own marked row inside the folded chain, got {member:?}"
    );
    assert!(
        rows.iter().any(|row| row.contains("fn b()")),
        "the opened member keeps its body inside the run, got {rows:?}"
    );
    assert!(
        !drawn_texts(&commands).iter().any(|text| text == "src/a.rs"),
        "the folded members stay hidden, got {rows:?}"
    );
}

/// A command's output keeps the colours the terminal resolved and adds no
/// emphasis of its own, and an underline is never one of them: no tool row is
/// underlined for being a path, a URL or a label, nor because the command
/// printed an SGR 4.
#[test]
fn tool_rows_underline_nothing_they_did_not_receive() {
    use crate::elements::terminal_block::{TerminalLine, TerminalRun};

    let app = AppContext::default();
    let plain = TerminalLine {
        text: "src/lib.rs".to_string(),
        kind: crate::elements::TerminalLineKind::Output,
        runs: Vec::new(),
    };
    let spans = crate::elements::chat_message_bubble::tool_body::output_spans(
        &plain,
        ToolCallStatus::Finished,
        &app,
    );
    assert_eq!(spans.len(), 1);
    assert!(!spans[0].underline, "a plain output line is not underlined");

    let emitted = TerminalLine {
        text: "underlined".to_string(),
        kind: crate::elements::TerminalLineKind::Output,
        runs: vec![TerminalRun {
            text: "underlined".to_string(),
            color: None,
            background: None,
            bold: false,
            dim: false,
            italic: false,
            underline: true,
            strikeout: false,
        }],
    };
    let spans = crate::elements::chat_message_bubble::tool_body::output_spans(
        &emitted,
        ToolCallStatus::Finished,
        &app,
    );
    assert!(
        !spans[0].underline,
        "a row stays plain even when the command it ran printed an SGR 4"
    );
}

/// Two collapsed tool rows touch, and a row that is open keeps the block's gap
/// between itself and its neighbour. The pitch between two headers is therefore
/// one row's height while both are collapsed, and that height plus the gap once
/// one of them is open.
#[test]
fn two_collapsed_tool_rows_touch_and_an_open_one_keeps_its_gap() {
    let app = AppContext::default();
    let baselines = |commands: &[RenderCommand]| -> Vec<f32> {
        let mut ys: Vec<f32> = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText { origin, .. } => Some(origin.y),
                _ => None,
            })
            .collect();
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
        // A row's leading glyph sits a few points off its words' baseline; one
        // row is one cluster, not two.
        let mut rows: Vec<f32> = Vec::new();
        for y in ys {
            if rows.last().is_none_or(|last| y - last > 6.0) {
                rows.push(y);
            }
        }
        rows
    };

    let band = crate::elements::chat_typography::BAND_PADDING_Y * 2.0;
    let sm = app.theme.spacing_px(crate::theme::SpacingToken::Sm);
    // A call still running draws no body, so a pair of them isolates the gap.
    let pending = |path: &str| {
        call(
            "read_file",
            &format!(r#"{{\"path\":\"{path}\"}}"#),
            ToolCallStatus::Running,
            None,
        )
    };
    let pending_calls = || vec![pending("src/lib.rs"), pending("src/other.rs")];

    let (one, one_height) = paint_tool_calls(vec![pending("src/lib.rs")]);
    assert_eq!(baselines(&one).len(), 1, "a collapsed read is one row");
    let row_height = one_height - band;

    let (touching, two_height) = paint_tool_calls(pending_calls());
    let rows = baselines(&touching);
    assert_eq!(rows.len(), 2, "two collapsed rows draw nothing else");
    assert!(
        (rows[1] - rows[0] - row_height).abs() < 0.5,
        "two collapsed rows touch: the headers stand {} apart against a {} row",
        rows[1] - rows[0],
        row_height
    );
    assert!(
        (two_height - one_height - row_height).abs() < 0.5,
        "the second collapsed row adds exactly one row: {} against {}",
        two_height - one_height,
        row_height
    );

    let modes = [ToolDisplayMode::Collapsed, ToolDisplayMode::Expanded];
    let (open, open_height) = paint_tool_calls_with_modes(pending_calls(), Some(&modes));
    let rows = baselines(&open);
    assert!(
        (rows[1] - rows[0] - row_height - sm).abs() < 0.5,
        "an open neighbour keeps the gap: the headers stand {} apart against {} + {}",
        rows[1] - rows[0],
        row_height,
        sm
    );
    assert!(
        (open_height - two_height - sm).abs() < 0.5,
        "the gap is the whole of what opening a body-less neighbour adds: {} against {}",
        open_height - two_height,
        sm
    );
}

/// A question reads as `Ask User <question>`, the way a read reads
/// `Read <path>`: the tool's name never reaches the row with its underscore,
/// and no row draws a rule under itself.
#[test]
fn an_asked_question_draws_as_an_ask_user_row() {
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "ask_user",
            r#"{"question":"Which environment?","quick_replies":["staging","production"]}"#,
            ToolCallStatus::Finished,
            Some("Answer to question 'Which environment?': staging"),
        )],
        Some(ToolDisplayMode::Expanded),
    );
    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|text| text.contains("Ask User")),
        "the row leads with the tool's name read as words: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("Which environment?")),
        "the question is the row's operand: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("staging")),
        "the answer is the row's body: {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text.contains('_')),
        "no tool name is drawn with an underscore under it: {texts:?}"
    );
    assert!(
        !commands.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { rect, .. } if rect.height() <= 1.5
        )),
        "the row draws no underline rule"
    );
}

/// A member the user opened inside an opened run is bracketed on its own: the
/// run's pair of strokes ends above it, and the member's own pair frames its
/// A member the user opened inside an opened run is bracketed on its own: the
/// run's pair of strokes ends above it, and the member's own pair frames its
/// header and its body. Both pairs sit in the same gutter, so the rail reads as
/// one line that steps aside for the block it holds instead of running through
/// it.
#[test]
fn an_opened_member_inside_an_open_run_is_framed_on_its_own() {
    let app = AppContext::default();
    let mut calls = vec![
        call(
            "read_file",
            r#"{"path":"src/a.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn a() {}"),
        ),
        call(
            "read_file",
            r#"{"path":"src/b.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn b() {}"),
        ),
    ];
    for (index, call) in calls.iter_mut().enumerate() {
        call.id = format!("call-{index}");
    }
    let fold = HashMap::from([
        (run_key(&calls, 0), ToolDisplayMode::Expanded),
        (tool_fold_key(&calls[1], 1), ToolDisplayMode::Expanded),
    ]);
    let (commands, _) = paint_tool_calls_with_fold_map(calls, fold);

    let strokes = bracket_strokes(&commands, &app);
    assert_eq!(
        strokes.len(),
        4,
        "the run keeps a pair of its own and the opened member gets one, got {strokes:?}"
    );
    let (run_pair, member_pair) = (&strokes[..2], &strokes[2..]);
    assert_eq!(
        run_pair[0].min_y(),
        run_pair[1].min_y(),
        "the run's pair starts together: {strokes:?}"
    );
    assert_eq!(
        member_pair[0].min_y(),
        member_pair[1].min_y(),
        "the member's pair starts together: {strokes:?}"
    );
    assert!(
        run_pair[0].max_y() < member_pair[0].min_y(),
        "the run's strokes stop above the member's own block, got {strokes:?}"
    );
    // The member's pair starts on its own row and carries on under it, into the
    // body the row opened: that is the block the rail is drawn around now.
    let member_row = run_origin(&commands, "src/b.rs");
    assert!(
        run_pair[0].max_y() <= member_row.y,
        "the run's pair ends at or above the member's row, got {strokes:?} against {member_row:?}"
    );
    assert!(
        member_pair[0].min_y() <= member_row.y,
        "the member's pair starts on its own row, got {strokes:?} against {member_row:?}"
    );
    assert!(
        member_pair[0].max_y() > member_row.y + 1.0,
        "the member's pair runs on past its header into its body, got {strokes:?} against {member_row:?}"
    );
    for pair in strokes.chunks(2) {
        assert!(
            (pair[0].min_x() - strokes[0].min_x()).abs() < 0.5
                && (pair[1].max_x() - strokes[1].max_x()).abs() < 0.5,
            "every stroke sits in the same gutter, got {strokes:?}"
        );
    }
}

/// A double click inside an opened section closes it again. The two clicks the
/// gesture is made of still advance the fold as they always do; the double click
/// that lands after them puts it back to `Collapsed`.
#[test]
fn a_double_click_on_an_opened_section_collapses_it() {
    let app = AppContext::default();
    let read = call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some("fn main() {}"),
    );
    let key = tool_fold_key(&read, 0);
    let fold = Rc::new(RefCell::new(HashMap::from([(
        key.clone(),
        ToolDisplayMode::Expanded,
    )])));
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(vec![read])
        .with_tool_fold(fold.clone())
        .with_tool_hover(Rc::new(RefCell::new(None)));

    let commands = paint_bubble(&mut bubble, &app);
    // A point in the section's body: the rows there carry no gesture of their
    // own, so the block the rail frames is what answers the double click.
    let section = bracket_strokes(&commands, &app)
        .into_iter()
        .next()
        .expect("the opened section keeps its rail");
    let body = vec2f(section.min_x() + 10.0, section.max_y() - 4.0);
    double_click_at(&mut bubble, &app, body);
    assert_eq!(
        fold.borrow().get(&key),
        Some(&ToolDisplayMode::Collapsed),
        "a double click in the open section closes it"
    );
}

/// A double click on an opened run's header folds the run back to its header.
#[test]
fn a_double_click_on_an_open_run_closes_it_to_its_header() {
    let app = AppContext::default();
    let read = call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some("fn main() {}"),
    );
    let group = run_key(std::slice::from_ref(&read), 0);
    let fold = Rc::new(RefCell::new(HashMap::from([(
        group.clone(),
        ToolDisplayMode::Expanded,
    )])));
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(vec![read])
        .with_tool_fold(fold.clone())
        .with_tool_hover(Rc::new(RefCell::new(None)));

    let commands = paint_bubble(&mut bubble, &app);
    let header = run_origin(&commands, "Read 1 file");
    double_click_at(&mut bubble, &app, header + vec2f(2.0, 4.0));
    assert_eq!(
        fold.borrow().get(&group),
        Some(&ToolDisplayMode::Collapsed),
        "a double click on the run's header folds the run"
    );
}

/// A double click on a row that is already closed leaves it closed: the clicks
/// of the gesture step it open, and the double click puts it back rather than
/// leaving it on the state the presses walked to.
#[test]
fn a_double_click_on_a_closed_row_leaves_it_closed() {
    let app = AppContext::default();
    // A spawn ends a chain and keeps its own row, so it is the shape a closed
    // row can be double clicked on without a run's header in the way.
    let spawn = call(
        "run_agent",
        r#"{"description":"map the call sites"}"#,
        ToolCallStatus::Finished,
        Some("done"),
    );
    let key = tool_fold_key(&spawn, 0);
    let fold = Rc::new(RefCell::new(HashMap::new()));
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(vec![spawn])
        .with_tool_fold(fold.clone())
        .with_tool_hover(Rc::new(RefCell::new(None)));

    let commands = paint_bubble(&mut bubble, &app);
    let row = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. } if text.starts_with('◆') => {
                Some(*origin)
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "the closed row draws its diamond: {:?}",
                drawn_texts(&commands)
            )
        });
    double_click_at(&mut bubble, &app, row + vec2f(2.0, 4.0));
    assert_eq!(
        fold.borrow().get(&key),
        Some(&ToolDisplayMode::Collapsed),
        "the double click closes the row the presses opened"
    );
}

/// A double click inside an opened member closes that member and leaves the run
/// around it open: the innermost block the rail frames answers first.
#[test]
fn a_double_click_inside_an_opened_member_closes_that_member() {
    let app = AppContext::default();
    let mut calls = vec![
        call(
            "read_file",
            r#"{"path":"src/a.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn a() {}"),
        ),
        call(
            "read_file",
            r#"{"path":"src/b.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn b() {}"),
        ),
    ];
    for (index, call) in calls.iter_mut().enumerate() {
        call.id = format!("call-{index}");
    }
    let group = run_key(&calls, 0);
    let member_key = tool_fold_key(&calls[1], 1);
    let fold = Rc::new(RefCell::new(HashMap::from([
        (group.clone(), ToolDisplayMode::Expanded),
        (member_key.clone(), ToolDisplayMode::Expanded),
    ])));
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(calls)
        .with_tool_fold(fold.clone())
        .with_tool_hover(Rc::new(RefCell::new(None)));

    let commands = paint_bubble(&mut bubble, &app);
    // The member's own pair is the second block the rail draws; a point near
    // its bottom is in the member's body.
    let strokes = bracket_strokes(&commands, &app);
    assert_eq!(
        strokes.len(),
        4,
        "the run and the opened member are each framed, got {strokes:?}"
    );
    let member = strokes[2];
    double_click_at(&mut bubble, &app, vec2f(member.min_x() + 10.0, member.max_y() - 4.0));
    assert_eq!(
        fold.borrow().get(&member_key),
        Some(&ToolDisplayMode::Collapsed),
        "the double click closes the member it landed in"
    );
    assert_eq!(
        fold.borrow().get(&group),
        Some(&ToolDisplayMode::Expanded),
        "and leaves the run it sits in open"
    );
}
