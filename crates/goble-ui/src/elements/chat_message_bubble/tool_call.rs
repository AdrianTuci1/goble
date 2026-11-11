use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::chat_content::{tool_fold_key, ChatAction, InlineSpan as ModelSpan, InlineStyle, SubAgentRow, ToolCall, ToolDisplayMode};
use crate::elements::{resolve_inline_span, CrossAxisAlignment, Element, Empty, Flex, HoverRow, InlineText, TextSpan};
use crate::geometry::vec2f;
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily, SpacingToken};
use goble_core::harness::{subagent_subject, tool_row, ToolCallStatus, ToolKind};
use super::quote_rail::QuoteRail;
use super::tool_body::build_tool_call_body;

/// The mark every tool row draws before its words, whatever its family or
/// state: grok-build's filled diamond.
const TOOL_MARK: &str = "◆ ";

/// The mark an aggregated chain header draws: grok-build's dotted diamond,
/// which tells a folded run apart from a single call at a glance.
const VERB_GROUP_MARK: &str = "◈ ";

/// The arrow a tool row shows while the pointer is over it. It takes the
/// diamond's place in the same two columns, so the words never move.
const HOVER_ARROW: &str = "❯ ";

/// The prefix the app-owned fold map uses for a folded run, so a group's fold
/// state never collides with the call that anchors the run.
const VERB_GROUP_KEY_PREFIX: &str = "verb-group:";

/// The row's own text size. A sub-agent row draws nothing beside it: the
/// controls that enter and stop a child belong to the pane's strip, not to the
/// reply the child was spawned from.
pub(super) const TOOL_ROW_FONT_SIZE: f32 = 13.0;

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
    let mut resolved = resolve_inline_span(text, bold, italic, is_code, is_link, app);
    // A run that named its own colour keeps it in every style: the marker is
    // the source's statement, so it overrides the theme's text, accent or code
    // colour.
    if let Some(color) = span.color {
        resolved = resolved.with_color(color);
    }

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

/// The diffstat a recorded diff states, in the row's own ` +N/-M` style. Empty
/// when the diff carries no change, so a row never states ` +0/-0`.
fn recorded_diffstat(diff: &str) -> String {
    let (mut added, mut removed) = (0usize, 0usize);
    for hunk in crate::elements::parse_unified_diff(diff) {
        added += hunk.added();
        removed += hunk.removed();
    }
    if added == 0 && removed == 0 {
        String::new()
    } else {
        format!(" +{added}/-{removed}")
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
        | ToolKind::Delete
        | ToolKind::Edit
        | ToolKind::List
        | ToolKind::WebFetch => app.theme.color(ColorToken::Path),
        ToolKind::Execute => app.theme.color(ColorToken::Command),
        ToolKind::Search | ToolKind::WebSearch => app.theme.color(ColorToken::Success),
        _ => app.theme.color(ColorToken::Text),
    }
}

/// The buckets a chain's header summarises, using grok-build's vocabulary. A
/// chain folds every tool family except a spawn and a question: a spawn draws
/// the child it started — its description and status line — and a question is
/// waiting on the user, so both render their own rows and end the chain.
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
    Command,
    EditFile,
    McpCall,
}

impl VerbGroupKind {
    /// The verb the run's header uses: present tense while any member runs,
    /// past tense once the run is still.
    fn verb(self, running: bool) -> &'static str {
        let (past, present) = match self {
            Self::File | Self::Skill => ("Read", "Reading"),
            Self::Search | Self::WebSearch | Self::MemorySearch | Self::IntegrationSearch => {
                ("Searched", "Searching")
            }
            Self::Dir => ("Listed", "Listing"),
            Self::WebFetch => ("Fetched", "Fetching"),
            Self::Command => ("Ran", "Running"),
            Self::EditFile => ("Edited", "Editing"),
            Self::McpCall => ("Called", "Calling"),
        };
        if running {
            present
        } else {
            past
        }
    }

    /// The noun, pluralized by the bucket's count.
    fn noun(self, count: usize) -> &'static str {
        let (one, many) = match self {
            Self::File | Self::EditFile => ("file", "files"),
            Self::Skill => ("skill", "skills"),
            Self::Search => ("pattern", "patterns"),
            Self::Dir => ("dir", "dirs"),
            Self::WebFetch | Self::WebSearch => ("website", "websites"),
            Self::MemorySearch => ("memory", "memories"),
            Self::IntegrationSearch | Self::McpCall => ("MCP tool", "MCP tools"),
            Self::Command => ("command", "commands"),
        };
        if count == 1 {
            one
        } else {
            many
        }
    }

    /// Whether the bucket totals a diffstat beside its count.
    fn is_edit(self) -> bool {
        self == Self::EditFile
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

/// The bucket a call joins, read from the row's parsed family. Every family has
/// a bucket except a spawn (whose row carries the child) and a question (which
/// is waiting on the user): those end the chain and draw their own row.
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
        ToolKind::Execute => Some(VerbGroupKind::Command),
        ToolKind::Create | ToolKind::Delete | ToolKind::Edit => Some(VerbGroupKind::EditFile),
        ToolKind::UseTool => Some(VerbGroupKind::McpCall),
        // A call no family claims draws its own row: folding it would trade
        // the only name it has for a count.
        ToolKind::SubAgent | ToolKind::Ask | ToolKind::Other => None,
    }
}

/// Whether a member is still running.
fn member_running(call: &ToolCall) -> bool {
    matches!(
        call.status,
        ToolCallStatus::Pending | ToolCallStatus::Running
    )
}

/// Whether a member failed.
fn member_failed(call: &ToolCall) -> bool {
    call.status == ToolCallStatus::Error
}

/// One bucket of a chain's label: how many calls of a kind it holds, the
/// distinct websites a web search reached, and the lines its edits touched.
#[derive(Default)]
struct ChainBucket {
    kind: Option<VerbGroupKind>,
    calls: usize,
    sources: std::collections::HashSet<String>,
    added: usize,
    removed: usize,
}

/// The lines a recorded diff added and removed.
fn diff_totals(diff: &str) -> (usize, usize) {
    let mut added = 0usize;
    let mut removed = 0usize;
    for hunk in crate::elements::parse_unified_diff(diff) {
        added += hunk.added();
        removed += hunk.removed();
    }
    (added, removed)
}

/// The lines a call's change touched: its recorded diff when the harness
/// carried one, else the diffstat the call's own arguments imply. The parse
/// reads a replace's old and new text into the row's ` +A/-R` detail, which is
/// the same count a recorded diff would state.
fn call_diff_totals(call: &ToolCall) -> (usize, usize) {
    if let Some(diff) = call.diff.as_deref() {
        let totals = diff_totals(diff);
        if totals.0 + totals.1 > 0 {
            return totals;
        }
    }
    let arguments: serde_json::Value =
        serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
    parse_diffstat(&tool_row(&call.name, &arguments, call.result.as_deref()).detail)
        .unwrap_or((0, 0))
}

/// Read a row's ` +A/-R` detail back into its two counts.
fn parse_diffstat(detail: &str) -> Option<(usize, usize)> {
    let rest = detail.split('+').nth(1)?;
    let (added, removed) = rest.split_once('/')?;
    let removed = removed.strip_prefix('-')?;
    Some((added.trim().parse().ok()?, removed.trim().parse().ok()?))
}

/// The aggregated label a folded chain draws: buckets in first-appearance order,
/// each `Verb N noun`, an edit bucket's ` +A/-R` diffstat, and grok-build's
/// ` · N failed` suffix. Running flips every bucket's tense, as one flag for the
/// chain rather than per member.
fn verb_group_label(
    calls: &[ToolCall],
    start: usize,
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
) -> (String, bool, usize) {
    let mut buckets: Vec<ChainBucket> = Vec::new();
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
        running |= member_running(call);
        if member_failed(call) {
            failed += 1;
        }
        let position = buckets
            .iter()
            .position(|bucket| bucket.kind == Some(kind))
            .unwrap_or_else(|| {
                buckets.push(ChainBucket {
                    kind: Some(kind),
                    ..ChainBucket::default()
                });
                buckets.len() - 1
            });
        let Some(bucket) = buckets.get_mut(position) else {
            continue;
        };
        bucket.calls += 1;
        if kind == VerbGroupKind::WebSearch {
            if let Some(result) = call.result.as_deref() {
                bucket
                    .sources
                    .extend(goble_core::harness::web_search_sources(result));
            }
        }
        if kind.is_edit() {
            let (added, removed) = call_diff_totals(call);
            bucket.added += added;
            bucket.removed += removed;
        }
    }
    let mut label = String::new();
    for (index, bucket) in buckets.iter().enumerate() {
        let Some(kind) = bucket.kind else {
            continue;
        };
        let count = if kind == VerbGroupKind::WebSearch && !bucket.sources.is_empty() {
            bucket.sources.len()
        } else {
            bucket.calls
        };
        if index > 0 {
            label.push_str(", ");
        }
        label.push_str(kind.verb(running));
        label.push(' ');
        label.push_str(&count.to_string());
        label.push(' ');
        label.push_str(kind.noun(count));
        if kind.is_edit() && bucket.added + bucket.removed > 0 {
            label.push_str(&format!(" +{}/-{}", bucket.added, bucket.removed));
        }
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
        if !matches!(
            run_step(&tool_calls[index], mode_of(index)),
            RunStep::Member(_)
        ) {
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

/// The chain's header: its aggregated label behind a dotted diamond, and no
/// band behind it. A closed chain reads quiet; the pointer or an open chain
/// turns it white and swaps the diamond for the arrow. The failed suffix stays
/// in the error colour. A click opens or folds the whole chain. The run is
/// delimited by the rail around it, not by punctuation inside its words.
fn verb_group_header(
    label: &str,
    failed: usize,
    key: &str,
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    hover: &Rc<RefCell<Option<String>>>,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let open = verb_group_open(fold, key);
    let hovered = hover.borrow().as_deref() == Some(key);
    let active = open || hovered;
    let base = app.theme.color(if active {
        ColorToken::Text
    } else {
        ColorToken::Muted
    });
    let run = |text: String, color: ColorU, weight: FontWeight| -> TextSpan {
        TextSpan::plain(text)
            .with_color(color)
            .with_family(FontFamily::Mono)
            .with_weight(weight)
    };
    let mut spans = vec![
        run(
            if active { HOVER_ARROW } else { VERB_GROUP_MARK }.to_string(),
            base,
            FontWeight::Regular,
        ),
        run(label.to_string(), base, FontWeight::Bold),
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
    HoverRow::new(
        InlineText::new(spans)
            .with_font_size(TOOL_ROW_FONT_SIZE)
            .with_line_height(1.4)
            .finish(),
    )
    .with_quiet_hover()
    .with_hover_key(hover.clone(), key.clone())
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

/// One call drawn on its own: its header and the body its own fold calls for.
/// `chain_open` is true for a member of a chain the user opened, which draws
/// its label white whatever the member's own fold is.
fn single_call_column(
    call: &ToolCall,
    index: usize,
    chain_open: bool,
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    hover: &Rc<RefCell<Option<String>>>,
    sub_agents: &HashMap<String, SubAgentRow>,
    sub_agent_model: &str,
    app: &crate::elements::AppContext,
) -> (Box<dyn Element>, bool) {
    let key = tool_fold_key(call, index);
    let fallback = call.default_display_mode();
    let mode = fold.borrow().get(&key).copied().unwrap_or(fallback);
    // The record S4 holds for the child a call spawned, found by the call's
    // own id (`parent_call_id` is the spawn call's id). It exists only for a
    // sub-agent-shaped call, so one lookup serves the row.
    let record = sub_agents.get(&call.id);
    let header = tool_call_header(
        call,
        &key,
        fallback,
        mode,
        chain_open,
        fold,
        hover,
        record,
        sub_agent_model,
        app,
    );
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
    (call_column.finish(), mode == ToolDisplayMode::Collapsed)
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
/// Consecutive folded calls stack into one chain, exactly as grok-build folds
/// them: `◈ Ran 3 commands, Read 2 files, Edited 1 file +12/-3`. The chain is
/// delimited by a rail in the gutter rather than by punctuation in its label.
/// A spawn and a question end the chain and keep their own rows. Opening the
/// chain shows its members, and a member the user opened stays visible without
/// splitting it.
pub(super) fn build_tool_call_rows(
    tool_calls: &[ToolCall],
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    hover: &Rc<RefCell<Option<String>>>,
    sub_agents: &HashMap<String, SubAgentRow>,
    sub_agent_model: &str,
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
            let (element, collapsed) = single_call_column(
                &tool_calls[index],
                index,
                false,
                fold,
                hover,
                sub_agents,
                sub_agent_model,
                app,
            );
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
        let (label, _running, failed) = verb_group_label(&tool_calls[start..end], start, fold);
        let mut group = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0)
            .with_child(verb_group_header(
                &label, failed, &group_key, fold, hover, app,
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
                open,
                fold,
                hover,
                sub_agents,
                sub_agent_model,
                app,
            );
            group = group.with_child(element);
        }
        // The run is delimited by a rail in the gutter, the way the pager
        // brackets a block, so its extent is read without punctuation between
        // the words.
        let rail = QuoteRail::new(group.finish(), app.theme.spacing_px(SpacingToken::Sm))
            .with_color(ColorToken::Border)
            .finish();
        column = push(column, rail, !open);
        index = end.max(start + 1);
    }
    column.finish()
}

/// The operand a live child's row draws: while it runs, the delegation itself
/// (`running: “<description>”`); once it ends, the description alone, which is
/// what grok-build's finished row reads.
fn sub_agent_subject(record: &SubAgentRow) -> String {
    let description = subagent_subject(&record.description);
    if record.is_running() {
        format!("running: {description}")
    } else {
        description
    }
}

/// The dim tail a live child's row draws: what the child is doing while it
/// runs — `Responding`, and the model it is responding with when the pane knows
/// one — and its terminal phrase and outcome once it ends.
fn sub_agent_detail(record: &SubAgentRow, model: &str) -> String {
    if !record.is_running() {
        return record.status_line();
    }
    let model = model.trim();
    let doing = if model.is_empty() {
        "Responding".to_string()
    } else {
        format!("Responding ({model})")
    };
    format!("· {doing}")
}

/// The row a call draws: the call parsed into the words its family gives it
/// ([`tool_row`]) — the bold verb, the operand its arguments carry and the dim
/// detail its result states. A call is named by its tool only when no family
/// claims it. `chain_open` marks a member of a chain the user opened: it is
/// indented under the chain's header and its label reads white.
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
    chain_open: bool,
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    hover: &Rc<RefCell<Option<String>>>,
    sub_agent: Option<&SubAgentRow>,
    sub_agent_model: &str,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let arguments: serde_json::Value =
        serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
    let mut parsed = tool_row(&call.name, &arguments, call.result.as_deref());
    // A recorded change states its own diffstat: what the tool actually added
    // and removed, which a write's arguments alone cannot say.
    if let Some(diff) = call.diff.as_deref() {
        let stat = recorded_diffstat(diff);
        if !stat.is_empty() {
            parsed.detail = stat;
        }
    }
    if let Some(record) = sub_agent {
        // A child's own record carries what the arguments cannot: what the
        // child is doing and how long it has been doing it. The chat states
        // the delegation and nothing else; the controls that enter and stop
        // the child belong to the pane's strip.
        parsed.verb = "Agent ";
        parsed.subject = sub_agent_subject(record);
        parsed.detail = sub_agent_detail(record, sub_agent_model);
    } else if parsed.kind == ToolKind::SubAgent {
        // grok-build names the spawned child's row by what it is, then the
        // description the parent gave it.
        parsed.verb = "Agent ";
        parsed.subject = match argument_str(&arguments, "description") {
            Some(description) => subagent_subject(description),
            None => call.name.clone(),
        };
    }

    // The row is one line of runs: the verb in bold, the operand and the
    // detail, each at its own colour and separated by the spaces they already
    // carry. A member of an open chain is indented under its header and read
    // white; a stand-alone row starts where prose starts.
    // One wrapping flow rather than a row of runs: a long operand or detail
    // breaks inside the transcript's width, and an over-wide token breaks
    // inside it too, instead of being drawn past the pane's right edge.
    let run = |text: String, color: ColorU, weight: FontWeight| -> TextSpan {
        TextSpan::plain(text)
            .with_color(color)
            .with_family(FontFamily::Mono)
            .with_weight(weight)
    };
    // Every call wears the diamond, whatever its family or fold; the pointer
    // swaps it for the arrow in the same two columns.
    let hovered = hover.borrow().as_deref() == Some(key);
    let mark_color = if chain_open || hovered {
        ColorToken::Text
    } else {
        ColorToken::Muted
    };
    let mut spans = vec![run(
        if hovered { HOVER_ARROW } else { TOOL_MARK }.to_string(),
        app.theme.color(mark_color),
        FontWeight::Regular,
    )];
    if !parsed.verb.is_empty() {
        let token = if chain_open || hovered {
            ColorToken::Text
        } else {
            match mode {
                ToolDisplayMode::Collapsed => ColorToken::Muted,
                _ => ColorToken::Text,
            }
        };
        spans.push(run(
            parsed.verb.to_string(),
            app.theme.color(token),
            FontWeight::Bold,
        ));
    }
    if !parsed.subject.is_empty() {
        // The operand keeps its own colour while folded: it is the one thing a
        // folded row is read for. A member of an open chain reads white with
        // the rest of the chain's own words; only a thought stays quiet, and a
        // thought is never a member of a chain.
        let color = if chain_open {
            app.theme.color(ColorToken::Text)
        } else {
            tool_subject_color(parsed.kind, mode, app)
        };
        spans.push(run(
            parsed.subject.clone(),
            color,
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
    let hover_key = key.to_string();
    let toggle_key = key.to_string();
    HoverRow::new(
        InlineText::new(spans)
            .with_font_size(TOOL_ROW_FONT_SIZE)
            .with_line_height(1.4)
            .finish(),
    )
    .with_quiet_hover()
    .with_hover_key(hover.clone(), hover_key)
    .with_on_click(move || {
        let mut folds = toggle.borrow_mut();
        let current = folds.get(&toggle_key).copied().unwrap_or(fallback);
        folds.insert(toggle_key.clone(), current.next());
    })
    .finish()
}

/// One body row of a tool call: mono, at the tool row's size.
pub(super) fn tool_body_row(
    text: impl Into<String>,
    color: ColorToken,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    // Inline flow, like the header: a long command or path breaks at the
    // transcript's width, and an over-wide token breaks inside it.
    InlineText::new(vec![TextSpan::plain(text.into())
        .with_color(app.theme.color(color))
        .with_family(FontFamily::Mono)])
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
    .with_background(crate::elements::Fill::Solid(
        app.theme.color(ColorToken::Bg),
    ))
    .finish()
}

/// A string argument, when the call carries one.
pub(super) fn argument_str<'a>(arguments: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    arguments.get(key).and_then(|value| value.as_str())
}
