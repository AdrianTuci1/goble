use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::chat_content::{tool_fold_key, ChatAction, InlineSpan as ModelSpan, InlineStyle, SubAgentRow, SubAgentRowStatus, ToolCall, ToolDisplayMode};
use crate::elements::{resolve_inline_span, Chip, CrossAxisAlignment, Element, Empty, Expanded, Flex, InlineText, MainAxisSize, Text, TextSpan};
use crate::geometry::vec2f;
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily, SpacingToken};
use goble_core::harness::{subagent_subject, tool_row, ToolCallStatus, ToolKind};
use super::tool_body::build_tool_call_body;

/// The mark every tool call draws, whatever its state: grok-build's filled
/// diamond. The state is carried by the mark's colour and never by swapping the
/// glyph, so a call's row has the same shape before, while and after it runs.
pub(super) const TOOL_MARK: &str = "◆";

/// The chrome before a folded run's aggregated label: grok-build's dotted
/// diamond, drawn in the run's state colour (error, running or quiet).
const VERB_GROUP_MARK: &str = "◈";

/// The prefix the app-owned fold map uses for a folded run, so a group's fold
/// state never collides with the call that anchors the run.
const VERB_GROUP_KEY_PREFIX: &str = "verb-group:";

/// The row's own text size. An action hanging off a row is a size down while
/// the row is collapsed and the row's own size once it is open: grok-build
/// keeps its action hints off the row entirely, in the shortcuts bar, and
/// goble draws the one action a row owns — opening a sub-agent's own view — at
/// 10 px while the row is retracted.
pub(super) const TOOL_ROW_FONT_SIZE: f32 = 13.0;

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

/// The colour a tool call's mark carries: the state, not the tool. Every call
/// walks the same four states, and each one owns a colour a reader can tell
/// apart at a glance — queued is quiet, work in flight is amber, a call that
/// finished is green and a failure is red. The colour is the state; the mark
/// never changes shape.
fn tool_mark_token(status: ToolCallStatus) -> ColorToken {
    match status {
        ToolCallStatus::Pending => ColorToken::Muted,
        ToolCallStatus::Running => ColorToken::Warning,
        ToolCallStatus::Finished => ColorToken::Success,
        ToolCallStatus::Error => ColorToken::Error,
    }
}

/// The colour a call's mark is drawn in for a row with no live record: its
/// family's shape and its persisted status.
fn mark_token(call: &ToolCall, sub_agent: Option<&SubAgentRow>) -> ColorToken {
    match sub_agent {
        // A child's own record is the row's state, and its four statuses are
        // the same four the tool-call path carries.
        Some(record) => record.status_affordance().1,
        None => tool_mark_token(call.status),
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

/// The colour of the operand a row names: a path takes grok-build's warm
/// orange, a command its yellow, a search's pattern and glob the success
/// colour, and a row with no operand class the row's own text colour. A
/// collapsed row is muted whole, exactly as the pager folds it.
fn tool_subject_color(
    kind: ToolKind,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> ColorU {
    if mode == ToolDisplayMode::Collapsed {
        return app.theme.color(ColorToken::Muted);
    }
    match kind {
        ToolKind::Read
        | ToolKind::Create
        | ToolKind::Edit
        | ToolKind::List
        | ToolKind::WebFetch => app.theme.color(ColorToken::Path),
        ToolKind::Execute => app.theme.color(ColorToken::Command),
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

/// The buckets grok-build folds into an aggregated run header. The label-only
/// kinds (`Command`, `EditFile`, `McpCall`, `OtherTool`) are deliberately
/// absent: those calls render their own rows and end a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VerbGroupKind {
    File,
    Skill,
    Search,
    Dir,
    WebFetch,
    WebSearch,
    MemorySearch,
    IntegrationSearch,
    Subagent,
}

impl VerbGroupKind {
    /// The verb the run's header uses: present tense while any member runs,
    /// past tense once the run is still.
    fn verb(self, running: bool) -> &'static str {
        let (past, present) = match self {
            Self::File | Self::Skill => ("Read", "Reading"),
            Self::Search
            | Self::WebSearch
            | Self::MemorySearch
            | Self::IntegrationSearch => ("Searched", "Searching"),
            Self::Dir => ("Listed", "Listing"),
            Self::WebFetch => ("Fetched", "Fetching"),
            Self::Subagent => ("Ran", "Running"),
        };
        if running { present } else { past }
    }

    /// The noun, pluralized by the bucket's count.
    fn noun(self, count: usize) -> &'static str {
        let (one, many) = match self {
            Self::File => ("file", "files"),
            Self::Skill => ("skill", "skills"),
            Self::Search => ("pattern", "patterns"),
            Self::Dir => ("dir", "dirs"),
            Self::WebFetch | Self::WebSearch => ("website", "websites"),
            Self::MemorySearch => ("memory", "memories"),
            Self::IntegrationSearch => ("MCP tool", "MCP tools"),
            Self::Subagent => ("subagent", "subagents"),
        };
        if count == 1 { one } else { many }
    }
}

/// One call's place in a run, grok-build's [`RunStep`]: a folded call anchors
/// and counts; a call the user opened keeps its own rows and stays transparent;
/// anything else ends the run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunStep {
    Member(VerbGroupKind),
    Transparent(VerbGroupKind),
    Break,
}

/// Classify one call for a run walk. The group a call belongs to comes from the
/// same parse the row uses, so a skill read buckets as a skill and a plain read
/// as a file. A call's own fold decides membership: collapsed calls are what a
/// run folds, and a call the user opened is transparent exactly as it is in
/// grok-build, so opening a member never dissolves its group.
fn run_step(call: &ToolCall, mode: ToolDisplayMode) -> RunStep {
    let Some(kind) = verb_group_kind(call) else {
        return RunStep::Break;
    };
    if mode == ToolDisplayMode::Collapsed {
        RunStep::Member(kind)
    } else {
        RunStep::Transparent(kind)
    }
}

/// The bucket a call joins, read from the row's parsed family. `None` for the
/// action kinds a run must not fold across: a command, an edit, a tool
/// dispatch, or a call no family claims.
fn verb_group_kind(call: &ToolCall) -> Option<VerbGroupKind> {
    let arguments: serde_json::Value =
        serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
    let row = tool_row(&call.name, &arguments, call.result.as_deref());
    match row.kind {
        ToolKind::Read => Some(VerbGroupKind::File),
        ToolKind::Skill => Some(VerbGroupKind::Skill),
        ToolKind::List => Some(VerbGroupKind::Dir),
        ToolKind::Search => Some(VerbGroupKind::Search),
        ToolKind::WebFetch => Some(VerbGroupKind::WebFetch),
        ToolKind::WebSearch => Some(VerbGroupKind::WebSearch),
        ToolKind::MemorySearch => Some(VerbGroupKind::MemorySearch),
        ToolKind::SearchTools => Some(VerbGroupKind::IntegrationSearch),
        ToolKind::SubAgent => Some(VerbGroupKind::Subagent),
        ToolKind::Execute | ToolKind::Create | ToolKind::Edit | ToolKind::UseTool | ToolKind::Other => None,
    }
}

/// Whether a member is still running, with a sub-agent's live record taking
/// precedence over the spawn call's own status.
fn member_running(call: &ToolCall, record: Option<&SubAgentRow>) -> bool {
    match record {
        Some(record) => record.status == SubAgentRowStatus::Running,
        None => matches!(call.status, ToolCallStatus::Pending | ToolCallStatus::Running),
    }
}

/// Whether a member failed. A cancelled child is deliberate, not a failure, so
/// only a failed record feeds the header's red count.
fn member_failed(call: &ToolCall, record: Option<&SubAgentRow>) -> bool {
    match record {
        Some(record) => record.status == SubAgentRowStatus::Failed,
        None => call.status == ToolCallStatus::Error,
    }
}

/// The aggregated label a folded run draws: buckets in first-appearance order,
/// each `Verb N noun`, plus grok-build's ` · N failed` suffix. Running flips
/// every bucket's tense, as one flag for the run rather than per member.
fn verb_group_label(
    calls: &[ToolCall],
    start: usize,
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    sub_agents: &HashMap<String, SubAgentRow>,
) -> (String, bool, usize) {
    let mut buckets: Vec<(VerbGroupKind, usize)> = Vec::new();
    let mut running = false;
    let mut failed = 0usize;
    for (offset, call) in calls.iter().enumerate() {
        let index = start + offset;
        let key = tool_fold_key(call, index);
        let mode = fold
            .borrow()
            .get(&key)
            .copied()
            .unwrap_or_else(|| call.default_display_mode());
        let RunStep::Member(kind) = run_step(call, mode) else {
            continue;
        };
        let record = sub_agents.get(&call.id);
        running |= member_running(call, record);
        if member_failed(call, record) {
            failed += 1;
        }
        match buckets.iter_mut().find(|(bucket, _)| *bucket == kind) {
            Some((_, count)) => *count += 1,
            None => buckets.push((kind, 1)),
        }
    }
    let mut label = String::new();
    for (index, (kind, count)) in buckets.iter().enumerate() {
        if index > 0 {
            label.push_str(", ");
        }
        label.push_str(kind.verb(running));
        label.push(' ');
        label.push_str(&count.to_string());
        label.push(' ');
        label.push_str(kind.noun(*count));
    }
    (label, running, failed)
}

/// The app-owned key of the run anchored by `calls[start]`, stable while the
/// run's first call is: the group's fold survives the per-frame rebuild exactly
/// as a call's own fold does.
fn verb_group_key(calls: &[ToolCall], start: usize) -> String {
    format!(
        "{VERB_GROUP_KEY_PREFIX}{}",
        tool_fold_key(&calls[start], start)
    )
}

/// Whether a run is open. The group has two states — folded into its header, or
/// opened to its member rows — so anything but `Expanded` is folded.
fn verb_group_open(fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>, key: &str) -> bool {
    fold.borrow().get(key).copied() == Some(ToolDisplayMode::Expanded)
}

/// One foldable row of a tool sequence, in display order: a whole verb run
/// under its aggregated header, or a call that stands on its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ToolFoldTarget {
    Group {
        key: String,
        start: usize,
        end: usize,
    },
    Call {
        key: String,
        index: usize,
    },
}

/// The foldable rows [`build_tool_call_rows`] draws, in the order it draws
/// them. The transcript's arrow keys use this to find the last row they can
/// open or close without duplicating the run walk.
pub(crate) fn tool_fold_targets(
    tool_calls: &[ToolCall],
    fold: &HashMap<String, ToolDisplayMode>,
) -> Vec<ToolFoldTarget> {
    let mode_of = |index: usize| {
        let key = tool_fold_key(&tool_calls[index], index);
        fold.get(&key)
            .copied()
            .unwrap_or_else(|| tool_calls[index].default_display_mode())
    };
    let mut targets = Vec::new();
    let mut index = 0usize;
    while index < tool_calls.len() {
        if !matches!(run_step(&tool_calls[index], mode_of(index)), RunStep::Member(_)) {
            let key = tool_fold_key(&tool_calls[index], index);
            targets.push(ToolFoldTarget::Call { key, index });
            index += 1;
            continue;
        }
        let start = index;
        let mut end = start + 1;
        let mut stop = start + 1;
        while stop < tool_calls.len() {
            match run_step(&tool_calls[stop], mode_of(stop)) {
                RunStep::Member(_) => end = stop + 1,
                RunStep::Transparent(_) => {}
                RunStep::Break => break,
            }
            stop += 1;
        }
        let group_key = verb_group_key(tool_calls, start);
        let open = fold.get(&group_key).copied() == Some(ToolDisplayMode::Expanded);
        targets.push(ToolFoldTarget::Group {
            key: group_key,
            start,
            end,
        });
        // An open run draws its member rows, so they are foldable rows of the
        // transcript too. They follow the header in display order: Right opens
        // the newest member after the run, and Left closes the members before
        // it closes the run back to its header.
        if open {
            for member in start..end {
                targets.push(ToolFoldTarget::Call {
                    key: tool_fold_key(&tool_calls[member], member),
                    index: member,
                });
            }
        }
        index = end.max(start + 1);
    }
    targets
}

/// The folded run's header: grok-build's dotted diamond in the run's state
/// colour and the bold aggregated label, with the failed suffix in the error
/// colour. A click on the row opens or folds the whole run.
fn verb_group_header(
    label: &str,
    running: bool,
    failed: usize,
    key: &str,
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let token = if failed > 0 {
        ColorToken::Error
    } else if running {
        ColorToken::Warning
    } else {
        ColorToken::Success
    };
    let run = |text: String, color: ColorU, weight: FontWeight| -> TextSpan {
        TextSpan::plain(text)
            .with_color(color)
            .with_family(FontFamily::Mono)
            .with_weight(weight)
    };
    let mut spans = vec![
        run(
            format!("{VERB_GROUP_MARK} "),
            app.theme.color(token),
            FontWeight::Regular,
        ),
        run(
            label.to_string(),
            app.theme.color(ColorToken::Text),
            FontWeight::Bold,
        ),
    ];
    if failed > 0 {
        spans.push(run(
            format!(" · {failed} failed"),
            app.theme.color(ColorToken::Error),
            FontWeight::Regular,
        ));
    }

    let toggle = fold.clone();
    let key = key.to_string();
    Chip::flat(
        InlineText::new(spans)
            .with_font_size(TOOL_ROW_FONT_SIZE)
            .with_line_height(1.4)
            .finish(),
    )
        .with_on_click(move || {
            let mut folds = toggle.borrow_mut();
            let open = folds.get(&key).copied() == Some(ToolDisplayMode::Expanded);
            folds.insert(
                key.clone(),
                if open {
                    ToolDisplayMode::Collapsed
                } else {
                    ToolDisplayMode::Expanded
                },
            );
        })
        .finish()
}

/// One call drawn on its own: its header (with the sub-agent open target) and
/// the body its own fold calls for.
fn single_call_column(
    call: &ToolCall,
    index: usize,
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    sub_agents: &HashMap<String, SubAgentRow>,
    on_action: &Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    app: &crate::elements::AppContext,
) -> (Box<dyn Element>, bool) {
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
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(2.0)
            // The header takes the space the open action leaves, so a long
            // subject wraps inside the pane instead of pushing the action out.
            .with_child(Expanded::new(header).with_flex(1.0).finish())
            .with_child(open)
            .finish(),
        None => header,
    };
    // Stretch, so the header and the body are measured at the transcript's
    // width: a long command or path wraps inside the pane instead of running
    // past its right edge.
    let mut call_column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(2.0)
        .with_child(header);

    for row in build_tool_call_body(call, mode, record, app) {
        call_column = call_column.with_child(row);
    }
    (
        call_column.finish(),
        mode == ToolDisplayMode::Collapsed,
    )
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
///
/// Consecutive folded calls that belong to a verb-groupable family stack into
/// one run header, exactly as grok-build folds them: `◈ Read 3 files, Searched
/// 1 pattern`. A command, edit or unclassified tool ends the run and keeps its
/// own row. Opening the run shows its members, and a member the user opened
/// stays visible without splitting the run.
pub(super) fn build_tool_call_rows(
    tool_calls: &[ToolCall],
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    sub_agents: &HashMap<String, SubAgentRow>,
    on_action: &Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    // The gap between two rows is a child rather than the column's own spacing,
    // because it is not the same for every pair: two consecutive collapsed rows
    // touch, and every other pair keeps the block's gap.
    let mut previous_was_collapsed: Option<bool> = None;
    let mut push = |mut column: Flex, element: Box<dyn Element>, collapsed: bool| {
        if let Some(previous) = previous_was_collapsed {
            if !previous || !collapsed {
                column = column.with_child(Empty::new().with_size(vec2f(0.0, spacing)).finish());
            }
        }
        previous_was_collapsed = Some(collapsed);
        column.with_child(element)
    };

    let mut index = 0usize;
    while index < tool_calls.len() {
        let key = tool_fold_key(&tool_calls[index], index);
        let mode = fold
            .borrow()
            .get(&key)
            .copied()
            .unwrap_or_else(|| tool_calls[index].default_display_mode());
        // A transparent call — one the user opened — cannot anchor a run; it
        // keeps its own rows, and the next folded call starts a fresh run.
        if !matches!(run_step(&tool_calls[index], mode), RunStep::Member(_)) {
            let (element, collapsed) =
                single_call_column(&tool_calls[index], index, fold, sub_agents, on_action, app);
            column = push(column, element, collapsed);
            index += 1;
            continue;
        }

        // Walk forward to the last folded member, exactly as `scan_run_forward`:
        // a transparent member inside the span is claimed but does not extend
        // it, so a trailing opened call stays outside the run.
        let start = index;
        let mut end = start + 1;
        let mut stop = start + 1;
        while stop < tool_calls.len() {
            let key = tool_fold_key(&tool_calls[stop], stop);
            let mode = fold
                .borrow()
                .get(&key)
                .copied()
                .unwrap_or_else(|| tool_calls[stop].default_display_mode());
            match run_step(&tool_calls[stop], mode) {
                RunStep::Member(_) => {
                    end = stop + 1;
                }
                RunStep::Transparent(_) => {}
                RunStep::Break => break,
            }
            stop += 1;
        }

        let group_key = verb_group_key(tool_calls, start);
        let open = verb_group_open(fold, &group_key);
        let (label, running, failed) =
            verb_group_label(&tool_calls[start..end], start, fold, sub_agents);
        let mut group = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0)
            .with_child(verb_group_header(
                &label, running, failed, &group_key, fold, app,
            ));
        // Folded, a run shows only its header — except for a member the user
        // already opened, which grok-build keeps on screen and transparent.
        // Opened, every member is drawn, its own fold deciding its body.
        for member in start..end {
            let member_key = tool_fold_key(&tool_calls[member], member);
            let member_mode = fold
                .borrow()
                .get(&member_key)
                .copied()
                .unwrap_or_else(|| tool_calls[member].default_display_mode());
            if !open && member_mode == ToolDisplayMode::Collapsed {
                continue;
            }
            let (element, _) = single_call_column(
                &tool_calls[member],
                member,
                fold,
                sub_agents,
                on_action,
                app,
            );
            group = group.with_child(element);
        }
        column = push(column, group.finish(), !open);
        index = end.max(start + 1);
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

    // The row is one line of runs: the mark, the verb in bold, the operand and
    // the detail, each at its own colour and separated by the spaces they
    // already carry. The row starts where prose starts — no rail and no inset
    // — so a call reads as part of the reply rather than as a nested block.
    // One wrapping flow rather than a row of runs: a long operand or detail
    // breaks inside the transcript's width, and an over-wide token breaks
    // inside it too, instead of being drawn past the pane's right edge.
    let run = |text: String, color: ColorU, weight: FontWeight| -> TextSpan {
        TextSpan::plain(text)
            .with_color(color)
            .with_family(FontFamily::Mono)
            .with_weight(weight)
    };
    let mut spans = vec![run(format!("{TOOL_MARK} "), mark, FontWeight::Regular)];
    if !parsed.verb.is_empty() {
        spans.push(run(
            parsed.verb.to_string(),
            label_color(mode, app),
            FontWeight::Bold,
        ));
    }
    if !parsed.subject.is_empty() {
        // The operand keeps its own colour while folded: it is the one thing a
        // folded row is read for.
        spans.push(run(
            parsed.subject.clone(),
            tool_subject_color(parsed.kind, mode, app),
            FontWeight::Regular,
        ));
    }
    // One space between the operand and the detail, whether the family spells
    // its detail with a leading space (` (2 entries)`, ` +1/-1`) or not.
    let detail = parsed.detail.trim_start();
    if !detail.is_empty() {
        spans.push(run(
            format!(" {detail}"),
            app.theme.color(ColorToken::Muted),
            FontWeight::Regular,
        ));
    }

    let toggle = fold.clone();
    let key = key.to_string();
    Chip::flat(
        InlineText::new(spans)
            .with_font_size(TOOL_ROW_FONT_SIZE)
            .with_line_height(1.4)
            .finish(),
    )
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
    // Inline flow, like the header: a long command or path breaks at the
    // transcript's width, and an over-wide token breaks inside it.
    InlineText::new(vec![
        TextSpan::plain(text.into())
            .with_color(app.theme.color(color))
            .with_family(FontFamily::Mono),
    ])
    .with_font_size(TOOL_ROW_FONT_SIZE)
    .with_line_height(1.4)
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
