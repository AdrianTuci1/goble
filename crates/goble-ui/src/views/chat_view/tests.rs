use super::*;
use crate::elements::chat_content::{ChatFragment, ChatRole};
use crate::elements::{AppContext, LayoutContext, SizeConstraint};
use crate::geometry::vec2f;
use crate::theme::ColorToken;

#[test]
fn chat_view_layouts_with_messages() {
    let app = AppContext::default();
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("Hi")],
    )];
    let mut view = ChatView::new().with_messages(messages);
    let size = view.layout(
        SizeConstraint::loose(vec2f(600.0, 800.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(size.x > 0.0);
    assert!(size.y > 0.0);
}

#[test]
fn chat_view_renders_markdown_message() {
    let app = AppContext::default();
    let messages = vec![ChatMessage::from_markdown(
        ChatRole::Assistant,
        "**bold** and `code`",
    )];
    let mut view = ChatView::new().with_messages(messages);
    let size = view.layout(
        SizeConstraint::loose(vec2f(600.0, 800.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(size.x > 0.0);
    assert!(size.y > 0.0);
}

/// R1: the transcript is full-width rows, not pills. The agent's reply
/// paints no box, the user's message is a square band running across the
/// pane, and every row insets its text by 10-15 px of padding.
#[test]
fn the_transcript_rows_span_the_pane_with_ten_to_fifteen_px_of_padding() {
    use crate::render::RenderCommand;
    use crate::test_util::{command_counts, render_element};

    let app = AppContext::default();
    let width = 600.0;
    let messages = vec![
        ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Salut")]),
        ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Bine ai venit")],
        ),
    ];
    let mut view = ChatView::new().with_messages(messages).finish();
    let commands = render_element(&mut view, vec2f(width, 480.0), &app);
    let counts = command_counts(&commands);
    assert_eq!(counts.stroke_rect, 0, "no transcript row draws a border");

    let band = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::SurfaceRaised) => {
                Some((*rect, *corner_radius))
            }
            _ => None,
        })
        .expect("the user's message paints its band");
    assert_eq!(band.1, 0.0, "the user's band has square corners");
    assert_eq!(
        (band.0.min_x(), band.0.max_x()),
        (0.0, width),
        "the user's band runs across the pane"
    );

    let text_x = |label: &str| {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    origin,
                    text: drawn,
                    ..
                } if drawn == label => Some(origin.x),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label:?} is drawn"))
    };
    let user_x = text_x("Salut");
    let reply_x = text_x("Bine ai venit");
    assert_eq!(
        user_x, reply_x,
        "the user's message and the agent's reply share the pane's left edge"
    );
    assert!(
        (10.0..=15.0).contains(&user_x),
        "the row keeps 10-15 px of padding, got {user_x}"
    );
}

#[test]
fn chat_view_empty_state_layouts() {
    let app = AppContext::default();
    let mut view = ChatView::new().with_empty_state("Start chatting", "Type below");
    let size = view.layout(
        SizeConstraint::loose(vec2f(600.0, 800.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(size.x > 0.0);
    assert!(size.y > 0.0);
}

#[test]
fn chat_view_transcript_clips_to_the_viewport_and_follows_the_stream() {
    use crate::test_util::{command_counts, render_element};

    let app = AppContext::default();
    let scroll = Rc::new(RefCell::new(ScrollState::following()));
    let messages = (0..40)
        .map(|i| {
            ChatMessage::new(
                ChatRole::Assistant,
                vec![ChatFragment::text(format!("streamed line {i}"))],
            )
        })
        .collect();
    let mut view = ChatView::new()
        .with_messages(messages)
        .with_scroll_state(scroll.clone())
        .finish();
    let commands = render_element(&mut view, vec2f(600.0, 480.0), &app);
    let counts = command_counts(&commands);
    assert!(
        counts.clip_rect > 0 && counts.pop_clip > 0,
        "the transcript clips to its viewport"
    );
    assert!(
        scroll.borrow().max_offset() > 0.0,
        "40 messages are taller than the pane"
    );
    assert_eq!(
        scroll.borrow().offset(),
        scroll.borrow().max_offset(),
        "the transcript opens on the latest message"
    );
}

/// The transcript takes the height it is given and the composer stays at the
/// bottom of the pane: a long conversation scrolls under the input instead of
/// carrying it out of the view. The outer column must claim its height
/// (`MainAxisSize::Max`); without that the `Expanded` transcript is laid out
/// unbounded, the column reports the whole conversation's height and the input
/// lands below the pane (measured at y=5964 in a 480 px view before the fix).
#[test]
fn the_composer_stays_at_the_bottom_of_the_pane_with_a_long_transcript() {
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let height = 480.0;
    let messages: Vec<ChatMessage> = (0..40)
        .map(|i| {
            ChatMessage::new(
                ChatRole::Assistant,
                vec![ChatFragment::text(format!("streamed line {i}"))],
            )
        })
        .collect();
    let mut view = ChatView::new()
        .with_messages(messages)
        .with_scroll_state(Rc::new(RefCell::new(ScrollState::following())))
        .finish();
    let commands = render_element(&mut view, vec2f(600.0, height), &app);
    let editor = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { text, origin, .. } if text == "Ask anything..." => {
                Some(origin.y)
            }
            _ => None,
        })
        .expect("the composer's editor is drawn");
    assert!(
        editor > height * 0.5 && editor < height,
        "the input stays at the bottom of the pane: {editor} of {height}"
    );
    let lowest = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { origin, .. } => Some(origin.y),
            _ => None,
        })
        .fold(0.0_f32, f32::max);
    assert!(
        lowest < height,
        "nothing is drawn below the pane: {lowest} of {height}"
    );
}

/// The pane's topbar is an overlay, not the transcript's first row: with the
/// dots' tray open, the panel is painted after the messages, so it covers the
/// text, and it opens leftwards from a trigger at the pane's right edge, so it
/// stays inside the view instead of spilling past it.
#[test]
fn the_pane_header_tray_paints_over_the_transcript_and_fits_the_view() {
    use crate::elements::{
        Container, Empty, Fill, Flex, MainAxisSize, PopupMenu, PopupMenuItem, PopupMenuPosition,
        Spacer,
    };
    use crate::render::RenderCommand;
    use crate::test_util::render_element;
    use std::cell::RefCell;
    use std::rc::Rc;

    let app = AppContext::default();
    let width = 600.0;
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("the transcript under the tray")],
    )];

    // The pane's topbar: the trigger pushed to the right end, as the header's
    // own row does with its spacer, and the tray open.
    let open = Rc::new(RefCell::new(true));
    let menu = PopupMenu::new(
        Empty::new().with_size(vec2f(32.0, 32.0)).finish(),
        vec![
            PopupMenuItem::new("Copy"),
            PopupMenuItem::new("Clear transcript"),
        ],
    )
    .with_open(open)
    .with_position(PopupMenuPosition::BelowEnd)
    .finish();
    let header = Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_child(Spacer::new().finish())
            .with_child(menu)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
    .finish();

    let mut view = ChatView::new()
        .with_messages(messages)
        .with_header(header)
        .finish();
    let commands = render_element(&mut view, vec2f(width, 480.0), &app);

    let raised = app.theme.color(ColorToken::SurfaceRaised);
    let (panel_index, panel) = commands
        .iter()
        .enumerate()
        .find_map(|(index, command)| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == raised && rect.height() > 40.0 =>
            {
                Some((index, *rect))
            }
            _ => None,
        })
        .expect("the open tray paints its panel");
    let text_index = commands
        .iter()
        .position(|command| {
            matches!(
                command,
                RenderCommand::DrawText { text, .. } if text.contains("the transcript under the tray")
            )
        })
        .expect("the transcript paints its message");
    assert!(
        panel_index > text_index,
        "the tray paints after the transcript, so it covers it: panel {panel_index}, text {text_index}"
    );
    // The trigger is flush with the view's right edge, so a leftwards panel
    // stops exactly there and extends back into the view.
    assert!(
        (panel.max_x() - width).abs() < 0.01
            && panel.min_x() >= 0.0
            && panel.max_y() <= 480.0,
        "the tray opens leftwards and stays inside the view: {panel:?}"
    );
}

#[test]
fn chat_view_renders_inline_screen_as_image() {
    use crate::test_util::{command_counts, render_element};

    let app = AppContext::default();
    let pixels = vec![0u8; 200 * 120 * 4];
    let mut view = ChatView::new()
        .with_inline_screen("inline-1", 1, 200, 120, Arc::from(pixels))
        .finish();
    let commands = render_element(&mut view, vec2f(600.0, 800.0), &app);
    let counts = command_counts(&commands);
    assert!(
        counts.draw_image > 0,
        "a live inline screen should paint the frame image"
    );
    assert!(
        counts.draw_text > 0,
        "the harness-in-control header should render text"
    );
    assert_eq!(
        counts.stroke_rect, 0,
        "the inline desktop handoff paints a band, not a bordered card"
    );
}

#[test]
fn chat_view_renders_screen_link_card_without_frame() {
    use crate::test_util::{command_counts, render_element};

    let app = AppContext::default();
    let mut view = ChatView::new()
        .with_screen_link(Some("rdp://host:3389".to_string()))
        .finish();
    let commands = render_element(&mut view, vec2f(600.0, 800.0), &app);
    let counts = command_counts(&commands);
    assert_eq!(
        counts.draw_image, 0,
        "no live frame is drawn when only a URI link is present"
    );
    assert!(
        counts.draw_text > 0,
        "the handoff card should render a label and action"
    );
    assert_eq!(
        counts.stroke_rect, 0,
        "the screen-link handoff paints a band, not a bordered card"
    );
}

#[test]
fn chat_view_renders_the_queued_prompt_as_a_borderless_band() {
    use crate::test_util::{command_counts, render_element};

    let app = AppContext::default();
    // A message keeps the view on the transcript branch: the queued prompt
    // only appears alongside a conversation.
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("working…")],
    )];
    let mut view = ChatView::new()
        .with_messages(messages)
        .with_queued_prompt(Some("run the tests".to_string()))
        .finish();
    let commands = render_element(&mut view, vec2f(600.0, 800.0), &app);
    let counts = command_counts(&commands);
    assert!(
        counts.draw_text > 0,
        "the queued prompt should render its label and text"
    );
    assert_eq!(
        counts.stroke_rect, 0,
        "the queued prompt paints a band, not a bordered card"
    );
}

/// `e` advances every tool call in the transcript through
/// `Collapsed -> Truncated -> Expanded -> Collapsed`, and the app-owned map
/// is what a rebuild reads back.
#[test]
fn e_cycles_the_transcript_tool_folds_and_survives_a_rebuild() {
    use crate::elements::chat_content::ToolCall;
    use crate::event::{DispatchedEvent, ModifiersState};
    use crate::test_util::render_element;
    use goble_core::harness::ToolCallStatus;

    let app = AppContext::default();
    let call = ToolCall {
        id: "call_read".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"src/lib.rs"}"#.to_string(),
        status: ToolCallStatus::Finished,
        result: Some(
            (1..=12)
                .map(|i| format!("line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    };
    let messages = vec![ChatMessage::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(vec![call])];
    let fold = Rc::new(RefCell::new(HashMap::new()));

    // A builder so every "frame" is a fresh view over the same app-owned map
    // and messages, exactly as the app rebuilds it.
    let build = |fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>| -> Box<dyn Element> {
        ChatView::new()
            .with_messages(messages.clone())
            .with_tool_fold(fold.clone())
            .finish()
    };
    let has_text = |commands: &[crate::render::RenderCommand], needle: &str| {
        // A highlighted body arrives as several runs, so the needle is
        // matched against the whole drawn text, not one run.
        let drawn: String = commands
            .iter()
            .filter_map(|c| match c {
                crate::render::RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        drawn.contains(needle)
    };
    let press_e = |view: &mut Box<dyn Element>| {
        view.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "e".to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut crate::elements::EventContext::default(),
            &app,
        );
    };

    let mut view = build(&fold);
    let collapsed = render_element(&mut view, vec2f(600.0, 800.0), &app);
    assert!(
        has_text(&collapsed, "src/lib.rs"),
        "the folded read's header carries its path"
    );
    assert!(
        !has_text(&collapsed, "line 1"),
        "the read body is hidden while folded"
    );

    press_e(&mut view);
    let mut view = build(&fold);
    let truncated = render_element(&mut view, vec2f(600.0, 800.0), &app);
    assert!(
        has_text(&truncated, "line 1") && has_text(&truncated, "line 12"),
        "truncated shows the head and tail of the file"
    );
    assert!(
        !has_text(&truncated, "line 7"),
        "truncated elides the middle"
    );

    press_e(&mut view);
    let mut view = build(&fold);
    let expanded = render_element(&mut view, vec2f(600.0, 800.0), &app);
    assert!(
        has_text(&expanded, "line 7"),
        "expanded shows the whole file"
    );

    press_e(&mut view);
    let mut view = build(&fold);
    let folded_again = render_element(&mut view, vec2f(600.0, 800.0), &app);
    assert!(
        !has_text(&folded_again, "line 1"),
        "a third press folds the call back"
    );
}

/// A background chat pane sharing the tree does not take the fold key.
#[test]
fn a_background_chat_pane_does_not_take_the_fold_key() {
    use crate::elements::chat_content::ToolCall;
    use crate::event::{DispatchedEvent, ModifiersState};
    use goble_core::harness::ToolCallStatus;

    let app = AppContext::default();
    let call = ToolCall {
        id: "call_read".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"src/lib.rs"}"#.to_string(),
        status: ToolCallStatus::Finished,
        result: Some("line 1\nline 2".to_string()),
    };
    let messages = vec![ChatMessage::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(vec![call])];
    let fold = Rc::new(RefCell::new(HashMap::new()));
    let mut view: Box<dyn Element> = ChatView::new()
        .with_messages(messages)
        .with_tool_fold(fold.clone())
        .with_pane_active(false)
        .finish();

    let consumed = view.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: "e".to_string(),
            modifiers: ModifiersState::none(),
        },
        &mut crate::elements::EventContext::default(),
        &app,
    );
    assert!(!consumed, "a background pane must not consume the key");
    assert!(
        fold.borrow().is_empty(),
        "a background pane must not fold its calls"
    );
}

/// The turn-status footer is drawn above the composer's divider when work is
/// in flight; it is information only, so the view relays no click from it.
/// The whole agent transcript, painted as one frame: the user's message, the
/// agent's prose and reasoning, a tool call of every family in every fold — one
/// of them still in flight — a sub-agent's row, an interrupted turn and the
/// footer. Not one pill, anywhere: the pane's rows are square and borderless,
/// and the only bordered box is the shared terminal command block.
#[test]
fn the_agent_transcript_paints_no_pill() {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::time::Duration;

    use crate::elements::chat_content::{
        tool_fold_key, SubAgentRow, SubAgentRowStatus, ToolCall, ToolDisplayMode,
    };
    use crate::elements::{
        AskUserUi, CommandProposalUi, Element as _, TurnStatus, WorkKind, WorkKindCount,
    };
    use crate::test_util::{assert_no_row_border, assert_pill_free, render_element};
    use goble_core::harness::ToolCallStatus;

    let app = AppContext::default();
    let call = |name: &str, arguments: &str, status: ToolCallStatus, result: Option<&str>| ToolCall {
        id: format!("call_{name}"),
        name: name.to_string(),
        arguments: arguments.to_string(),
        status,
        result: result.map(str::to_string),
    };

    let read = call("read_file", r#"{"path":"src/lib.rs","start_line":1,"end_line":3}"#, ToolCallStatus::Finished, Some("1: one\n2: two"));
    let command = call("run_command", r#"{"command":"cargo test"}"#, ToolCallStatus::Error, Some("2 tests failed"));
    let edit = call("edit_file", r#"{"path":"src/lib.rs","old_text":"let a = 1;","new_text":"let a = 2;"}"#, ToolCallStatus::Finished, Some("edited"));
    let in_flight = call("codebase_search", r#"{"pattern":"fn main","path":"src"}"#, ToolCallStatus::Running, None);
    let spawn = call("spawn_subagent", r#"{"subagent_type":"reviewer","input":"audit the migration"}"#, ToolCallStatus::Running, None);

    let calls = vec![read, command, edit, in_flight, spawn.clone()];
    // Every fold is drawn at once: each call is painted in its default fold,
    // and the ones a user opened are in the app-owned map.
    let fold: HashMap<String, ToolDisplayMode> = calls
        .iter()
        .enumerate()
        .map(|(index, call)| {
            let mode = match index {
                0 => ToolDisplayMode::Truncated,
                1 => ToolDisplayMode::Expanded,
                2 => ToolDisplayMode::Expanded,
                _ => ToolDisplayMode::Truncated,
            };
            (tool_fold_key(call, index), mode)
        })
        .collect();

    let messages = vec![
        ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Fix the build")]),
        ChatMessage::new(
            ChatRole::Assistant,
            vec![
                ChatFragment::reasoning("step-0", "Thinking", "the build fails on the parser", true),
                ChatFragment::text("Reading the parser's own row, then running the tests."),
            ],
        )
        .with_tool_calls(calls),
    ];

    let record = SubAgentRow {
        child_id: "conv-child-1".to_string(),
        subagent_type: "reviewer".to_string(),
        description: "audit the migration".to_string(),
        status: SubAgentRowStatus::Running,
        activity: "reading the schema".to_string(),
        elapsed: Duration::from_millis(4500),
        turns: 2,
        tool_calls: 4,
        tokens: 1234,
        outcome: None,
        background: true,
    };

    let mut view = ChatView::new()
        .with_messages(messages)
        .with_tool_fold(Rc::new(RefCell::new(fold)))
        .with_sub_agents(HashMap::from([(spawn.id.clone(), record)]))
        // An interrupted turn: the agent asked a question and proposed a
        // command, and another prompt is queued behind it.
        .with_pending_ask(Some(AskUserUi::new(
            "Which environment?",
            vec!["staging".to_string(), "production".to_string()],
        )))
        .with_command_proposal(Some(CommandProposalUi::new(
            "cmd-1",
            vec!["cargo test".to_string(), "cargo clippy".to_string()],
            "~/Projects/goble",
        )))
        .with_queued_prompt(Some("then run the linter".to_string()))
        .with_turn_status(TurnStatus::still_running(vec![WorkKindCount {
            kind: WorkKind::Execution,
            count: 1,
        }]))
        .without_composer()
        .finish();

    let commands = render_element(&mut view, vec2f(600.0, 800.0), &app);
    assert_pill_free(&commands, "the agent transcript");
    assert_no_row_border(&commands, "the agent transcript");
}

/// The transcript's own rows keep a real tool call drawn in every fold, so the
/// pane-level pill lock above is not vacuous.
#[test]
fn the_transcript_draws_the_tool_rows_the_lock_covers() {
    use crate::render::RenderCommand;
    use crate::test_util::render_element;
    use goble_core::harness::ToolCallStatus;
    use crate::elements::chat_content::ToolCall;

    let app = AppContext::default();
    let read = ToolCall {
        id: "call_read".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"src/lib.rs"}"#.to_string(),
        status: ToolCallStatus::Finished,
        result: Some("1: fn main() {}".to_string()),
    };
    let edit = ToolCall {
        id: "call_edit".to_string(),
        name: "edit_file".to_string(),
        arguments: r#"{"path":"src/main.rs","old_text":"a","new_text":"b"}"#.to_string(),
        status: ToolCallStatus::Running,
        result: None,
    };
    let messages = vec![ChatMessage::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(vec![read.clone(), edit.clone()])];
    let mut view = ChatView::new()
        .with_messages(messages)
        .without_composer()
        .finish();
    let texts: Vec<String> = render_element(&mut view, vec2f(600.0, 400.0), &app)
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    for expected in ["Read ", "src/lib.rs", "Edit "] {
        assert!(
            texts.iter().any(|text| text == expected),
            "the transcript draws {expected:?}, got {texts:?}"
        );
    }
}

/// The still-running footer is information without a cue: it states the work in
/// flight and takes no pointer event.
#[test]
fn chat_view_draws_the_turn_status_footer_without_a_cue() {
    use crate::elements::{TurnStatus, WorkKind, WorkKindCount};
    use crate::test_util::render_element;

    let app = AppContext::default();
    let status = TurnStatus::still_running(vec![WorkKindCount {
        kind: WorkKind::Execution,
        count: 1,
    }]);
    let mut view: Box<dyn Element> = ChatView::new().with_turn_status(status.clone()).finish();
    let commands = render_element(&mut view, vec2f(600.0, 800.0), &app);
    let cue_origin = commands
        .iter()
        .find_map(|c| match c {
            crate::render::RenderCommand::DrawText { origin, text, .. }
                if text.contains("still running") =>
            {
                Some(*origin)
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            let texts: Vec<&str> = commands
                .iter()
                .filter_map(|c| match c {
                    crate::render::RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            panic!("the view draws the footer's still-running line: {texts:?}");
        });

    // An idle footer is not added to the column at all, so an idle view
    // draws no footer.
    let mut idle: Box<dyn Element> = ChatView::new().finish();
    let idle_commands = render_element(&mut idle, vec2f(600.0, 800.0), &app);
    assert!(
        !idle_commands.iter().any(|c| matches!(
            c,
            crate::render::RenderCommand::DrawText { text, .. } if text.contains("still running")
        )),
        "an idle view draws no footer"
    );

    // The drawn still-running line is not a hit target: the view's dispatch
    // path does not consume the click.
    let position = cue_origin + vec2f(2.0, 2.0);
    let mut ctx = crate::elements::EventContext::default();
    for event in [
        crate::event::DispatchedEvent::MouseDown {
            position,
            button: 0,
        },
        crate::event::DispatchedEvent::MouseUp {
            position,
            button: 0,
        },
    ] {
        assert!(
            !view.dispatch_event(&event, &mut ctx, &app),
            "the footer line takes no pointer event"
        );
    }
}

/// A read-only transcript drops the composer and takes Escape as its way
/// back; without the hook Escape stays unconsumed.
#[test]
fn a_composerless_view_shows_no_input_line_and_hands_escape_to_its_handler() {
    use crate::event::{DispatchedEvent, ModifiersState};
    use crate::test_util::render_element;

    let app = AppContext::default();
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("child says hello")],
    )];
    let escape = |view: &mut Box<dyn Element>| {
        view.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Escape".to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut crate::elements::EventContext::default(),
            &app,
        )
    };

    let fired = Rc::new(RefCell::new(0));
    let fired_clone = fired.clone();
    let mut view: Box<dyn Element> = ChatView::new()
        .with_messages(messages.clone())
        .without_composer()
        .with_on_escape(move || *fired_clone.borrow_mut() += 1)
        .finish();
    let commands = render_element(&mut view, vec2f(600.0, 800.0), &app);
    let texts: Vec<&str> = commands
        .iter()
        .filter_map(|c| match c {
            crate::render::RenderCommand::DrawText { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|t| t.contains("child says hello")),
        "the transcript still renders: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("Ask anything")),
        "a read-only transcript draws no composer: {texts:?}"
    );
    assert!(escape(&mut view), "the hook consumes Escape");
    assert_eq!(*fired.borrow(), 1, "the hook fires once per press");
    assert!(escape(&mut view), "and again on the next trip");
    assert_eq!(*fired.borrow(), 2, "the round trip can be made twice");

    // The default view keeps its composer and its old Escape behaviour.
    let mut with_composer: Box<dyn Element> = ChatView::new().with_messages(messages).finish();
    let commands = render_element(&mut with_composer, vec2f(600.0, 800.0), &app);
    let texts: Vec<&str> = commands
        .iter()
        .filter_map(|c| match c {
            crate::render::RenderCommand::DrawText { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|t| t.contains("Ask anything")),
        "the chat pane keeps its input line: {texts:?}"
    );
    assert!(
        !escape(&mut with_composer),
        "without a hook Escape is left for someone else"
    );
}

/// The transcript's closing footer: a Fork affordance, and the conversation's
/// token usage beside it. Neither is a physical button — the footer draws no
/// band at rest, and the pointer over one draws its band — and the counts are
/// the provider's own: with none reported the slot says so rather than drawing
/// a zero.
#[test]
fn the_transcript_footer_shows_fork_and_the_reported_token_usage() {
    use crate::elements::interactive::contains;
    use crate::elements::{LayoutContext, PaintContext, SizeConstraint};
    use crate::geometry::RectF;
    use crate::render::{RenderCommand, Renderer};
    use goble_core::llm::TokenUsage;

    /// Lay the view out and paint it, optionally with the pointer at `cursor`,
    /// returning the frame's commands.
    fn paint_with(
        view: &mut Box<dyn crate::elements::Element>,
        cursor: Option<Vector2F>,
        app: &AppContext,
    ) -> Vec<RenderCommand> {
        let size = vec2f(600.0, 800.0);
        let _ = view.layout(
            SizeConstraint::loose(size),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        if let Some(position) = cursor {
            ctx.cursor_inside = true;
            ctx.cursor_position = position;
        }
        view.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default()
    }

    fn fills(commands: &[RenderCommand]) -> Vec<RectF> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect()
    }

    fn texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn text_origin(commands: &[RenderCommand], needle: &str) -> Vector2F {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == needle => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle:?} is drawn: {:?}", texts(commands)))
    }

    /// The fills that could be a row's band: the frame's own background and the
    /// 1px rules are not candidates for a control's hover band.
    fn bands(commands: &[RenderCommand]) -> Vec<RectF> {
        fills(commands)
            .into_iter()
            .filter(|rect| rect.height() <= 40.0)
            .collect()
    }

    /// The band the pointer at `point` sits in: the frame draws a row-height
    /// fill under it that the rest frame did not, and no band at all when the
    /// pointer is over a control with no resting chrome.
    fn band_at(rest: &[RenderCommand], hovered: &[RenderCommand], point: Vector2F) -> RectF {
        let rest = bands(rest);
        let band = bands(hovered)
            .into_iter()
            .find(|candidate| contains(*candidate, point) && !rest.contains(candidate));
        band.unwrap_or_else(|| panic!("the pointer at {point:?} is inside a hovered band"))
    }

    let app = AppContext::default();
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("Hi")],
    )];
    let forked = std::rc::Rc::new(std::cell::RefCell::new(0usize));
    let forked_clone = forked.clone();
    let open = std::rc::Rc::new(std::cell::RefCell::new(false));
    let mut view = ChatView::new()
        .with_messages(messages.clone())
        .with_usage(TokenUsage {
            input: 1_234,
            cached: Some(800),
            output: 56,
        })
        .with_usage_open(open.clone())
        .with_on_fork(move || *forked_clone.borrow_mut() += 1)
        .finish();

    // At rest: both affordances are there, and neither is a filled control.
    let rest = paint_with(&mut view, None, &app);
    let drawn = texts(&rest);
    assert!(drawn.iter().any(|t| t == "Fork"), "no fork affordance: {drawn:?}");
    assert!(
        drawn.iter().any(|t| t == "1,290 tokens"),
        "the total is input + output, grouped: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|t| t.contains("cached")),
        "the detail stays collapsed until asked for: {drawn:?}"
    );
    let fork_origin = text_origin(&rest, "Fork");
    let usage_origin = text_origin(&rest, "1,290 tokens");
    let probe = |origin: Vector2F| origin + vec2f(1.0, 1.0);
    let resting = bands(&rest);
    assert!(
        !resting.iter().any(|band| contains(*band, probe(fork_origin))),
        "a resting fork draws no band: {resting:?}"
    );
    assert!(
        !resting.iter().any(|band| contains(*band, probe(usage_origin))),
        "a resting usage readout draws no band: {resting:?}"
    );

    // The pointer over one draws that one's band, and the other keeps none.
    let over_fork = paint_with(&mut view, Some(probe(fork_origin)), &app);
    let fork_band = band_at(&rest, &over_fork, probe(fork_origin));
    assert!(
        !contains(fork_band, probe(usage_origin)),
        "the band belongs to the fork row: {fork_band:?}"
    );
    assert_eq!(
        bands(&over_fork)
            .into_iter()
            .find(|band| contains(*band, probe(usage_origin))),
        None,
        "the fork's band does not reach the usage readout"
    );

    let over_usage = paint_with(&mut view, Some(probe(usage_origin)), &app);
    let usage_band = band_at(&rest, &over_usage, probe(usage_origin));
    assert!(
        !contains(usage_band, probe(fork_origin)),
        "the band belongs to the usage row: {usage_band:?}"
    );

    // Expanding the disclosure reveals the breakdown: the cached share rides
    // inside the input figure, so the line never reads as a sum.
    *open.borrow_mut() = true;
    let expanded = paint_with(&mut view, None, &app);
    let drawn = texts(&expanded);
    let detail = drawn
        .iter()
        .find(|t| t.contains("cached"))
        .unwrap_or_else(|| panic!("no usage detail: {drawn:?}"));
    assert!(detail.contains("Input 1,234 (800 cached)"), "{detail}");
    assert!(detail.contains("Output 56"), "{detail}");
    assert!(detail.contains("Total 1,290"), "{detail}");

    // A conversation whose provider has reported nothing keeps the slot and
    // says so, with no number invented to fill it, and no disclosure to open.
    let mut bare = ChatView::new()
        .with_messages(messages)
        .with_usage_open(std::rc::Rc::new(std::cell::RefCell::new(false)))
        .with_on_fork(|| {})
        .finish();
    let rest = paint_with(&mut bare, None, &app);
    let drawn = texts(&rest);
    assert!(drawn.iter().any(|t| t == "Fork"), "{drawn:?}");
    assert!(
        drawn.iter().any(|t| t == "no usage yet"),
        "the usage slot states that nothing was reported: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|t| t.contains("tokens")),
        "no fabricated zero: {drawn:?}"
    );
    let usage_origin = text_origin(&rest, "no usage yet");
    let over_usage = paint_with(&mut bare, Some(probe(usage_origin)), &app);
    let _ = band_at(&rest, &over_usage, probe(usage_origin));
}

/// The instructions the caller hands the view are drawn over the input's
/// separator, as the content's last row above it — the warp-new placement, where
/// the shortcuts view sits between the transcript and the input it describes.
/// A view handed none draws neither strip nor a gap where one would have been.
#[test]
fn the_composer_hints_sit_over_the_separator_and_only_when_given() {
    use crate::elements::ShortcutHint;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let width = 600.0;
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("Hi")],
    )];
    let line_of = |commands: &[RenderCommand], needle: &str| -> Option<f32> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawText { text, origin, .. } if text == needle => Some(origin.y),
            _ => None,
        })
    };
    // The pane-wide rules this frame drew: the input's separator is the one
    // over the bottom of the view.
    let rules = |commands: &[RenderCommand]| -> Vec<f32> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, .. }
                    if rect.height() <= 2.0 && rect.width() >= width - 0.5 =>
                {
                    Some(rect.min_y())
                }
                _ => None,
            })
            .collect()
    };

    let mut plain = ChatView::new().with_messages(messages.clone()).finish();
    let commands = render_element(&mut plain, vec2f(width, 520.0), &app);
    for text in ["commands", "tasks", "⌘"] {
        assert!(
            line_of(&commands, text).is_none(),
            "a view with no hints draws no strip ({text:?})"
        );
    }
    let plain_rules = rules(&commands);
    assert_eq!(plain_rules.len(), 1, "one separator divides input from content");

    let mut view = ChatView::new()
        .with_messages(messages)
        .with_composer_hints(vec![
            ShortcutHint::new(&["⌘", "K"], "commands"),
            ShortcutHint::new(&["⌘", "⇧", "W"], "tasks"),
        ])
        .finish();
    let commands = render_element(&mut view, vec2f(width, 520.0), &app);
    let strip = line_of(&commands, "commands").expect("the strip is drawn");
    let tasks = line_of(&commands, "tasks").expect("the whole strip is drawn");
    assert_eq!(strip, tasks, "the strip is one line");

    // The strip is the last row of the content: under the transcript, over the
    // separator, over the editor.
    let transcript = line_of(&commands, "Hi").expect("the message is drawn");
    let editor = line_of(&commands, "Ask anything...").expect("the editor is drawn");
    assert!(transcript < strip, "the strip closes the content");
    let separator = rules(&commands)
        .into_iter()
        .find(|y| *y > strip)
        .unwrap_or_else(|| panic!("a separator under the strip (strip at {strip})"));
    assert!(strip < separator, "the strip is over the separator");
    assert!(
        separator < editor,
        "and the separator is the input's top edge"
    );
    // The strip costs the pane nothing but its own height: the separator stays
    // where it would have been without it, since the input below is unchanged.
    assert!(
        (separator - plain_rules[0]).abs() < 0.5,
        "the input did not move: separator at {separator} against {} without the strip",
        plain_rules[0]
    );
}

