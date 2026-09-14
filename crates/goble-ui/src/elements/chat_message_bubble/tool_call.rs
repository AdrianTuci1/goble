use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::elements::chat_content::{tool_fold_key, ChatAction, InlineSpan as ModelSpan, InlineStyle, SubAgentRow, ToolCall, ToolDisplayMode};
use crate::elements::terminal_block::TerminalBlockPlumbing;
use crate::elements::{resolve_inline_span, Chip, CrossAxisAlignment, Element, Flex, Text, TextSpan};
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily, SpacingToken};
use goble_core::harness::{subagent_subject, tool_row, ToolCallStatus, ToolKind};
use super::tool_body::build_tool_call_body;

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

/// The status affordance drawn on a tool call's first row: a glyph and the
/// colour that carries the state. Read from the call's persisted
/// [`ToolCallStatus`], never inferred from the result text.
pub(super) fn tool_status_affordance(status: ToolCallStatus) -> (&'static str, ColorToken) {
    match status {
        ToolCallStatus::Pending => ("○", ColorToken::Muted),
        ToolCallStatus::Running => ("◐", ColorToken::Accent),
        ToolCallStatus::Finished => ("●", ColorToken::Success),
        ToolCallStatus::Error => ("◆", ColorToken::Error),
    }
}

/// Tool invocations drawn as inline rows in the pager idiom: a status glyph and
/// the tool on the first row, then the body its own definition calls for.
/// Nothing is wrapped in a border, a box or a card; a call with nothing to show
/// is a single row, and a call with a body expands in place. The exception is a
/// command, whose body is the terminal block.
///
/// Each call carries the three-state fold (`Collapsed -> Truncated -> Expanded`)
/// its shape starts in; the state lives in the app-owned `fold` map so it
/// survives the per-frame rebuild, and clicking the header advances it.
pub(super) fn build_tool_call_rows(
    tool_calls: &[ToolCall],
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    sub_agents: &HashMap<String, SubAgentRow>,
    on_action: &Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    plumbing: &TerminalBlockPlumbing,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);

    for (index, call) in tool_calls.iter().enumerate() {
        let key = tool_fold_key(call, index);
        let fallback = call.default_display_mode();
        let mode = fold.borrow().get(&key).copied().unwrap_or(fallback);
        // The record S4 holds for the child a call spawned, found by the call's
        // own id (`parent_call_id` is the spawn call's id). It exists only for a
        // sub-agent-shaped call, so one lookup serves the row and its target.
        let record = sub_agents.get(&call.id);
        let header = tool_call_header(call, &key, fallback, fold, record, app);
        // A sub-agent's row carries a second hit target beside the fold toggle:
        // the one that opens the child's own view.
        let header = match sub_agent_open_target(record, on_action, app) {
            Some(open) => Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(header)
                .with_child(open)
                .finish(),
            None => header,
        };
        let mut call_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(2.0)
            .with_child(header);

        for row in build_tool_call_body(call, mode, record, plumbing, app) {
            call_column = call_column.with_child(row);
        }
        column = column.with_child(call_column.finish());
    }
    column.finish()
}

/// The row a call always draws: the status glyph, then the call parsed into the
/// words its family gives it ([`grok_row`]) — the bold verb, the operand its
/// arguments carry, and the dim detail its result states. A call is named by
/// its tool only when no family claims it, so `read_file {"path":"src/lib.rs"}`
/// draws `● Read src/lib.rs`. The row is clickable: a click advances that
/// call's fold, the mouse arm of the same toggle.
pub(super) fn tool_call_header(
    call: &ToolCall,
    key: &str,
    fallback: ToolDisplayMode,
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    sub_agent: Option<&SubAgentRow>,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let (glyph, color) = match sub_agent {
        Some(record) => record.status_affordance(),
        None => tool_status_affordance(call.status),
    };
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

    // The row is one line of runs: the mark, the verb in bold, the operand and
    // the detail, each drawn at its own weight and colour and separated by the
    // spaces they already carry.
    let run = |text: String, color: ColorToken, weight: FontWeight| -> Box<dyn Element> {
        Text::new(text)
            .with_theme_color(color, app)
            .with_font_family(FontFamily::Mono)
            .with_font_size(12.0)
            .with_weight(weight)
            .finish()
    };
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(run(format!("{glyph} "), color, FontWeight::Regular));
    if !parsed.verb.is_empty() {
        row = row.with_child(run(parsed.verb.to_string(), ColorToken::Text, FontWeight::Bold));
    }
    if !parsed.subject.is_empty() {
        row = row.with_child(run(parsed.subject.clone(), ColorToken::Text, FontWeight::Regular));
    }
    if !parsed.detail.is_empty() {
        row = row.with_child(run(
            format!(" {}", parsed.detail),
            ColorToken::Muted,
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
    app: &crate::elements::AppContext,
) -> Option<Box<dyn Element>> {
    let record = sub_agent.filter(|record| !record.child_id.is_empty())?;
    let on_action = on_action.clone()?;
    let child_id = record.child_id.clone();
    Some(
        Chip::new(
            Text::new("open child")
                .with_theme_color(ColorToken::Accent, app)
                .with_font_family(FontFamily::Mono)
                .with_font_size(12.0)
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
        .with_font_size(12.0)
        .finish()
}

/// A string argument, when the call carries one.
pub(super) fn argument_str<'a>(arguments: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    arguments.get(key).and_then(|value| value.as_str())
}
