use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::chat_content::{tool_fold_key, ChatAction, InlineSpan as ModelSpan, InlineStyle, SubAgentRow, ToolCall, ToolDisplayMode};
use crate::elements::{resolve_inline_span, Chip, CrossAxisAlignment, Element, Empty, Flex, Text, TextSpan};
use crate::geometry::vec2f;
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily, SpacingToken};
use goble_core::harness::{subagent_subject, tool_kind_for, tool_row, ToolCallStatus, ToolKind};
use super::tool_body::build_tool_call_body;

/// The mark every tool call draws, whatever its state: grok-build's filled
/// diamond. The state is carried by the mark's colour and never by swapping the
/// glyph, so a call's row has the same shape before, while and after it runs.
pub(super) const TOOL_MARK: &str = "◆";

/// The rail drawn down the left of a call that is open. A collapsed call draws
/// none: the rail is what says the rows under a header belong to it.
pub(super) const TOOL_RAIL: &str = "┃";

/// The row's own text size. An action hanging off a row is a size down while
/// the row is collapsed and the row's own size once it is open: grok-build
/// keeps its action hints off the row entirely, in the shortcuts bar, and
/// goble draws the one action a row owns — opening a sub-agent's own view — at
/// 10 px while the row is retracted.
pub(super) const TOOL_ROW_FONT_SIZE: f32 = 12.0;

pub(super) const TOOL_ACTION_FONT_SIZE: f32 = 10.0;

/// How far a collapsed call's mark is blended into the background. grok-build
/// dims the accent of a collapsed tool diamond by half and keeps an open row's
/// at full strength.
const COLLAPSED_DIM: f32 = 0.5;

/// Resolve a model [`ModelSpan`] (text + style) into a renderable [`TextSpan`]
/// using the current theme colors. A link span also carries a click handler, so
/// the painted run is the live action target rather than only styled text.
pub(super) fn to_text_span(
    span: &ModelSpan,
    app: &crate::elements::AppContext,
    on_action: &Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
) -> TextSpan {
    let text = span.text.clone();
    let (bold, italic) = match &span.style {
        InlineStyle::Plain => (false, false),
        InlineStyle::Bold => (true, false),
        InlineStyle::Italic => (false, true),
        InlineStyle::BoldItalic => (true, true),
        InlineStyle::Code | InlineStyle::Link(_) => (false, false),
    };
    let is_code = matches!(span.style, InlineStyle::Code);
    let is_link = matches!(span.style, InlineStyle::Link(_));
    let resolved = resolve_inline_span(text, bold, italic, is_code, is_link, app);

    let InlineStyle::Link(url) = &span.style else {
        return resolved;
    };
    let on_action = on_action.clone();
    let url = url.clone();
    resolved.with_on_click(move || {
        if let Some(cb) = on_action.as_ref() {
            (cb.borrow_mut())(ChatAction::OpenUrl(url.clone()));
        }
    })
}

/// The colour a tool call's mark carries: the state, not the tool. A finished
/// read carries a quiet mark, a finished command the success colour, and a
/// failure the error colour whichever tool it was — grok-build's split, where
/// only a command's success is a success the row announces.
fn tool_mark_token(kind: ToolKind, status: ToolCallStatus) -> ColorToken {
    match status {
        ToolCallStatus::Pending | ToolCallStatus::Running => ColorToken::Accent,
        ToolCallStatus::Error => ColorToken::Error,
        ToolCallStatus::Finished => match kind {
            ToolKind::Execute => ColorToken::Success,
            _ => ColorToken::Muted,
        },
    }
}

/// The colour a call's mark is drawn in for a row with no live record: its
/// family's shape and its persisted status.
fn mark_token(call: &ToolCall, sub_agent: Option<&SubAgentRow>) -> ColorToken {
    match sub_agent {
        // A child's own record is the row's state, and its four statuses are
        // the same four the tool-call path carries.
        Some(record) => record.status_affordance().1,
        None => tool_mark_token(tool_kind_for(&call.name), call.status),
    }
}

/// The mark's colour in `mode`: blended half into the background while the call
/// is collapsed, at full strength once it is open. The rail keeps the undimmed
/// colour, so an open call's edge is the state at full strength even though its
/// mark is not.
fn tool_mark_color(
    token: ColorToken,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> ColorU {
    let color = app.theme.color(token);
    match mode {
        ToolDisplayMode::Collapsed => color.mix(&app.theme.color(ColorToken::Bg), COLLAPSED_DIM),
        _ => color,
    }
}

/// The colour of the operand a row names: a path, a command and a URL take the
/// warm colour grok-build paints them, a search's pattern and glob the success
/// colour, and a row with no operand class the row's own text colour.
fn tool_subject_color(kind: ToolKind, app: &crate::elements::AppContext) -> ColorU {
    match kind {
        ToolKind::Read
        | ToolKind::Create
        | ToolKind::Edit
        | ToolKind::List
        | ToolKind::Execute
        | ToolKind::WebFetch => app.theme.color(ColorToken::Warning),
        ToolKind::Search | ToolKind::WebSearch => app.theme.color(ColorToken::Success),
        _ => app.theme.color(ColorToken::Text),
    }
}

/// The label colour in `mode`: the row's own text while the call is open, the
/// quiet colour while it is collapsed — the same dimming grok-build applies to
/// a folded entry's label.
fn label_color(mode: ToolDisplayMode, app: &crate::elements::AppContext) -> ColorU {
    match mode {
        ToolDisplayMode::Collapsed => app.theme.color(ColorToken::Muted),
        _ => app.theme.color(ColorToken::Text),
    }
}

/// Tool invocations drawn as inline rows in the pager idiom: a status mark and
/// the tool on the first row, then the body its own definition calls for.
/// Nothing is wrapped in a border, a box or a card, and a call never carries a
/// terminal block — an agent's call is part of its reply, so its output is
/// drawn continuously under its own row. A block, with its header, copy button
/// and filter, belongs to a command the user ran, which the pane draws.
///
/// Each call carries the three-state fold (`Collapsed -> Truncated -> Expanded`)
/// its shape starts in; the state lives in the app-owned `fold` map so it
/// survives the per-frame rebuild, and clicking the header advances it.
pub(super) fn build_tool_call_rows(
    tool_calls: &[ToolCall],
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    sub_agents: &HashMap<String, SubAgentRow>,
    on_action: &Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    // The gap between two calls is a child rather than the column's own spacing,
    // because it is not the same for every pair: two consecutive collapsed calls
    // touch, and every other pair keeps the block's gap.
    let mut previous_was_collapsed: Option<bool> = None;

    for (index, call) in tool_calls.iter().enumerate() {
        let key = tool_fold_key(call, index);
        let fallback = call.default_display_mode();
        let mode = fold.borrow().get(&key).copied().unwrap_or(fallback);
        // The record S4 holds for the child a call spawned, found by the call's
        // own id (`parent_call_id` is the spawn call's id). It exists only for a
        // sub-agent-shaped call, so one lookup serves the row and its target.
        let record = sub_agents.get(&call.id);
        let header = tool_call_header(call, &key, fallback, mode, fold, record, app);
        // A sub-agent's row carries a second hit target beside the fold toggle:
        // the one that opens the child's own view.
        let header = match sub_agent_open_target(record, on_action, mode, app) {
            Some(open) => Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(2.0)
                .with_child(header)
                .with_child(open)
                .finish(),
            None => header,
        };
        let mut call_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(2.0)
            .with_child(header);

        for row in build_tool_call_body(call, mode, record, app) {
            call_column = call_column.with_child(row);
        }
        let collapsed = mode == ToolDisplayMode::Collapsed;
        if let Some(previous) = previous_was_collapsed {
            if !previous || !collapsed {
                column = column.with_child(Empty::new().with_size(vec2f(0.0, spacing)).finish());
            }
        }
        previous_was_collapsed = Some(collapsed);
        column = column.with_child(call_column.finish());
    }
    column.finish()
}

/// The row a call always draws: the rail while it is open, the status mark,
/// then the call parsed into the words its family gives it
/// ([`tool_row`]) — the bold verb, the operand its arguments carry and the dim
/// detail its result states. A call is named by its tool only when no family
/// claims it, so `read_file {"path":"src/lib.rs"}` draws `◆ Read src/lib.rs`.
/// The row is clickable: a click advances that call's fold, the mouse arm of
/// the same toggle.
///
/// The row carries no underline at any weight: grok-build paints a tool row's
/// path and URL plain even when they carry a link target, and the runs here are
/// drawn the same way.
pub(super) fn tool_call_header(
    call: &ToolCall,
    key: &str,
    fallback: ToolDisplayMode,
    mode: ToolDisplayMode,
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    sub_agent: Option<&SubAgentRow>,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    // A live child's record is the row's state; the row's own shape is whatever
    // the spawn call's family gives it.
    let token = mark_token(call, sub_agent);
    let mark = tool_mark_color(token, mode, app);
    let arguments: serde_json::Value =
        serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
    let mut parsed = tool_row(&call.name, &arguments, call.result.as_deref());
    if let Some(record) = sub_agent {
        // A child's own record carries what the arguments cannot: what the
        // child is doing and how long it has been doing it.
        parsed.subject = subagent_subject(&record.description);
        parsed.detail = record.status_line();
    } else if parsed.kind == ToolKind::SubAgent {
        parsed.subject = match argument_str(&arguments, "description") {
            Some(description) => subagent_subject(description),
            None => call.name.clone(),
        };
    }

    // The row is one line of runs: the rail, the mark, the verb in bold, the
    // operand and the detail, each at its own colour and separated by the
    // spaces they already carry.
    let run = |text: String, color: ColorU, weight: FontWeight| -> Box<dyn Element> {
        Text::new(text)
            .with_color(color)
            .with_font_family(FontFamily::Mono)
            .with_font_size(TOOL_ROW_FONT_SIZE)
            .with_weight(weight)
            .finish()
    };
    let mut row = Flex::row().with_cross_axis_alignment(CrossAxisAlignment::Center);
    if mode != ToolDisplayMode::Collapsed {
        row = row.with_child(run(
            format!("{TOOL_RAIL} "),
            app.theme.color(token),
            FontWeight::Regular,
        ));
    }
    row = row.with_child(run(
        format!("{TOOL_MARK} "),
        mark,
        FontWeight::Regular,
    ));
    if !parsed.verb.is_empty() {
        row = row.with_child(run(parsed.verb.to_string(), label_color(mode, app), FontWeight::Bold));
    }
    if !parsed.subject.is_empty() {
        // The operand keeps its own colour while folded: it is the one thing a
        // folded row is read for.
        row = row.with_child(run(
            parsed.subject.clone(),
            tool_subject_color(parsed.kind, app),
            FontWeight::Regular,
        ));
    }
    if !parsed.detail.is_empty() {
        row = row.with_child(run(
            format!(" {}", parsed.detail),
            app.theme.color(ColorToken::Muted),
            FontWeight::Regular,
        ));
    }

    let toggle = fold.clone();
    let key = key.to_string();
    Chip::new(row.finish())
        .with_on_click(move || {
            let mut folds = toggle.borrow_mut();
            let current = folds.get(&key).copied().unwrap_or(fallback);
            folds.insert(key.clone(), current.next());
        })
        .finish()
}

/// The sub-agent row's open target: a click fires [`ChatAction::OpenSubAgent`]
/// carrying the child's own conversation id, which opens the child view — the
/// child's own conversation shown in the pane, Esc back (S6). There is no
/// target without a live record or without a handler to receive it — a dead
/// affordance is not drawn.
pub(super) fn sub_agent_open_target(
    sub_agent: Option<&SubAgentRow>,
    on_action: &Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Option<Box<dyn Element>> {
    let record = sub_agent.filter(|record| !record.child_id.is_empty())?;
    let on_action = on_action.clone()?;
    let child_id = record.child_id.clone();
    let size = match mode {
        ToolDisplayMode::Collapsed => TOOL_ACTION_FONT_SIZE,
        _ => TOOL_ROW_FONT_SIZE,
    };
    Some(
        Chip::new(
            Text::new("open child")
                .with_theme_color(ColorToken::Accent, app)
                .with_font_family(FontFamily::Mono)
                .with_font_size(size)
                .finish(),
        )
        .with_on_click(move || {
            (on_action.borrow_mut())(ChatAction::OpenSubAgent(child_id.clone()));
        })
        .finish(),
    )
}

/// One body row of a tool call: mono, at the tool row's size.
pub(super) fn tool_body_row(
    text: impl Into<String>,
    color: ColorToken,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    Text::new(text.into())
        .with_theme_color(color, app)
        .with_font_family(FontFamily::Mono)
        .with_font_size(TOOL_ROW_FONT_SIZE)
        .finish()
}

/// A body row drawn from styled runs rather than one colour: a command's output
/// line, whose runs carry the colours the terminal resolved. It sits on the same
/// band the read excerpt's lines do, so every tool body reads as one surface
/// inside the reply.
pub(super) fn tool_body_span_row(
    spans: Vec<TextSpan>,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    crate::elements::Container::new(
        crate::elements::InlineText::new(spans)
            .with_font_size(TOOL_ROW_FONT_SIZE)
            .with_line_height(1.4)
            .finish(),
    )
    .with_background(crate::elements::Fill::Solid(app.theme.color(ColorToken::Bg)))
    .finish()
}

/// A string argument, when the call carries one.
pub(super) fn argument_str<'a>(arguments: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    arguments.get(key).and_then(|value| value.as_str())
}
