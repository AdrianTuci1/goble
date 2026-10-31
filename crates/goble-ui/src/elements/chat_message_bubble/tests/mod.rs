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

/// A fence draws no horizontal scroll, so a line longer than the pane must
/// fold inside it: nothing it paints may end past the row's right edge where
/// the user could not read it.
#[test]
fn a_long_code_fence_wraps_inside_the_pane() {
    let app = AppContext::default();
    let width = 420.0;
    let code = "let some_really_long_identifier = compute_something(one, two, three, four, five);";
    let mut element: Box<dyn Element> = Box::new(ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::code_block(Some("rust".to_string()), code)],
    ));
    let commands = render_element(&mut element, vec2f(width, 600.0), &app);
    for command in &commands {
        if let RenderCommand::DrawText {
            origin,
            text,
            font_size,
            font_weight,
            font_family,
            font_italic,
            ..
        } = command
        {
            let drawn = crate::platform::text_atlas::advance_width(
                text,
                *font_size,
                *font_weight,
                *font_family,
                *font_italic,
            );
            assert!(
                origin.x + drawn <= width + 0.5,
                "{text:?} ends at {} past the {width}-wide row",
                origin.x + drawn
            );
        }
    }
}

/// A table is drawn as a full grid: an outline around the whole table, a rule
/// between every pair of columns, and a rule between every pair of rows.
#[test]
fn a_table_draws_a_full_grid() {
    let app = AppContext::default();
    let width = 600.0;
    let mut element: Box<dyn Element> = Box::new(ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::table(
            vec!["File".into(), "Summary".into(), "Notes".into()],
            vec![
                vec!["a.rs".into(), "beta".into(), "gamma".into()],
                vec!["b.rs".into(), "delta".into(), "epsilon".into()],
            ],
        )],
    ));
    let commands = render_element(&mut element, vec2f(width, 900.0), &app);
    let rows = 3;
    let columns = 3;

    let outline = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::StrokeRect { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("the table must be outlined");

    let border = app.theme.color(ColorToken::Border);
    let horizontal: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == border && rect.height() == 1.0 && rect.width() > 1.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    let vertical: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == border && rect.width() == 1.0 && rect.height() > 1.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .collect();

    // Header plus every body row but the last: the outline closes the bottom.
    assert_eq!(
        horizontal.len(),
        rows - 1,
        "one rule under each row but the last, got {horizontal:?}"
    );
    for rule in &horizontal {
        assert!(
            (rule.width() - outline.width()).abs() < 0.5 && (rule.min_x() - outline.min_x()).abs() < 0.5,
            "a row rule spans the table: {rule:?} against outline {outline:?}"
        );
    }
    for pair in horizontal.windows(2) {
        assert!(
            pair[1].min_y() > pair[0].min_y(),
            "row rules sit on distinct boundaries, got {horizontal:?}"
        );
    }

    assert_eq!(
        vertical.len(),
        (columns - 1) * rows,
        "one rule between each pair of columns, in every row, got {vertical:?}"
    );
    let mut xs: Vec<f32> = vertical.iter().map(|rule| rule.min_x()).collect();
    xs.sort_by(f32::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    assert_eq!(xs.len(), columns - 1, "one rule per column boundary, got {xs:?}");
    for x in &xs {
        assert!(
            *x > outline.min_x() + 1.0 && *x < outline.max_x() - 1.0,
            "a column rule stands inside the outline, at {x}"
        );
        let seen = vertical
            .iter()
            .filter(|rule| (rule.min_x() - x).abs() < 0.5)
            .count();
        assert_eq!(seen, rows, "the rule at {x} must run through every row");
    }
    let top = vertical
        .iter()
        .map(|rule| rule.min_y())
        .fold(f32::INFINITY, f32::min);
    let bottom = vertical
        .iter()
        .map(|rule| rule.max_y())
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        (bottom - top - outline.height()).abs() < 0.5,
        "the column rules run the table's height: {} against outline {outline:?}",
        bottom - top
    );
}

/// A table's columns share one set of widths: a body cell starts at the same x
/// as its header, and no cell — including an unbroken token — runs past the
/// bubble's right edge.
#[test]
fn a_table_aligns_its_columns_and_stays_inside_the_pane() {
    let app = AppContext::default();
    let width = 600.0;
    let mut element: Box<dyn Element> = Box::new(ChatMessageBubble::new(
        ChatRole::Assistant,
        vec![ChatFragment::table(
            vec!["File".into(), "Summary".into(), "Notes".into()],
            vec![
                vec!["a.rs".into(), "beta".into(), "gamma".into()],
                vec![
                    "b.rs".into(),
                    "delta".into(),
                    "a_really_long_unbroken_token_that_would_run_past_the_right_edge".into(),
                ],
            ],
        )],
    ));
    let commands = render_element(&mut element, vec2f(width, 900.0), &app);

    let mut origins: std::collections::HashMap<&str, f32> = std::collections::HashMap::new();
    for command in &commands {
        if let RenderCommand::DrawText {
            origin,
            text,
            max_width,
            ..
        } = command
        {
            // The run is broken at `max_width`, so that is where its ink ends:
            // an over-wide token wraps inside the cell instead of running on.
            assert!(
                origin.x + max_width <= width + 0.5,
                "{text:?} is drawn past the {width}-wide bubble"
            );
            let label = match text.as_str() {
                "File" | "Summary" | "Notes" | "a.rs" | "b.rs" | "beta" | "gamma" | "delta" => {
                    Some(text.as_str())
                }
                _ => None,
            };
            if let Some(label) = label {
                origins.insert(label, origin.x);
            }
        }
    }

    for (label, column) in [("a.rs", "File"), ("beta", "Summary"), ("b.rs", "File"), ("delta", "Summary"), ("gamma", "Notes")] {
        let x = origins
            .get(label)
            .unwrap_or_else(|| panic!("{label:?} drew: {commands:?}"));
        let header_x = origins
            .get(column)
            .unwrap_or_else(|| panic!("{column:?} drew: {commands:?}"));
        assert!(
            (x - header_x).abs() < 0.5,
            "{label:?} starts at {x}, its column {column:?} at {header_x}"
        );
    }
}

#[test]
fn a_long_command_row_wraps_inside_the_pane() {
    let token = "a".repeat(600);
    let long = call(
        "run_command",
        &format!(r#"{{"command":"{token}"}}"#),
        ToolCallStatus::Finished,
        None,
    );
    let (commands, width) =
        paint_tool_calls_in_width(vec![long], ToolDisplayMode::Collapsed, 400.0);
    assert_runs_fit(&commands, width);
}

#[test]
fn a_long_command_output_wraps_inside_the_pane() {
    let token = "b".repeat(600);
    let long = call(
        "run_command",
        r#"{"command":"ls"}"#,
        ToolCallStatus::Finished,
        Some(&token),
    );
    let (commands, width) =
        paint_tool_calls_in_width(vec![long], ToolDisplayMode::Expanded, 400.0);
    assert_runs_fit(&commands, width);
}

#[test]
fn a_long_sub_agent_row_keeps_its_open_action_in_the_pane() {
    let token = "c".repeat(600);
    let spawn = call(
        "task",
        &format!(r#"{{"description":"{token}"}}"#),
        ToolCallStatus::Running,
        None,
    );
    let identity = spawn.id.clone();
    let mut sub_agents = HashMap::new();
    sub_agents.insert(
        identity,
        record(
            "child-1",
            SubAgentRowStatus::Running,
            "reading the migration",
            Duration::from_secs(3),
            None,
        ),
    );
    let app = AppContext::default();
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(vec![spawn])
        .with_sub_agents(sub_agents)
        .with_on_action(|_| {});
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
    // The header takes the row's remaining width and the action keeps its place
    // at the right edge: its label starts and ends inside the pane.
    for command in &commands {
        if let RenderCommand::DrawText { origin, .. } = command {
            assert!(
                origin.x <= size.x + 0.5,
                "a run starts past the {}-wide pane: {command:?}",
                size.x
            );
        }
    }
    let (origin, font_size, font_weight, font_family, font_italic) = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                origin,
                text,
                font_size,
                font_weight,
                font_family,
                font_italic,
                ..
            } if text == "open child" => {
                Some((*origin, *font_size, *font_weight, *font_family, *font_italic))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("the open action stays drawn: {:?}", drawn_texts(&commands)));
    let drawn = crate::platform::text_atlas::advance_width(
        "open child",
        font_size,
        font_weight,
        font_family,
        font_italic,
    );
    assert!(
        origin.x + drawn <= size.x + 0.5,
        "the open action ends at {} past the {}-wide pane",
        origin.x + drawn,
        size.x
    );
}

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
/// 10-15 px of padding. The user's row is led by the prompt marker on its
/// first line, which pushes that row's text one marker-width to the right.
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

    // The agent's reply keeps the transcript's padding at the edge.
    let reply_x = drawn_text_x(&reply, "Salut!");
    assert!(
        (10.0..=15.0).contains(&reply_x),
        "the agent's reply keeps 10-15 px of padding, got {reply_x}"
    );

    // The user's row draws the prompt marker on its first line, and the
    // message starts past it.
    let marker = message
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawIcon {
                name, origin, size, ..
            } if name == "prompt-arrow" => Some((*origin, *size)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the user's message draws its prompt marker: {message:?}"));
    assert!(
        !reply.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, .. } if name == "prompt-arrow"
        )),
        "the agent's reply draws no prompt marker"
    );
    let first_line = 14.0 * 1.2;
    let message_y = message
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawText {
                origin, text: drawn, ..
            } if drawn == "Salut!" => Some(origin.y),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the message's first line is drawn: {message:?}"));
    assert!(
        (marker.0.y + marker.1 / 2.0 - (message_y + first_line / 2.0)).abs() <= 1.0,
        "the marker is centred on the message's first line, got y {} for a line at {message_y}",
        marker.0.y
    );
    let message_x = drawn_text_x(&message, "Salut!");
    assert!(
        message_x >= marker.0.x + marker.1 + 2.0,
        "the message starts at {message_x}, past the marker ending at {}",
        marker.0.x + marker.1
    );
}

/// A user message wraps under its first character: the prompt marker takes
/// the first column and every line of the message starts at the same x, so a
/// long prompt reads as one indented block instead of running under the
/// marker.
#[test]
fn a_wrapped_user_message_hangs_under_its_first_character() {
    let long = "the quick brown fox jumps over the lazy dog ".repeat(8);
    let (commands, _) = paint_row(ChatRole::User, &long, 260.0);

    let mut line_starts: Vec<f32> = Vec::new();
    for command in &commands {
        if let RenderCommand::DrawText {
            origin,
            text: drawn,
            ..
        } = command
        {
            if drawn.trim().is_empty() {
                continue;
            }
            if !line_starts.iter().any(|x| (*x - origin.x).abs() < 0.5) {
                line_starts.push(origin.x);
            }
        }
    }
    assert!(
        line_starts.len() == 1,
        "every wrapped line starts at one x, got {line_starts:?}"
    );
    assert!(
        line_starts[0] > 15.0,
        "the text starts past the prompt marker, got {}",
        line_starts[0]
    );
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
    let modes = mode.map(|mode| vec![mode; calls.len()]);
    paint_tool_calls_with_modes(calls, modes.as_deref())
}

/// Paint the same bubble with each call in its own fold, so a test can hold one
/// call open and leave the next collapsed.
fn paint_tool_calls_with_modes(
    calls: Vec<ToolCall>,
    modes: Option<&[ToolDisplayMode]>,
) -> (Vec<RenderCommand>, f32) {
    let fold: HashMap<String, ToolDisplayMode> = match modes {
        Some(modes) => calls
            .iter()
            .enumerate()
            .map(|(index, call)| (tool_fold_key(call, index), modes[index]))
            .collect(),
        None => HashMap::new(),
    };
    paint_tool_calls_with_fold_map(calls, fold)
}

/// The app-owned key of the verb-group run anchored by `calls[start]`, the
/// same key the renderer toggles from the run's header.
fn run_key(calls: &[ToolCall], start: usize) -> String {
    format!("verb-group:{}", tool_fold_key(&calls[start], start))
}

/// Paint `calls` with the run anchored at `start` opened, so the member rows a
/// folded run hides are on screen.
fn paint_tool_calls_with_run_open(
    calls: Vec<ToolCall>,
    start: usize,
) -> (Vec<RenderCommand>, f32) {
    let fold = HashMap::from([(run_key(&calls, start), ToolDisplayMode::Expanded)]);
    paint_tool_calls_with_fold_map(calls, fold)
}

fn paint_tool_calls_with_fold_map(
    calls: Vec<ToolCall>,
    fold: HashMap<String, ToolDisplayMode>,
) -> (Vec<RenderCommand>, f32) {
    let app = AppContext::default();
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

/// Paint `calls` at `width` with every call in `mode`, so a test can hold a
/// long operand or result against the pane's right edge.
fn paint_tool_calls_in_width(
    calls: Vec<ToolCall>,
    mode: ToolDisplayMode,
    width: f32,
) -> (Vec<RenderCommand>, f32) {
    let app = AppContext::default();
    let fold: HashMap<String, ToolDisplayMode> = calls
        .iter()
        .enumerate()
        .map(|(index, call)| (tool_fold_key(call, index), mode))
        .collect();
    let mut bubble = ChatMessageBubble::new(ChatRole::Assistant, Vec::new())
        .with_tool_calls(calls)
        .with_tool_fold(Rc::new(RefCell::new(fold)));
    let size = bubble.layout(
        SizeConstraint::loose(vec2f(width, 400.0)),
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
    (commands, size.x)
}

/// Every drawn run ends inside `width`: a long command, path or output line
/// breaks at the pane's right edge instead of running past it.
fn assert_runs_fit(commands: &[RenderCommand], width: f32) {
    for command in commands {
        if let RenderCommand::DrawText {
            origin,
            text,
            max_width,
            ..
        } = command
        {
            assert!(
                origin.x + max_width <= width + 0.5,
                "{text:?} is drawn past the {width}-wide pane (origin {origin:?}, max width {max_width})"
            );
        }
    }
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
        diff: None,
    }
}

/// Lay a bubble out and paint it, returning its render commands.
fn paint_bubble(bubble: &mut ChatMessageBubble, app: &AppContext) -> Vec<RenderCommand> {
    paint_bubble_at(bubble, app, 400.0)
}

/// Lay a bubble out at `width` and paint it. A transcript is as wide as its
/// pane, so a test that pins a row's line needs the width the app would give
/// it.
fn paint_bubble_at(
    bubble: &mut ChatMessageBubble,
    app: &AppContext,
    width: f32,
) -> Vec<RenderCommand> {
    bubble.layout(
        SizeConstraint::loose(vec2f(width, 400.0)),
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
    let fold: HashMap<String, ToolDisplayMode> = match mode {
        Some(mode) => calls
            .iter()
            .enumerate()
            .map(|(index, call)| (tool_fold_key(call, index), mode))
            .collect(),
        None => HashMap::new(),
    };
    paint_sub_agent_with_fold(calls, records, fold)
}

/// Paint a bubble with its first verb-group run opened, so the sub-agent member
/// row — and the action target beside it — is on screen while the member itself
/// stays folded.
fn paint_sub_agent_run_open(
    calls: Vec<ToolCall>,
    records: HashMap<String, SubAgentRow>,
) -> (Vec<RenderCommand>, Rc<RefCell<Vec<ChatAction>>>) {
    let fold = HashMap::from([(run_key(&calls, 0), ToolDisplayMode::Expanded)]);
    paint_sub_agent_with_fold(calls, records, fold)
}

fn paint_sub_agent_with_fold(
    calls: Vec<ToolCall>,
    records: HashMap<String, SubAgentRow>,
    fold: HashMap<String, ToolDisplayMode>,
) -> (Vec<RenderCommand>, Rc<RefCell<Vec<ChatAction>>>) {
    let app = AppContext::default();
    let (mut bubble, actions) = sub_agent_bubble(calls, records, Rc::new(RefCell::new(fold)));
    (paint_bubble_at(&mut bubble, &app, 800.0), actions)
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
    reasoning_commands_in(expanded, key, true)
}

/// Paint one reasoning row in the phase the test needs: live (`done = false`)
/// or finished.
fn reasoning_commands_in(
    expanded: &Rc<RefCell<HashMap<String, bool>>>,
    key: &str,
    done: bool,
) -> Vec<RenderCommand> {
    let app = AppContext::default();
    let message = ChatMessage::new(
        ChatRole::Assistant,
        vec![ChatFragment::reasoning(
            key,
            "contemplating",
            "weighing the options carefully",
            done,
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
