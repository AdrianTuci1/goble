use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use crate::elements::SizeConstraint;
use crate::elements::chat_content::{tool_fold_key, ChatFragment, ChatRole, SubAgentRowStatus, ToolDisplayMode};
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

/// Each of the four statuses draws its own line on the parent transcript.
///
/// A running child is the delegation itself — the description and what it is
/// doing — and an ended one keeps its terminal phrase, with the elapsed time
/// inside it. The row carries no controls: the pane's strip owns those.
///
/// The row carries no status mark either: its terminal word and detail state
/// where the child is, and the row reads as part of the chain rather than as a
/// separate kind of line.
#[test]
fn each_sub_agent_status_draws_its_own_line() {
    let cases = [
        (
            SubAgentRowStatus::Running,
            "reading",
            Duration::from_millis(4500),
            None,
            "running: “audit the migration” · Responding",
        ),
        (
            SubAgentRowStatus::Completed,
            "",
            Duration::from_secs(43),
            Some("migration audited"),
            "completed in 43s",
        ),
        (
            SubAgentRowStatus::Failed,
            "",
            Duration::from_secs(12),
            Some("child turn failed"),
            "failed in 12s · child turn failed",
        ),
        (
            SubAgentRowStatus::Cancelled,
            "",
            Duration::from_secs(5),
            Some("stopped by the user"),
            "cancelled in 5.0s",
        ),
    ];
    for (status, activity, elapsed, outcome, line) in cases {
        let call = spawn_call(ToolCallStatus::Finished);
        let record = record("conv-child-1", status, activity, elapsed, outcome);
        // A spawn call draws its own row, collapsed by default: no run has to be
        // opened to reach the child this transcript exists to name.
        let (commands, _) = paint_sub_agent(
            vec![call.clone()],
            HashMap::from([(call.id, record)]),
            None,
        );
        let rows = drawn_row_texts(&commands);
        let row = rows
            .iter()
            .find(|row| row.contains("Agent "))
            .unwrap_or_else(|| panic!("status {status:?} draws its row, got {rows:?}"));
        // One ruler row: the verb, the child and what it is doing, with no
        // body and no card.
        assert!(
            row.starts_with("Agent "),
            "status {status:?} names the spawned child, got {row:?}"
        );
        assert!(
            row.contains("“audit the migration”"),
            "status {status:?} draws the description, got {row:?}"
        );
        assert!(
            row.contains(line),
            "status {status:?} must draw {line:?}, got {row:?}"
        );
        assert!(
            !row.contains("Stop") && !row.contains("open child"),
            "status {status:?} draws no controls of its own, got {row:?}"
        );
        assert_eq!(
            rows.iter().filter(|row| row.starts_with("Agent ")).count(),
            1,
            "the collapsed call draws one row, got {rows:?}"
        );
    }
}

/// A running child's row states what the child is doing and the model it is
/// doing it with, from the pane's own label. It carries neither the record's
/// activity nor its ticking clock: those belong to the pane's strip.
#[test]
fn a_running_sub_agent_row_states_responding_and_the_panes_model() {
    let call = spawn_call(ToolCallStatus::Finished);
    let live = record(
        "conv-child-1",
        SubAgentRowStatus::Running,
        "reading the schema",
        Duration::from_millis(1500),
        None,
    );
    let app = AppContext::default();
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(vec![call.clone()])
        .with_sub_agents(HashMap::from([(call.id.clone(), live.clone())]))
        .with_sub_agent_model("deepseek");
    let rows = drawn_row_texts(&paint_bubble_at(&mut bubble, &app, 800.0));
    let row = rows
        .iter()
        .find(|row| row.contains("Agent "))
        .unwrap_or_else(|| panic!("the running row is drawn, got {rows:?}"));
    assert!(
        row.contains("Agent running: “audit the migration” · Responding (deepseek)"),
        "the row names the delegation and the model, got {row:?}"
    );
    assert!(
        !row.contains("reading the schema") && !row.contains("1.5s"),
        "the activity and the elapsed time are the strip's, got {row:?}"
    );

    // A pane with no model label states the response without a parenthesis.
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(vec![call.clone()])
        .with_sub_agents(HashMap::from([(call.id.clone(), live)]));
    let rows = drawn_row_texts(&paint_bubble_at(&mut bubble, &app, 800.0));
    assert!(
        rows.iter()
            .any(|row| row.contains("running: “audit the migration” · Responding")),
        "an unknown model draws `Responding` alone, got {rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row.contains("Responding (")),
        "no empty parentheses are drawn, got {rows:?}"
    );

    // The terminal row states its duration inside its own phrase.
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
    let calls = vec![call.clone()];
    let (mut bubble, _) = sub_agent_bubble(
        calls,
        HashMap::from([(call.id.clone(), record)]),
        fold.clone(),
    );

    // A transcript is as wide as its pane. The record's line and the row's
    // actions share the row, so the test gives it a pane wide enough for the
    // whole line and checks the folding, not the wrapping.
    let paint = |bubble: &mut ChatMessageBubble| paint_bubble_at(bubble, &app, 800.0);
    let header_click = |bubble: &mut ChatMessageBubble| {
        let commands = paint(bubble);
        let origin = run_origin(&commands, "Agent ");
        click_at(bubble, &app, origin + vec2f(2.0, 4.0));
    };

    // A spawn call is its own row, so it is on screen without opening anything:
    // the child in quotes and the record's line are the whole folded shape.
    let folded = drawn_row_texts(&paint(&mut bubble));
    assert_eq!(
        folded.iter().filter(|row| row.contains("Agent ")).count(),
        1,
        "a collapsed spawn call draws its own header line, got {folded:?}"
    );
    assert!(
        !folded.iter().any(|row| row.contains("tool calls")),
        "a folded spawn draws none of the record's counters, got {folded:?}"
    );

    header_click(&mut bubble);
    assert_eq!(
        fold.borrow().get(&key),
        Some(&ToolDisplayMode::Truncated),
        "the first click truncates the call, like every other shape"
    );
    let truncated = drawn_row_texts(&paint(&mut bubble));
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
    let expanded = drawn_row_texts(&paint(&mut bubble));
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
        drawn_row_texts(&paint(&mut bubble))
            .iter()
            .filter(|row| row.contains("Agent "))
            .count(),
        1,
        "folded again the header is one line"
    );
}

/// The row carries no controls at all, whatever the child's status: entering
/// and stopping a child belong to the pane's sub-agent strip, and the chat says
/// only what the child is doing.
#[test]
fn a_sub_agent_row_draws_no_controls_of_its_own() {
    let call = spawn_call(ToolCallStatus::Running);
    let texts_for = |status: SubAgentRowStatus| {
        let record = record(
            "conv-child-1",
            status,
            "reading",
            Duration::from_secs(4),
            None,
        );
        let (commands, _) = paint_sub_agent(
            vec![call.clone()],
            HashMap::from([(call.id.clone(), record)]),
            None,
        );
        drawn_texts(&commands)
    };

    for status in [
        SubAgentRowStatus::Running,
        SubAgentRowStatus::Completed,
        SubAgentRowStatus::Failed,
        SubAgentRowStatus::Cancelled,
    ] {
        let texts = texts_for(status);
        assert!(
            !texts
                .iter()
                .any(|text| text == "Stop" || text == "open child"),
            "a {status:?} row draws no controls, got {texts:?}"
        );
    }

    // No record at all: the spawn call's own row is the fallback shape, and it
    // has no controls either.
    let (commands, _) = paint_sub_agent(vec![call], HashMap::new(), None);
    let texts = drawn_texts(&commands);
    assert!(
        !texts
            .iter()
            .any(|text| text == "Stop" || text == "open child"),
        "a row with no record draws no controls, got {texts:?}"
    );
}

/// Every call draws one quiet row whatever its status. The row carries no
/// status glyph: its words and its colour state where it is, and a resting row
/// reads in the quiet colour. A finished command folds into its chain header
/// the same way.
#[test]
fn a_resting_row_carries_no_status_mark() {
    let app = AppContext::default();
    for status in [
        ToolCallStatus::Pending,
        ToolCallStatus::Running,
        ToolCallStatus::Finished,
        ToolCallStatus::Error,
    ] {
        let (commands, _) = paint_tool_calls(vec![call("ls", "{}", status, None)]);
        assert_eq!(
            drawn_row_texts(&commands),
            vec!["ls".to_string()],
            "status {status:?} draws the tool's own words"
        );
        let runs = text_runs(&commands);
        assert!(
            runs.iter()
                .all(|(text, _, _, _)| text != "◆ " && text != "◈ "),
            "status {status:?} draws no mark, got {runs:?}"
        );
        assert_eq!(
            runs[0].1,
            app.theme.color(ColorToken::Muted),
            "status {status:?} rests in the quiet colour"
        );
    }

    let (commands, _) = paint_tool_calls(vec![call(
        "run_command",
        r#"{"command":"ls"}"#,
        ToolCallStatus::Finished,
        Some("file.txt"),
    )]);
    assert!(
        drawn_texts(&commands)
            .iter()
            .any(|text| text == "Ran 1 command"),
        "a finished command folds into its chain header"
    );
    assert!(
        text_runs(&commands)
            .iter()
            .all(|(text, _, _, _)| text != "◆ " && text != "◈ "),
        "a command's header carries no mark either"
    );
}

/// A `role="tool"` message carrying a terminal fragment is the command the
/// *user* ran inside the pane, and it is still the block the pane draws — with
/// the block's own controls while the pointer is over it. This is the one path
/// that keeps a segmented block; an agent's tool call takes none of it.
#[test]
fn tool_result_block_still_renders_for_a_user_command() {
    use crate::elements::TerminalFilter;

    let app = AppContext::default();
    let data = TerminalData::new("call_1", vec![TerminalLine::output("file.txt")]);
    let filters: Rc<RefCell<HashMap<String, TerminalFilter>>> =
        Rc::new(RefCell::new(HashMap::new()));
    let hovered = filters
        .borrow_mut()
        .entry(data.filter_key())
        .or_default()
        .clone();
    hovered.set_hovered(true);

    let paint = |bubble: &mut ChatMessageBubble| {
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
            .map(|r| r.commands().to_vec())
            .unwrap_or_default()
    };
    let copy_icons = |commands: &[RenderCommand]| {
        commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::DrawIcon { name, .. } if name == "copy"))
            .count()
    };

    let mut resting =
        ChatMessageBubble::new(ChatRole::Tool, vec![ChatFragment::terminal(data.clone())]);
    let commands = paint(&mut resting);
    assert_eq!(
        copy_icons(&commands),
        0,
        "a block whose pointer is elsewhere draws no controls"
    );
    assert!(
        drawn_texts(&commands).iter().any(|t| t == "file.txt"),
        "but its output is drawn all the same"
    );

    let mut bubble = ChatMessageBubble::new(ChatRole::Tool, vec![ChatFragment::terminal(data)])
        .with_terminal_filters(filters);
    let commands = paint(&mut bubble);
    assert!(
        copy_icons(&commands) >= 1,
        "expected a terminal block copy control under the pointer"
    );
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
