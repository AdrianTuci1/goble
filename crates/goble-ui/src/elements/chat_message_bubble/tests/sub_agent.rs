use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use crate::elements::SizeConstraint;
use crate::elements::chat_content::{tool_fold_key, ChatAction, ChatFragment, ChatRole, SubAgentRowStatus, ToolDisplayMode};
use crate::elements::{AppContext, LayoutContext, PaintContext, TerminalData, TerminalLine};
use crate::geometry::vec2f;
use crate::render::{RenderCommand, Renderer};
use crate::theme::ColorToken;
use goble_core::harness::ToolCallStatus;
use super::*;
use super::super::{ChatMessageBubble, SUB_AGENT_TRANSCRIPT_NOTE};

/// A sub-agent shows its own rows: its id, the input, then its output.
#[test]
fn sub_agent_call_shows_its_own_rows() {
    let (commands, _) = paint_tool_calls_with_fold(
        vec![call(
            "run_agent",
            r#"{"agent_id":"researcher","input":"summarize the changelog"}"#,
            ToolCallStatus::Running,
            None,
        )],
        Some(ToolDisplayMode::Expanded),
    );

    let texts = drawn_texts(&commands);
    assert!(
        texts.iter().any(|t| t == "researcher"),
        "the sub-agent id is shown, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t == "summarize the changelog"),
        "the sub-agent's input is shown, got {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("agent_id")),
        "the raw argument JSON is not shown, got {texts:?}"
    );
}

/// Each of the four statuses draws its own line on the parent transcript:
/// the description, then the status phrase — with the activity label and the
/// elapsed time only while the child runs.
#[test]
fn each_sub_agent_status_draws_its_own_line() {
    let cases = [
        (
            SubAgentRowStatus::Running,
            "reading",
            Duration::from_millis(4500),
            None,
            "◐",
            "running · reading · 4.5s",
        ),
        (
            SubAgentRowStatus::Completed,
            "",
            Duration::from_secs(43),
            Some("migration audited"),
            "●",
            "completed in 43s",
        ),
        (
            SubAgentRowStatus::Failed,
            "",
            Duration::from_secs(12),
            Some("child turn failed"),
            "◆",
            "failed in 12s · child turn failed",
        ),
        (
            SubAgentRowStatus::Cancelled,
            "",
            Duration::from_secs(5),
            Some("stopped by the user"),
            "◇",
            "cancelled in 5.0s",
        ),
    ];
    for (status, activity, elapsed, outcome, glyph, line) in cases {
        let call = spawn_call(ToolCallStatus::Finished);
        let record = record("conv-child-1", status, activity, elapsed, outcome);
        let (commands, _) =
            paint_sub_agent(vec![call.clone()], HashMap::from([(call.id, record)]), None);
        let rows = drawn_row_texts(&commands);
        let row = rows
            .iter()
            .find(|row| row.contains("Subagent"))
            .unwrap_or_else(|| panic!("status {status:?} draws its row, got {rows:?}"));
        // One ruler row: the status mark, the verb, the child in quotes and
        // the record's own line, with no body and no card.
        assert!(
            row.starts_with(glyph),
            "status {status:?} draws its own bullet {glyph:?}, got {row:?}"
        );
        assert!(
            row.contains("“audit the migration”"),
            "status {status:?} draws the description, got {row:?}"
        );
        assert!(
            row.contains(line),
            "status {status:?} must draw {line:?}, got {row:?}"
        );
        assert_eq!(rows.len(), 1, "the folded call is one row, got {rows:?}");
    }
}

/// A running row's activity and elapsed time come from the live record, so a
/// progress event moves the row; a child that ended keeps neither.
#[test]
fn a_running_sub_agent_row_carries_its_activity_and_elapsed_time() {
    let call = spawn_call(ToolCallStatus::Finished);
    let status_lines = |elapsed: u64, activity: &str| {
        let record = record(
            "conv-child-1",
            SubAgentRowStatus::Running,
            activity,
            Duration::from_millis(elapsed),
            None,
        );
        let (commands, _) = paint_sub_agent(
            vec![call.clone()],
            HashMap::from([(call.id.clone(), record)]),
            None,
        );
        drawn_row_texts(&commands)
    };

    let first = status_lines(1500, "reading the schema");
    assert!(
        first
            .iter()
            .any(|row| row.contains("running · reading the schema · 1.5s")),
        "the running row carries the activity and the elapsed time, got {first:?}"
    );
    let ticked = status_lines(2500, "running the tests");
    assert!(
        ticked
            .iter()
            .any(|row| row.contains("running · running the tests · 2.5s")),
        "the next progress event moves both segments of the row, got {ticked:?}"
    );
    // A child that is live before its first progress event says so rather
    // than drawing an empty label.
    let initializing = status_lines(0, "");
    assert!(
        initializing
            .iter()
            .any(|row| row.contains("running · initializing · 0.0s")),
        "a child with no activity yet draws `initializing`, got {initializing:?}"
    );

    // The terminal row states its duration inside its own phrase and drops
    // the activity segment.
    let record = record(
        "conv-child-1",
        SubAgentRowStatus::Completed,
        "running the tests",
        Duration::from_secs(30),
        Some("done"),
    );
    let (commands, _) = paint_sub_agent(
        vec![call.clone()],
        HashMap::from([("call_spawn_subagent".to_string(), record)]),
        None,
    );
    let rows = drawn_row_texts(&commands);
    assert!(
        rows.iter().any(|row| row.contains("completed in 30s")),
        "a completed row states its duration once, got {rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row.contains("running the tests")),
        "a completed row carries no activity label, got {rows:?}"
    );
}

/// The sub-agent row folds exactly like every other tool shape: one line
/// folded, the record's detail open, `Collapsed -> Truncated -> Expanded` on
/// the header click — and the expanded body says what the record does not
/// carry.
#[test]
fn the_sub_agent_row_folds_like_the_other_tool_shapes() {
    let app = AppContext::default();
    let call = spawn_call(ToolCallStatus::Finished);
    let key = tool_fold_key(&call, 0);
    let record = record(
        "conv-child-1",
        SubAgentRowStatus::Completed,
        "",
        Duration::from_secs(43),
        Some("line 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\nline 8\nline 9"),
    );
    let fold = Rc::new(RefCell::new(HashMap::new()));
    let (mut bubble, _) = sub_agent_bubble(
        vec![call.clone()],
        HashMap::from([(call.id.clone(), record)]),
        fold.clone(),
    );

    let header_click = |bubble: &mut ChatMessageBubble| {
        let commands = paint_bubble(bubble, &app);
        let origin = run_origin(&commands, "Subagent ");
        click_at(bubble, &app, origin + vec2f(2.0, 4.0));
    };

    // Folded it is the one ruler row, like every other shape's default.
    let folded = drawn_row_texts(&paint_bubble(&mut bubble, &app));
    assert_eq!(
        folded.len(),
        1,
        "a folded sub-agent draws one line, got {folded:?}"
    );

    header_click(&mut bubble);
    assert_eq!(
        fold.borrow().get(&key),
        Some(&ToolDisplayMode::Truncated),
        "the first click truncates the call, like every other shape"
    );
    let truncated = drawn_row_texts(&paint_bubble(&mut bubble, &app));
    assert!(
        truncated
            .iter()
            .any(|row| row == "sub-agent conv-child-1 · type reviewer"),
        "the open body names the child, got {truncated:?}"
    );
    assert!(
        truncated
            .iter()
            .any(|row| row == "2 turns · 4 tool calls · 1234 tokens · background"),
        "the open body carries the record's counters, got {truncated:?}"
    );
    assert!(
        truncated
            .iter()
            .any(|row| row.starts_with(SUB_AGENT_TRANSCRIPT_NOTE)),
        "the open body says the child's own rows are not in the record, got {truncated:?}"
    );
    assert!(
        truncated.iter().any(|row| row == "…"),
        "a truncated outcome elides its middle lines, got {truncated:?}"
    );
    assert!(
        !truncated.iter().any(|row| row == "line 6"),
        "the elided middle line is not drawn truncated, got {truncated:?}"
    );

    header_click(&mut bubble);
    assert_eq!(
        fold.borrow().get(&key),
        Some(&ToolDisplayMode::Expanded),
        "the second click expands the call"
    );
    let expanded = drawn_row_texts(&paint_bubble(&mut bubble, &app));
    assert!(
        expanded
            .iter()
            .any(|row| row.contains("line 1") && row.contains("line 9")),
        "expanded draws the whole outcome the record carries, got {expanded:?}"
    );
    assert!(
        !expanded.iter().any(|row| row == "…"),
        "expanded draws no ellipsis, got {expanded:?}"
    );

    header_click(&mut bubble);
    assert_eq!(
        fold.borrow().get(&key),
        Some(&ToolDisplayMode::Collapsed),
        "the third click folds the call back, like every other shape"
    );
    assert_eq!(
        drawn_row_texts(&paint_bubble(&mut bubble, &app)).len(),
        1,
        "folded again it is one line"
    );
}

/// A click on the row's open target fires the action that opens the child's
/// own view, carrying the child's conversation id, and leaves the fold alone.
/// With no record held there is no child to open, so no target is drawn.
#[test]
fn clicking_a_sub_agent_row_fires_the_open_action() {
    let app = AppContext::default();
    let call = spawn_call(ToolCallStatus::Finished);
    let key = tool_fold_key(&call, 0);
    let record = record(
        "conv-child-9",
        SubAgentRowStatus::Running,
        "reading",
        Duration::from_secs(4),
        None,
    );
    let fold = Rc::new(RefCell::new(HashMap::new()));
    let (mut bubble, actions) = sub_agent_bubble(
        vec![call.clone()],
        HashMap::from([(call.id.clone(), record.clone())]),
        fold.clone(),
    );
    let commands = paint_bubble(&mut bubble, &app);
    let position = run_origin(&commands, "open child") + vec2f(2.0, 4.0);
    click_at(&mut bubble, &app, position);
    assert_eq!(
        *actions.borrow(),
        vec![ChatAction::OpenSubAgent("conv-child-9".to_string())],
        "the row's click fires the open action with the child's id"
    );
    assert_eq!(
        fold.borrow().get(&key),
        None,
        "the open target is not the fold toggle"
    );

    // No record, no child: the row draws no dead affordance.
    let (commands, _) = paint_sub_agent(vec![call], HashMap::new(), None);
    let texts = drawn_texts(&commands);
    assert!(
        !texts.iter().any(|text| text == "open child"),
        "a row with no record draws no open target, got {texts:?}"
    );
}

/// The status affordance is read from the persisted status: the glyph and
/// colour change with it, and no result text is inspected to infer it.
#[test]
fn status_affordance_reflects_the_persisted_status() {
    let app = AppContext::default();
    let cases = [
        (ToolCallStatus::Pending, "○", ColorToken::Muted),
        (ToolCallStatus::Running, "◐", ColorToken::Accent),
        (ToolCallStatus::Finished, "●", ColorToken::Success),
        (ToolCallStatus::Error, "◆", ColorToken::Error),
    ];
    for (status, glyph, token) in cases {
        let (commands, _) = paint_tool_calls(vec![call("ls", "{}", status, None)]);
        // The mark is drawn with the row's trailing space.
        let mark = format!("{glyph} ");
        let drawn = commands.iter().find_map(|c| match c {
            RenderCommand::DrawText { text, color, .. } if *text == mark => Some(*color),
            _ => None,
        });
        assert_eq!(
            drawn,
            Some(app.theme.color(token)),
            "status {status:?} draws {glyph:?} in {token:?}"
        );
    }
}

/// A `role="tool"` result row still renders as its terminal block; the tool
/// call no longer adds a card beside it.
#[test]
fn tool_result_block_still_renders_for_a_tool_row() {
    let app = AppContext::default();
    let mut bubble = ChatMessageBubble::new(
        ChatRole::Tool,
        vec![ChatFragment::terminal(TerminalData::new(
            "call_1",
            vec![TerminalLine::output("file.txt")],
        ))],
    );
    let size = bubble.layout(
        SizeConstraint::loose(vec2f(400.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(size.x > 0.0);
    assert!(size.y > 0.0);

    let mut paint_ctx = PaintContext::new(Renderer::new());
    bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default();
    let copy_icons = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::DrawIcon { name, .. } if name == "copy"))
        .count();
    assert!(copy_icons >= 1, "expected a terminal block copy control");
    let text = drawn_texts(&commands);
    assert!(
        text.iter().any(|t| t == "call_1"),
        "expected the block title to be drawn, got {text:?}"
    );
    assert!(
        text.iter().any(|t| t == "file.txt"),
        "expected the block output to be drawn, got {text:?}"
    );
}
