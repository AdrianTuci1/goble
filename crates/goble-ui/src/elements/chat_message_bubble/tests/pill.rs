//! The pill lock: no agent transcript row is wrapped in a pill.
//!
//! grok-build has no agent pill — an agent's turn is a bullet and a line of
//! runs, and a tool call is the same row plus the body its family draws. These
//! tests fix that on our side: every row the agent's conversation paints, in
//! every fold and every status, is a full-width square band (or nothing at all)
//! with no rounded fill and no border around it.
//!
//! The one exception is the shared terminal command block, the section the
//! terminal pane draws for the same command; [`assert_pill_free`] recognizes it
//! as the block's own card and rejects every other rounded or bordered box.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use crate::elements::chat_content::{
    ChatFragment, ChatRole, ListItem, SubAgentRowStatus, ToolDisplayMode,
};
use crate::elements::{AppContext, TerminalData, TerminalLine};
use crate::geometry::vec2f;
use crate::render::RenderCommand;
use crate::test_util::{
    assert_no_row_border, assert_pill_free, bordered_control, pills, render_element,
};
use goble_core::harness::ToolCallStatus;
use super::*;
use super::super::ChatMessageBubble;

/// Every family's shape, one call each, under the names the parser answers to:
/// grok-build's aliases and our own harness tools. The arguments and results
/// are what each shape's body reads, so the sweep covers the bodies too, not
/// only the header row.
fn family_calls() -> Vec<ToolCall> {
    let cases: &[(&str, &str, &str)] = &[
        // grok-build's own tool names.
        ("read", r#"{"path":"src/lib.rs","start_line":1,"end_line":20}"#, "1: line one\n2: line two"),
        ("edit", r#"{"path":"src/lib.rs","old_text":"let a = 1;","new_text":"let a = 2;"}"#, "edited"),
        ("write", r#"{"path":"src/new.rs","content":"fn main() {}"}"#, "created"),
        ("bash", r#"{"command":"cargo test"}"#, "test result: ok"),
        ("grep", r#"{"pattern":"fn main","path":"src"}"#, "2 matches"),
        ("ls", r#"{"path":"src"}"#, "a.rs\nb.rs"),
        ("web_fetch", r#"{"url":"https://x.ai/docs"}"#, "fetched"),
        ("search_tool", r#"{"query":"memory"}"#, "3 results"),
        ("use_tool", r#"{"server":"github","tool":"list_issues"}"#, "[]"),
        ("memory_search", r#"{"query":"preferences"}"#, "2 results"),
        ("skill", r#"{"name":"pdf"}"#, "ok"),
        // Our harness tools.
        ("create_agent", r#"{"name":"researcher","instructions":"read a lot"}"#, "created"),
        ("update_agent", r#"{"id":"researcher","instructions":"read more"}"#, "updated"),
        ("create_workflow", r#"{"name":"nightly","steps":[]}"#, "created"),
        ("update_workflow", r#"{"id":"nightly","steps":[]}"#, "updated"),
        ("create_team", r#"{"name":"reviewers"}"#, "created"),
        ("update_team", r#"{"id":"reviewers"}"#, "updated"),
        ("run_command", r#"{"command":"cargo test","args":["--lib"]}"#, "test result: ok"),
        ("credentials", r#"{"agent_id":"researcher"}"#, "none"),
        ("principals", r#"{"agent_id":"researcher"}"#, "none"),
        ("user_guide", r#"{"topic":"mcp"}"#, "the guide"),
        ("list_entities", r#"{"entity_type":"agent"}"#, "researcher\nreviewer"),
        ("search_store", r#"{"query":"nightly","entity_types":["workflow"]}"#, "search results: [Workflow]"),
        ("deploy_agent", r#"{"agent_id":"researcher"}"#, "deployed"),
        ("deploy_workflow", r#"{"workflow_id":"nightly"}"#, "deployed"),
        ("schedule_workflow", r#"{"workflow_id":"nightly","cron":"0 2 * * *"}"#, "scheduled"),
        ("get_execution_status", r#"{"execution_id":"exec-1"}"#, "running"),
        ("read_file", r#"{"path":"src/main.rs"}"#, "1: fn main() {}"),
        ("write_file", r#"{"path":"src/new.rs","content":"fn main() {}"}"#, "written"),
        ("delete_agent", r#"{"agent_id":"researcher"}"#, "deleted"),
        ("delete_workflow", r#"{"workflow_id":"nightly"}"#, "deleted"),
        ("delete_team", r#"{"team_id":"reviewers"}"#, "deleted"),
        ("rename_file", r#"{"from":"a.rs","to":"b.rs"}"#, "renamed"),
        ("delete_file", r#"{"path":"a.rs"}"#, "deleted"),
        ("git_status", "{}", "nothing to commit"),
        ("git_diff", r#"{"path":"src/lib.rs"}"#, "--- a\n+++ b"),
        ("git_commit", r#"{"message":"fix","files":["src/lib.rs"]}"#, "committed"),
        ("codebase_search", r#"{"pattern":"fn main","path":"src"}"#, "2 matches\nsrc/main.rs:1: fn main"),
        ("install_mcp_server", r#"{"id":"github","command":"npx"}"#, "installed"),
        ("delete_mcp_server", r#"{"id":"github"}"#, "deleted"),
        ("update_mcp_server", r#"{"id":"github","command":"npx"}"#, "updated"),
        ("search_mcp_servers", r#"{"query":"github"}"#, "1 result"),
        ("discover_mcp_tools", r#"{"id":"github"}"#, "list_issues"),
        ("list_mcp_servers", "{}", "github"),
        ("web_search", r#"{"query":"grok build","max_results":3,"advanced":false}"#, "URL: https://x.ai"),
        ("read_url", r#"{"url":"https://x.ai/docs"}"#, "the page"),
        ("execute_python_code", r#"{"code":"print(1)"}"#, "1"),
        ("run_agent", r#"{"agent_id":"researcher","input":"summarize the changelog"}"#, "the summary"),
        ("edit_file", r#"{"path":"src/lib.rs","old_text":"let a = 1;","new_text":"let a = 2;"}"#, "edited"),
        ("memory_write", r#"{"agent_id":"researcher","content":"prefers tabs"}"#, "stored"),
        ("memory_read", r#"{"agent_id":"researcher"}"#, "prefers tabs"),
        ("open_screen", r#"{"host":"mac-mini"}"#, "opened"),
    ];
    let mut calls: Vec<ToolCall> = cases
        .iter()
        .map(|(name, arguments, result)| call(name, arguments, ToolCallStatus::Finished, Some(result)))
        .collect();
    calls.push(spawn_call(ToolCallStatus::Finished));
    calls
}

/// No agent row is a pill. Prose, markdown, a user's own message, a reasoning
/// step, a tool call of every family in every fold, a tool call still in flight
/// and a sub-agent's row: each is a square, borderless row.
#[test]
fn no_agent_row_paints_a_pill() {
    let app = AppContext::default();

    // Prose and markdown rows, in both roles.
    let mut prose = ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![
            ChatFragment::heading(2, "Findings"),
            ChatFragment::list(
                vec![
                    ListItem::new(vec![ChatFragment::text("one")]),
                    ListItem::new(vec![ChatFragment::text("two")]),
                ],
                None,
            ),
            ChatFragment::code("cargo test"),
            ChatFragment::link("the docs", "https://x.ai/docs"),
        ],
    );
    let commands = paint_bubble(&mut prose, &app);
    assert_pill_free(&commands, "the agent's markdown row");
    assert_no_row_border(&commands, "the agent's markdown row");

    let mut user = ChatMessageBubble::new(ChatRole::User, vec![ChatFragment::text("Salut!")]);
    let commands = paint_bubble(&mut user, &app);
    assert_pill_free(&commands, "the user's message");
    assert_no_row_border(&commands, "the user's message");

    // A reasoning step, collapsed and expanded.
    for expanded in [false, true] {
        let mut reasoning = ChatMessageBubble::new(
            ChatRole::Assistant,
            vec![ChatFragment::reasoning("step-0", "Thinking", "weighing options", true)],
        )
        .with_reasoning_expanded(Rc::new(RefCell::new(HashMap::from([(
            "step-0".to_string(),
            expanded,
        )]))));
        let what = if expanded {
            "an expanded reasoning row"
        } else {
            "a collapsed reasoning row"
        };
        let commands = paint_bubble(&mut reasoning, &app);
        assert_pill_free(&commands, what);
        assert_no_row_border(&commands, what);
    }

    // Every family, every fold.
    for mode in [
        ToolDisplayMode::Collapsed,
        ToolDisplayMode::Truncated,
        ToolDisplayMode::Expanded,
    ] {
        let calls = family_calls();
        let (commands, _) = paint_tool_calls_with_fold(calls.clone(), Some(mode));
        assert_pill_free(&commands, &format!("{mode:?} tool-call rows"));
        assert_no_row_border(&commands, &format!("{mode:?} tool-call rows"));

        // The same calls without a result: the row a turn in flight draws.
        for status in [
            ToolCallStatus::Pending,
            ToolCallStatus::Running,
            ToolCallStatus::Error,
        ] {
            let in_flight: Vec<ToolCall> = calls
                .iter()
                .map(|call| ToolCall { status, ..call.clone() })
                .collect();
            let (commands, _) = paint_tool_calls_with_fold(in_flight, Some(mode));
            let what = format!("{mode:?} {status:?} tool-call rows");
            assert_pill_free(&commands, &what);
            assert_no_row_border(&commands, &what);
        }
    }
}

/// A sub-agent's row is the parent transcript's own: the status mark, the verb,
/// the quoted description and the child's live line — with no pill, in every
/// status and every fold. Its record-backed body paints no pill either.
#[test]
fn no_sub_agent_row_paints_a_pill() {
    let cases = [
        (SubAgentRowStatus::Running, "reading the schema", Duration::from_millis(4500), None),
        (SubAgentRowStatus::Completed, "", Duration::from_millis(43000), Some("the report")),
        (SubAgentRowStatus::Failed, "", Duration::from_millis(12000), Some("child turn failed")),
        (SubAgentRowStatus::Cancelled, "", Duration::from_millis(5000), Some("cancelled by the user")),
    ];
    for (status, activity, elapsed, outcome) in cases {
        for mode in [
            ToolDisplayMode::Collapsed,
            ToolDisplayMode::Truncated,
            ToolDisplayMode::Expanded,
        ] {
            let call = spawn_call(match status {
                SubAgentRowStatus::Running => ToolCallStatus::Running,
                SubAgentRowStatus::Failed => ToolCallStatus::Error,
                _ => ToolCallStatus::Finished,
            });
            let record = record("conv-child-1", status, activity, elapsed, outcome);
            let (commands, _) = paint_sub_agent(
                vec![call.clone()],
                HashMap::from([(call.id.clone(), record)]),
                Some(mode),
            );
            let what = format!("{status:?} {mode:?} sub-agent row");
            assert_pill_free(&commands, &what);
            assert_no_row_border(&commands, &what);

            // A spawn call with no live record — a transcript re-read after a
            // restart — draws the arguments it was given.
            let (commands, _) = paint_sub_agent(vec![call], HashMap::new(), Some(mode));
            assert_pill_free(&commands, &format!("{status:?} {mode:?} sub-agent arguments"));
        }
    }
}

/// A `role="tool"` row is the shared terminal block and nothing else; its card
/// is the one box the transcript may draw, and it is not a pill around an agent
/// row.
#[test]
fn a_tool_result_row_draws_only_the_shared_block() {
    let app = AppContext::default();
    let mut bubble = ChatMessageBubble::new(
        ChatRole::Tool,
        vec![ChatFragment::terminal(TerminalData::new(
            "call_1",
            vec![TerminalLine::output("hi")],
        ))],
    );
    let commands = paint_bubble(&mut bubble, &app);
    assert!(
        bordered_control(&commands, &app).is_some(),
        "a command row draws the shared terminal block's card"
    );
    assert_pill_free(&commands, "a tool result row");
}

/// The panels a run draws inside the transcript are bands, not pills: the
/// interruption cards and the still-running footer.
#[test]
fn no_interruption_panel_paints_a_pill() {
    use crate::elements::{TurnStatus, TurnStatusFooter, WorkKind, WorkKindCount};

    let app = AppContext::default();
    let mut ask = crate::elements::AskUserCard::new(
        "Which environment should I deploy to?",
        vec!["staging".to_string(), "production".to_string()],
    )
    .finish();
    assert_pill_free(
        &render_element(&mut ask, vec2f(600.0, 300.0), &app),
        "the ask-user card",
    );

    let footer = TurnStatusFooter::new(TurnStatus::still_running(vec![WorkKindCount {
        kind: WorkKind::Command,
        count: 2,
    }]))
    .finish();
    let mut footer = footer;
    let commands = render_element(&mut footer, vec2f(600.0, 40.0), &app);
    assert_pill_free(&commands, "the still-running footer");
    assert_no_row_border(&commands, "the still-running footer");
}

/// The lock catches the regression shape: the same row wrapped in the bordered,
/// raised card the old pager-style widget drew.
#[test]
fn the_lock_catches_a_row_wrapped_in_a_card() {
    use crate::color::ColorU;
    use crate::elements::{Container, Fill};

    let app = AppContext::default();
    let row = ChatMessageBubble::new(
        ChatRole::Assistant,
        Vec::new(),
    )
    .with_tool_calls(vec![call("read_file", r#"{"path":"src/lib.rs"}"#, ToolCallStatus::Finished, Some("1: fn main() {}"))])
    .finish();
    let mut wrapped: Box<dyn Element> = Container::new(row)
        .with_background(Fill::Solid(ColorU::new(30, 30, 30, 255)))
        .with_corner_radius(app.theme.radius_px())
        .finish();

    let wrapped = render_element(&mut wrapped, vec2f(400.0, 400.0), &app);
    assert!(
        !pills(&wrapped).is_empty(),
        "a row wrapped in a rounded card must be reported"
    );

    // The same row unwrapped is pill-free, so the lock is measuring the wrap.
    let mut row = ChatMessageBubble::new(
        ChatRole::Assistant,
        Vec::new(),
    )
    .with_tool_calls(vec![call("read_file", r#"{"path":"src/lib.rs"}"#, ToolCallStatus::Finished, Some("1: fn main() {}"))])
    .finish();
    assert_pill_free(
        &render_element(&mut row, vec2f(400.0, 400.0), &app),
        "the same row",
    );
}

/// The lock is real: a rounded fill or a border on a row is reported, and the
/// shared command block is the only box that is not.
#[test]
fn the_lock_reports_a_box_that_appears() {
    use crate::color::ColorU;
    use crate::geometry::rectf;

    let app = AppContext::default();
    let radius = app.theme.radius_px();
    let card_rect = rectf(0.0, 0.0, 100.0, 20.0);
    let beside_rect = rectf(0.0, 40.0, 100.0, 20.0);
    let fill = |rect, corner_radius| RenderCommand::FillRect {
        rect,
        color: ColorU::new(20, 20, 20, 255),
        corner_radius,
    };
    let border = |rect| RenderCommand::StrokeRect {
        rect,
        color: ColorU::new(20, 20, 20, 255),
        width: 1.0,
        corner_radius: 0.0,
    };

    let boxed = vec![fill(beside_rect, 6.0)];
    assert_eq!(
        pills(&boxed).len(),
        1,
        "a rounded fill is a pill: {:?}",
        pills(&boxed)
    );
    assert_eq!(
        pills(&[border(beside_rect)]).len(),
        1,
        "a border is a pill"
    );

    // The block's own card: its border and a rounded fill over the same rect.
    let card = vec![border(card_rect), fill(card_rect, radius)];
    assert!(
        pills(&card).is_empty(),
        "the shared command block's card is not a pill, got {:?}",
        pills(&card)
    );
    assert_eq!(
        bordered_control(&card, &app),
        Some(card_rect),
        "the block's card is recognized"
    );

    // A box beside the card is still a pill.
    let beside = vec![border(card_rect), fill(card_rect, radius), fill(beside_rect, 6.0)];
    assert_eq!(
        pills(&beside).len(),
        1,
        "a rounded fill outside the card is a pill"
    );
}
