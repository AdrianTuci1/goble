use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::chat_content::{tool_fold_key, ChatRole, ToolDisplayMode};
use crate::elements::TerminalFilter;
use crate::elements::{terminal_block, AppContext, TerminalData, TerminalStatus};
use crate::geometry::vec2f;
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

/// grok-build's tool row: the verb is bold and the operand keeps its own
/// colour. A `read` therefore draws a bold `Read ` before a path in the theme's
/// warm operand colour, not one plain run.
#[test]
fn a_tool_row_draws_a_bold_verb_and_an_accented_operand() {
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "read_file",
            r#"{"path":"src/lib.rs"}"#,
            ToolCallStatus::Finished,
            Some("1: fn main() {}"),
        )],
        Some(ToolDisplayMode::Expanded),
    );
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

    let (verb_color, verb_weight) = run("Read ").expect("the bold verb is drawn");
    assert_eq!(
        verb_weight,
        FontWeight::Bold,
        "the verb is bold, as in grok-build"
    );
    assert_eq!(verb_color, app.theme.color(ColorToken::Text));

    let (path_color, path_weight) = run("src/lib.rs").expect("the operand is drawn");
    assert_eq!(
        path_weight,
        FontWeight::Regular,
        "the operand stays at the row's regular weight"
    );
    assert_eq!(
        path_color,
        app.theme.color(ColorToken::Path),
        "the path takes grok-build's warm path colour"
    );
}

/// A call with nothing to show collapses to a single row — the tool's own
/// name, on one baseline, with no argument or result row. `ls` carries no path
/// here, so the parse has no operand to name and falls back to the tool,
/// exactly as it does for a call no family claims.
///
/// The row carries no status mark: a resting row is quiet, and the pointer
/// brightens it. Its state is its words and its colour, not a second glyph.
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
        vec!["ls".to_string()],
        "only the tool name is drawn"
    );
    assert_eq!(drawn_texts(&commands).len(), 1, "one row of one run");
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
        texts.iter().any(|t| t == "List Entities"),
        "the tool name is drawn as words, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "2 entities"),
        "the result row is drawn, got {texts:?}"
    );
    assert_eq!(
        drawn_row_texts(&commands),
        vec!["List Entities agent".to_string(), "2 entities".to_string()],
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
        texts.iter().any(|t| t == "cargo test -p goble-ui"),
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

    // grok-build's `Run` row: a bold primary verb, then the command in the
    // pager's yellow.
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
        run("Run "),
        Some((app.theme.color(ColorToken::Text), FontWeight::Bold)),
        "the Run verb is bold primary text"
    );
    assert_eq!(
        run("cargo test -p goble-ui"),
        Some((app.theme.color(ColorToken::Command), FontWeight::Regular)),
        "the command takes the pager's command colour"
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
    let borders = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
        .count();
    assert_eq!(borders, 0, "an agent's call draws no block frame");

    // What it does draw: the row that names the command, then the output.
    let runs = text_runs(&commands);
    assert!(
        runs.iter().any(|(text, _, _, _)| text == command),
        "the row names the command it ran, got {runs:?}"
    );
    assert!(
        runs.iter().any(|(text, _, _, _)| text == output),
        "the output is drawn continuously under it, got {runs:?}"
    );
    assert!(
        runs.iter().all(|(_, _, _, size)| *size == 13.0),
        "the row and its body are one size, got {runs:?}"
    );

    // The block's own title is nowhere: the command is drawn once, by the row
    // that names it, not a second time as a block header.
    let titles = runs
        .iter()
        .filter(|(text, _, _, _)| text == command)
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
    let block_runs = text_runs(&render_element(&mut block, vec2f(600.0, 200.0), &app));
    assert!(
        block_runs.iter().any(|(text, _, _, _)| text == "❯ "),
        "the pane's block draws its own prompt line, got {block_runs:?}"
    );

    // An agent's call to the same command has no block, so it draws no prompt
    // line and no block title: the difference is the whole point.
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "run_command",
            r#"{"command":"cargo test -p goble-ui"}"#,
            ToolCallStatus::Finished,
            Some("test result: ok. 231 passed"),
        )],
        Some(ToolDisplayMode::Expanded),
    );
    let transcript_runs = text_runs(&commands);
    assert!(
        !transcript_runs.iter().any(|(text, _, _, _)| text == "❯ "),
        "an agent's call draws no block prompt, got {transcript_runs:?}"
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
        texts.iter().any(|t| t == "src/lib.rs"),
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
        vec![
            "  (".to_string(),
            "Read 1 file".to_string(),
            ")".to_string()
        ],
        "the folded run draws only its aggregated label in outer parentheses"
    );

    let (commands, _) = paint_tool_calls_with_run_open(calls, 0);
    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t == "src/lib.rs"),
        "the opened run's member row carries its path, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "line 1"),
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
        run("Read "),
        Some((app.theme.color(ColorToken::Text), FontWeight::Bold)),
        "a member of an open chain reads white and bold"
    );
    assert_eq!(
        run("src/lib.rs"),
        Some((app.theme.color(ColorToken::Text), FontWeight::Regular)),
        "the member's path reads white inside an open chain"
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
        header.ends_with("+1/-1)"),
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
    let header = ["◆ ", "Read ", "src/lib.rs"];
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

    let band = app.theme.color(ColorToken::Bg);
    let bands = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == band))
        .count();
    assert_eq!(bands, 10, "each read line draws its own band, got {bands}");
}

/// `count` numbered source lines, the shape the harness's `read_file` returns.
fn source_lines(count: usize) -> String {
    (1..=count).map(|i| format!("fn f{i}() {{}}\n")).collect()
}

/// A large read is bounded by what is on screen: expanded draws the cap's head
/// lines and one residue band naming the rest, so a frame's work no longer
/// follows the file's length.
#[test]
fn an_expanded_read_is_capped_and_names_the_lines_it_leaves_out() {
    let result = source_lines(2000);
    let app = AppContext::default();
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "read_file",
            r#"{"path":"src/lib.rs"}"#,
            ToolCallStatus::Finished,
            Some(&result),
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let band = app.theme.color(ColorToken::Bg);
    let bands = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == band))
        .count();
    assert_eq!(
        bands,
        EXPANDED_LINE_CAP + 1,
        "the cap's lines and one residue band, got {bands}"
    );

    let rows = drawn_row_texts(&commands);
    assert!(
        rows.iter().any(|row| row.ends_with("fn f200() {}")),
        "the cap's last line is drawn, got {rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row.ends_with("fn f201() {}")),
        "the line past the cap is not drawn, got {rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row.ends_with("fn f2000() {}")),
        "the file's last line is not drawn, got {rows:?}"
    );
    let residue = format!("… {} more lines", 2000 - EXPANDED_LINE_CAP);
    assert!(
        rows.iter().any(|row| row.contains(residue.as_str())),
        "one row names the remainder, got {rows:?}"
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
    assert!(
        texts.iter().any(|t| t == " 200  "),
        "the cap's last line carries its own number, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == " 201  "),
        "no number is drawn past the cap, got {texts:?}"
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
        let header = run_origin(&commands, "Read ");
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
        texts.iter().any(|t| t == "src/lib.rs"),
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
        texts.iter().any(|t| t == "rust async"),
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

/// A tool row starts where prose starts: no left rail, in either fold, and its
/// first run is the row's own text. The row and its body read as part of the
/// reply rather than as a nested block.
#[test]
fn a_tool_row_aligns_with_the_text_around_it() {
    let app = AppContext::default();
    for mode in [ToolDisplayMode::Collapsed, ToolDisplayMode::Expanded] {
        let key = tool_fold_key(
            &call(
                "read_file",
                r#"{"path":"src/lib.rs"}"#,
                ToolCallStatus::Finished,
                Some("fn main() {}\n"),
            ),
            0,
        );
        let mut bubble = ChatMessageBubble::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Prose starts here.")],
        )
        .with_tool_calls(vec![call(
            "read_file",
            r#"{"path":"src/lib.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn main() {}\n"),
        )])
        .with_tool_fold(Rc::new(RefCell::new(HashMap::from([(key, mode)]))));
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

        let origin_x = |prefix: &str| {
            commands.iter().find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text.starts_with(prefix) => {
                    Some(origin.x)
                }
                _ => None,
            })
        };
        let prose = origin_x("Prose starts here.").expect("the prose is drawn");
        let row_start = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text.starts_with("  (") || text.starts_with("Read ") =>
                {
                    Some(origin.x)
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "{mode:?}: the row's own text is drawn: {:?}",
                    drawn_texts(&commands)
                )
            });
        assert!(
            (row_start - prose).abs() < 0.5,
            "{mode:?}: the tool row starts in the prose column, but its text is at {row_start} and prose at {prose}"
        );
        assert!(
            !drawn_texts(&commands).iter().any(|text| text == "┃ "),
            "{mode:?} row draws no rail: {:?}",
            drawn_texts(&commands)
        );
    }
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
        resting_texts.iter().any(|text| text == "  ("),
        "a resting chain keeps the arrow's space, got {resting_texts:?}"
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
        open_texts.iter().any(|text| text.contains("❯ (")),
        "an open chain draws its arrow, got {open_texts:?}"
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
            .any(|text| text.contains("❯ (")),
        "the chain under the pointer draws its arrow"
    );
    assert!(
        !drawn_texts(&hovered_commands)
            .iter()
            .any(|text| text == "src/lib.rs"),
        "a bright chain stays closed until it is clicked"
    );
}

/// The row is drawn at 13 px, and its retracted and open shapes draw the same
/// text: the controls that enter and stop a child live in the pane's strip, so
/// the row owns no action of its own.
#[test]
fn a_sub_agent_row_is_thirteen_px_with_no_action_text() {
    let call = spawn_call(ToolCallStatus::Running);
    let record = record(
        "conv-child-1",
        crate::elements::chat_content::SubAgentRowStatus::Running,
        "reading",
        std::time::Duration::from_secs(4),
        None,
    );
    let (folded, _) = paint_sub_agent(
        vec![call.clone()],
        HashMap::from([(call.id.clone(), record.clone())]),
        None,
    );
    let sizes: HashMap<String, f32> = text_runs(&folded)
        .into_iter()
        .map(|(text, _, _, size)| (text, size))
        .collect();
    assert_eq!(
        sizes.get("Agent "),
        Some(&13.0),
        "the row itself stays 13 px"
    );
    assert!(
        !sizes.contains_key("open child"),
        "the row draws no action, got {sizes:?}"
    );

    let (open, _) = paint_sub_agent(
        vec![call.clone()],
        HashMap::from([(call.id, record)]),
        Some(ToolDisplayMode::Expanded),
    );
    let sizes: HashMap<String, f32> = text_runs(&open)
        .into_iter()
        .map(|(text, _, _, size)| (text, size))
        .collect();
    assert_eq!(sizes.get("Agent "), Some(&13.0), "an open row stays 13 px");
    assert!(
        !sizes.contains_key("open child"),
        "an open row draws no action either, got {sizes:?}"
    );
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
        "  (Read 2 files, Searched 1 pattern, Listed 1 dir, Ran 1 command)",
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
        vec!["  (Reading 3 files · 1 failed)".to_string()],
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
        rows.iter().any(|row| row.contains("(Read 2 files)")),
        "the header counts the folded members around the opened one, got {rows:?}"
    );
    let member = rows
        .iter()
        .find(|row| row.contains("src/b.rs"))
        .unwrap_or_else(|| panic!("the opened member draws its row, got {rows:?}"));
    assert!(
        member.starts_with("Read "),
        "the opened member draws its own row inside the folded chain, got {member:?}"
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
                RenderCommand::DrawText { origin, .. } => Some((origin.y * 10.0).round() / 10.0),
                _ => None,
            })
            .collect();
        ys.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ys.dedup();
        ys
    };

    let md = app.theme.spacing_px(crate::theme::SpacingToken::Md);
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
    let row_height = one_height - 2.0 * md;

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
        texts.iter().any(|text| text == "Ask User "),
        "the row leads with the tool's name read as words: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text == "Which environment?"),
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
