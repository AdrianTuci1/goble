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
use super::super::ChatMessageBubble;

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
/// A running child leads with the spinner frame, the delegation's own mark,
/// and an ended one with the status diamond in its status colour. Both keep the
/// description and the detail that says where the child is.
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
        // One ruler row: the mark, the verb, the child and what it is doing,
        // with no body and no card.
        let expected_mark = if status == SubAgentRowStatus::Running {
            // The first spinner frame at phase 0.
            "◐"
        } else {
            "◆"
        };
        assert!(
            row.starts_with(&format!("{expected_mark} Agent ")),
            "status {status:?} names the spawned child behind its mark, got {row:?}"
        );
        assert!(
            row.contains("“audit the migration”"),
            "status {status:?} draws the description, got {row:?}"
        );
        assert!(
            row.contains(line),
            "status {status:?} must draw {line:?}, got {row:?}"
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row.starts_with(&format!("{expected_mark} Agent ")))
                .count(),
            1,
            "the call draws one marked row, got {rows:?}"
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

/// A sub-agent card is not a fold: grok-build's sub-agent block is always one
/// line, and a click on it selects the child — the pane opens its takeover
/// rather than an outcome body the chat does not own.
#[test]
fn a_sub_agent_card_opens_its_child_and_never_folds() {
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
    let (mut bubble, actions) = sub_agent_bubble(
        calls,
        HashMap::from([(call.id.clone(), record)]),
        fold.clone(),
    );

    // A transcript is as wide as its pane. The record's line and the card's
    // controls share the row, so the test gives it a pane wide enough for the
    // whole line.
    let paint = |bubble: &mut ChatMessageBubble| paint_bubble_at(bubble, &app, 800.0);
    let folded = drawn_row_texts(&paint(&mut bubble));
    assert_eq!(
        folded.iter().filter(|row| row.contains("Agent ")).count(),
        1,
        "the card draws its own line, got {folded:?}"
    );
    assert!(
        !folded.iter().any(|row| row.contains("tool calls")),
        "the card draws none of the record's counters, got {folded:?}"
    );

    // A click on the card's words enters the child.
    let commands = paint(&mut bubble);
    let origin = run_origin(&commands, "Agent ");
    click_at(&mut bubble, &app, origin + vec2f(2.0, 4.0));
    assert_eq!(
        *actions.borrow(),
        vec![ChatAction::OpenSubAgent("conv-child-1".to_string())],
        "a click on the card opens the child it names"
    );
    assert_eq!(
        fold.borrow().get(&key),
        None,
        "the card never advances a fold"
    );
    assert_eq!(
        drawn_row_texts(&paint(&mut bubble))
            .iter()
            .filter(|row| row.contains("Agent "))
            .count(),
        1,
        "the card stays one line after the click"
    );
}

/// The card carries the two controls the pane's dock does, in place: the one
/// that enters the child and, while it runs, the one that stops it. They fire
/// the same actions, so every surface hands the app the same request.
#[test]
fn a_sub_agent_card_carries_its_open_and_stop_controls() {
    let app = AppContext::default();
    let call = spawn_call(ToolCallStatus::Running);
    let running = record(
        "conv-child-1",
        SubAgentRowStatus::Running,
        "reading",
        Duration::from_secs(4),
        None,
    );
    let (mut bubble, actions) = sub_agent_bubble(
        vec![call.clone()],
        HashMap::from([(call.id.clone(), running)]),
        Rc::new(RefCell::new(HashMap::new())),
    );
    let commands = paint_bubble_at(&mut bubble, &app, 800.0);
    let open = icon_origin(&commands, "arrow-up").expect("the card's open control");
    let stop = icon_origin(&commands, "x-close").expect("a running child's stop control");
    click_at(&mut bubble, &app, open);
    click_at(&mut bubble, &app, stop);
    assert_eq!(
        *actions.borrow(),
        vec![
            ChatAction::OpenSubAgent("conv-child-1".to_string()),
            ChatAction::StopSubAgent("conv-child-1".to_string()),
        ],
        "the card's controls carry the child they belong to"
    );

    // An ended child has nothing left to stop, and a dead affordance is not
    // drawn.
    let ended = record(
        "conv-child-1",
        SubAgentRowStatus::Completed,
        "",
        Duration::from_secs(43),
        Some("done"),
    );
    let (commands, _) = paint_sub_agent(
        vec![call.clone()],
        HashMap::from([(call.id.clone(), ended)]),
        None,
    );
    assert!(
        icon_origin(&commands, "arrow-up").is_some(),
        "an ended child is still enterable"
    );
    assert!(
        icon_origin(&commands, "x-close").is_none(),
        "an ended child draws no stop"
    );

    // No record at all: the spawn call's own row is the fallback shape, and it
    // has no controls.
    let (commands, _) = paint_sub_agent(vec![call], HashMap::new(), None);
    assert!(
        icon_origin(&commands, "arrow-up").is_none()
            && icon_origin(&commands, "x-close").is_none(),
        "a row with no record draws no controls"
    );
}

/// The card reads a step under the reply: 12 px, like grok-build's status line,
/// rather than the tool row's 13.
#[test]
fn a_sub_agent_card_reads_a_step_under_the_tool_rows() {
    let call = spawn_call(ToolCallStatus::Running);
    let record = record(
        "conv-child-1",
        SubAgentRowStatus::Running,
        "reading",
        Duration::from_secs(4),
        None,
    );
    let (commands, _) = paint_sub_agent(
        vec![call.clone()],
        HashMap::from([(call.id.clone(), record)]),
        None,
    );
    let sizes: Vec<f32> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { font_size, .. } => Some(*font_size),
            _ => None,
        })
        .collect();
    assert!(
        sizes.iter().all(|size| *size <= 12.0),
        "the card's text stays under the tool row's size, got {sizes:?}"
    );
}

/// Where a drawn icon's centre is, which is what a click lands on.
fn icon_origin(commands: &[RenderCommand], name: &str) -> Option<crate::geometry::Vector2F> {
    commands.iter().find_map(|command| match command {
        RenderCommand::DrawIcon {
            name: drawn,
            origin,
            size,
            ..
        } if drawn == name => Some(*origin + vec2f(*size / 2.0, *size / 2.0)),
        _ => None,
    })
}

/// Every call draws one row whatever its status. The diamond is the row's
/// fixed mark; its words and its colour state where it is, and a resting row
/// reads in the quiet colour. A finished command folds into its chain header,
/// which wears the dotted diamond.
#[test]
fn a_resting_row_carries_its_diamond() {
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
            vec!["◆ ls".to_string()],
            "status {status:?} draws its diamond and the tool's own words"
        );
        let runs = text_runs(&commands);
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
            .any(|(text, _, _, _)| text == "◈ "),
        "a command's folded header wears the dotted diamond"
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
