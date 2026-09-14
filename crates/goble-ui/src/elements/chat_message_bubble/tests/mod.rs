use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use crate::color::ColorU;
use crate::elements::chat_content::ChatMessage;
use crate::elements::chat_content::{tool_fold_key, ChatAction, ChatFragment, ChatRole, SubAgentRow, SubAgentRowStatus, ToolCall, ToolDisplayMode};
use crate::elements::{AppContext, LayoutContext, PaintContext};
use crate::elements::{Element, SizeConstraint};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::geometry::vec2f;
use crate::platform::text_atlas::FontWeight;
use crate::render::{RenderCommand, Renderer};
use crate::test_util::{command_counts, render_element};
use crate::theme::{ColorToken, FontFamily};
use goble_core::harness::ToolCallStatus;
use super::*;

mod interaction;
mod markdown;
mod pill;
mod sub_agent;
mod tool_call;

#[test]
fn bubble_layouts_non_zero() {
    let app = AppContext::default();
    let mut bubble =
        ChatMessageBubble::new(ChatRole::Assistant, vec![ChatFragment::text("Hello")]);
    let size = bubble.layout(
        SizeConstraint::loose(vec2f(400.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(size.x > 0.0);
    assert!(size.y > 0.0);
}

/// Lay a one-paragraph bubble out in a `width`-wide pane and paint it,
/// returning its commands and its measured size.
fn paint_row(role: ChatRole, text: &str, width: f32) -> (Vec<RenderCommand>, Vector2F) {
    let app = AppContext::default();
    let mut element: Box<dyn Element> =
        Box::new(ChatMessageBubble::new(role, vec![ChatFragment::text(text)]));
    let commands = render_element(&mut element, vec2f(width, 400.0), &app);
    let size = element.size().expect("the row is laid out");
    (commands, size)
}

/// The `x` the run `text` starts at.
fn drawn_text_x(commands: &[RenderCommand], text: &str) -> f32 {
    commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                origin,
                text: drawn,
                ..
            } if drawn == text => Some(origin.x),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{text:?} is drawn"))
}

/// R1: a transcript row is a full-width band, not a pill. The agent's own
/// reply paints no box at all, the user's message is one square grey band
/// spanning the pane, and both inset their text by the transcript's
/// 10-15 px of padding.
#[test]
fn transcript_rows_are_full_width_bands_not_pills() {
    let app = AppContext::default();
    let width = 600.0;

    let (reply, reply_size) = paint_row(ChatRole::Assistant, "Salut!", width);
    let (message, message_size) = paint_row(ChatRole::User, "Salut!", width);

    assert_eq!(
        reply_size.x, width,
        "the agent's reply spans the pane's full width"
    );
    assert_eq!(
        message_size.x, width,
        "the user's message spans the pane's full width"
    );

    // The agent's reply: nothing is painted behind it.
    let reply_counts = command_counts(&reply);
    assert_eq!(
        reply_counts.fill_rect, 0,
        "the agent's reply paints no background"
    );
    assert_eq!(
        reply_counts.stroke_rect, 0,
        "the agent's reply draws no border"
    );

    // The user's message: one square grey band, edge to edge.
    let message_counts = command_counts(&message);
    assert_eq!(
        message_counts.fill_rect, 1,
        "the user's message paints one band"
    );
    assert_eq!(
        message_counts.stroke_rect, 0,
        "the user's message draws no border"
    );
    let band = message
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
        .expect("the user's message paints its grey band");
    assert_eq!(band.1, 0.0, "the user's band has square corners");
    assert_eq!(
        (band.0.min_x(), band.0.max_x()),
        (0.0, width),
        "the user's band runs across the pane"
    );

    // Both rows carry the transcript's padding at the edge.
    for (role, commands) in [(ChatRole::Assistant, &reply), (ChatRole::User, &message)] {
        let text_x = drawn_text_x(commands, "Salut!");
        assert!(
            (10.0..=15.0).contains(&text_x),
            "{role:?} keeps 10-15 px of padding, got {text_x}"
        );
    }
}

/// Paint an assistant bubble holding only `calls`; return its render
/// commands and its measured height. The calls open in their per-shape
/// default fold (folded).
fn paint_tool_calls(calls: Vec<ToolCall>) -> (Vec<RenderCommand>, f32) {
    paint_tool_calls_with_fold(calls, None)
}

/// Paint the same bubble with every call forced to `mode` (or its per-shape
/// default when `mode` is `None`).
fn paint_tool_calls_with_fold(
    calls: Vec<ToolCall>,
    mode: Option<ToolDisplayMode>,
) -> (Vec<RenderCommand>, f32) {
    let app = AppContext::default();
    let fold: HashMap<String, ToolDisplayMode> = match mode {
        Some(mode) => calls
            .iter()
            .enumerate()
            .map(|(index, call)| (tool_fold_key(call, index), mode))
            .collect(),
        None => HashMap::new(),
    };
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(calls)
        .with_tool_fold(Rc::new(RefCell::new(fold)));
    let size = bubble.layout(
        SizeConstraint::loose(vec2f(400.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default();
    (commands, size.y)
}

fn drawn_texts(commands: &[RenderCommand]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// How many distinct rows (by text baseline) the commands draw.
fn drawn_rows(commands: &[RenderCommand]) -> usize {
    let mut rows: Vec<i32> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { origin, .. } => Some((origin.y * 10.0).round() as i32),
            _ => None,
        })
        .collect();
    rows.sort_unstable();
    rows.dedup();
    rows.len()
}

/// The drawn text of each visual row, its runs concatenated left to right,
/// so a line painted as several runs (highlighted code) reads as one
/// string.
fn drawn_row_texts(commands: &[RenderCommand]) -> Vec<String> {
    let mut runs: Vec<(i32, f32, String)> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { text, origin, .. } => {
                Some(((origin.y * 10.0).round() as i32, origin.x, text.clone()))
            }
            _ => None,
        })
        .collect();
    runs.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap()));
    let mut rows: Vec<(i32, String)> = Vec::new();
    for (y, _, text) in runs {
        match rows.last_mut() {
            Some((last_y, line)) if *last_y == y => line.push_str(&text),
            _ => rows.push((y, text)),
        }
    }
    rows.into_iter().map(|(_, line)| line).collect()
}

/// The drawn text runs as `(text, colour, family, size)`, so two elements
/// can be compared without depending on where each was laid out.
fn text_runs(commands: &[RenderCommand]) -> Vec<(String, ColorU, FontFamily, f32)> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText {
                text,
                color,
                font_family,
                font_size,
                ..
            } => Some((text.clone(), *color, *font_family, *font_size)),
            _ => None,
        })
        .collect()
}

fn call(name: &str, arguments: &str, status: ToolCallStatus, result: Option<&str>) -> ToolCall {
    ToolCall {
        id: format!("call_{name}"),
        name: name.to_string(),
        arguments: arguments.to_string(),
        status,
        result: result.map(str::to_string),
    }
}

/// Lay a bubble out and paint it, returning its render commands.
fn paint_bubble(bubble: &mut ChatMessageBubble, app: &AppContext) -> Vec<RenderCommand> {
    bubble.layout(
        SizeConstraint::loose(vec2f(400.0, 400.0)),
        &mut LayoutContext::default(),
        app,
    );
    let mut paint_ctx = PaintContext::new(Renderer::new());
    bubble.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default()
}

/// A live record for the `call` tool call, as the app holds it from S4's
/// `chat:subagent_*` events.
fn record(
    child_id: &str,
    status: SubAgentRowStatus,
    activity: &str,
    elapsed: Duration,
    outcome: Option<&str>,
) -> SubAgentRow {
    SubAgentRow {
        child_id: child_id.to_string(),
        subagent_type: "reviewer".to_string(),
        description: "audit the migration".to_string(),
        status,
        activity: activity.to_string(),
        elapsed,
        turns: 2,
        tool_calls: 4,
        tokens: 1234,
        outcome: outcome.map(str::to_string),
        background: true,
    }
}

/// The spawn call a sub-agent record belongs to, keyed by its id.
fn spawn_call(status: ToolCallStatus) -> ToolCall {
    call(
        "spawn_subagent",
        r#"{"subagent_type":"reviewer","input":"audit the migration"}"#,
        status,
        None,
    )
}

/// The bubble holding `calls` with the pane's live `records`, folding through
/// the app-owned `fold` map, plus the actions its rows fire.
fn sub_agent_bubble(
    calls: Vec<ToolCall>,
    records: HashMap<String, SubAgentRow>,
    fold: Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
) -> (ChatMessageBubble, Rc<RefCell<Vec<ChatAction>>>) {
    let actions = Rc::new(RefCell::new(Vec::new()));
    let sink = actions.clone();
    let bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(calls)
        .with_tool_fold(fold)
        .with_sub_agents(records)
        .with_on_action(move |action| sink.borrow_mut().push(action));
    (bubble, actions)
}

/// Paint the same bubble with every call in `mode` (or its per-shape default
/// when `mode` is `None`) and return its commands.
fn paint_sub_agent(
    calls: Vec<ToolCall>,
    records: HashMap<String, SubAgentRow>,
    mode: Option<ToolDisplayMode>,
) -> (Vec<RenderCommand>, Rc<RefCell<Vec<ChatAction>>>) {
    let app = AppContext::default();
    let fold: HashMap<String, ToolDisplayMode> = match mode {
        Some(mode) => calls
            .iter()
            .enumerate()
            .map(|(index, call)| (tool_fold_key(call, index), mode))
            .collect(),
        None => HashMap::new(),
    };
    let (mut bubble, actions) = sub_agent_bubble(calls, records, Rc::new(RefCell::new(fold)));
    bubble.layout(
        SizeConstraint::loose(vec2f(600.0, 600.0)),
        &mut LayoutContext::default(),
        &app,
    );
    (paint_bubble(&mut bubble, &app), actions)
}

/// The top-left corner of the run `label` was painted at.
fn run_origin(commands: &[RenderCommand], label: &str) -> crate::geometry::Vector2F {
    commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. } if text == label => Some(*origin),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!("the paragraph must draw the run {label:?}, got {commands:?}")
        })
}

/// Click `bubble` at an absolute position: press then release.
fn click_at(
    bubble: &mut ChatMessageBubble,
    app: &AppContext,
    position: crate::geometry::Vector2F,
) {
    let mut event_ctx = crate::elements::EventContext::default();
    let down = DispatchedEvent::MouseDown {
        position,
        button: 0,
    };
    let up = DispatchedEvent::MouseUp {
        position,
        button: 0,
    };
    bubble.dispatch_event(&down, &mut event_ctx, app);
    bubble.dispatch_event(&up, &mut event_ctx, app);
}

/// Paint a Markdown message and return the style of every text run it draws.
fn painted_runs(markdown: &str) -> Vec<(String, FontWeight, bool)> {
    let app = AppContext::default();
    let message = ChatMessage::from_markdown(ChatRole::Assistant, markdown);
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments);
    bubble.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
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
        .into_iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText {
                text,
                font_weight,
                font_italic,
                ..
            } => Some((text, font_weight, font_italic)),
            _ => None,
        })
        .collect()
}

fn run_style(markdown: &str) -> (FontWeight, bool) {
    painted_runs(markdown)
        .into_iter()
        .find_map(|(text, weight, italic)| (text == "x").then_some((weight, italic)))
        .unwrap_or_else(|| panic!("no run drawing \"x\" for {markdown:?}"))
}

/// Paint a bubble holding one reasoning row and return its commands.
fn reasoning_commands(
    expanded: &Rc<RefCell<HashMap<String, bool>>>,
    key: &str,
) -> Vec<RenderCommand> {
    let app = AppContext::default();
    let message = ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::reasoning(
            key,
            "contemplating",
            "weighing the options carefully",
            true,
        )],
    );
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, message.fragments)
        .with_reasoning_expanded(expanded.clone());
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
}
