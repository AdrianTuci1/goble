use super::*;
use crate::elements::chat_content::{ChatFragment, ChatRole};
use crate::elements::{AppContext, LayoutContext, SizeConstraint};
use crate::geometry::vec2f;
use crate::theme::{ColorToken, SpacingToken};

#[test]
fn a_failed_turn_draws_its_reason_in_the_transcript() {
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("Partial answer")],
    )];
    let mut view = ChatView::new()
        .with_messages(messages)
        .with_turn_error(Some("Incorrect API key provided".to_string()))
        .finish();
    let commands = render_element(&mut view, vec2f(600.0, 480.0), &app);
    let texts: Vec<&str> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|text| *text == "Agent turn failed"));
    assert!(
        texts
            .iter()
            .any(|text| text.contains("Incorrect API key provided")),
        "the reason the turn failed is drawn in the transcript"
    );
}

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

/// The transcript's rows are full-width, not pills: the agent's reply paints
/// no box and keeps DSH's 16 px inset, while the user's own message is the one
/// surface — a right-aligned, capped bubble at 20 px radius with a 10 × 16
/// inset.
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
    // The composer is borderless (owner feedback): no row of the pane — and no
    // composer surface — draws an outline. The composer's two surfaces are
    // rounded *fills*: the `Surface` card at `COMPOSER_CARD_RADIUS` and its
    // `SurfaceRaised` input segment at `COMPOSER_INPUT_RADIUS`, so a rounded box
    // in the command list is the composer, never a transcript row's pill.
    let outlines = commands
        .iter()
        .filter(|command| matches!(command, RenderCommand::StrokeRect { .. }))
        .count();
    assert_eq!(
        outlines, 0,
        "no pane row or composer surface draws a border"
    );
    let rounded: Vec<f32> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { corner_radius, .. } if *corner_radius > 0.0 => {
                Some(*corner_radius)
            }
            _ => None,
        })
        .collect();
    assert!(
        rounded
            .iter()
            .any(|radius| (*radius - crate::elements::COMPOSER_CARD_RADIUS).abs() < 0.01),
        "the composer card is a rounded fill: {rounded:?}"
    );
    assert!(
        rounded
            .iter()
            .any(|radius| (*radius - crate::elements::COMPOSER_INPUT_RADIUS).abs() < 0.01),
        "the composer's input segment is a rounded fill: {rounded:?}"
    );
    let _ = command_counts(&commands);

    // The user's message is DSH's right-aligned bubble: capped, rounded at
    // 20 and carrying the 10 × 16 inset.
    let band = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::SurfaceRaised)
                && (*corner_radius - crate::elements::chat_typography::USER_BUBBLE_RADIUS)
                    .abs()
                    < 0.01 =>
            {
                Some((*rect, *corner_radius))
            }
            _ => None,
        })
        .expect("the user's message paints its bubble");
    assert_eq!(
        band.1,
        crate::elements::chat_typography::USER_BUBBLE_RADIUS,
        "the user's bubble is rounded at DSH's 20 px"
    );
    assert!(
        band.0.width() <= width * 0.82 + 0.01,
        "the user's bubble is capped at the lane's share: {} against {width}",
        band.0.width()
    );
    assert_eq!(
        band.0.height(),
        14.0 * crate::elements::chat_typography::USER_LH
            + 2.0 * crate::elements::chat_typography::BAND_PADDING_Y,
        "the bubble is inset 10 px over and under its 14 / 22 line"
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
    assert!(
        (29.0..=32.0).contains(&reply_x),
        "the agent's row keeps DSH's 16 px band inset, got {reply_x}"
    );

    // No prompt marker: the user's text starts one bubble inset in from the
    // right-aligned bubble's edge.
    assert!(
        !commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, .. } if name == "prompt-arrow"
        )),
        "the user's message draws no prompt marker"
    );
    assert!(
        (user_x - (band.0.min_x() + crate::elements::chat_typography::BAND_PADDING_X)).abs() < 0.01,
        "the user's text starts at the bubble's 16 px inset: {user_x} against {:?}",
        band.0
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

/// A cloud agent's landing leads the empty state with the cloud, keeps the
/// worker connection's notice above it, and its composer invites the run it
/// starts. A pane without a landing icon keeps the notice alone, as before.
#[test]
fn the_cloud_landing_draws_its_mark_over_the_notice() {
    use crate::elements::{Container, Text};
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let draw = |landing: bool| {
        let notice = Container::new(Text::new("Connected to worker dev").finish()).finish();
        let mut view = ChatView::new()
            .with_empty_state("New cloud agent", "It runs on the worker.")
            .with_composer_placeholder("Kick off a cloud agent")
            .with_notice(Some(notice));
        if landing {
            view = view.with_empty_state_icon("cloud");
        }
        render_element(
            &mut view.finish(),
            vec2f(600.0, 800.0),
            &AppContext::default(),
        )
    };

    let cloud = draw(true);
    assert!(
        cloud.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, .. } if name == "cloud"
        )),
        "the landing leads with the cloud: {cloud:?}"
    );
    assert!(
        cloud.iter().any(|command| matches!(
            command,
            RenderCommand::DrawText { text, .. } if text == "Connected to worker dev"
        )),
        "the worker's state stays above the landing: {cloud:?}"
    );
    assert!(
        cloud.iter().any(|command| matches!(
            command,
            RenderCommand::DrawText { text, .. } if text == "Kick off a cloud agent"
        )),
        "the composer invites the run: {cloud:?}"
    );

    let plain = draw(false);
    assert!(
        !plain.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, .. } if name == "cloud"
        )),
        "a pane without a landing draws no cloud: {plain:?}"
    );
}

/// The empty state: "New conversation" and its invitation are centered inside
/// a rounded bordered frame, the frame is inset from the pane's side edges and
/// capped in width, and it hangs a third of the free height above the composer
/// rather than dead center — the placement the reference welcome box uses.
#[test]
fn the_empty_state_is_centered_in_a_frame() {
    use crate::elements::text::measure_text;
    use crate::geometry::RectF;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    const TITLE: &str = "New conversation";
    const SUBTITLE: &str = "Ask anything to get started.";

    fn draw(width: f32, height: f32) -> Vec<RenderCommand> {
        let app = AppContext::default();
        let mut view = ChatView::new().with_empty_state(TITLE, SUBTITLE).finish();
        render_element(&mut view, vec2f(width, height), &app)
    }

    fn text_origin(commands: &[RenderCommand], needle: &str) -> Vector2F {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == needle => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle:?} is drawn"))
    }

    fn inside(point: Vector2F, rect: RectF) -> bool {
        point.x >= rect.min_x()
            && point.x <= rect.max_x()
            && point.y >= rect.min_y()
            && point.y <= rect.max_y()
    }

    /// The rounded border the empty state's own text sits in.
    fn empty_frame(commands: &[RenderCommand], text: Vector2F) -> RectF {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::StrokeRect {
                    rect,
                    corner_radius,
                    ..
                } if *corner_radius > 0.0 && inside(text, *rect) => Some(*rect),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the empty state's text sits inside a rounded frame"))
    }

    let app = AppContext::default();
    let margin = app.theme.spacing_px(SpacingToken::Xl);

    // A pane narrower than the frame's cap: the frame spans it minus the
    // margin on each side.
    let commands = draw(600.0, 800.0);
    let title = text_origin(&commands, TITLE);
    let subtitle = text_origin(&commands, SUBTITLE);
    let frame = empty_frame(&commands, title);

    assert!(
        inside(subtitle, frame),
        "both lines sit in the frame: subtitle {subtitle:?} is outside {frame:?}"
    );
    assert!(
        inside(title, frame),
        "the title sits in the frame: {title:?} is outside {frame:?}"
    );

    // Centered inside it, not hugging an edge.
    let title_middle = title.x + measure_text(TITLE, 12.0, 1.2, f32::INFINITY).x / 2.0;
    let frame_middle = (frame.min_x() + frame.max_x()) / 2.0;
    assert!(
        (title_middle - frame_middle).abs() <= 1.0,
        "the title is centered in the frame: text middle {title_middle}, frame middle {frame_middle}"
    );


    // A pane wider than the cap: the frame takes the cap and stays centered,
    // rather than running the full width or hugging the left margin.
    let wide = draw(1200.0, 800.0);
    let wide_title = text_origin(&wide, TITLE);
    let wide_frame = empty_frame(&wide, wide_title);
    assert!(
        (wide_frame.width() - super::transcript::EMPTY_FRAME_MAX_WIDTH).abs() < 0.01,
        "a wide pane caps the frame at {}, got {}",
        super::transcript::EMPTY_FRAME_MAX_WIDTH,
        wide_frame.width()
    );
    assert!(
        (wide_frame.min_x() - (1200.0 - wide_frame.width()) / 2.0).abs() < 0.01,
        "the capped frame is centered in the pane, got {wide_frame:?}"
    );

    // It sits a third of the free height down: 600 px more pane moves it
    // 200 px, where a box centered in the slack would move 300.
    let taller = draw(600.0, 1400.0);
    let taller_frame = empty_frame(&taller, text_origin(&taller, TITLE));
    let moved = taller_frame.min_y() - frame.min_y();
    assert!(
        (moved - 200.0).abs() <= 1.0,
        "the frame hangs a third of the slack above the composer, so 600 px more pane moves it \
         200 px, got {moved}"
    );
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
    // The tray hangs under the pane header, in the top of the view; the
    // composer's input segment is also a raised fill, so the panel is picked by
    // where it is, not by being the first raised fill in the frame.
    let (panel_index, panel) = commands
        .iter()
        .enumerate()
        .find_map(|(index, command)| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == raised && rect.height() > 40.0 && rect.min_y() < 200.0 =>
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
        (panel.max_x() - width).abs() < 0.01 && panel.min_x() >= 0.0 && panel.max_y() <= 480.0,
        "the tray opens leftwards and stays inside the view: {panel:?}"
    );
}

/// The pane's work dock is chrome around the transcript: it takes the row
/// directly under the header the column reserved and hands the rest to the
/// conversation, so it never scrolls with the messages.
#[test]
fn the_work_dock_sits_under_the_header_and_above_the_transcript() {
    use crate::elements::chat_content::{SubAgentPanel, SubAgentRow, SubAgentRowStatus};
    use crate::elements::Empty;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;
    use std::time::Duration;

    let app = AppContext::default();
    let header_height = 44.0;
    let record = SubAgentRow {
        child_id: "conv-child-1".to_string(),
        subagent_type: "worker".to_string(),
        description: "Write simple slugify function".to_string(),
        status: SubAgentRowStatus::Running,
        activity: String::new(),
        elapsed: Duration::from_millis(8700),
        turns: 1,
        tool_calls: 2,
        tokens: 300,
        outcome: None,
        background: false,
    };
    let panel = SubAgentPanel::new(vec![record], "deepseek", true).finish();
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("transcript marker")],
    )];
    let mut view = ChatView::new()
        .with_messages(messages)
        .with_header(Empty::new().with_size(vec2f(0.0, header_height)).finish())
        .with_work_dock(Some(panel))
        .finish();
    let commands = render_element(&mut view, vec2f(600.0, 480.0), &app);

    let origin_of = |label: &str| {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { origin, text, .. } if text == label => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the pane draws {label:?}"))
    };
    let strip = origin_of("Subagents");
    let transcript = origin_of("transcript marker");
    assert!(
        strip.y >= header_height - 0.5,
        "the strip starts under the reserved header, at {} of {header_height}",
        strip.y
    );
    assert!(
        transcript.y > strip.y,
        "the transcript starts below the strip: strip {} transcript {}",
        strip.y,
        transcript.y
    );
    // The strip continues the bar rather than sitting under it as a separate
    // band: its surface starts exactly where the bar's reserved height ends and
    // spans the pane, so no seam shows between the two.
    let band = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == app.theme.color(ColorToken::Surface) && rect.height() > 10.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the strip paints its own band");
    assert!(
        (band.min_y() - header_height).abs() < 0.5,
        "the strip's band starts where the bar ends ({header_height}), got {band:?}"
    );
    assert!(
        (band.width() - 600.0).abs() < 0.5,
        "the strip's band spans the pane, got {band:?}"
    );
}

#[test]
fn chat_view_renders_the_queued_prompt_as_a_borderless_band() {
    use crate::render::RenderCommand;
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
    // The composer is borderless (owner feedback), so the pane draws no
    // outline at all; the queued prompt is a flat band rather than a card, so
    // the only rounded surfaces in the pane are the composer's own card and
    // input segment — the queued prompt adds none of its own.
    let outlines = commands
        .iter()
        .filter(|command| matches!(command, RenderCommand::StrokeRect { .. }))
        .count();
    assert_eq!(
        outlines, 0,
        "the borderless composer and the queued band draw no outline"
    );
    let rounded: Vec<f32> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { corner_radius, .. } if *corner_radius > 0.0 => {
                Some(*corner_radius)
            }
            _ => None,
        })
        .collect();
    // The composer's own rounded surfaces: the card and its input segment.
    // The rich input draws no send control any more (the owner removed the
    // circular send button), and nothing else in this pane is rounded.
    let composer_radii = [
        crate::elements::COMPOSER_CARD_RADIUS,
        crate::elements::COMPOSER_INPUT_RADIUS,
    ];
    assert_eq!(
        rounded.len(),
        composer_radii.len(),
        "only the composer's card and input segment are rounded; the queued prompt paints no card of its own: {rounded:?}"
    );
    assert!(
        composer_radii
            .iter()
            .all(|wanted| rounded.iter().any(|radius| (radius - wanted).abs() < 0.01)),
        "the rounded surfaces are the composer's own: {rounded:?}"
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
        diff: None,
    };
    let messages =
        vec![ChatMessage::new(ChatRole::Assistant, Vec::new()).with_tool_calls(vec![call])];
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
        has_text(&collapsed, "Read 1 file"),
        "the folded read stacks into its run's aggregated header"
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

/// The transcript has no cursor, so Right opens the newest foldable row the
/// tail carries — first the folded run, then the call inside it — and Left
/// closes the same rows back. grok-build's arrow pair, on the row the pane
/// tails.
#[test]
fn right_opens_the_newest_fold_and_left_closes_it() {
    use crate::elements::chat_content::ToolCall;
    use crate::event::{DispatchedEvent, ModifiersState};
    use crate::test_util::render_element;
    use goble_core::harness::ToolCallStatus;

    let app = AppContext::default();
    let call = |id: &str, path: &str| ToolCall {
        id: id.to_string(),
        name: "read_file".to_string(),
        arguments: format!(r#"{{"path":"{path}"}}"#),
        status: ToolCallStatus::Finished,
        result: Some(
            (1..=12)
                .map(|i| format!("{path} line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        diff: None,
    };
    let messages = vec![ChatMessage::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(vec![call("call_a", "src/a.rs"), call("call_b", "src/b.rs")])];
    let fold = Rc::new(RefCell::new(HashMap::new()));

    let build = |fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>| -> Box<dyn Element> {
        ChatView::new()
            .with_messages(messages.clone())
            .with_tool_fold(fold.clone())
            .finish()
    };
    let press = |view: &mut Box<dyn Element>, key: &str| {
        view.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: key.to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut crate::elements::EventContext::default(),
            &app,
        );
    };
    let drawn = |view: &mut Box<dyn Element>| -> String {
        render_element(view, vec2f(600.0, 800.0), &app)
            .iter()
            .filter_map(|command| match command {
                crate::render::RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    };

    let mut view = build(&fold);
    let folded = drawn(&mut view);
    assert!(
        folded.contains("Read 2 files"),
        "the two reads start as one folded run: {folded}"
    );
    assert!(
        !folded.contains("src/a.rs"),
        "the folded run hides its members: {folded}"
    );

    // Right opens the run: its member rows appear, bodies still folded.
    press(&mut view, "ArrowRight");
    let mut view = build(&fold);
    let run = drawn(&mut view);
    assert!(
        run.contains("src/a.rs") && run.contains("src/b.rs"),
        "the run opens to its members: {run}"
    );
    assert!(
        !run.contains("src/b.rs line 7"),
        "opening the run does not open the call's body: {run}"
    );

    // The next Right opens the newest call in full.
    press(&mut view, "ArrowRight");
    let mut view = build(&fold);
    let opened = drawn(&mut view);
    assert!(
        opened.contains("src/b.rs line 7"),
        "the newest call opens whole: {opened}"
    );
    assert!(
        !opened.contains("src/a.rs line 7"),
        "the older call stays folded: {opened}"
    );

    // Left closes that call, then the run.
    press(&mut view, "ArrowLeft");
    let mut view = build(&fold);
    let closed = drawn(&mut view);
    assert!(
        !closed.contains("src/b.rs line 7") && closed.contains("src/b.rs"),
        "Left closes the call's body, not the run: {closed}"
    );

    press(&mut view, "ArrowLeft");
    let mut view = build(&fold);
    let folded_again = drawn(&mut view);
    assert!(
        folded_again.contains("Read 2 files") && !folded_again.contains("src/a.rs"),
        "Left closes the run back to its header: {folded_again}"
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
        diff: None,
    };
    let messages =
        vec![ChatMessage::new(ChatRole::Assistant, Vec::new()).with_tool_calls(vec![call])];
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
/// and the only bordered box is the shared terminal command block. The
/// approval card that closes a suspended turn is a control block of its own,
/// so it is checked apart from the row lock.
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
    let call =
        |name: &str, arguments: &str, status: ToolCallStatus, result: Option<&str>| ToolCall {
            id: format!("call_{name}"),
            name: name.to_string(),
            arguments: arguments.to_string(),
            status,
            result: result.map(str::to_string),
            diff: None,
        };

    let read = call(
        "read_file",
        r#"{"path":"src/lib.rs","start_line":1,"end_line":3}"#,
        ToolCallStatus::Finished,
        Some("1: one\n2: two"),
    );
    let command = call(
        "run_command",
        r#"{"command":"cargo test"}"#,
        ToolCallStatus::Error,
        Some("2 tests failed"),
    );
    let edit = call(
        "edit_file",
        r#"{"path":"src/lib.rs","old_text":"let a = 1;","new_text":"let a = 2;"}"#,
        ToolCallStatus::Finished,
        Some("edited"),
    );
    let in_flight = call(
        "codebase_search",
        r#"{"pattern":"fn main","path":"src"}"#,
        ToolCallStatus::Running,
        None,
    );
    let spawn = call(
        "spawn_subagent",
        r#"{"subagent_type":"reviewer","input":"audit the migration"}"#,
        ToolCallStatus::Running,
        None,
    );

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

    // The user's rounded bubble is the deliberate exception to the no-pill
    // rule; this test is about every agent row around it.
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![
            ChatFragment::reasoning("step-0", "Thinking", "the build fails on the parser", true),
            ChatFragment::text("Reading the parser's own row, then running the tests."),
        ],
    )
    .with_tool_calls(calls)];

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
        // An interrupted turn: the agent asked a question and another prompt
        // is queued behind it.
        .with_pending_ask(Some(AskUserUi::new(
            "Which environment?",
            vec!["staging".to_string(), "production".to_string()],
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

    // The approval card is a control block, the way the terminal command block
    // is: it closes the same transcript and brings its own decision buttons.
    let mut with_card = ChatView::new()
        .with_messages(vec![ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Running the suite.")],
        )])
        .with_command_proposal(Some(CommandProposalUi::new(
            "cmd-1",
            vec!["cargo test".to_string()],
            "~/Projects/goble",
        )))
        .without_composer()
        .finish();
    let drawn: Vec<String> = render_element(&mut with_card, vec2f(600.0, 400.0), &app)
        .iter()
        .filter_map(|command| match command {
            crate::render::RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    for expected in [
        "OK if I run this command and read the output?",
        "cargo test",
        "Run",
    ] {
        assert!(
            drawn.iter().any(|text| text == expected),
            "the transcript closes with the approval card drawing {expected:?}, got {drawn:?}"
        );
    }
}

/// The child takeover draws no work dock, so the spinner on the sub-agent card
/// and in its title band is the only animation there. It keeps the frame clock
/// running only while the child still works.
#[test]
fn a_live_sub_agent_keeps_the_transcript_animating() {
    use std::collections::HashMap;
    use std::time::Duration;

    use crate::elements::chat_content::{SubAgentRow, SubAgentRowStatus, ToolCall};
    use goble_core::harness::ToolCallStatus;

    let spawn = ToolCall {
        id: "call_subagent".to_string(),
        name: "spawn_subagent".to_string(),
        arguments: r#"{"subagent_type":"reviewer","input":"audit the migration"}"#.to_string(),
        status: ToolCallStatus::Running,
        result: None,
        diff: None,
    };
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("Auditing the migration.")],
    )
    .with_tool_calls(vec![spawn.clone()])];
    let record = |status| SubAgentRow {
        child_id: "conv-child-1".to_string(),
        subagent_type: "reviewer".to_string(),
        description: "audit the migration".to_string(),
        status,
        activity: "reading the schema".to_string(),
        elapsed: Duration::from_millis(4500),
        turns: 2,
        tool_calls: 4,
        tokens: 1234,
        outcome: None,
        background: true,
    };
    let view = |row| {
        ChatView::new()
            .with_messages(messages.clone())
            .with_sub_agents(HashMap::from([(spawn.id.clone(), row)]))
            .without_composer()
            .finish()
    };

    assert!(
        view(record(SubAgentRowStatus::Running)).wants_animation(),
        "a running child keeps the spinner turning"
    );
    assert!(
        !view(record(SubAgentRowStatus::Completed)).wants_animation(),
        "a finished child leaves the pane at rest"
    );
    assert!(
        !ChatView::new().with_messages(messages).wants_animation(),
        "no child record asks for no frames"
    );
}

/// The pending ask closes the transcript as grok-new's "Agent questions" band:
/// a titled header with Skip all and Next, then the question and one numbered
/// row per answer, ending in "Other…".
#[test]
fn the_pending_ask_closes_the_transcript_as_the_agent_questions_band() {
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let mut view = ChatView::new()
        .with_messages(vec![ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Which host should I query?")],
        )])
        .with_pending_ask(Some(AskUserUi::new(
            "Which environment?",
            vec!["staging".to_string(), "production".to_string()],
        )))
        .without_composer()
        .finish();
    let drawn: Vec<String> = render_element(&mut view, vec2f(600.0, 500.0), &app)
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();

    for expected in [
        "Agent questions",
        "Skip all",
        "Next",
        "Which environment?",
        "1",
        "staging",
        "2",
        "production",
        "3",
        "Other...",
    ] {
        assert!(
            drawn.iter().any(|text| text == expected),
            "the transcript closes with the ask panel drawing {expected:?}, got {drawn:?}"
        );
    }
    assert!(
        !drawn
            .iter()
            .any(|text| text == "Agent asks" || text == "Send answer"),
        "the old ask surface is gone: {drawn:?}"
    );
}

/// While the approval card waits, the whole rich input is gone: the card is
/// the pane's only typing surface until Run or Reject settles the decision.
#[test]
fn a_pending_command_hides_the_rich_input() {
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let mut view = ChatView::new()
        .with_messages(vec![ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Running the suite.")],
        )])
        .with_composer_focused(true)
        .with_composer_value("a half-written prompt")
        .with_composer_caret(Rc::new(RefCell::new(
            "a half-written prompt".chars().count(),
        )))
        .with_command_proposal(Some(CommandProposalUi::new(
            "cmd-1",
            vec!["cargo test".to_string()],
            "~/Projects/goble",
        )))
        .finish();
    let drawn: Vec<String> = render_element(&mut view, vec2f(600.0, 500.0), &app)
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    for hidden in [
        "Ask anything...",
        "Waiting for your decision…",
        "Message the agent while it waits…",
        "a half-written prompt",
    ] {
        assert!(
            !drawn.iter().any(|text| text == hidden),
            "the rich input is not drawn at all while the card waits ({hidden:?}): {drawn:?}"
        );
    }
    assert!(
        drawn
            .iter()
            .any(|text| text == "OK if I run this command and read the output?"),
        "the approval card is the only input surface: {drawn:?}"
    );
}

/// The question panel takes the same slot as the approval card: while it waits
/// the rich input is gone, so a digit pressed at the pane picks an answer
/// instead of being typed into a composer.
#[test]
fn a_pending_ask_hides_the_rich_input_and_takes_the_digits() {
    use crate::elements::EventContext;
    use crate::event::{DispatchedEvent, ModifiersState};
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let ask = AskUserUi::new(
        "Which environment?",
        vec!["staging".to_string(), "production".to_string()],
    );
    let probe = ask.clone();
    let mut view = ChatView::new()
        .with_messages(vec![ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Ready when you are.")],
        )])
        // The composer held the keyboard before the question arrived.
        .with_composer_focused(true)
        .with_composer_value("a half-written prompt")
        .with_pending_ask(Some(ask))
        .finish();
    let drawn: Vec<String> = render_element(&mut view, vec2f(600.0, 500.0), &app)
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    for hidden in ["Ask anything...", "a half-written prompt"] {
        assert!(
            !drawn.iter().any(|text| text == hidden),
            "the rich input is not drawn while the question waits ({hidden:?}): {drawn:?}"
        );
    }
    assert!(
        drawn.iter().any(|text| text == "Agent questions"),
        "the question panel is the only input surface: {drawn:?}"
    );

    let digit = DispatchedEvent::KeyDown {
        key: "1".to_string(),
        modifiers: ModifiersState::none(),
    };
    assert!(
        view.dispatch_event(&digit, &mut EventContext::default(), &app),
        "the panel takes the digit"
    );
    assert_eq!(
        probe.selected_index(),
        Some(0),
        "the digit picks the first answer instead of typing into a composer"
    );
}

/// The card keeps the keys while it waits, whatever the composer's last focus
/// state was: typing edits the command Enter will run.
#[test]
fn a_pending_command_keeps_the_keys_on_the_card() {
    use crate::elements::EventContext;
    use crate::event::{DispatchedEvent, ModifiersState};
    use crate::test_util::render_element;

    let app = AppContext::default();
    let changed: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let log = changed.clone();
    let draft = Rc::new(RefCell::new("cargo test".to_string()));
    let mut view = ChatView::new()
        .with_messages(vec![ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Running the suite.")],
        )])
        // The composer held the keyboard before the proposal arrived.
        .with_composer_focused(true)
        .with_composer_value("a half-written prompt")
        .with_composer_caret(Rc::new(RefCell::new(
            "a half-written prompt".chars().count(),
        )))
        .with_composer_on_change(move |text| log.borrow_mut().push(text))
        .with_command_proposal(Some(CommandProposalUi::new(
            "cmd-1",
            vec!["cargo test".to_string()],
            "~/Projects/goble",
        )))
        .with_command_proposal_draft(
            draft.clone(),
            Rc::new(RefCell::new("cargo test".chars().count())),
            Rc::new(RefCell::new(ScrollState::default())),
        )
        .finish();
    render_element(&mut view, vec2f(600.0, 500.0), &app);

    view.dispatch_event(
        &DispatchedEvent::KeyDown {
            // `e` is also the transcript's fold key; the card owns it while a
            // command waits, so it must land in the command field.
            key: "e".to_string(),
            modifiers: ModifiersState::none(),
        },
        &mut EventContext::default(),
        &app,
    );

    assert!(
        changed.borrow().is_empty(),
        "the hidden composer takes no draft: {:?}",
        changed.borrow()
    );
    assert_eq!(
        draft.borrow().as_str(),
        "cargo teste",
        "the keys belong to the card's command"
    );
}

/// The transcript's own rows keep a real tool call drawn in every fold, so the
/// pane-level pill lock above is not vacuous.
#[test]
fn the_transcript_draws_the_tool_rows_the_lock_covers() {
    use crate::elements::chat_content::ToolCall;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;
    use goble_core::harness::ToolCallStatus;

    let app = AppContext::default();
    let read = ToolCall {
        id: "call_read".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"src/lib.rs"}"#.to_string(),
        status: ToolCallStatus::Finished,
        result: Some("1: fn main() {}".to_string()),
        diff: None,
    };
    let edit = ToolCall {
        id: "call_edit".to_string(),
        name: "edit_file".to_string(),
        arguments: r#"{"path":"src/main.rs","old_text":"a","new_text":"b"}"#.to_string(),
        status: ToolCallStatus::Running,
        result: None,
        diff: None,
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
    for expected in ["Reading 1 file, Editing 1 file +1/-1"] {
        assert!(
            texts.iter().any(|text| text == expected),
            "the transcript draws {expected:?}, got {texts:?}"
        );
    }
    assert!(
        !texts
            .iter()
            .any(|text| text.contains('◈') || text.contains('◆') || text.contains('❯')),
        "the transcript's tool rows draw no glyph, got {texts:?}"
    );
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
    // Remote chat removed the footer's still-running cue; the view must not
    // draw it any more.
    assert!(
        !commands.iter().any(|c| match c {
            crate::render::RenderCommand::DrawText { text, .. } => text.contains("still running"),
            _ => false,
        }),
        "the removed still-running cue is not drawn"
    );

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

/// The transcript's closing footer is DeepSeek Harness' completed-turn usage
/// area: a muted `database` pill reading `Usage 490 tok` beside the Fork action
/// (the DSH branch glyph), both 28 px rows that rest without a band and band
/// under the pointer. A press opens the details panel above the pill; the panel
/// carries only the fields the provider reported and lists the Goble additions
/// (Consumed = spend, Total = call size) rather than inventing DSH's routes,
/// cache write or reasoning buckets.
#[test]
fn the_transcript_footer_shows_fork_and_the_reported_token_usage() {
    use crate::elements::interactive::contains;
    use crate::elements::{HoverChipLayer, LayoutContext, PaintContext, SizeConstraint};
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
        let _ = view.layout(SizeConstraint::loose(size), &mut LayoutContext, app);
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

    /// The frame's last layer: the floating boxes queued during paint (the
    /// usage panel rides this, above the transcript's clip).
    fn paint_layer(app: &AppContext) -> Vec<RenderCommand> {
        let mut ctx = PaintContext::new(Renderer::new());
        HoverChipLayer::new().paint(vec2f(0.0, 0.0), &mut ctx, app);
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

    fn icons(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
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

    // At rest: DSH's pill and glyph, and neither affordance is a filled control.
    let rest = paint_with(&mut view, None, &app);
    let drawn = texts(&rest);
    let drawn_icons = icons(&rest);
    assert!(
        drawn.iter().any(|t| t == "Fork"),
        "no fork affordance: {drawn:?}"
    );
    assert!(
        drawn.iter().any(|t| t == "Usage 490 tok"),
        "the pill reads the conversation's spend in DSH's idiom: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|t| t == "490 tokens"),
        "the old accent readout is gone: {drawn:?}"
    );
    assert!(
        !drawn
            .iter()
            .any(|t| t.contains("cached") || t.contains("Input")),
        "the details panel stays closed until asked for: {drawn:?}"
    );
    assert!(
        drawn_icons.iter().any(|name| name == "branch"),
        "the Fork row carries the DSH branch glyph: {drawn_icons:?}"
    );
    assert!(
        drawn_icons.iter().any(|name| name == "database"),
        "the pill carries the usage glyph: {drawn_icons:?}"
    );
    assert!(
        !drawn_icons.iter().any(|name| name == "activity"),
        "the old activity glyph is gone from the footer: {drawn_icons:?}"
    );
    assert!(
        !drawn_icons.iter().any(|name| name == "git-branch"),
        "the git-graph glyph is not the fork mark: {drawn_icons:?}"
    );

    let fork_origin = text_origin(&rest, "Fork");
    let usage_origin = text_origin(&rest, "Usage 490 tok");
    assert_eq!(
        fork_origin.y, usage_origin.y,
        "the two affordances are one closing row"
    );
    let probe = |origin: Vector2F| origin + vec2f(1.0, 1.0);
    let resting = bands(&rest);
    assert!(
        !resting
            .iter()
            .any(|band| contains(*band, probe(fork_origin))),
        "a resting fork draws no band: {resting:?}"
    );
    assert!(
        !resting
            .iter()
            .any(|band| contains(*band, probe(usage_origin))),
        "a resting usage pill draws no band: {resting:?}"
    );

    // The pointer over one draws that one's 28 px band, and the other keeps none.
    let _ = paint_layer(&app);
    let over_fork = paint_with(&mut view, Some(probe(fork_origin)), &app);
    let fork_band = band_at(&rest, &over_fork, probe(fork_origin));
    assert!(
        (fork_band.height() - 28.0).abs() < 0.05,
        "the band is DSH's 28 px pill, got {}",
        fork_band.height()
    );
    assert!(
        !contains(fork_band, probe(usage_origin)),
        "the band belongs to the fork row: {fork_band:?}"
    );
    assert_eq!(
        bands(&over_fork)
            .into_iter()
            .find(|band| contains(*band, probe(usage_origin))),
        None,
        "the fork's band does not reach the usage pill"
    );

    let _ = paint_layer(&app);
    let over_usage = paint_with(&mut view, Some(probe(usage_origin)), &app);
    let usage_band = band_at(&rest, &over_usage, probe(usage_origin));
    assert_eq!(usage_band.height(), 28.0);
    assert!(
        !contains(usage_band, probe(fork_origin)),
        "the band belongs to the usage row: {usage_band:?}"
    );

    // Expanding the disclosure reveals DSH's accounting panel above the pill.
    *open.borrow_mut() = true;
    let _expanded = paint_with(&mut view, None, &app);
    assert!(
        app.hover_chips.borrow().len() == 1,
        "the open panel queued itself for the frame's last layer"
    );
    let layer = paint_layer(&app);
    assert!(
        app.hover_chips.borrow().is_empty(),
        "the last layer drains the queue every frame"
    );
    let drawn = texts(&layer);
    for needle in [
        "Session usage",
        "Cache hit",
        "64.8%",
        "Input (uncached)",
        "434",
        "Cache read",
        "800",
        "Output",
        "56",
        "Session total",
        "1,290",
    ] {
        assert!(
            drawn.iter().any(|text| text == needle),
            "the panel shows {needle:?}: {drawn:?}"
        );
    }
    // The panel hangs above the pill, DSH's anchor with its 8 px gap.
    let panel = fills(&layer)
        .into_iter()
        .find(|rect| rect.width() >= 300.0 && rect.height() > 100.0)
        .expect("the panel's raised surface is drawn");
    assert!(
        panel.max_y() <= usage_origin.y - 8.0,
        "the panel opens above the pill: {panel:?} against {usage_origin:?}"
    );
    assert!(
        panel.min_y() >= 0.0,
        "and inside the pane's top edge: {panel:?}"
    );
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
        !drawn.iter().any(|t| t.contains("tok")),
        "no fabricated zero: {drawn:?}"
    );
    let usage_origin = text_origin(&rest, "no usage yet");
    let over_usage = paint_with(&mut bare, Some(probe(usage_origin)), &app);
    assert_eq!(
        bands(&over_usage)
            .into_iter()
            .find(|band| contains(*band, probe(usage_origin))),
        None,
        "the empty slot is a reading, not a control: no hover band"
    );
}

/// The details panel names only the fields the provider actually reported: a
/// provider with no cache accounting gets the plain `Input` label and no cache
/// rows (unknown is not a miss), while a reported miss is drawn as `0%` / `0`.
#[test]
fn the_details_panel_names_only_the_fields_the_provider_reported() {
    use crate::elements::{HoverChipLayer, LayoutContext, PaintContext, SizeConstraint};
    use crate::render::{RenderCommand, Renderer};
    use goble_core::llm::TokenUsage;

    fn paint_panel(usage: TokenUsage) -> Vec<String> {
        let app = AppContext::default();
        let open = std::rc::Rc::new(std::cell::RefCell::new(true));
        let mut view = ChatView::new()
            .with_messages(vec![ChatMessage::new(
                ChatRole::Assistant,
                vec![ChatFragment::text("Hi")],
            )])
            .with_usage(usage)
            .with_usage_open(open)
            .with_on_fork(|| {})
            .finish();
        let size = vec2f(600.0, 800.0);
        let _ = view.layout(SizeConstraint::loose(size), &mut LayoutContext, &app);
        let mut ctx = PaintContext::new(Renderer::new());
        view.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        let mut layer_ctx = PaintContext::new(Renderer::new());
        HoverChipLayer::new().paint(vec2f(0.0, 0.0), &mut layer_ctx, &app);
        layer_ctx
            .renderer
            .take()
            .map(|renderer| {
                renderer
                    .commands()
                    .iter()
                    .filter_map(|command| match command {
                        RenderCommand::DrawText { text, .. } => Some(text.clone()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    let unknown = paint_panel(TokenUsage {
        input: 1_234,
        cached: None,
        output: 56,
    });
    for needle in [
        "Session usage",
        "Input",
        "1,234",
        "Output",
        "56",
        "Session total",
        "1,290",
    ] {
        assert!(
            unknown.iter().any(|text| text == needle),
            "the uncached panel shows {needle:?}: {unknown:?}"
        );
    }
    for gone in [
        "Cache hit",
        "Cached input",
        "Cache read",
        "Uncached input",
        "Cache write",
        "Provider / model",
    ] {
        assert!(
            !unknown.iter().any(|text| text == gone),
            "the panel does not invent {gone:?}: {unknown:?}"
        );
    }

    // A reported miss is a figure, not an omission.
    let reported = paint_panel(TokenUsage {
        input: 1_000,
        cached: Some(0),
        output: 10,
    });
    for needle in ["Cache hit", "0%", "Cache read"] {
        assert!(
            reported.iter().any(|text| text == needle),
            "a reported cache miss shows {needle:?}: {reported:?}"
        );
    }
}

/// The pill and its panel are drawn from theme tokens, so the same anatomy
/// reads on the light palette as on the default dark one.
#[test]
fn the_usage_dialog_shows_per_call_and_session_numbers() {
    use crate::elements::{HoverChipLayer, LayoutContext, PaintContext, SizeConstraint};
    use crate::render::{RenderCommand, Renderer};
    use goble_core::llm::TokenUsage;

    let app = AppContext::default();
    let open = Rc::new(RefCell::new(false));
    let mut view = ChatView::new()
        .with_messages(vec![ChatMessage::new(
            ChatRole::User,
            vec![ChatFragment::text("hello")],
        )])
        .with_usage(TokenUsage {
            input: 900,
            cached: Some(600),
            output: 100,
        })
        .with_usage_detail(
            Some(TokenUsage {
                input: 500,
                cached: Some(400),
                output: 20,
            }),
            3,
            Some(8_000),
        )
        .with_usage_open(open.clone())
        .finish();
    let _ = view.layout(
        SizeConstraint::loose(vec2f(700.0, 800.0)),
        &mut LayoutContext::default(),
        &app,
    );
    *open.borrow_mut() = true;
    let _ = view.layout(
        SizeConstraint::loose(vec2f(700.0, 800.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut ctx = PaintContext::new(Renderer::new());
    view.paint(vec2f(0.0, 0.0), &mut ctx, &app);
    let _ = ctx.renderer.take();
    let mut layer_ctx = PaintContext::new(Renderer::new());
    HoverChipLayer::new().paint(vec2f(0.0, 0.0), &mut layer_ctx, &app);
    let layer = layer_ctx
        .renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default();
    let drawn: Vec<String> = layer
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    for needle in [
        "Session usage",
        "Calls",
        "3",
        "Context",
        "500 / 8K · 6.2%",
        "Last call",
        "Session total",
        "1,000",
    ] {
        assert!(
            drawn.iter().any(|text| text == needle),
            "the dialog shows {needle:?}: {drawn:?}"
        );
    }
    assert!(
        drawn.iter().any(|text| text.contains("per model call")),
        "the per-call attribution is stated: {drawn:?}"
    );
}

#[test]
fn the_usage_dialog_says_when_numbers_are_unknown() {
    use crate::elements::{HoverChipLayer, LayoutContext, PaintContext, SizeConstraint};
    use crate::render::{RenderCommand, Renderer};
    use goble_core::llm::TokenUsage;

    let app = AppContext::default();
    let open = Rc::new(RefCell::new(false));
    let mut view = ChatView::new()
        .with_messages(vec![ChatMessage::new(
            ChatRole::User,
            vec![ChatFragment::text("hello")],
        )])
        .with_usage(TokenUsage {
            input: 10,
            cached: None,
            output: 2,
        })
        .with_usage_open(open.clone())
        .finish();
    let _ = view.layout(
        SizeConstraint::loose(vec2f(700.0, 800.0)),
        &mut LayoutContext::default(),
        &app,
    );
    *open.borrow_mut() = true;
    let _ = view.layout(
        SizeConstraint::loose(vec2f(700.0, 800.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut ctx = PaintContext::new(Renderer::new());
    view.paint(vec2f(0.0, 0.0), &mut ctx, &app);
    let _ = ctx.renderer.take();
    let mut layer_ctx = PaintContext::new(Renderer::new());
    HoverChipLayer::new().paint(vec2f(0.0, 0.0), &mut layer_ctx, &app);
    let layer = layer_ctx
        .renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default();
    let drawn: Vec<String> = layer
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        drawn.iter().filter(|text| *text == "—").count() >= 2,
        "unknown model/context/calls read as an honest dash: {drawn:?}"
    );
    assert!(
        drawn
            .iter()
            .any(|text| text.contains("not reported") || text.contains("No model call")),
        "the dialog names what the host does not report: {drawn:?}"
    );
}

#[test]
fn the_usage_area_follows_the_light_theme_too() {
    use crate::elements::{HoverChipLayer, LayoutContext, PaintContext, SizeConstraint};
    use crate::render::{RenderCommand, Renderer};
    use goble_core::llm::TokenUsage;

    let app = AppContext {
        theme: crate::theme::Theme::light(),
        ..AppContext::default()
    };
    let open = std::rc::Rc::new(std::cell::RefCell::new(true));
    let mut view: Box<dyn Element> = ChatView::new()
        .with_messages(vec![ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Hi")],
        )])
        .with_usage(TokenUsage {
            input: 1_234,
            cached: Some(800),
            output: 56,
        })
        .with_usage_open(open)
        .with_on_fork(|| {})
        .finish();
    let _ = view.layout(
        SizeConstraint::loose(vec2f(600.0, 800.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut ctx = PaintContext::new(Renderer::new());
    view.paint(vec2f(0.0, 0.0), &mut ctx, &app);
    let _ = ctx
        .renderer
        .take()
        .map(|renderer| renderer.commands().to_vec());

    let mut layer_ctx = PaintContext::new(Renderer::new());
    HoverChipLayer::new().paint(vec2f(0.0, 0.0), &mut layer_ctx, &app);
    let layer = layer_ctx
        .renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default();
    let drawn: Vec<String> = layer
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        drawn.iter().any(|text| text == "Session usage"),
        "{drawn:?}"
    );
    assert!(drawn.iter().any(|text| text == "64.8%"), "{drawn:?}");
    assert!(
        layer.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { color, .. }
                if *color == app.theme.color(ColorToken::SurfaceRaised)
        )),
        "the light theme's raised surface is the panel fill"
    );
    assert!(
        layer.iter().any(|command| matches!(
            command,
            RenderCommand::DrawText { color, .. } if *color == app.theme.color(ColorToken::Text)
        )),
        "the panel's values use the light theme's text colour"
    );
}

/// The usage disclosure closes without a second press on the pill: an outside
/// pointer press, Escape (consumed only while open) and a scroll of its anchor
/// each put it away.
#[test]
fn the_usage_popover_closes_without_a_second_click() {
    use crate::elements::{Element, EventContext, LayoutContext, PaintContext, SizeConstraint};
    use crate::event::{DispatchedEvent, ModifiersState};
    use crate::render::Renderer;
    use goble_core::llm::TokenUsage;

    let app = AppContext::default();
    let open = std::rc::Rc::new(std::cell::RefCell::new(true));
    let mut view: Box<dyn Element> = ChatView::new()
        .with_messages(vec![ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Hi")],
        )])
        .with_usage(TokenUsage {
            input: 1_234,
            cached: Some(800),
            output: 56,
        })
        .with_usage_open(open.clone())
        .with_on_fork(|| {})
        .finish();
    let size = vec2f(600.0, 800.0);
    let _ = view.layout(
        SizeConstraint::loose(size),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    view.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let mut ctx = EventContext;

    // A press anywhere outside the panel and the pill closes it, and keeps its
    // own meaning.
    assert!(!view.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: vec2f(10.0, 40.0),
            button: 0,
        },
        &mut ctx,
        &app,
    ));
    assert!(!*open.borrow(), "an outside press closes the panel");

    *open.borrow_mut() = true;
    let _ = view.layout(
        SizeConstraint::loose(size),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    view.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let handled = view.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: "Escape".to_string(),
            modifiers: ModifiersState::default(),
        },
        &mut ctx,
        &app,
    );
    assert!(handled, "the open panel consumes Escape");
    assert!(!*open.borrow(), "and closes itself");

    *open.borrow_mut() = true;
    let _ = view.layout(
        SizeConstraint::loose(size),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    view.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let handled = view.dispatch_event(
        &DispatchedEvent::Scroll {
            delta: vec2f(0.0, -10.0),
            precise: false,
        },
        &mut ctx,
        &app,
    );
    assert!(!handled, "the wheel keeps its own meaning");
    assert!(!*open.borrow(), "the panel closes when its anchor scrolls");
}

/// The pure formatting helpers: DSH's compact counts and its cache-hit share,
/// which never rounds a partial hit to 100%.
#[test]
fn usage_formatting_helpers_match_the_reference() {
    use super::transcript::{cache_hit_percent, compact_tokens};

    assert_eq!(compact_tokens(490), "490");
    assert_eq!(compact_tokens(12_345), "12.3K");
    assert_eq!(compact_tokens(517_000), "517K");
    assert_eq!(compact_tokens(1_234_567), "1.2M");

    assert_eq!(cache_hit_percent(0, 0), None, "no prompt input, no share");
    assert_eq!(cache_hit_percent(1_234, 1_234), Some("100".to_string()));
    assert_eq!(cache_hit_percent(0, 1_000), Some("0%".to_string()));
    assert_eq!(
        cache_hit_percent(999_999, 1_000_000),
        Some("99.9".to_string()),
        "a partial hit never rounds up to a full one"
    );
    assert_eq!(cache_hit_percent(800, 1_234), Some("64.8%".to_string()));
}

/// The closing row waits for the turn: while the agent is still working, or
/// suspended on a command decision, the conversation has no fork and no token
/// count to read yet.
#[test]
fn the_footer_waits_until_the_turn_finishes() {
    use crate::elements::{TurnActivity, TurnStatus};
    use crate::render::RenderCommand;
    use crate::test_util::render_element;
    use goble_core::llm::TokenUsage;

    let app = AppContext::default();
    let usage = TokenUsage {
        input: 1_234,
        cached: Some(800),
        output: 56,
    };
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("Working on it")],
    )];
    let drawn = |view: &mut Box<dyn Element>| -> Vec<String> {
        render_element(view, vec2f(600.0, 480.0), &app)
            .into_iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    };

    let mut busy = ChatView::new()
        .with_messages(messages.clone())
        .with_usage(usage)
        .with_on_fork(|| {})
        .with_turn_status(TurnStatus::busy(TurnActivity::Thinking, None, 0))
        .finish();
    let labels = drawn(&mut busy);
    assert!(
        !labels
            .iter()
            .any(|text| text == "Fork" || text.contains("tokens")),
        "a running turn draws no closing row: {labels:?}"
    );

    let mut waiting = ChatView::new()
        .with_messages(messages.clone())
        .with_usage(usage)
        .with_on_fork(|| {})
        .with_command_proposal(Some(CommandProposalUi::new(
            "call-1",
            vec!["git status".to_string()],
            "~/Projects/goble",
        )))
        .finish();
    let labels = drawn(&mut waiting);
    assert!(
        !labels
            .iter()
            .any(|text| text == "Fork" || text.contains("tokens")),
        "a pending decision draws no closing row: {labels:?}"
    );

    // A stale disclosure flag is put away while the footer is not drawn, so a
    // turn that ends later does not open a panel nobody asked for.
    let open = std::rc::Rc::new(std::cell::RefCell::new(true));
    let mut busy = ChatView::new()
        .with_messages(messages.clone())
        .with_usage(usage)
        .with_usage_open(open.clone())
        .with_on_fork(|| {})
        .with_turn_status(TurnStatus::busy(TurnActivity::Thinking, None, 0))
        .finish();
    let _ = drawn(&mut busy);
    assert!(
        !*open.borrow(),
        "a footer-less frame resets the disclosure flag"
    );

    let mut done = ChatView::new()
        .with_messages(messages)
        .with_usage(usage)
        .with_on_fork(|| {})
        .finish();
    let labels = drawn(&mut done);
    assert!(
        labels.iter().any(|text| text == "Fork")
            && labels.iter().any(|text| text == "Usage 490 tok"),
        "once the turn is over the row is back: {labels:?}"
    );
}

/// The instructions the caller hands the view are the composer's **fixed row
/// under the card**, aligned to the right (owner: Enter send, Cmd+Enter send to
/// cloud, tasks… belong below the card). A view handed none draws no strip and
/// no gap where one would have been, and no rule separates the content from the
/// card.
#[test]
fn the_composer_hints_sit_below_the_composer_and_only_when_given() {
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
    // The composer is a floating, borderless card now, so its rounded
    // `Surface` fill is what locates the input block.
    let composer_card = |commands: &[RenderCommand]| -> crate::geometry::RectF {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::Surface)
                    && (*corner_radius - crate::elements::COMPOSER_CARD_RADIUS).abs() < 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the composer draws its card")
    };
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
    let plain_commands = render_element(&mut plain, vec2f(width, 520.0), &app);
    for text in ["commands", "tasks", "⌘"] {
        assert!(
            line_of(&plain_commands, text).is_none(),
            "a view with no hints draws no strip ({text:?})"
        );
    }
    assert!(
        rules(&plain_commands).is_empty(),
        "no separator divides the input from the content: {:?}",
        rules(&plain_commands)
    );
    let plain_card = composer_card(&plain_commands);

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

    // The strip is **under** the card now, and over nothing but its own row.
    let card = composer_card(&commands);
    let editor = line_of(&commands, "Ask anything...").expect("the editor is drawn");
    assert!(
        strip > card.max_y(),
        "the strip is under the input card: strip {strip}, card bottom {}",
        card.max_y()
    );
    assert!(
        card.min_y() < editor && editor < card.max_y(),
        "the editor stays inside the card: card {card:?}, editor {editor}"
    );
    // Right-aligned: the first hint's label starts in the right half.
    let strip_x = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. } if text == "commands" => Some(origin.x),
            _ => None,
        })
        .expect("the strip is drawn");
    assert!(
        strip_x > width / 2.0,
        "the hint row is aligned right: {strip_x} of {width}"
    );
    assert!(
        rules(&commands).is_empty(),
        "no rule separates the strip from the composer: {:?}",
        rules(&commands)
    );
    // The hint row is outside the card: the card keeps exactly the height it
    // had in the view handed no hints.
    assert!(
        (card.height() - plain_card.height()).abs() < 0.5,
        "the hint row does not change the card's height: {} against {}",
        card.height(),
        plain_card.height()
    );
}

/// The fixed row is right-aligned and pinned under the card, not inside it:
/// the card's bottom edge is above the caps and the caps' trailing edge sits at
/// the card's trailing gutter, so no hint can grow the card.
#[test]
fn the_hint_row_sits_under_the_card_on_the_right() {
    use crate::elements::ShortcutHint;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let width = 600.0;
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("Hi")],
    )];
    let build = |hints: Vec<ShortcutHint>| {
        ChatView::new()
            .with_messages(messages.clone())
            .with_composer_hints(hints)
            .finish()
    };
    let mut empty = build(Vec::new());
    let empty_commands = render_element(&mut empty, vec2f(width, 520.0), &app);
    let mut stated = build(vec![
        ShortcutHint::new(&["⌘", "K"], "commands"),
        ShortcutHint::new(&["⌘", "⇧", "W"], "tasks"),
    ]);
    let commands = render_element(&mut stated, vec2f(width, 520.0), &app);

    let card_of = |commands: &[RenderCommand]| -> crate::geometry::RectF {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::Surface)
                    && (*corner_radius - crate::elements::COMPOSER_CARD_RADIUS).abs() < 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the composer draws its card")
    };
    let stated_card = card_of(&commands);
    let empty_card = card_of(&empty_commands);
    assert!(
        (stated_card.height() - empty_card.height()).abs() < 0.5,
        "the row is outside the card, so the card keeps its height: {} against {}",
        stated_card.height(),
        empty_card.height()
    );
    // 424: hints are plain icons/text, so their bounds come from the key
    // glyphs rather than a cap stroke.
    let caps: Vec<crate::geometry::RectF> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawIcon {
                origin, name, size, ..
            } if name.starts_with("key-") => {
                Some(crate::geometry::rectf(origin.x, origin.y, *size, *size))
            }
            _ => None,
        })
        .collect();
    assert!(
        !caps.is_empty(),
        "the strip draws its key glyphs: {commands:?}"
    );
    let caps_top = caps
        .iter()
        .map(|rect| rect.min_y())
        .fold(f32::INFINITY, f32::min);
    let caps_right = caps
        .iter()
        .map(|rect| rect.max_x())
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        caps_top > stated_card.max_y(),
        "the caps are under the card: caps {caps_top}, card bottom {}",
        stated_card.max_y()
    );
    assert!(
        caps_right > width - 80.0,
        "the caps sit in the row's trailing half: {caps_right} of {width}"
    );
}

/// The composer card is the pane's bottom block and no rule sits between it
/// and the content: the floating input is its own surface (the old separator
/// over the rich input is gone), and it pins to the pane's bottom edge.
#[test]
fn the_input_card_is_the_bottom_block_with_no_rule_over_it() {
    use crate::elements::ShortcutHint;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let width = 600.0;
    let height = 520.0;
    let mut view = ChatView::new()
        .with_composer_path("~/Projects/goble")
        .with_composer_hints(vec![ShortcutHint::new(&["⌘", "↵"], "new conversation")])
        .finish();
    let commands = render_element(&mut view, vec2f(width, height), &app);

    let editor = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. } if text == "Ask anything..." => {
                Some(origin.y)
            }
            _ => None,
        })
        .expect("the rich input is drawn");
    // No horizontal rule anywhere over the input: the old separator between
    // the content and the rich input is gone.
    let rules: Vec<f32> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, .. }
                if rect.height() <= 2.0 && rect.width() >= width - 0.5 && rect.min_y() < editor =>
            {
                Some(rect.min_y())
            }
            _ => None,
        })
        .collect();
    assert!(
        rules.is_empty(),
        "no rule separates the input from the content: {rules:?}"
    );

    // The composer card's rounded top edge is the input's top edge, and the
    // card pins the pane's bottom.
    let composer_card = commands
        .iter()
        .find_map(|command| match command {
            // The card is a `Surface` fill at the card radius: the composer is
            // borderless now, so the outline that used to locate it is gone.
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::Surface)
                && (*corner_radius - crate::elements::COMPOSER_CARD_RADIUS).abs() < 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the composer draws its card");
    // The card is the block above the fixed instruction row: the row is the
    // last line of the pane, under the card, and nothing paints past the pane.
    assert!(
        composer_card.max_y() > height - 80.0 && composer_card.max_y() < height - 10.0,
        "the input card sits above its instruction row: card {:?} in a {height} tall view",
        composer_card
    );
    let below = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. }
                if origin.y > composer_card.max_y() + 0.5 =>
            {
                Some((text.clone(), *origin))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        below.iter().any(|(text, _)| text == "new conversation"),
        "the instruction row is the only thing under the card: {below:?}"
    );
    assert!(
        below.iter().all(|(_, origin)| origin.y < height),
        "the instruction row stays inside the pane: {below:?}"
    );
    assert!(
        composer_card.min_y() < editor && editor < composer_card.max_y(),
        "the card's top edge is over the editor and its bottom under it: card {:?}, editor {editor}",
        composer_card
    );

    // There is no context band any more: the retired directory control never
    // draws, and the input segment fills the card's content under its casing.
    assert!(
        !commands.iter().any(
            |command| matches!(command, RenderCommand::DrawIcon { name, .. } if name == "folder")
        ),
        "the retired directory band never draws: {commands:?}"
    );
    let segment = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::SurfaceRaised)
                && (*corner_radius - crate::elements::COMPOSER_INPUT_RADIUS).abs() < 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the composer draws its input segment");
    assert!(
        (segment.min_x() - composer_card.min_x() - crate::elements::COMPOSER_CARD_INSET).abs() < 0.5
            && (segment.min_y() - composer_card.min_y() - crate::elements::COMPOSER_CARD_INSET)
                .abs()
                < 0.5,
        "the input segment is the card's one content box: segment {segment:?}, card {composer_card:?}"
    );
}

/// The models the model control opens are drawn exactly where the draft's
/// commands are: one band in the one slot directly over the composer card. The
/// instruction row moved under the card, so both bands sit above the card and
/// the hints sit below it. Which band is up is the composer's open flags, and
/// the slot does not move between them.
#[test]
fn the_model_band_opens_over_the_strip_like_the_command_list() {
    use crate::elements::{
        HoverChipLayer, PaintContext, PopupMenuItem, ShortcutHint, SlashMenuItem,
    };
    use crate::render::{RenderCommand, Renderer};
    use crate::test_util::render_element;
    use std::cell::RefCell;
    use std::rc::Rc;

    let app = AppContext::default();
    let width = 600.0;
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("Hi")],
    )];
    let hints = vec![ShortcutHint::new(&["⌘", "K"], "commands")];
    let models = vec![
        PopupMenuItem::new("gpt-4o").selected(),
        PopupMenuItem::new("o3-mini"),
    ];
    let commands = vec![
        SlashMenuItem::new("help", "List the commands"),
        SlashMenuItem::new("clear", "Clear the transcript"),
    ];
    let line_of = |commands: &[RenderCommand], needle: &str| -> f32 {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == needle => Some(origin.y),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle:?} is drawn"))
    };

    // Closed, the model list is nowhere on the frame: its rows are not drawn.
    let open = Rc::new(RefCell::new(false));
    let index = Rc::new(RefCell::new(0));
    let build = |open: Rc<RefCell<bool>>, index: Rc<RefCell<usize>>| {
        ChatView::new()
            .with_messages(messages.clone())
            .with_composer_hints(hints.clone())
            .with_composer_value("hi")
            .with_composer_model_label("gpt-4o")
            .with_composer_model_menu(models.clone(), open, index, |_| {})
    };
    let mut closed = build(open.clone(), index.clone()).finish();
    let frame = render_element(&mut closed, vec2f(width, 520.0), &app);
    assert!(
        !frame.iter().any(|command| matches!(
            command,
            RenderCommand::DrawText { text, .. } if text == "o3-mini"
        )),
        "a closed model list draws no rows: {frame:?}"
    );

    // Open: the rows are above the composer card; the instruction row is under
    // the card now.
    *open.borrow_mut() = true;
    let mut opened = build(open.clone(), index.clone()).finish();
    let frame = render_element(&mut opened, vec2f(width, 520.0), &app);
    let model_row = line_of(&frame, "o3-mini");
    let strip = line_of(&frame, "commands");
    // The draft the composer was handed, drawn by the editor under the strip.
    let editor = line_of(&frame, "hi");
    // The composer card is the input block under the strip (borderless, so its
    // rounded `Surface` fill is its top edge).
    let card_top = frame
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::Surface)
                && (*corner_radius - crate::elements::COMPOSER_CARD_RADIUS).abs() < 0.01 =>
            {
                Some(rect.min_y())
            }
            _ => None,
        })
        .expect("the composer draws its card");
    assert!(
        model_row < card_top,
        "the list opens above the input card: {model_row} against {card_top}"
    );
    assert!(
        card_top < editor && editor < strip,
        "the editor is inside the card and the instruction row is under it: \
         {card_top} {editor} {strip}"
    );

    // The same view with a command draft: the command list is the anchored
    // popover, so it rides the frame's last-painted layer, and it starts exactly
    // where the model list did — one slot, not two placements that look alike.
    let dismissed = Rc::new(RefCell::new(false));
    let mut commanded = ChatView::new()
        .with_messages(messages)
        .with_composer_hints(hints)
        .with_composer_value("/")
        .with_composer_model_label("gpt-4o")
        .with_composer_model_menu(models, open.clone(), index, |_| {})
        .with_composer_slash_menu(commands, Rc::new(RefCell::new(0)), dismissed)
        .finish();
    let mut frame = render_element(&mut commanded, vec2f(width, 520.0), &app);
    let mut layer_ctx = PaintContext::new(Renderer::new());
    HoverChipLayer::new().paint(vec2f(0.0, 0.0), &mut layer_ctx, &app);
    frame.extend(
        layer_ctx
            .renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default(),
    );
    let command_row = line_of(&frame, "/clear");
    assert!(
        (command_row - model_row).abs() < 0.5,
        "the two bands share one slot: command row at {command_row}, model row at {model_row}"
    );
}

/// The slash-command list is an overlay, not a row of the pane: opening it must
/// leave the composer's rect and the transcript's content origin exactly where
/// they were closed. The panel itself is queued on the frame's last-painted
/// layer, above the composer, and its keys still move/take/dismiss the menu.
#[test]
fn the_slash_menu_opens_above_the_composer_without_moving_anything() {
    use crate::elements::{EventContext, HoverChipLayer, PaintContext, SlashMenuItem};
    use crate::event::{DispatchedEvent, ModifiersState};
    use crate::geometry::RectF;
    use crate::render::{RenderCommand, Renderer};
    use crate::test_util::render_element;

    fn composer_card(commands: &[RenderCommand], app: &AppContext) -> RectF {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::Surface)
                    && (*corner_radius - crate::elements::COMPOSER_CARD_RADIUS).abs() < 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("the composer card is drawn: {commands:?}"))
    }

    fn text_origin(commands: &[RenderCommand], needle: &str) -> Vector2F {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == needle => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle:?} is drawn: {commands:?}"))
    }

    let app = AppContext::default();
    let width = 600.0;
    let height = 520.0;
    let messages = vec![
        ChatMessage::new(ChatRole::User, vec![ChatFragment::text("Hello there")]),
        ChatMessage::new(ChatRole::Assistant, vec![ChatFragment::text("Hi")]),
    ];
    let commands = vec![
        SlashMenuItem::new("help", "List the commands"),
        SlashMenuItem::new("clear", "Clear the transcript"),
    ];
    let index = Rc::new(RefCell::new(0));
    let caret = Rc::new(RefCell::new(1));
    // Start closed: Escape has already put the list away for this draft.
    let dismissed = Rc::new(RefCell::new(true));
    let accepted: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let changed: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let dismiss_for_key = dismissed.clone();
    let accepted_for_key = accepted.clone();
    let changed_for_key = changed.clone();
    let mut view = ChatView::new()
        .with_messages(messages)
        .with_composer_focused(true)
        .with_composer_value("/")
        .with_composer_caret(caret.clone())
        .with_composer_slash_menu(commands, index.clone(), dismissed.clone())
        .with_composer_on_slash_accept(move |row| accepted_for_key.borrow_mut().push(row))
        .with_composer_on_slash_dismiss(move || *dismiss_for_key.borrow_mut() = true)
        .with_composer_on_change(move |text| changed_for_key.borrow_mut().push(text))
        .finish();

    // Closed frame. The same view is rendered again open below, so everything
    // but the menu's open flag is byte-for-byte the same input.
    let closed_frame = render_element(&mut view, vec2f(width, height), &app);
    let closed_card = composer_card(&closed_frame, &app);
    let closed_message = text_origin(&closed_frame, "Hi");
    assert!(
        app.hover_chips.borrow().is_empty(),
        "a closed list queues no overlay"
    );

    // Open the same draft's list and render the same view at the same width.
    *dismissed.borrow_mut() = false;
    let open_frame = render_element(&mut view, vec2f(width, height), &app);
    assert_eq!(
        format!("{closed_frame:?}"),
        format!("{open_frame:?}"),
        "the menu is an overlay: the frame's own tree (composer rect, transcript \
         origin, caret) is identical open and closed"
    );
    assert_eq!(
        composer_card(&open_frame, &app),
        closed_card,
        "the composer keeps its rect when the menu opens"
    );
    assert_eq!(
        text_origin(&open_frame, "Hi"),
        closed_message,
        "the transcript content keeps its origin when the menu opens"
    );
    assert_eq!(
        *caret.borrow(),
        1,
        "the editor keeps its caret when the menu opens"
    );

    // The menu rides the last layer, after the composer painted.
    let mut layer_ctx = PaintContext::new(Renderer::new());
    let queued = app.hover_chips.borrow().len();
    app.open_menus.begin_paint();
    HoverChipLayer::new().paint(vec2f(0.0, 0.0), &mut layer_ctx, &app);
    app.open_menus.end_paint();
    let layer = layer_ctx
        .renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default();
    assert_eq!(queued, 1, "the open list queues itself for the last layer");
    assert!(
        app.hover_chips.borrow().is_empty(),
        "the last layer drains the queue every frame"
    );

    let panel_card = layer
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == app.theme.color(ColorToken::SurfaceRaised)
                && (*corner_radius - 8.0).abs() < 0.01 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the menu panel is drawn by the last layer");
    assert!(
        panel_card.max_y() <= closed_card.min_y(),
        "the panel is anchored above the composer: {panel_card:?} against {closed_card:?}"
    );
    assert!(
        panel_card.min_y() >= 0.0 && panel_card.min_x() >= 0.0 && panel_card.max_x() <= width,
        "the panel is clamped inside the pane/window: {panel_card:?}"
    );
    assert!(
        layer.iter().any(
            |command| matches!(command, RenderCommand::DrawText { text, .. } if text == "/help")
        ),
        "the list's rows are drawn in the overlay: {layer:?}"
    );
    assert_eq!(
        app.open_menus.id(),
        Some("composer.slash"),
        "the overlay is the window's open menu, so an outside press dismisses it"
    );
    // Command order: the panel is painted after (above) the composer card.
    let mut combined = open_frame.clone();
    combined.extend(layer.clone());
    let composer_at = combined
        .iter()
        .position(|command| {
            matches!(command, RenderCommand::FillRect { rect, color, corner_radius }
                if *rect == closed_card
                    && *color == app.theme.color(ColorToken::Surface)
                    && (*corner_radius - crate::elements::COMPOSER_CARD_RADIUS).abs() < 0.01)
        })
        .expect("the composer card is in the frame");
    let panel_at = combined
        .iter()
        .position(|command| {
            matches!(command, RenderCommand::FillRect { rect, color, corner_radius }
                if *rect == panel_card
                    && *color == app.theme.color(ColorToken::SurfaceRaised)
                    && (*corner_radius - 8.0).abs() < 0.01)
        })
        .expect("the menu panel is in the last layer");
    assert!(
        panel_at > composer_at,
        "the menu panel paints above the composer (commands {panel_at} vs {composer_at})"
    );
    // A press on the panel's surface is the menu's, not an outside dismissal.
    assert!(
        app.open_menus
            .pointer_on_surface(vec2f(panel_card.min_x() + 20.0, panel_card.min_y() + 20.0)),
        "the open menu owns the pointer over its own panel"
    );

    // The panel is out of the tree, so the wrapper must forward its press to
    // the menu's rows: click the first row (heading 22 + row 34, padded 4).
    let first_row = vec2f(
        panel_card.min_x() + 100.0,
        panel_card.min_y() + 4.0 + 22.0 + 17.0,
    );
    let mut event_ctx = EventContext::default();
    for event in [
        DispatchedEvent::MouseDown {
            position: first_row,
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: first_row,
            button: 0,
        },
    ] {
        assert!(
            view.dispatch_event(&event, &mut event_ctx, &app),
            "the overlay takes the press on its row"
        );
    }
    assert_eq!(
        *accepted.borrow(),
        vec![0],
        "a click on the menu's row takes that row"
    );
    accepted.borrow_mut().clear();

    // Keyboard navigation still runs through the composer: Down/Up move the
    // highlight, Enter takes the row, Escape puts the list away.
    let key = |key: &str| DispatchedEvent::KeyDown {
        key: key.to_string(),
        modifiers: ModifiersState::none(),
    };
    assert!(
        view.dispatch_event(&key("ArrowDown"), &mut event_ctx, &app),
        "Down belongs to the open list"
    );
    assert_eq!(*index.borrow(), 1, "Down moves the highlight");
    assert!(
        view.dispatch_event(&key("ArrowUp"), &mut event_ctx, &app),
        "Up belongs to the open list"
    );
    assert_eq!(*index.borrow(), 0, "Up moves the highlight back");

    // Filtering is the editor's job while the list is up: a letter still lands
    // in the draft (which the host re-filters) instead of being eaten by the
    // menu or moving the caret.
    assert!(
        view.dispatch_event(&key("c"), &mut event_ctx, &app),
        "a plain letter is still the editor's"
    );
    assert_eq!(
        changed.borrow().last().map(String::as_str),
        Some("/c"),
        "the typed letter is the next query"
    );
    assert_eq!(
        *caret.borrow(),
        2,
        "typing into the menu's query keeps the caret with the draft"
    );
    assert!(
        !*dismissed.borrow(),
        "typing does not dismiss the list; the host re-filters it"
    );

    assert!(
        view.dispatch_event(&key("Enter"), &mut event_ctx, &app),
        "Enter takes the selected row"
    );
    assert_eq!(*accepted.borrow(), vec![0], "Enter accepts the highlight");
    assert!(
        view.dispatch_event(&key("Escape"), &mut event_ctx, &app),
        "Escape belongs to the open list"
    );
    assert!(*dismissed.borrow(), "Escape puts the list away");
    // And the closed list takes no layer space again.
    let _ = render_element(&mut view, vec2f(width, height), &app);
    assert!(
        app.hover_chips.borrow().is_empty(),
        "a dismissed list queues no overlay"
    );
}

/// The files the app hands the view draw as chips over the editor, above the
/// draft they belong to: what the attach control's picker returned is visible
/// where it was attached, and a view handed none draws no chip row.
#[test]
fn the_attachments_draw_as_chips_over_the_editor() {
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

    let mut bare = ChatView::new().with_messages(messages.clone()).finish();
    let frame = render_element(&mut bare, vec2f(width, 520.0), &app);
    assert!(
        line_of(&frame, "/tmp/notes.md").is_none(),
        "a view handed no attachments draws no chip: {frame:?}"
    );

    let mut attached = ChatView::new()
        .with_messages(messages)
        .with_composer_attachments(vec!["/tmp/notes.md".to_string()])
        .finish();
    let frame = render_element(&mut attached, vec2f(width, 520.0), &app);
    let chip = line_of(&frame, "/tmp/notes.md").expect("the attachment is drawn");
    let editor = line_of(&frame, "Ask anything...").expect("the editor is drawn");
    assert!(
        chip < editor,
        "the chip is over the editor it belongs to: {chip} against {editor}"
    );
}

/// The whole-transcript filter is a real bar, not an always-on tray: it draws
/// nothing while it is down, and raising it shows the query field, names what it
/// filters and counts the terminal lines the query keeps.
#[test]
fn the_transcript_filter_bar_stays_down_until_it_is_raised() {
    use crate::elements::{TerminalData, TerminalFilter, TerminalLine};
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let width = 600.0;
    let filter = TerminalFilter::default();
    let block = TerminalData {
        title: "printf hi".to_string(),
        lines: vec![
            TerminalLine::command("printf hi"),
            TerminalLine::output("alpha line"),
            TerminalLine::output("beta line"),
        ],
        status: None,
        meta: None,
    };
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::terminal(block)],
    )];
    let mut view = ChatView::new()
        .with_messages(messages)
        .with_global_terminal_filter(Some(filter.clone()))
        .finish();

    let texts = |commands: &[RenderCommand]| -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    };

    let down = render_element(&mut view, vec2f(width, 520.0), &app);
    assert!(
        !texts(&down).iter().any(|t| t == "Filter terminal output"),
        "a filter that is down draws no bar: {:?}",
        texts(&down)
    );
    assert!(
        texts(&down).iter().any(|t| t == "alpha line")
            && texts(&down).iter().any(|t| t == "beta line"),
        "the block draws its whole output: {:?}",
        texts(&down)
    );

    filter.open_bar();
    let up = render_element(&mut view, vec2f(width, 520.0), &app);
    let drawn = texts(&up);
    assert!(
        drawn.iter().any(|t| t == "Filter terminal output"),
        "a raised filter draws its field: {drawn:?}"
    );
    assert!(
        drawn.iter().any(|t| t == "3 of 3"),
        "an empty query keeps every line the block carries: {drawn:?}"
    );

    // Typing narrows the transcript: the count follows the query, and the lines
    // the query drops are not drawn at all.
    filter.set_query("beta");
    let typed = render_element(&mut view, vec2f(width, 520.0), &app);
    let drawn = texts(&typed);
    assert!(
        drawn.iter().any(|t| t == "1 of 3"),
        "the count follows the query: {drawn:?}"
    );
    assert!(
        drawn.iter().any(|t| t == "beta line"),
        "the kept line stays drawn: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|t| t == "alpha line"),
        "the dropped line is not drawn: {drawn:?}"
    );
}

/// What "Fork and usage keep their place under the last message" means, read
/// off the drawn frame rather than asserted by hand: the footer is the closing
/// row of the *scrolled* transcript content — one column gap below the last
/// message's own row, inside the viewport the transcript clips to, moving with
/// that message when the transcript scrolls, and holding the transcript's room
/// under itself so it never sits on the input's separator.
mod the_footers_place {
    use super::*;
    use crate::elements::interactive::contains;
    use crate::elements::{LayoutContext, PaintContext, SizeConstraint};
    use crate::geometry::RectF;
    use crate::render::{RenderCommand, Renderer};
    use goble_core::llm::TokenUsage;

    /// One user turn (so the last message paints a measurable row band), the
    /// repo's own fold/scroll state, and a footer with both affordances wired.
    fn pane(last_message: &str, scroll: Rc<RefCell<ScrollState>>) -> Box<dyn Element> {
        ChatView::new()
            .with_messages(vec![ChatMessage::new(
                ChatRole::User,
                vec![ChatFragment::text(last_message)],
            )])
            .with_scroll_state(scroll)
            .with_usage(TokenUsage {
                input: 1_234,
                cached: Some(800),
                output: 56,
            })
            .with_on_fork(|| {})
            .finish()
    }

    /// The same pane over a conversation long enough to scroll.
    fn long_pane(scroll: Rc<RefCell<ScrollState>>) -> Box<dyn Element> {
        let messages = (0..40)
            .map(|i| {
                ChatMessage::new(
                    ChatRole::User,
                    vec![ChatFragment::text(format!("streamed line {i}"))],
                )
            })
            .collect();
        ChatView::new()
            .with_messages(messages)
            .with_scroll_state(scroll)
            .with_usage(TokenUsage {
                input: 1_234,
                cached: Some(800),
                output: 56,
            })
            .with_on_fork(|| {})
            .finish()
    }

    /// One frame at `size`, the way the app draws it, with the pointer at
    /// `cursor` while painting when one is given.
    fn frame(
        view: &mut Box<dyn Element>,
        size: Vector2F,
        app: &AppContext,
        cursor: Option<Vector2F>,
    ) -> Vec<RenderCommand> {
        let _ = view.layout(SizeConstraint::loose(size), &mut LayoutContext, app);
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

    fn text_origin(commands: &[RenderCommand], needle: &str) -> Vector2F {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == needle => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{needle:?} is drawn"))
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

    /// The last message's own row: the pane-wide band it paints behind its text,
    /// taken as the bottom-most one that starts above `above` (the footer's ink).
    fn last_row(commands: &[RenderCommand], app: &AppContext, above: f32) -> RectF {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == app.theme.color(ColorToken::SurfaceRaised)
                        && rect.max_y() <= above =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .max_by(|a, b| a.max_y().total_cmp(&b.max_y()))
            .unwrap_or_else(|| panic!("the last message paints its row band"))
    }

    /// The rect of the affordance the pointer at `point` is inside: the band
    /// that frame draws and the resting frame does not.
    fn hovered_row(rest: &[RenderCommand], hovered: &[RenderCommand], point: Vector2F) -> RectF {
        let bands = |commands: &[RenderCommand]| -> Vec<RectF> {
            commands
                .iter()
                .filter_map(|command| match command {
                    RenderCommand::FillRect { rect, .. } if rect.height() <= 40.0 => Some(*rect),
                    _ => None,
                })
                .collect()
        };
        let resting = bands(rest);
        bands(hovered)
            .into_iter()
            .find(|band| contains(*band, point) && !resting.contains(band))
            .unwrap_or_else(|| {
                panic!("the affordance at {point:?} paints a band under the pointer")
            })
    }

    /// The viewport the transcript clips its content to.
    fn viewport(commands: &[RenderCommand]) -> RectF {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::ClipRect(rect) => Some(*rect),
                _ => None,
            })
            .expect("the transcript clips to its viewport")
    }

    fn assert_close(what: &str, got: f32, want: f32) {
        assert!((got - want).abs() < 0.05, "{what}: got {got}, want {want}");
    }

    /// The two affordances close the transcript as one row, directly under the
    /// last message, inside the scrolled viewport rather than over the input.
    #[test]
    fn the_footer_is_the_closing_row_of_the_scrolled_content() {
        let app = AppContext::default();
        let size = vec2f(600.0, 520.0);
        let md = app.theme.spacing_px(SpacingToken::Md);
        let scroll = Rc::new(RefCell::new(ScrollState::following()));
        let mut view = pane("Salut", scroll);

        let rest = frame(&mut view, size, &app, None);
        let fork = text_origin(&rest, "Fork");
        let usage = text_origin(&rest, "Usage 490 tok");
        assert_eq!(fork.y, usage.y, "the two affordances are one closing row");
        assert!(
            usage.x > fork.x,
            "the usage disclosure follows the fork on that row: {usage:?} against {fork:?}"
        );

        let row = last_row(&rest, &app, fork.y);
        assert!(
            fork.y > row.max_y(),
            "the footer is under the last message: {fork:?} against {row:?}"
        );

        let probe = fork + vec2f(1.0, 1.0);
        let hovered = frame(&mut view, size, &app, Some(probe));
        let footer = hovered_row(&rest, &hovered, probe);
        assert_close(
            "the footer sits one column gap under the last message",
            footer.min_y() - row.max_y(),
            md,
        );

        let viewport = viewport(&rest);
        assert!(
            footer.min_y() >= viewport.min_y() && footer.max_y() <= viewport.max_y(),
            "the footer is inside the scrolled viewport: {footer:?} in {viewport:?}"
        );
        assert!(
            footer.max_y() < viewport.max_y(),
            "the transcript keeps its room under that closing row: {footer:?} in {viewport:?}"
        );
    }

    /// Short content is still flushed to the end of the viewport, and the slack
    /// that does it lands above the message: the footer keeps the same `md` gap
    /// under the last message at two pane heights, and the room below it is the
    /// footer's own — so it does not read as part of the input under it.
    #[test]
    fn the_footer_keeps_its_gap_under_the_last_message_when_the_content_is_short() {
        let app = AppContext::default();
        let md = app.theme.spacing_px(SpacingToken::Md);
        for height in [520.0_f32, 900.0] {
            let size = vec2f(600.0, height);
            let scroll = Rc::new(RefCell::new(ScrollState::following()));
            let mut view = pane("Salut", scroll.clone());
            let rest = frame(&mut view, size, &app, None);
            let fork = text_origin(&rest, "Fork");
            let row = last_row(&rest, &app, fork.y);
            let probe = fork + vec2f(1.0, 1.0);
            let hovered = frame(&mut view, size, &app, Some(probe));
            let footer = hovered_row(&rest, &hovered, probe);
            let viewport = viewport(&rest);
            assert_eq!(
                scroll.borrow().max_offset(),
                0.0,
                "one message is shorter than a {height} px pane, so nothing scrolls"
            );
            assert_close(
                &format!("the gap under the last message at {height}"),
                footer.min_y() - row.max_y(),
                md,
            );
            assert_close(
                &format!("the transcript's room under its closing row at {height}"),
                viewport.max_y() - footer.max_y(),
                md,
            );
            assert!(
                row.min_y() - viewport.min_y() > 0.0,
                "the anchor's slack is above the message, not between it and the footer"
            );
        }
    }

    /// The footer is inside the scrolled content: a scroll delta carries it by
    /// exactly that much, and reading the history from the top takes it out of
    /// the viewport with the last message instead of stranding it.
    #[test]
    fn scrolling_the_transcript_carries_the_footer_with_the_last_message() {
        let app = AppContext::default();
        let size = vec2f(600.0, 480.0);
        let scroll = Rc::new(RefCell::new(ScrollState::following()));
        let mut view = long_pane(scroll.clone());

        let pinned = frame(&mut view, size, &app, None);
        let viewport = viewport(&pinned);
        let pinned_fork = text_origin(&pinned, "Fork");
        assert!(
            scroll.borrow().max_offset() > 0.0,
            "40 messages are taller than the pane"
        );
        assert!(
            pinned_fork.y >= viewport.min_y() && pinned_fork.y <= viewport.max_y(),
            "the footer is in view at the end of the stream: {pinned_fork:?} in {viewport:?}"
        );

        // The user scrolls up through the conversation. The delta stays small
        // enough that the footer is still on screen: a run outside the
        // viewport is culled, so its geometry cannot be read from the frame.
        scroll.borrow_mut().scroll_by(-8.0);
        let scrolled = frame(&mut view, size, &app, None);
        assert_close(
            "the footer moves with the transcript's scroll offset",
            text_origin(&scrolled, "Fork").y - pinned_fork.y,
            8.0,
        );

        // All the way back: the footer has left the viewport, with the message.
        scroll.borrow_mut().scroll_by(-10_000.0);
        let top = frame(&mut view, size, &app, None);
        assert_eq!(
            scroll.borrow().offset(),
            0.0,
            "the transcript is at its start"
        );
        assert!(
            !texts(&top).iter().any(|text| text == "Fork"),
            "at the start of the history the footer is out of the viewport, not stranded over \
             the composer"
        );
    }

    /// While the last message is still growing the footer stays under it: the
    /// anchored transcript ends on the same row in every frame, at the same
    /// `md` gap from the row above, rather than jumping between frames.
    #[test]
    fn the_footer_stays_under_the_last_message_while_it_streams() {
        let app = AppContext::default();
        let size = vec2f(600.0, 520.0);
        let md = app.theme.spacing_px(SpacingToken::Md);
        let scroll = Rc::new(RefCell::new(ScrollState::following()));

        let mut first = pane("Salut", scroll.clone());
        let rest = frame(&mut first, size, &app, None);
        let fork = text_origin(&rest, "Fork");
        let row = last_row(&rest, &app, fork.y);
        let probe = fork + vec2f(1.0, 1.0);
        let hovered = frame(&mut first, size, &app, Some(probe));
        let footer = hovered_row(&rest, &hovered, probe);

        // The same conversation, frame after frame, with the last message's
        // text still arriving: each frame is a rebuild with more of it.
        for (frame_index, chunk) in [120_usize, 260, 400].into_iter().enumerate() {
            let mut streaming = pane(&"x".repeat(chunk), scroll.clone());
            let grown = frame(&mut streaming, size, &app, None);
            let grown_fork = text_origin(&grown, "Fork");
            let grown_row = last_row(&grown, &app, grown_fork.y);
            assert!(
                grown_row.height() > row.height(),
                "the last message grew by frame {frame_index}: {:?} against {row:?}",
                grown_row
            );
            let probe = grown_fork + vec2f(1.0, 1.0);
            let hovered = frame(&mut streaming, size, &app, Some(probe));
            let grown_footer = hovered_row(&grown, &hovered, probe);
            let viewport = viewport(&grown);
            assert_eq!(
                scroll.borrow().offset(),
                0.0,
                "the growing message does not move the transcript's offset"
            );
            assert!(
                grown_footer.min_y() >= viewport.min_y()
                    && grown_footer.max_y() <= viewport.max_y(),
                "the footer stays inside the scrolled viewport while the message grows: \
                 {grown_footer:?} in {viewport:?}"
            );
            assert_close(
                "the footer does not jump while the message grows",
                grown_footer.min_y(),
                footer.min_y(),
            );
            assert_close(
                "the footer is still the row under the growing message",
                grown_footer.min_y() - grown_row.max_y(),
                md,
            );
        }
    }

    /// A transcript with no message draws no footer at all: a fresh
    /// conversation is the empty state, and the footer arrives with the first
    /// message it belongs under.
    #[test]
    fn an_empty_transcript_draws_no_footer() {
        let app = AppContext::default();
        let scroll = Rc::new(RefCell::new(ScrollState::following()));
        let mut view = ChatView::new()
            .with_scroll_state(scroll)
            .with_usage(TokenUsage {
                input: 1_234,
                cached: Some(800),
                output: 56,
            })
            .with_on_fork(|| {})
            .finish();
        let commands = frame(&mut view, vec2f(600.0, 520.0), &app, None);
        let drawn = texts(&commands);
        assert!(
            drawn.iter().any(|text| text == "New conversation"),
            "the empty state is what a fresh pane shows: {drawn:?}"
        );
        assert!(
            !drawn.iter().any(|text| text == "Fork"),
            "no message to sit under, so no fork: {drawn:?}"
        );
        assert!(
            !drawn
                .iter()
                .any(|text| text.contains("tokens") || text.contains("usage")),
            "and no usage disclosure: {drawn:?}"
        );
    }
}

/// Drag-to-copy over the agent transcript: the press freezes the rows the last
/// paint recorded, the drag extends the selection, and the release copies what
/// it covered. A click with no drag copies nothing and leaves no highlight.
mod drag_to_copy {
    use super::*;
    use crate::elements::{Element, PaintContext, TranscriptSelection};
    use crate::event::DispatchedEvent;
    use crate::geometry::Vector2F;
    use crate::render::{RenderCommand, Renderer};

    /// Lay the view out and paint it, returning the commands. Painting is what
    /// fills the transcript's text recording, so every test here paints once
    /// before it presses.
    fn paint(view: &mut ChatView, size: Vector2F, app: &AppContext) -> Vec<RenderCommand> {
        let _ = view.layout(SizeConstraint::loose(size), &mut LayoutContext, app);
        let mut ctx = PaintContext::new(Renderer::new());
        view.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    fn two_messages() -> Vec<ChatMessage> {
        vec![
            ChatMessage::new(ChatRole::Assistant, vec![ChatFragment::text("alpha beta")]),
            ChatMessage::new(ChatRole::Assistant, vec![ChatFragment::text("gamma delta")]),
        ]
    }

    fn copy_sink() -> (Rc<RefCell<Vec<String>>>, Rc<RefCell<dyn FnMut(String)>>) {
        let copied = Rc::new(RefCell::new(Vec::new()));
        let sink = copied.clone();
        (
            copied,
            Rc::new(RefCell::new(move |text: String| {
                sink.borrow_mut().push(text);
            })),
        )
    }

    fn drag(view: &mut ChatView, start: Vector2F, end: Vector2F, app: &AppContext) {
        let mut ctx = crate::elements::EventContext;
        for event in [
            DispatchedEvent::MouseDown {
                position: start,
                button: 0,
            },
            DispatchedEvent::MouseMove { position: end },
            DispatchedEvent::MouseUp {
                position: end,
                button: 0,
            },
        ] {
            view.dispatch_event(&event, &mut ctx, app);
        }
    }

    #[test]
    fn a_drag_across_the_transcript_copies_it_on_release() {
        let app = AppContext::default();
        let (copied, on_copy) = copy_sink();
        let selection = Rc::new(RefCell::new(TranscriptSelection::default()));
        let mut view = ChatView::new()
            .with_messages(two_messages())
            .with_transcript_selection(selection.clone())
            .with_on_copy_terminal(Some(on_copy));

        paint(&mut view, vec2f(600.0, 480.0), &app);
        let recording = view.text_recording.borrow().clone();
        assert!(
            recording.lines.len() >= 2,
            "both messages record their rows: {:?}",
            recording
                .lines
                .iter()
                .map(|line| line.text())
                .collect::<Vec<_>>()
        );
        let first = &recording.lines[0];
        let last = recording.lines.last().unwrap();
        let start = vec2f(first.left + 1.0, first.top + first.height / 2.0);
        let end = vec2f(last.left + 400.0, last.top + last.height / 2.0);

        drag(&mut view, start, end, &app);

        let copied = copied.borrow();
        assert_eq!(copied.len(), 1, "the release copies once");
        assert!(
            copied[0].starts_with("alpha beta"),
            "the copy starts at the press: {:?}",
            copied[0]
        );
        assert!(
            copied[0].ends_with("gamma delta"),
            "and runs through the release: {:?}",
            copied[0]
        );
        assert!(selection.borrow().has_selection());

        // The highlight is painted over the transcript on the next frame, and
        // the glyphs under it are redrawn in the background colour so selected
        // text changes colour instead of keeping its own.
        let commands = paint(&mut view, vec2f(600.0, 480.0), &app);
        let band = app.theme.color(ColorToken::Selection);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { rect, color, .. }
                    if *color == band && rect.height() > 1.0
            )),
            "the selection is drawn over the text it covers"
        );
        let contrast = app.theme.color(ColorToken::Bg);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, color, .. }
                    if *color == contrast && text.starts_with("alpha")
            )),
            "the selected glyphs are repainted in the contrast colour"
        );
    }

    #[test]
    fn a_click_without_a_drag_copies_nothing() {
        let app = AppContext::default();
        let (copied, on_copy) = copy_sink();
        let selection = Rc::new(RefCell::new(TranscriptSelection::default()));
        let mut view = ChatView::new()
            .with_messages(two_messages())
            .with_transcript_selection(selection.clone())
            .with_on_copy_terminal(Some(on_copy));

        paint(&mut view, vec2f(600.0, 480.0), &app);
        let recording = view.text_recording.borrow().clone();
        let line = &recording.lines[0];
        let point = vec2f(line.left + 4.0, line.top + line.height / 2.0);

        drag(&mut view, point, point, &app);

        assert!(
            copied.borrow().is_empty(),
            "a click never replaces the clipboard"
        );
        assert!(
            !selection.borrow().has_selection(),
            "and leaves no one-character highlight behind"
        );
    }

    #[test]
    fn a_press_outside_the_transcript_puts_the_highlight_away() {
        let app = AppContext::default();
        let selection = Rc::new(RefCell::new(TranscriptSelection::default()));
        let (_, on_copy) = copy_sink();
        let mut view = ChatView::new()
            .with_messages(two_messages())
            .with_transcript_selection(selection.clone())
            .with_on_copy_terminal(Some(on_copy));

        paint(&mut view, vec2f(600.0, 480.0), &app);
        let recording = view.text_recording.borrow().clone();
        let first = &recording.lines[0];
        let last = recording.lines.last().unwrap();
        let start = vec2f(first.left + 1.0, first.top + first.height / 2.0);
        let end = vec2f(last.left + 400.0, last.top + last.height / 2.0);
        drag(&mut view, start, end, &app);
        assert!(selection.borrow().has_selection());

        // A press on the composer band is not a text gesture.
        let mut ctx = crate::elements::EventContext;
        view.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(300.0, 470.0),
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert!(!selection.borrow().has_selection());
    }

    #[test]
    fn a_bulleted_reply_wraps_inside_the_pane_at_every_width() {
        use crate::elements::chat_content::ListItem;
        use crate::platform::text_atlas::advance_width;
        use crate::render::RenderCommand;
        use crate::test_util::render_element;

        let app = AppContext::default();
        let items = ["Agents", "Teams & Workflows", "MCP servers"]
            .into_iter()
            .map(|name| {
                ListItem::new(vec![ChatFragment::text(format!(
                    "{name} — create or update agents with specific prompts and tool \
                     access, then let them run on the work you describe in the \
                     conversation."
                ))])
            })
            .collect();
        let messages = vec![ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::list(items, None)],
        )];
        let mut view = ChatView::new().with_messages(messages).finish();

        let widest = |commands: &[RenderCommand]| {
            commands
                .iter()
                .filter_map(|command| match command {
                    RenderCommand::DrawText {
                        origin,
                        text,
                        font_size,
                        font_weight,
                        font_family,
                        font_italic,
                        max_width,
                        ..
                    } => {
                        let advance = advance_width(
                            text,
                            *font_size,
                            *font_weight,
                            *font_family,
                            *font_italic,
                        );
                        Some(origin.x + advance.min(*max_width))
                    }
                    _ => None,
                })
                .fold(0.0f32, f32::max)
        };

        // A wide frame first, then a series of shrinks: a cached tree would
        // keep the wide run's lines and draw them past the new edge.
        let _ = render_element(&mut view, vec2f(900.0, 600.0), &app);
        for width in [600.0, 400.0, 260.0, 180.0] {
            let commands = render_element(&mut view, vec2f(width, 600.0), &app);
            let marker = commands.iter().any(|command| {
                matches!(
                    command,
                    RenderCommand::DrawText { text, .. } if text == "• "
                )
            });
            assert!(
                marker,
                "the reply is drawn as a list in a {width}-wide pane"
            );
            let ink = widest(&commands);
            assert!(
                ink <= width + 0.5,
                "the list's ink ends at {ink} in a {width}-wide pane"
            );
        }
    }
}

/// Grok Build's sticky turn: the latest user message pins to the transcript's
/// top edge while its own bubble has scrolled above it, and stays for the whole
/// turn that bubble opens — through every reply and tool row — until the next
/// user prompt reaches that edge. Only the latest user message ever pins, and
/// the pinned row is the user's own prompt band: the same grey fill, the same
/// `❯` marker and the same 14 px line as the bubble it names.


/// The in-chat notice surfaces — the queued prompt band, the failed-turn block
/// and the footer pills — draw DSH's type at defaults: the queued prompt is the
/// user's own 14 / 22 line over a 13 / 24 label, the failure title is 13 / 20
/// semibold in the error ink over a 13 / 20 quiet reason, and the usage pill is
/// a 12 / 20 quiet label with a 14 px glyph in DSH's 28 px pill.
#[test]
fn the_notice_surfaces_draw_dsh_type() {
    use crate::elements::chat_typography;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;
    use goble_core::llm::TokenUsage;

    let app = AppContext::default();
    let messages = vec![ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("Partial answer")],
    )];
    let mut view = ChatView::new()
        .with_messages(messages)
        .with_queued_prompt(Some("run the tests".to_string()))
        .with_turn_error(Some("Incorrect API key provided".to_string()))
        .with_usage(TokenUsage {
            input: 400,
            cached: Some(100),
            output: 90,
        })
        .finish();
    let commands = render_element(&mut view, vec2f(600.0, 900.0), &app);

    let run = |prefix: &str| {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText {
                    text,
                    font_size,
                    line_height,
                    color,
                    font_weight,
                    font_family,
                    ..
                } if text.starts_with(prefix) => {
                    Some((*font_size, *line_height, *color, *font_weight, *font_family))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("the run {prefix:?} is drawn"))
    };

    // The queued prompt: DSH's user-message type over the quiet label.
    let prompt = run("run the tests");
    assert_eq!(prompt.0, 14.0, "the queued prompt is the 14 px user line");
    assert!(
        (prompt.1 - chat_typography::USER_LH).abs() < 1e-5,
        "the queued prompt sits on the 22 px user line, got {}",
        prompt.1
    );
    assert_eq!(prompt.2, app.theme.color(ColorToken::Text));
    let pending = run("Pending message");
    assert_eq!(pending.0, 13.0, "the queued header is the process tier");
    assert!((pending.1 - chat_typography::ROW_LH).abs() < 1e-5);
    assert_eq!(pending.2, app.theme.color(ColorToken::Muted));

    // The failed turn: 13 / 20 semibold error title, 13 / 20 quiet reason.
    let title = run("Agent turn failed");
    assert_eq!(title.0, 13.0);
    assert!((title.1 - chat_typography::ROW_TIGHT_LH).abs() < 1e-5);
    assert_eq!(title.2, app.theme.color(ColorToken::Error));
    assert_eq!(title.3, crate::platform::text_atlas::FontWeight::SemiBold);
    let reason = run("Incorrect API key provided");
    assert_eq!(reason.0, 13.0);
    assert!((reason.1 - chat_typography::ROW_TIGHT_LH).abs() < 1e-5);
    assert_eq!(reason.2, app.theme.color(ColorToken::Muted));

    // The usage pill: a 12 / 20 label and a 14 px glyph in a 28 px pill.
    let label = run("Usage ");
    assert_eq!(label.0, 12.0);
    assert!((label.1 - chat_typography::PILL_LH).abs() < 1e-5);
    assert_eq!(label.2, app.theme.color(ColorToken::Muted));
    let icons: Vec<f32> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawIcon { name, size, .. } if name == "database" => Some(*size),
            _ => None,
        })
        .collect();
    assert!(
        icons.iter().any(|size| (*size - 14.0).abs() < 1e-5),
        "the usage pill's glyph is 14 px, got {icons:?}"
    );
}

// ---- Per-pane content lane: the content column, the full-width header ------

/// A stand-in for the pane's header overlay in the lane tests: it takes the
/// whole width it is given and paints it in `color`, `height` tall.
struct ProbeHeader {
    height: f32,
    color: crate::color::ColorU,
    size: Option<Vector2F>,
    origin: Option<crate::elements::Point>,
}

impl Element for ProbeHeader {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(constraint.max.x, self.height);
        self.size = Some(size);
        size
    }

    fn paint(
        &mut self,
        origin: Vector2F,
        ctx: &mut crate::elements::PaintContext,
        _app: &AppContext,
    ) {
        self.origin = Some(crate::elements::Point::from_vec2f(
            origin,
            Default::default(),
        ));
        let size = self.size.unwrap_or(Vector2F::zero());
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rect(
                crate::geometry::rectf(origin.x, origin.y, size.x, size.y),
                self.color,
            );
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<crate::elements::Point> {
        self.origin
    }
}

/// A chat view wired to the per-pane content lane, over one message, with its
/// drag gestures recorded. `header` is the pane's own topbar overlay, if any.
fn laned_view_with_header(
    preference: Option<f32>,
    header: Option<Box<dyn Element>>,
) -> (
    Box<dyn Element>,
    Rc<std::cell::RefCell<Vec<(f32, crate::elements::LaneEdge, f32, f32)>>>,
) {
    let starts: Rc<std::cell::RefCell<Vec<(f32, crate::elements::LaneEdge, f32, f32)>>> =
        Rc::new(std::cell::RefCell::new(Vec::new()));
    let recorded = Rc::clone(&starts);
    let mut view = ChatView::new()
        .with_messages(vec![ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Hi")],
        )])
        .with_content_lane(
            preference,
            false,
            None,
            Rc::new(std::cell::RefCell::new(
                move |pointer_x: f32, edge, base_width: f32, column: f32| {
                    recorded
                        .borrow_mut()
                        .push((pointer_x, edge, base_width, column));
                },
            )),
            Rc::new(std::cell::RefCell::new(|_x: f32| {})),
            Rc::new(std::cell::RefCell::new(|| {})),
        );
    if let Some(header) = header {
        view = view.with_header(header);
    }
    (view.finish(), starts)
}

fn laned_view(
    preference: Option<f32>,
) -> (
    Box<dyn Element>,
    Rc<std::cell::RefCell<Vec<(f32, crate::elements::LaneEdge, f32, f32)>>>,
) {
    laned_view_with_header(preference, None)
}

/// The view's content column: the widest `Bg` fill the lane paints the content
/// in. (The view's other backgrounds are `Surface`/raised.)
fn view_content_column(commands: &[crate::render::RenderCommand], app: &AppContext) -> RectF {
    use crate::render::RenderCommand;
    let content = app.theme.color(ColorToken::ContentBg);
    commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. } if *color == content => Some(*rect),
            _ => None,
        })
        .max_by(|a, b| (a.width() * a.height()).total_cmp(&(b.width() * b.height())))
        .expect("the lane paints the content column")
}

/// The lane meters the content against the width the view is given: the ratio
/// column on a wide view, full width on a narrow one.
#[test]
fn the_chat_content_is_centred_in_the_width_the_view_is_given() {
    use crate::test_util::render_element;

    let app = AppContext::default();
    let (mut wide, _starts) = laned_view(None);
    let commands = render_element(&mut wide, vec2f(1600.0, 900.0), &app);
    let column = view_content_column(&commands, &app);
    assert!(
        (column.width() - 920.0).abs() < 0.5,
        "64% of 1600 clamps to the 920 ceiling: {column:?}"
    );
    assert!(
        (column.min_x() - 340.0).abs() < 0.5,
        "the column is centred: {column:?}"
    );

    let (mut narrow, _starts) = laned_view(None);
    let commands = render_element(&mut narrow, vec2f(800.0, 900.0), &app);
    let column = view_content_column(&commands, &app);
    assert!(
        (column.width() - 800.0).abs() < 0.5 && (column.min_x() - 0.0).abs() < 0.5,
        "under 856 the lane is a full-width pass-through: {column:?}"
    );
}

/// The pane header is an overlay at the full pane width and does NOT resize
/// with the content lane: the content column is capped and centred under it,
/// and the lane's draggable edges start below the header band, so a press on
/// the header stays the header's own gesture.
#[test]
fn the_pane_header_spans_the_full_width_over_the_centred_content() {
    use crate::elements::LaneEdge;
    use crate::event::DispatchedEvent;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let header = ProbeHeader {
        height: 36.0,
        color: app.theme.color(ColorToken::Surface),
        size: None,
        origin: None,
    }
    .finish();
    let (mut view, starts) = laned_view_with_header(None, Some(header));
    let commands = render_element(&mut view, vec2f(1600.0, 900.0), &app);

    // The header: a full-width, 36-tall Surface band at the top of the view.
    let surface = app.theme.color(ColorToken::Surface);
    let band = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == surface
                    && (rect.height() - 36.0).abs() < 0.5
                    && (rect.min_x() - 0.0).abs() < 0.5 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the header band is painted");
    assert!(
        (band.width() - 1600.0).abs() < 0.5,
        "the pane header spans the whole pane, not the lane column: {band:?}"
    );
    let column = view_content_column(&commands, &app);
    assert!(
        (column.width() - 920.0).abs() < 0.5 && (column.min_x() - 340.0).abs() < 0.5,
        "the content is still the centred column: {column:?}"
    );

    // The lane's right strip: 24 px outside the column's right edge.
    let strip_x = 340.0 + 920.0 + 24.0 + 5.0;
    let mut ctx = crate::elements::EventContext::default();
    let press = |y: f32| DispatchedEvent::MouseDown {
        position: vec2f(strip_x, y),
        button: 0,
    };
    assert!(
        !view.dispatch_event(&press(10.0), &mut ctx, &app),
        "a press on the pane header band is not a lane drag"
    );
    assert!(
        starts.borrow().is_empty(),
        "no drag starts over the header: {:?}",
        starts.borrow()
    );
    assert!(
        view.dispatch_event(&press(100.0), &mut ctx, &app),
        "under the header the strip grabs the press"
    );
    assert_eq!(
        *starts.borrow(),
        vec![(strip_x, LaneEdge::Right, 920.0, 1600.0)]
    );
}

/// A committed per-pane preference lays that view's content out at its width;
/// a preference cannot activate a narrow view.
#[test]
fn a_committed_lane_preference_lays_the_content_at_that_width() {
    use crate::test_util::render_element;

    let app = AppContext::default();
    let (mut wide, _starts) = laned_view(Some(1000.0));
    let commands = render_element(&mut wide, vec2f(1600.0, 900.0), &app);
    let column = view_content_column(&commands, &app);
    assert!(
        (column.width() - 1000.0).abs() < 0.5 && (column.min_x() - 300.0).abs() < 0.5,
        "the preference wins over the ratio: {column:?}"
    );

    let (mut narrow, _starts) = laned_view(Some(1000.0));
    let commands = render_element(&mut narrow, vec2f(800.0, 900.0), &app);
    let column = view_content_column(&commands, &app);
    assert!(
        (column.width() - 800.0).abs() < 0.5,
        "a preference cannot activate a narrow lane: {column:?}"
    );
}

/// The pane's chrome band spans the pane while the cards inside it are bounded
/// to the conversation column: the work dock's band is a full-width square
/// `Surface` band under the header, and its announcement cards start and end at
/// the lane the transcript under them is centred in.
#[test]
fn the_work_dock_band_spans_the_pane_while_its_cards_are_lane_bounded() {
    use crate::elements::chat_content::{WorkDock, WorkGroup, WorkRow, WorkSection};
    use crate::elements::{chat_typography, lane_column};
    use crate::geometry::RectF;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;
    use std::cell::RefCell;
    use std::rc::Rc;

    let app = AppContext::default();
    let width = 1600.0;
    let groups = vec![WorkGroup::new(
        WorkSection::Subagents,
        vec![WorkRow::new("c1", "Subagent", "Audit the migration").spinning(true)],
        true,
    )];
    // The dock bounds its cards to the view's lane; the band itself stays the
    // full pane width.
    let dock = WorkDock::new(groups).with_content_lane(None).finish();
    let header = ProbeHeader {
        height: 36.0,
        color: app.theme.color(ColorToken::Surface),
        size: None,
        origin: None,
    }
    .finish();
    let mut view = ChatView::new()
        .with_messages(vec![ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Hi")],
        )])
        .with_header(header)
        .with_work_dock(Some(dock))
        .with_content_lane(
            None,
            false,
            None,
            Rc::new(RefCell::new(
                |pointer_x: f32, edge: crate::elements::LaneEdge, base: f32, column: f32| {
                    let _ = (pointer_x, edge, base, column);
                },
            )),
            Rc::new(RefCell::new(|_: f32| {})),
            Rc::new(RefCell::new(|| {})),
        )
        .finish();
    let commands = render_element(&mut view, vec2f(width, 900.0), &app);

    // The band: square, full pane width, directly under the header.
    let surface = app.theme.color(ColorToken::Surface);
    let band = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == surface
                && *corner_radius == 0.0
                && rect.height() > 40.0
                && rect.min_y() >= 35.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the dock paints its full-width band");
    assert_eq!(
        band.min_x(),
        0.0,
        "the band reaches the pane's edge: {band:?}"
    );
    assert_eq!(band.max_x(), width, "and its other edge: {band:?}");
    assert_eq!(band.width(), width);
    assert!(
        (band.min_y() - 36.0).abs() < 0.5,
        "the band starts where the header ends: {band:?}"
    );

    // The cards: the lane column, inset by the band's own padding.
    let radius = chat_typography::announcement_radius(&app);
    let cards: Vec<RectF> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::StrokeRect {
                rect,
                corner_radius,
                ..
            } if (*corner_radius - radius).abs() < 0.01 => Some(*rect),
            _ => None,
        })
        .collect();
    assert!(!cards.is_empty(), "the dock draws its announcement card");
    let (gutter, lane) = lane_column(width, None);
    let md = app.theme.spacing_px(crate::theme::SpacingToken::Md);
    for card in &cards {
        assert!(
            (card.min_x() - (gutter + md)).abs() < 0.5,
            "the card starts at the lane's inset edge: {card:?} vs gutter {gutter}"
        );
        assert!(
            (card.max_x() - (gutter + lane - md)).abs() < 0.5,
            "and ends at the lane's other edge: {card:?}"
        );
    }
    // The transcript's own column is the same lane: the chrome lines up with
    // the conversation under it.
    let column = view_content_column(&commands, &app);
    assert!(
        (column.min_x() - gutter).abs() < 0.5 && (column.width() - lane).abs() < 0.5,
        "the conversation column is the lane the cards use: {column:?} vs {gutter}/{lane}"
    );
    let band_centre = (band.min_x() + band.max_x()) / 2.0;
    let card_centre = (cards[0].min_x() + cards[0].max_x()) / 2.0;
    assert!(
        (band_centre - card_centre).abs() < 0.5,
        "the bounded cards are centred in the band: {card_centre} vs {band_centre}"
    );
}

/// The adjustable conversation column is the darker `Bg` token while the pane
/// around it keeps its `Surface`: the lane reads as a distinct darker column
/// (the pane card and gutters are the app's `Surface` behind the view).
#[test]
fn the_lane_column_paints_the_one_surface() {
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let (mut view, _starts) = laned_view(None);
    let commands = render_element(&mut view, vec2f(1600.0, 900.0), &app);

    let bg = app.theme.color(ColorToken::ContentBg);
    let column = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. } if *color == bg => Some(*rect),
            _ => None,
        })
        .max_by(|a, b| (a.width() * a.height()).total_cmp(&(b.width() * b.height())))
        .expect("the lane paints its content column");
    assert_eq!(
        (column.width(), column.min_x()),
        (920.0, 340.0),
        "the content column is the centred lane: {column:?}"
    );
    assert_eq!(
        bg,
        app.theme.color(ColorToken::ContentBg),
        "the lane paints the content material"
    );
}

/// The completed turn's changed-files card: real per-call diff counts, a
/// disclosure that reveals the file rows, a Review action that reaches the app,
/// and no card at all for a turn that changed nothing.
#[test]
fn a_completed_turn_shows_its_changed_files_and_review_action() {
    use crate::elements::chat_content::ToolCall;
    use crate::event::DispatchedEvent;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;
    use goble_core::harness::ToolCallStatus;

    let app = AppContext::default();
    let diff = "--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,2 +1,4 @@\n one\n-two\n+two\n+three\n+four\n";
    let message = ChatMessage::new(ChatRole::Assistant, vec![ChatFragment::text("done")])
        .with_tool_calls(vec![ToolCall {
            id: "c1".to_string(),
            name: "edit_file".to_string(),
            arguments: r#"{"path":"src/a.rs"}"#.to_string(),
            status: ToolCallStatus::Finished,
            result: None,
            diff: Some(diff.to_string()),
        }]);
    let summary = crate::elements::chat_content::changed_files_for_message(&message)
        .expect("the diff makes a summary");
    assert_eq!((summary.added(), summary.removed()), (3, 1), "{summary:?}");
    let open = Rc::new(RefCell::new(false));
    let reviewed = Rc::new(RefCell::new(0usize));
    let reviewed_cb = reviewed.clone();
    let mut view = ChatView::new()
        .with_messages(vec![message])
        .with_changed_files_open(open.clone())
        .with_on_review_changes(move || *reviewed_cb.borrow_mut() += 1)
        .finish();
    let commands = render_element(&mut view, vec2f(700.0, 800.0), &app);
    let texts: Vec<&str> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    // S: the collapsed card is a count header, not a file tile.
    for needle in ["1 file changed", "+3", "-1", "Review"] {
        assert!(
            texts.iter().any(|text| *text == needle),
            "the card shows {needle:?}: {texts:?}"
        );
    }
    assert!(
        !texts.iter().any(|text| *text == "a.rs"),
        "the row stays folded until the header is clicked: {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| *text == "src/a.rs"),
        "the rows stay folded until the disclosure is opened: {texts:?}"
    );

    let review = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. } if text == "Review" => Some(*origin),
            _ => None,
        })
        .expect("the Review action is drawn");
    let mut ctx = crate::elements::EventContext::default();
    for event in [
        DispatchedEvent::MouseDown {
            position: review + vec2f(6.0, 6.0),
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: review + vec2f(6.0, 6.0),
            button: 0,
        },
    ] {
        view.dispatch_event(&event, &mut ctx, &app);
    }
    assert_eq!(*reviewed.borrow(), 1, "Review opens the diff pane");

    *open.borrow_mut() = true;
    let commands = render_element(&mut view, vec2f(700.0, 800.0), &app);
    let texts: Vec<&str> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|text| *text == "a.rs") && texts.iter().any(|text| *text == "src"),
        "opening the card reveals the file row's name and directory: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| *text == "+3") && texts.iter().any(|text| *text == "-1"),
        "the row carries its own counts: {texts:?}"
    );

    // A turn with no recorded diff draws no card.
    let clean = ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::text("nothing changed")],
    );
    let mut clean_view = ChatView::new()
        .with_messages(vec![clean])
        .with_on_review_changes(|| {})
        .finish();
    let commands = render_element(&mut clean_view, vec2f(700.0, 500.0), &app);
    assert!(
        !commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawText { text, .. } if text.contains("file changed")
        )),
        "a clean turn has no changed-files card"
    );
}

/// S: the changed-files card is one collapsible header; expanding it reveals
/// every row (no four-row fold, no 60 pt tile) with the path and counts.
#[test]
fn the_changed_files_card_expands_from_its_header() {
    use crate::elements::chat_content::{ChangedFile, ChangedFiles};
    use crate::event::DispatchedEvent;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let files = (0..6)
        .map(|index| ChangedFile {
            path: format!("src/file_{index}.rs"),
            added: (index + 1) as u64,
            removed: index as u64,
            status: if index == 0 {
                'A'
            } else if index == 5 {
                'D'
            } else {
                'M'
            },
        })
        .collect();
    let summary = ChangedFiles { files };
    let open = Rc::new(RefCell::new(false));
    let mut card = super::changed_files_card::build_changed_files_card(
        &app,
        &summary,
        open.clone(),
        Rc::new(RefCell::new(|| {})),
    );
    let commands = render_element(&mut card, vec2f(700.0, 800.0), &app);
    let texts: Vec<String> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    for needle in ["6 files changed", "+21", "-15", "Review"] {
        assert!(
            texts.iter().any(|text| text == needle),
            "the closed card shows {needle:?}: {texts:?}"
        );
    }
    assert!(
        !texts.iter().any(|text| text == "file_0.rs"),
        "the closed card hides every row: {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text == "Show all 6 files"),
        "S removed the four-row fold control"
    );
    assert_eq!(card.size().expect("laid out").x, 700.0);

    let toggle = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. } if text == "6 files changed" => {
                Some(*origin)
            }
            _ => None,
        })
        .expect("the header is drawn");
    let mut ctx = crate::elements::EventContext::default();
    for event in [
        DispatchedEvent::MouseDown {
            position: toggle + vec2f(4.0, 4.0),
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: toggle + vec2f(4.0, 4.0),
            button: 0,
        },
    ] {
        card.dispatch_event(&event, &mut ctx, &app);
    }
    assert!(*open.borrow(), "clicking the header opens the list");

    let mut card = super::changed_files_card::build_changed_files_card(
        &app,
        &summary,
        open.clone(),
        Rc::new(RefCell::new(|| {})),
    );
    let commands = render_element(&mut card, vec2f(700.0, 800.0), &app);
    let texts: Vec<String> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    for row in ["file_0.rs", "file_5.rs", "src"] {
        assert!(
            texts.iter().any(|text| text == row),
            "the open card shows {row:?}: {texts:?}"
        );
    }
    assert!(
        !texts.iter().any(|text| text == "Show fewer"),
        "there is no fold footer any more: {texts:?}"
    );
}

/// A one-file card uses the file's basename as its title and never prints the
/// empty side of the change's counts (`+0` / `-0`).
#[test]
fn a_single_file_card_shows_its_count_and_skips_zero_counts() {
    use crate::elements::chat_content::{ChangedFile, ChangedFiles};
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    let app = AppContext::default();
    let summary = ChangedFiles {
        files: vec![ChangedFile {
            path: "src/deep/panes.rs".to_string(),
            added: 12,
            removed: 0,
            status: 'M',
        }],
    };
    let mut card = super::changed_files_card::build_changed_files_card(
        &app,
        &summary,
        Rc::new(RefCell::new(true)),
        Rc::new(RefCell::new(|| {})),
    );
    let commands = render_element(&mut card, vec2f(700.0, 800.0), &app);
    let texts: Vec<String> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|text| text == "1 file changed"),
        "the single-file card is titled by its count: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text == "panes.rs"),
        "the open card shows the filename: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text == "+12"),
        "the header shows the added count: {texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text == "-0"),
        "the empty side is omitted: {texts:?}"
    );
}
