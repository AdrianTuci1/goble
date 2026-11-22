use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::tool_block_gesture::ToolBlockGesture;
use super::tool_body::build_tool_call_body;
use crate::color::ColorU;
use crate::elements::chat_content::{
    tool_fold_key, ChatAction, InlineSpan as ModelSpan, InlineStyle, SubAgentRow, ToolCall,
    ToolDisplayMode,
};
use crate::elements::chat_typography;
use crate::elements::{
    resolve_inline_span, Chip, CrossAxisAlignment, Element, Empty, Expanded, Flex, HoverRow, Icon,
    InlineText, MainAxisSize, TextSpan,
};
use crate::geometry::vec2f;
use crate::theme::{ColorToken, FontFamily, SpacingToken};
use goble_core::harness::{subagent_subject, tool_row, ToolCallStatus, ToolKind};

/// The prefix the app-owned fold map uses for a folded run, so a group's fold
/// state never collides with the call that anchors the run.
const VERB_GROUP_KEY_PREFIX: &str = "verb-group:";

/// The row's own text size: DSH's 13 px process-row tier, from the shared chat
/// type sheet. A sub-agent's card reads at the same size; only its controls are
/// a step smaller.
#[allow(dead_code)] // kept as the module's public statement of the row size
pub(super) const TOOL_ROW_FONT_SIZE: f32 = chat_typography::ROW_SIZE;

/// A sub-agent card's controls. grok-build draws a sub-agent as a single small
/// status line with an animated bullet and its control targets at the row's far
/// edge, so the card reads as chrome over the reply rather than as another
/// command the agent ran.
const SUB_AGENT_CONTROL_SIZE: f32 = 12.0;

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
    let mut resolved = resolve_inline_span(text.clone(), bold, italic, is_code, is_link, app);
    // A run that named its own colour keeps it in every style: the marker is
    // the source's statement, so it overrides the theme's text, accent or code
    // colour.
    if let Some(color) = span.color {
        resolved = resolved.with_color(color);
    }

    // Inline code that names a workspace file still opens the file, but the
    // run keeps DSH's plain code pill: no file-type icon leads it.
    if is_code {
        if let Some(path) = crate::elements::chat_content::inline_code_file_path(&text) {
            let on_action = on_action.clone();
            let path = path.to_string();
            resolved = resolved.with_on_click(move || {
                if let Some(cb) = on_action.as_ref() {
                    (cb.borrow_mut())(ChatAction::OpenFile(path.clone()));
                }
            });
        }
    }
    // A link keeps its own ink and underline-free run; DSH does not prefix a
    // destination with a glyph.

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

/// The colour of the operand a row names. DSH draws the whole tool row in one
/// tertiary ink, so every kind flattens to Goble's quiet ink; the per-kind
/// colour language is kept below as the one-line revert seam if the owner ever
/// wants grok-build's warm path / yellow command back.
fn tool_subject_color(
    _kind: ToolKind,
    _mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> ColorU {
    let _revert = |kind: ToolKind| match kind {
        ToolKind::Read
        | ToolKind::Create
        | ToolKind::Delete
        | ToolKind::Edit
        | ToolKind::List
        | ToolKind::WebFetch => app.theme.color(ColorToken::Path),
        ToolKind::Execute => app.theme.color(ColorToken::Command),
        ToolKind::Search | ToolKind::WebSearch => app.theme.color(ColorToken::Success),
        _ => app.theme.color(ColorToken::Text),
    };
    chat_typography::chat_ink_quiet(app)
}

/// A process-row run: the Settings family, so both a row's words and its
/// glyphs follow the theme family.
fn row_span(text: String, color: ColorU, app: &crate::elements::AppContext) -> TextSpan {
    TextSpan::plain(text)
        .with_color(color)
        .with_family(chat_typography::chat_font(app))
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

/// The callback a double click on a block runs: the fold goes back to
/// `Collapsed`, never on to the next state. The two presses of the gesture run
/// the block's own click first, so what this closes is the section the presses
/// left open.
fn collapse_on_double_click(
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    key: String,
) -> impl FnMut() + 'static {
    let fold = fold.clone();
    move || {
        fold.borrow_mut()
            .insert(key.clone(), ToolDisplayMode::Collapsed);
    }
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
/// delimited by its aggregated header, not by punctuation inside its words.
fn verb_group_header(
    label: &str,
    failed: usize,
    key: &str,
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    hover: &Rc<RefCell<Option<String>>>,
    member_hovered: bool,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let open = verb_group_open(fold, key);
    let hovered = hover.borrow().as_deref() == Some(key);
    // The header carries the arrow whenever the block reads as selected: open,
    // under the pointer, or with one of its member rows under it — the states
    // an open call or a run under the pointer reads in.
    let active = open || hovered || member_hovered;
    let base = app.theme.color(if active {
        ColorToken::Text
    } else {
        ColorToken::Muted
    });
    // DSH's disclosure row: plain words at 13 / 24 in the quiet ink (hover /
    // open reads primary). The group carries no glyph, only the chevron the
    // expanded body reveals through its rows.
    let mut spans = vec![row_span(label.to_string(), base, app)];
    if failed > 0 {
        spans.push(row_span(
            format!(" · {failed} failed"),
            app.theme.color(ColorToken::Error),
            app,
        ));
    }
    let words = InlineText::new(spans)
        .with_font_size(chat_typography::ROW_SIZE)
        .with_line_height(chat_typography::ROW_LH)
        .finish();
    // DSH's disclosure: the aggregated run is the one tool row with a small
    // trailing chevron (right closed, down open). A member call is plain text.
    let chevron = Icon::new(if open {
        "chevron-down"
    } else {
        "chevron-right"
    })
    .with_size(12.0)
    .with_theme_color(
        if active {
            ColorToken::Text
        } else {
            ColorToken::Muted
        },
        app,
    )
    .finish();
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(words)
        .with_child(chevron)
        .finish();

    let toggle = fold.clone();
    let key = key.to_string();
    let close_key = key.clone();
    HoverRow::new(row)
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
        // A double click folds the chain back to its header however the two presses
        // before it left the fold: the run closes, it does not walk on.
        .with_on_double_click(collapse_on_double_click(fold, close_key))
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
    spinner_phase: f32,
    on_action: &Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    app: &crate::elements::AppContext,
) -> (Box<dyn Element>, bool) {
    let key = tool_fold_key(call, index);
    let fallback = call.default_display_mode();
    // The record S4 holds for the child a call spawned, found by the call's
    // own id (`parent_call_id` is the spawn call's id). It exists only for a
    // sub-agent-shaped call, so one lookup serves the row.
    let record = sub_agents.get(&call.id);
    // A child's card is not a fold: grok-build's sub-agent block is always
    // collapsed, and the card's own controls are what enter and stop the
    // child. A stale fold entry is ignored rather than opening a body the card
    // no longer has.
    let mode = if record.is_some() {
        ToolDisplayMode::Collapsed
    } else {
        fold.borrow().get(&key).copied().unwrap_or(fallback)
    };
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
        spinner_phase,
        on_action,
        app,
    );
    // Stretch, so the header and the body are measured at the transcript's
    // width: a long command or path wraps inside the pane instead of running
    // past its right edge.
    let mut call_column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        // DSH's expanded body sits 4 px under the row that names it.
        .with_spacing(4.0)
        .with_child(header);

    if record.is_none() {
        let rows = build_tool_call_body(call, mode, record, app);
        if !rows.is_empty() {
            // DSH draws an expanded body as one quiet card under the row, its
            // left edge on the row's text origin, with the body capped and
            // scrollable past the cap.
            call_column = call_column.with_child(super::tool_body::tool_body_card(&key, rows, app));
        }
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
/// them: `◈ Ran 3 commands, Read 2 files, Edited 1 file +12/-3`. The chain opens
/// with its aggregated header, and no rail is drawn beside its rows. A spawn and
/// a question end the chain and keep their own rows. Opening the chain shows its
/// members, and a member the user opened stays visible without splitting it.
pub(super) fn build_tool_call_rows(
    tool_calls: &[ToolCall],
    fold: &Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    hover: &Rc<RefCell<Option<String>>>,
    sub_agents: &HashMap<String, SubAgentRow>,
    sub_agent_model: &str,
    spinner_phase: f32,
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
            let (element, collapsed) = single_call_column(
                &tool_calls[index],
                index,
                false,
                fold,
                hover,
                sub_agents,
                sub_agent_model,
                spinner_phase,
                on_action,
                app,
            );
            // A double click anywhere in the block closes it again. The block
            // draws no frame: an open body is its own selected read, a
            // collapsed one changes only under the pointer.
            let element = ToolBlockGesture::new(element)
                .with_on_double_click(collapse_on_double_click(fold, key))
                .finish();
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
        let member_keys: Vec<String> = (start..end)
            .map(|member| tool_fold_key(&tool_calls[member], member))
            .collect();
        let member_hovered = {
            let hovered = hover.borrow();
            let hovered = hovered.as_deref();
            member_keys.iter().any(|key| hovered == Some(key.as_str()))
        };
        let (label, _running, failed) = verb_group_label(&tool_calls[start..end], start, fold);
        // The run's gesture is attached per segment rather than around the whole
        // run: a member the user opened is a block of its own, so a double click
        // inside it closes the member rather than the run. `segment` gathers the
        // piece being drawn — the header and the folded members beside it — and
        // is flushed into `blocks` whenever an opened member interrupts it.
        let mut blocks: Vec<Box<dyn Element>> = Vec::new();
        let mut segment = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0)
            .with_child(verb_group_header(
                &label,
                failed,
                &group_key,
                fold,
                hover,
                member_hovered,
                app,
            ));
        // The header is what the first segment always holds, so it is never
        // empty; a segment started after an opened member is, until a folded
        // member lands in it.
        let mut segment_empty = false;
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
            let (element, collapsed) = single_call_column(
                &tool_calls[member],
                member,
                open,
                fold,
                hover,
                sub_agents,
                sub_agent_model,
                spinner_phase,
                on_action,
                app,
            );
            if collapsed {
                segment = segment.with_child(element);
                segment_empty = false;
                continue;
            }
            // An opened member is its own block: the run's gesture covers only
            // the header and the folded members beside it, the member's own
            // gesture covers its header and its body, and the next piece of the
            // run starts a segment of its own.
            if !segment_empty {
                blocks.push(
                    ToolBlockGesture::new(segment.finish())
                        .with_on_double_click(collapse_on_double_click(fold, group_key.clone()))
                        .finish(),
                );
            }
            segment = Flex::column()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(2.0);
            segment_empty = true;
            blocks.push(
                ToolBlockGesture::new(element)
                    .with_on_double_click(collapse_on_double_click(fold, member_key))
                    .finish(),
            );
        }
        if !segment_empty {
            blocks.push(
                ToolBlockGesture::new(segment.finish())
                    .with_on_double_click(collapse_on_double_click(fold, group_key.clone()))
                    .finish(),
            );
        }
        // The run carries one gesture over its rows and draws no frame: the
        // label carries no punctuation of its own.
        let mut group = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0);
        for block in blocks {
            group = group.with_child(block);
        }
        column = push(column, group.finish(), !open);
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
/// A spawned child draws a different row, [`sub_agent_card`]: the record's
/// live status, and the controls that enter and stop the child.
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
    spinner_phase: f32,
    on_action: &Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
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
        // The card reads the child's own record: its mark turns while the
        // child runs, and its controls are the two gestures the pane's dock
        // makes — enter the child, and stop it.
        return sub_agent_card(record, sub_agent_model, spinner_phase, on_action, app);
    }

    // The row is one wrapping flow: the verb, the operand and the detail,
    // separated by the spaces they already carry. A long operand or detail
    // breaks inside the transcript's width, and an over-wide token breaks
    // inside it too, instead of being drawn past the pane's right edge.
    //
    // DSH's tool row is a plain nested text row: one tertiary ink (hover /
    // open reads primary), its verb at the row's regular weight. No glyph,
    // marker, dot or status icon leads it.
    let hovered = hover.borrow().as_deref() == Some(key);
    let active = chain_open || hovered || mode != ToolDisplayMode::Collapsed;
    let quiet = chat_typography::chat_ink_quiet(app);
    let primary = chat_typography::chat_ink(app);
    let text_ink = if active { primary } else { quiet };
    // Deliberate deviation from DSH's `[title] gap8 [2×2 dot] gap8 [summary]`
    // split: Goble's row is one wrapping prose flow (a long path or command
    // breaks inside the pane instead of being truncated), and a fixed 2×2 box
    // cannot sit between two runs of one wrapping `InlineText`. The dot tokens
    // (`DOT_SIZE`/`DOT_RADIUS`/`DOT_GAP`) stay in the type sheet as the seam
    // for a future single-line truncating row; the row's ink and weight are
    // DSH's.
    let mut spans: Vec<TextSpan> = Vec::new();
    if !parsed.verb.is_empty() {
        spans.push(row_span(parsed.verb.to_string(), text_ink, app));
    }
    if !parsed.subject.is_empty() {
        // The operand keeps the row's ink; the per-kind colour language lives
        // in `tool_subject_color` as the revert seam.
        let color = if active {
            primary
        } else {
            tool_subject_color(parsed.kind, mode, app)
        };
        spans.push(row_span(parsed.subject.clone(), color, app));
    }
    // One space between the operand and the detail, whether the family spells
    // its detail with a leading space (` (2 entries)`, ` +1/-1`) or not. A
    // failed call's summary is DSH's error ink; a stopped one would be the warn
    // label, which Goble has no separate status for.
    let detail = parsed.detail.trim_start();
    if !detail.is_empty() {
        let color = if call.status == ToolCallStatus::Error {
            app.theme.color(ColorToken::Error)
        } else if active {
            primary
        } else {
            quiet
        };
        spans.push(row_span(format!(" {detail}"), color, app));
    }
    let words = InlineText::new(spans)
        .with_font_size(chat_typography::ROW_SIZE)
        .with_line_height(chat_typography::ROW_LH)
        .finish();
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(Expanded::new(words).with_flex(1.0).finish())
        .finish();

    let toggle = fold.clone();
    let hover_key = key.to_string();
    let toggle_key = key.to_string();
    HoverRow::new(row)
        .with_quiet_hover()
        .with_hover_key(hover.clone(), hover_key)
        .with_on_click(move || {
            let mut folds = toggle.borrow_mut();
            let current = folds.get(&toggle_key).copied().unwrap_or(fallback);
            folds.insert(toggle_key.clone(), current.next());
        })
        // A double click closes the call the row opened, without stepping it on
        // through `Truncated` and `Expanded` a third time.
        .with_on_double_click(collapse_on_double_click(fold, key.to_string()))
        .finish()
}

/// A spawned child's card, in grok-build's shape: one small line whose leading
/// mark turns while the child runs and settles into its status colour once it
/// ends, with the control that enters the child and, while it runs, the one
/// that stops it. A click anywhere on the line enters the child, the way
/// grok-build's selected sub-agent block opens its takeover.
fn sub_agent_card(
    record: &SubAgentRow,
    model: &str,
    spinner_phase: f32,
    on_action: &Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let running = record.is_running();
    let _ = (spinner_phase, record.status_affordance());
    // DSH's sub-agent row: one 13 / 24 process line in the quiet ink. The
    // status is stated in the words; no mark or icon leads the row.
    let quiet = chat_typography::chat_ink_quiet(app);
    let detail = sub_agent_detail(record, model);
    let mut spans = vec![
        row_span("Agent ".to_string(), quiet, app),
        row_span(sub_agent_subject(record), quiet, app),
    ];
    if !detail.trim().is_empty() {
        // A running child's detail already opens with its own `·`; an ended
        // one's phrase gets one here, so every card reads as one line.
        let separator = if detail.starts_with('·') {
            " "
        } else {
            " · "
        };
        spans.push(row_span(format!("{separator}{detail}"), quiet, app));
    }
    let line = InlineText::new(spans)
        .with_font_size(chat_typography::ROW_SIZE)
        .with_line_height(chat_typography::ROW_LH)
        .finish();
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(Expanded::new(line).with_flex(1.0).finish());
    let child_id = record.child_id.clone();
    if !child_id.is_empty() {
        row = row.with_child(sub_agent_control(
            "arrow-up",
            ColorToken::Muted,
            ChatAction::OpenSubAgent(child_id.clone()),
            on_action,
            app,
        ));
        if running {
            row = row.with_child(sub_agent_control(
                "x-close",
                ColorToken::Error,
                ChatAction::StopSubAgent(child_id.clone()),
                on_action,
                app,
            ));
        }
    }
    let row = row.finish();
    let Some(callback) = on_action.as_ref().filter(|_| !child_id.is_empty()) else {
        return row;
    };
    let callback = callback.clone();
    HoverRow::new(row)
        .with_quiet_hover()
        .with_on_click(move || {
            (callback.borrow_mut())(ChatAction::OpenSubAgent(child_id.clone()));
        })
        .finish()
}

/// One of a card's two icon controls: a bare hit target around the glyph,
/// firing the chat action with the child it belongs to.
fn sub_agent_control(
    icon: &'static str,
    token: ColorToken,
    action: ChatAction,
    on_action: &Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let mut chip = Chip::flat(
        Icon::new(icon)
            .with_size(SUB_AGENT_CONTROL_SIZE)
            .with_theme_color(token, app)
            .finish(),
    );
    if let Some(callback) = on_action {
        let callback = callback.clone();
        chip = chip.with_on_click(move || (callback.borrow_mut())(action.clone()));
    }
    chip.finish()
}

/// One body row of a tool call: mono, at the tool row's size.
pub(super) fn tool_body_row(
    text: impl Into<String>,
    color: ColorToken,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    // Inline flow, like the header: a long command or path breaks at the
    // transcript's width, and an over-wide token breaks inside it. DSH draws a
    // tool body with the code stack at 11 / 16.
    InlineText::new(vec![TextSpan::plain(text.into())
        .with_color(app.theme.color(color))
        .with_family(FontFamily::Mono)])
    .with_font_size(chat_typography::CODE_SIZE)
    .with_line_height(chat_typography::CODE_SMALL_LH)
    .finish()
}

/// A body row drawn from styled runs rather than one colour: a command's output
/// line, whose runs carry the colours the terminal resolved. It sits on the same
/// band the read excerpt's lines do, so every tool body reads as one surface
/// inside the reply.
pub(super) fn tool_body_span_row(
    spans: Vec<TextSpan>,
    _app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    // The one card the caller wraps the body in owns the surface, so a run
    // carries no band of its own; only the runs' terminal colours differ.
    InlineText::new(spans)
        .with_font_size(chat_typography::CODE_SIZE)
        .with_line_height(chat_typography::CODE_SMALL_LH)
        .finish()
}

/// A string argument, when the call carries one.
pub(super) fn argument_str<'a>(arguments: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    arguments.get(key).and_then(|value| value.as_str())
}
