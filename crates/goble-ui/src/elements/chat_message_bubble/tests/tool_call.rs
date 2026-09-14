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
use crate::theme::ColorToken;
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
        vec!["● ls".to_string()],
        "only the status mark and the tool name are drawn"
    );
    assert_eq!(drawn_texts(&commands).len(), 2, "one row of two runs");
}

/// A tool whose definition declares no shape (here `list_entities`) expands
/// in place: its arguments and its result are drawn as their own rows
/// beneath the header.
#[test]
fn expanded_generic_tool_call_draws_arguments_and_result_in_place() {
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
        texts.iter().any(|t| t == r#"{"kind":"agent"}"#),
        "the arguments row is drawn, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "2 entities"),
        "the result row is drawn, got {texts:?}"
    );
    assert_eq!(
        drawn_rows(&commands),
        3,
        "header + arguments + result are three inline rows, got {texts:?}"
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

/// A command is drawn as the terminal block: its command line, its output
/// and the block's own title, never the raw argument JSON.
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

/// The segment a command the agent ran is drawn as is the terminal block,
/// and it renders identically in the two places a block appears: the
/// transcript and terminal mode.
#[test]
fn a_command_segment_is_the_same_block_terminal_mode_draws() {
    let app = AppContext::default();
    let data = TerminalData::for_command(
        "cargo test -p goble-ui",
        "test result: ok. 231 passed",
        TerminalStatus::Success,
    );

    // Terminal mode: the block on its own, exactly as the pane draws it.
    let mut block = terminal_block(&data, TerminalFilter::default(), None, None);
    let block_runs = text_runs(&render_element(&mut block, vec2f(600.0, 200.0), &app));

    // The transcript: the same command as the agent's tool call, expanded
    // so the block (not the folded header) is what is compared.
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
        !block_runs.is_empty(),
        "the shared block draws its title, command and output"
    );
    // The transcript draws the tool-call header (the mark, the verb and the
    // command it runs for) and then the block itself; the block runs must match
    // exactly.
    assert_eq!(
        &transcript_runs[3..],
        block_runs.as_slice(),
        "the transcript's command segment must be the block terminal mode draws"
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
    let header = ["●", "read_file", "src/lib.rs"];
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

