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
use crate::theme::{ColorToken, FontFamily};
use goble_core::harness::ToolCallStatus;
use goble_core::harness::{tool_kind_for, tool_row, ToolKind};
use super::*;
use super::super::{ChatMessageBubble, read_excerpt};

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

/// A call with nothing to show collapses to a single row — the status mark and
/// the tool's own name, on one baseline, with no argument or result row. `ls`
/// carries no path here, so the parse has no operand to name and falls back to
/// the tool, exactly as it does for a call no family claims.
///
/// The mark is the one diamond every state draws, coloured by the state: a
/// collapsed call's is the quiet colour, because a finished `ls` is a read-line
/// tool and not the success a finished command announces.
#[test]
fn collapsed_tool_call_renders_in_one_row() {
    let (commands, _) =
        paint_tool_calls(vec![call("ls", "{}", ToolCallStatus::Finished, None)]);

    assert_eq!(
        drawn_rows(&commands),
        1,
        "a collapsed tool call must occupy one row"
    );
    assert_eq!(
        drawn_row_texts(&commands),
        vec!["◆ ls".to_string()],
        "only the status mark and the tool name are drawn"
    );
    assert_eq!(drawn_texts(&commands).len(), 2, "one row of two runs");
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
        texts.iter().any(|t| t == "list_entities"),
        "the tool name is drawn, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "2 entities"),
        "the result row is drawn, got {texts:?}"
    );
    assert_eq!(
        drawn_row_texts(&commands),
        vec!["┃ ◆ list_entities agent".to_string(), "2 entities".to_string()],
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
        !rows.iter().any(|row| row.contains('{') || row.contains('[')),
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
        runs.iter().all(|(_, _, _, size)| *size == 12.0),
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

/// A read shows a path rather than the arguments blob.
#[test]
fn read_call_shows_the_path() {
    let (commands, _) = paint_tool_calls(vec![call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some("fn main() {}"),
    )]);

    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t == "src/lib.rs"),
        "the path is shown, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains(r#"{"path""#)),
        "the raw argument JSON is not shown, got {texts:?}"
    );
}

/// A read starts folded: one header line carrying the path, nothing else.
#[test]
fn folded_read_draws_one_line() {
    let result: String = (1..=12).map(|i| format!("line {i}\n")).collect();
    let (commands, _) = paint_tool_calls(vec![call(
        "read_file",
        r#"{"path":"src/lib.rs"}"#,
        ToolCallStatus::Finished,
        Some(&result),
    )]);

    assert_eq!(
        drawn_rows(&commands),
        1,
        "a folded read must occupy one header row"
    );
    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t == "src/lib.rs"),
        "the folded read's header carries its path, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t == "line 1"),
        "the file body is hidden while folded, got {texts:?}"
    );
}

/// A command starts folded: its header carries the command and its output
/// is not drawn at all.
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
        texts.iter().any(|t| t == "cargo test -p goble-ui"),
        "the folded command's header carries the command, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("test result")),
        "the command output is hidden while folded, got {texts:?}"
    );
}

/// An edit starts folded carrying its `+N/-M` diffstat, and its hunks are not
/// drawn until it is opened.
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
        .find(|row| row.contains("Edit"))
        .unwrap_or_else(|| panic!("the folded edit draws its row, got {rows:?}"));
    assert!(
        header.contains("src/lib.rs"),
        "the folded edit's header carries the path, got {header:?}"
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
    let header = ["┃ ", "◆ ", "Read ", "src/lib.rs"];
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

/// A click on a call's header advances its fold
/// `Collapsed -> Truncated -> Expanded -> Collapsed`; the app-owned map is
/// what holds it.
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

    // Lay the header out so it has click bounds; it starts folded.
    let commands = paint_bubble(&mut bubble, &app);
    let header = run_origin(&commands, "Read ");
    click_at(&mut bubble, &app, header + vec2f(2.0, 4.0));
    assert_eq!(
        fold.borrow().get(&key),
        Some(&ToolDisplayMode::Truncated),
        "the first click truncates the call"
    );

    let _ = paint_bubble(&mut bubble, &app);
    click_at(&mut bubble, &app, header + vec2f(2.0, 4.0));
    assert_eq!(
        fold.borrow().get(&key),
        Some(&ToolDisplayMode::Expanded),
        "the second click expands the call"
    );

    let _ = paint_bubble(&mut bubble, &app);
    click_at(&mut bubble, &app, header + vec2f(2.0, 4.0));
    assert_eq!(
        fold.borrow().get(&key),
        Some(&ToolDisplayMode::Collapsed),
        "the third click folds the call back"
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

/// A call that is open draws the rail down its left edge; a folded one draws
/// none. The rail is what says the rows under a header belong to that call, so
/// a folded row is one line with nothing hanging off it.
#[test]
fn the_rail_is_drawn_only_while_the_call_is_open() {
    let call = || {
        call(
            "read_file",
            r#"{"path":"src/lib.rs"}"#,
            ToolCallStatus::Finished,
            Some("fn main() {}\n"),
        )
    };
    let (folded, _) = paint_tool_calls(vec![call()]);
    assert!(
        !drawn_texts(&folded).iter().any(|t| t == "┃ "),
        "a folded call draws no rail"
    );

    let (open, _) = paint_tool_calls_with_fold(vec![call()], Some(ToolDisplayMode::Expanded));
    let rail = text_runs(&open)
        .into_iter()
        .find(|(text, _, _, _)| text == "┃ ")
        .expect("an open call draws its rail");
    assert_eq!(rail.2, FontFamily::Mono, "the rail is a mono cell");
    assert_eq!(rail.3, 12.0, "the rail is drawn at the row's own size");
}

/// The state is the mark's colour, and it is dimmed while the call is folded.
/// An open call's mark is at full strength, so the fold and the state read off
/// the same row without either of them being a second glyph.
#[test]
fn the_mark_is_dimmed_while_the_call_is_folded() {
    let app = AppContext::default();
    let call = || {
        call(
            "run_command",
            r#"{"command":"ls"}"#,
            ToolCallStatus::Finished,
            Some("file.txt"),
        )
    };
    let mark = |commands: &[RenderCommand]| {
        text_runs(commands)
            .into_iter()
            .find(|(text, _, _, _)| text == "◆ ")
            .expect("the mark is drawn")
            .1
    };

    let (folded, _) = paint_tool_calls(vec![call()]);
    let (open, _) = paint_tool_calls_with_fold(vec![call()], Some(ToolDisplayMode::Expanded));
    let success = app.theme.color(ColorToken::Success);
    assert_eq!(mark(&open), success, "an open call's mark is at full strength");
    assert_eq!(
        mark(&folded),
        success.mix(&app.theme.color(ColorToken::Bg), 0.5),
        "a folded call's mark is blended half into the background"
    );
}

/// The row and its body are drawn at 12 px, and the one action a row owns at
/// 10 px while the row is retracted. The sizes are the row's shape, so a body
/// line is never smaller than the row that names it.
#[test]
fn the_row_is_twelve_px_and_a_retracted_action_is_ten() {
    let call = spawn_call(ToolCallStatus::Running);
    let record = record(
        "conv-child-1",
        crate::elements::chat_content::SubAgentRowStatus::Running,
        "reading",
        std::time::Duration::from_secs(4),
        None,
    );
    let (folded, _) = paint_sub_agent(vec![call.clone()], HashMap::from([(call.id.clone(), record.clone())]), None);
    let sizes: HashMap<String, f32> = text_runs(&folded)
        .into_iter()
        .map(|(text, _, _, size)| (text, size))
        .collect();
    assert_eq!(
        sizes.get("open child"),
        Some(&10.0),
        "the action is a size down while the row is retracted, got {sizes:?}"
    );
    assert_eq!(sizes.get("◆ "), Some(&12.0), "the row itself stays 12 px");

    let (open, _) = paint_sub_agent(
        vec![call.clone()],
        HashMap::from([(call.id, record)]),
        Some(ToolDisplayMode::Expanded),
    );
    let sizes: HashMap<String, f32> = text_runs(&open)
        .into_iter()
        .map(|(text, _, _, size)| (text, size))
        .collect();
    assert_eq!(
        sizes.get("open child"),
        Some(&12.0),
        "an open row draws its action at the row's own size, got {sizes:?}"
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
    assert!(
        !spans[0].underline,
        "a plain output line is not underlined"
    );

    let emitted = TerminalLine {
        text: "underlined".to_string(),
        kind: crate::elements::TerminalLineKind::Output,
        runs: vec![TerminalRun {
            text: "underlined".to_string(),
            color: None,
            bold: false,
            italic: false,
            underline: true,
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
