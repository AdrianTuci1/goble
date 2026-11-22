use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use super::read_excerpt::{read_excerpt, read_excerpt_column, EXPANDED_LINE_CAP};
use super::tool_call::{argument_str, tool_body_row, tool_body_span_row};
use crate::elements::chat_content::{SubAgentRow, SubAgentRowStatus, ToolCall, ToolDisplayMode};
use crate::elements::chat_typography;
use crate::elements::{
    parse_unified_diff, Axis, ConstrainedBox, Container, CrossAxisAlignment, Diff, DiffLine,
    DiffLineKind, EdgeInsets, Element, Flex, Hunk, InlineText, Padding, ScrollState, Scrollable,
    TerminalData, TerminalLine, TerminalStatus, TextSpan,
};
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily};
use goble_core::harness::{one_line, tool_kind_for, web_search_sources, ToolCallStatus, ToolKind};

/// First / last body lines a truncated read draws, and the fewer lines a
/// truncated command's output draws — grok-build's per-block truncation counts.
pub(super) const READ_TRUNCATION: (usize, usize) = (5, 3);

pub(super) const COMMAND_TRUNCATION: (usize, usize) = (2, 3);

/// The characters one drawn body line may carry before it is elided. A minified
/// bundle, a one-line JSON dump or a generated lock file is one line of tens of
/// kilobytes, and the wrapper shapes it again on every frame: the line is what
/// the pane draws, so the line is what is bounded.
pub(super) const MAX_LINE_CHARS: usize = 2_000;

/// The bytes one expanded body draws, its elision row aside. A result is drawn
/// so it can be read; past this many bytes the rest is elided and named, so a
/// frame's cost stops following the result's length.
pub(super) const MAX_BODY_BYTES: usize = 16 * 1024;

/// The expanded-body scroll regions this thread keeps, and how many. The tree
/// is rebuilt every frame, so a body's scroll offset has to live somewhere
/// that outlives the element instance; the app threads its own maps for the
/// tool folds and terminal filters, but an expanded body's nested scroll has no
/// such map, so it is memoised here on the render path exactly like the
/// highlighted-block memo in [`super::read_excerpt`]: no lock, no worker, and
/// bounded so a long conversation cannot grow it without limit.
const BODY_SCROLL_ENTRIES: usize = 32;

thread_local! {
    static BODY_SCROLL: RefCell<Vec<(String, Rc<RefCell<ScrollState>>)>> =
        RefCell::new(Vec::new());
}

/// The scroll region of the expanded body drawn under the tool row `key`,
/// created on first use and kept for the next frame so a wheel over an
/// expanded body actually moves it.
fn body_scroll_state(key: &str) -> Rc<RefCell<ScrollState>> {
    BODY_SCROLL.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((_, state)) = cache.iter().find(|(cached, _)| cached == key) {
            return Rc::clone(state);
        }
        let state = Rc::new(RefCell::new(ScrollState::default()));
        if cache.len() >= BODY_SCROLL_ENTRIES {
            cache.remove(0);
        }
        cache.push((key.to_string(), Rc::clone(&state)));
        state
    })
}

/// An expanded tool body as the one card DSH draws it in: the quiet code-block
/// surface, a 0.5 px hairline, the card radius, DSH's own inset, indented to
/// the row's text origin ([`chat_typography::BODY_INDENT`]) and capped with a
/// nested scroll past [`chat_typography::CARD_MAX_INNER_HEIGHT`].
pub(super) fn tool_body_card(
    key: &str,
    rows: Vec<Box<dyn Element>>,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    for row in rows {
        column = column.with_child(row);
    }
    let scroll = Scrollable::new(column.finish(), Axis::Vertical)
        .with_state(body_scroll_state(key))
        .with_fit_content(true)
        .finish();
    let card = Container::new(
        ConstrainedBox::new(scroll)
            .with_max_height(chat_typography::CARD_MAX_INNER_HEIGHT)
            .finish(),
    )
    .with_background(chat_typography::card_surface(app))
    .with_border(chat_typography::card_border(app))
    .with_corner_radius(chat_typography::card_radius(app))
    .with_padding(chat_typography::CARD_PADDING)
    .finish();
    // The body sits under the row's text origin, not under its glyph box.
    Padding::new(
        card,
        EdgeInsets::new(chat_typography::BODY_INDENT, 0.0, 0.0, 0.0),
    )
    .finish()
}

/// `line` cut to [`MAX_LINE_CHARS`] characters, with `…` where it was cut.
pub(super) fn elide_line(line: &str) -> Cow<'_, str> {
    match line.char_indices().nth(MAX_LINE_CHARS) {
        Some((cut, _)) => Cow::Owned(format!("{}…", &line[..cut])),
        None => Cow::Borrowed(line),
    }
}

/// The bytes [`elide_line`] yields for `line`: what a body's byte cap has to
/// count, without building the elided string to measure it.
pub(super) fn elided_len(line: &str) -> usize {
    match line.char_indices().nth(MAX_LINE_CHARS) {
        Some((cut, _)) => cut + '…'.len_utf8(),
        None => line.len(),
    }
}

/// The head and tail of `text`'s lines with an ellipsis between them, or all of
/// them when there are no more than `first + last`. Each line is elided to
/// [`MAX_LINE_CHARS`]: the head a fold draws is bounded whatever the file holds.
pub(super) fn truncate_lines(text: &str, truncation: (usize, usize)) -> Vec<String> {
    let (first, last) = truncation;
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= first + last {
        return lines
            .into_iter()
            .map(|line| elide_line(line).into_owned())
            .collect();
    }
    let mut truncated: Vec<String> = lines[..first]
        .iter()
        .map(|line| elide_line(line).into_owned())
        .collect();
    truncated.push("…".to_string());
    for line in &lines[lines.len() - last..] {
        truncated.push(elide_line(line).into_owned());
    }
    truncated
}

/// The head and tail of `items` with the middle elided, or all of them when
/// there are no more than `first + last`.
pub(super) fn truncate_items<T: Clone>(items: &[T], truncation: (usize, usize)) -> Vec<T> {
    let (first, last) = truncation;
    if items.len() <= first + last {
        return items.to_vec();
    }
    let mut truncated = items[..first].to_vec();
    truncated.extend_from_slice(&items[items.len() - last..]);
    truncated
}

/// The tool's result as the rows it is drawn in. Folded, it is nothing (the
/// header already carries the summary); truncated, it is the head and tail of
/// its lines; expanded, it is the whole result as one row.
pub(super) fn result_rows(
    call: &ToolCall,
    mode: ToolDisplayMode,
    truncation: (usize, usize),
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let Some(result) = call.result.as_deref() else {
        return Vec::new();
    };
    folded_text_rows(result, mode, truncation, app)
}

/// A block of text as the fold draws it: nothing folded, the head and tail
/// truncated, the head as one row expanded. The sub-agent record's own outcome
/// folds the same way a tool result does.
///
/// Expanded is bounded by [`EXPANDED_LINE_CAP`] lines and [`MAX_BODY_BYTES`]
/// bytes, and truncated elides each line to [`MAX_LINE_CHARS`]: the row is a
/// whole block of text, and laying out a file's worth of it again on each frame
/// is what makes a large result cost the transcript.
pub(super) fn folded_text_rows(
    text: &str,
    mode: ToolDisplayMode,
    truncation: (usize, usize),
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    if text.is_empty() {
        return Vec::new();
    }
    match mode {
        ToolDisplayMode::Collapsed => Vec::new(),
        ToolDisplayMode::Truncated => truncate_lines(text, truncation)
            .into_iter()
            .map(|line| tool_body_row(line, ColorToken::Muted, app))
            .collect(),
        ToolDisplayMode::Expanded => expanded_text_rows(text, app),
    }
}

/// One block of text as the rows an expanded body draws: whole lines while the
/// line and byte caps hold, each line elided to [`MAX_LINE_CHARS`], then one row
/// naming the bytes left out.
fn expanded_text_rows(text: &str, app: &crate::elements::AppContext) -> Vec<Box<dyn Element>> {
    let mut drawn = String::new();
    let mut drawn_lines = 0usize;
    let mut consumed = 0usize;
    for line in text.lines() {
        let cut = elide_line(line);
        if drawn_lines == EXPANDED_LINE_CAP
            || (!drawn.is_empty() && drawn.len() + 1 + cut.len() > MAX_BODY_BYTES)
        {
            break;
        }
        if drawn_lines > 0 {
            drawn.push('\n');
        }
        drawn.push_str(&cut);
        drawn_lines += 1;
        consumed += line.len() + 1;
    }
    let remaining = text.len().saturating_sub(consumed);
    let mut rows = Vec::with_capacity(2);
    if !drawn.is_empty() {
        rows.push(tool_body_row(drawn, ColorToken::Muted, app));
    }
    if remaining > 0 {
        rows.push(tool_body_row(
            format!("… {remaining} more bytes"),
            ColorToken::Muted,
            app,
        ));
    }
    rows
}

/// The body a tool call shows, shaped by the family its name parses into. The
/// header already names the call and its operand, so a body opens with the
/// result itself: a read's excerpt, a command's output, an edit's hunks. A
/// folded call has no body; the fold is the only thing this adds.
pub(super) fn build_tool_call_body(
    call: &ToolCall,
    mode: ToolDisplayMode,
    sub_agent: Option<&SubAgentRow>,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    if mode == ToolDisplayMode::Collapsed {
        return Vec::new();
    }
    let arguments: serde_json::Value =
        serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
    match tool_kind_for(&call.name) {
        ToolKind::Execute => command_body(&arguments, call, mode, app),
        ToolKind::Read => read_body(&arguments, call, mode, app),
        ToolKind::Create => write_body(&arguments, call, mode, app),
        ToolKind::Edit | ToolKind::Delete => diff_body(&arguments, call, mode, app),
        ToolKind::Search | ToolKind::WebSearch => search_body(call, mode, app),
        // A fetch, a listing, a memory read and a tool search all draw what the
        // tool returned and nothing else: their row already names the operand.
        ToolKind::WebFetch
        | ToolKind::List
        | ToolKind::MemorySearch
        | ToolKind::SearchTools
        | ToolKind::Ask
        | ToolKind::Skill => result_rows(call, mode, READ_TRUNCATION, app),
        ToolKind::SubAgent => sub_agent_body(&arguments, call, mode, sub_agent, app),
        ToolKind::UseTool => generic_body(call, mode, app),
        ToolKind::Other => other_body(call, mode, app),
    }
}

/// A command: what it printed, drawn continuously under the row that names it.
///
/// It is not a terminal block. An agent's call is part of its reply, so it
/// carries no block header, no copy button and no filter — those belong to a
/// command the user ran, which the pane draws as the block. The command line
/// itself is not repeated here either: the row above already names it, and
/// grok-build likewise hides a command whose row states it.
pub(super) fn command_body(
    arguments: &serde_json::Value,
    call: &ToolCall,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let Some(command) = argument_str(arguments, "command") else {
        return result_rows(call, mode, READ_TRUNCATION, app);
    };
    let output = call.result.as_deref().unwrap_or("");
    // The block's own data is the one place that resolves the terminal's ANSI
    // colours, so the output is read from it and drawn as rows rather than
    // parsed a second time. Its first line is the command, drawn by the header.
    let data = TerminalData::for_command(command, output, command_status(call.status));
    let lines: Vec<&TerminalLine> = data.lines.iter().skip(1).collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let (first, last) = COMMAND_TRUNCATION;
    let elided = mode == ToolDisplayMode::Truncated && lines.len() > first + last;
    let mut rows: Vec<Box<dyn Element>> = Vec::with_capacity(lines.len() + 1);
    let mut drawn_bytes = 0usize;
    let mut drawn_rows = 0usize;
    for (index, line) in lines.iter().enumerate() {
        let hidden = elided && index >= first && index < lines.len() - last;
        if hidden {
            // The elision is marked once, where the middle it drops begins.
            if index == first {
                rows.push(tool_body_row(
                    format!("… +{} lines", lines.len() - first - last),
                    ColorToken::Muted,
                    app,
                ));
            }
            continue;
        }
        // Each output line is its own row, so both the line and the body are
        // bounded: a minified line or a megabyte of output would otherwise be
        // shaped on every frame.
        if drawn_rows == EXPANDED_LINE_CAP
            || (drawn_rows > 0 && drawn_bytes + line.text.len() > MAX_BODY_BYTES)
        {
            let rest: usize = lines[index..].iter().map(|line| line.text.len() + 1).sum();
            rows.push(tool_body_row(
                format!("… {rest} more bytes"),
                ColorToken::Muted,
                app,
            ));
            break;
        }
        drawn_bytes += line.text.len() + 1;
        drawn_rows += 1;
        rows.push(output_row(line, call.status, app));
    }
    rows
}

/// One output line's runs cut to [`MAX_LINE_CHARS`] characters: the colours the
/// terminal resolved are kept, the tail of a minified line is not.
fn elide_spans(spans: Vec<TextSpan>) -> Vec<TextSpan> {
    let mut budget = MAX_LINE_CHARS;
    let mut out = Vec::with_capacity(spans.len());
    for mut span in spans {
        if budget == 0 {
            break;
        }
        let chars = span.text.chars().count();
        if chars > budget {
            span.text = span.text.chars().take(budget).collect();
            span.text.push('…');
            budget = 0;
        } else {
            budget -= chars;
        }
        out.push(span);
    }
    out
}

/// One line of a command's output as the runs it is drawn in: the runs the
/// terminal resolved, or the line's own text. A failed command's output is
/// drawn in the error colour whatever the terminal resolved, so the reason a
/// row is red is the reason the row reports.
pub(super) fn output_spans(
    line: &TerminalLine,
    status: ToolCallStatus,
    app: &crate::elements::AppContext,
) -> Vec<TextSpan> {
    let fallback = match status {
        ToolCallStatus::Error => ColorToken::Error,
        _ => ColorToken::Text,
    };
    if line.runs.is_empty() {
        return vec![TextSpan::plain(line.text.clone())
            .with_family(FontFamily::Mono)
            .with_color(app.theme.color(fallback))];
    }
    line.runs
        .iter()
        .map(|run| {
            let mut span = TextSpan::plain(run.text.clone())
                .with_family(FontFamily::Mono)
                .with_color(run.color.unwrap_or_else(|| app.theme.color(fallback)));
            if run.bold {
                span = span.with_weight(FontWeight::Bold);
            }
            if run.italic {
                span = span.with_italic(true);
            }
            // A tool body is never underlined, not even for an SGR 4 the command
            // printed: the row's runs are the agent's reply, and a reply is read
            // as prose. The pane's own terminal block, which reproduces a
            // command's output, keeps the attribute.
            span.with_underline(false)
        })
        .collect()
}

/// One output line drawn as its runs, on the body band.
fn output_row(
    line: &TerminalLine,
    status: ToolCallStatus,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    tool_body_span_row(elide_spans(output_spans(line, status, app)), app)
}

/// The block's state, read from the call's persisted status. A call that has
/// not reached a terminal state is the block still running.
pub(super) fn command_status(status: ToolCallStatus) -> TerminalStatus {
    match status {
        ToolCallStatus::Pending | ToolCallStatus::Running => TerminalStatus::Running,
        ToolCallStatus::Finished => TerminalStatus::Success,
        ToolCallStatus::Error => TerminalStatus::Error,
    }
}

/// A read: once open, the file's own lines as an editor excerpt. The header's
/// own row already names the path, so the body is the file. A folded read draws
/// nothing but that row.
///
/// The excerpt is a right-aligned line-number gutter and the line's content
/// through H1's highlighter ([`crate::syntax::highlight`]), each line on its
/// own background band, the shape grok-build's `render_content_lines` builds.
/// Only the lines the fold draws are joined and tokenised, so the body's cost
/// follows the pane rather than the file. A read with no usable line
/// information falls back to plain rows rather than drawing a number it cannot
/// justify.
pub(super) fn read_body(
    arguments: &serde_json::Value,
    call: &ToolCall,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let Some(excerpt) = read_excerpt(arguments, call) else {
        return result_rows(call, mode, READ_TRUNCATION, app);
    };
    let path = argument_str(arguments, "path").unwrap_or("");
    vec![read_excerpt_column(&excerpt, mode, path, app)]
}

/// A file being written: the change the write recorded, drawn as diff rows
/// under the row that names the path. A write that recorded no diff — a row an
/// older build wrote, or a write that changed nothing — falls back to the
/// tool's own confirmation, which is not a file's contents and so is never
/// numbered.
pub(super) fn write_body(
    arguments: &serde_json::Value,
    call: &ToolCall,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    match recorded_hunks(call) {
        Some(hunks) => diff_rows(hunks, argument_str(arguments, "path"), call, mode, app),
        None => path_body(call, mode, app),
    }
}

/// The confirmation a mutation reports, drawn alone: its result, which is not
/// a file's contents and so is never numbered.
pub(super) fn path_body(
    call: &ToolCall,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    result_rows(call, mode, READ_TRUNCATION, app)
}

/// A mutation of an existing file — an edit or a deletion — as diff rows under
/// the row that already names the path. The recorded diff is the one the tool
/// took while it ran, so its line numbers are the file's own and its hunks
/// carry their context. A row with no recorded diff falls back to the
/// replacement the arguments spell, which is all an older record has.
pub(super) fn diff_body(
    arguments: &serde_json::Value,
    call: &ToolCall,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let path = argument_str(arguments, "path");
    if let Some(hunks) = recorded_hunks(call) {
        return diff_rows(hunks, path, call, mode, app);
    }
    let mut rows = Vec::new();
    if let (Some(old_text), Some(new_text)) = (
        argument_str(arguments, "old_text"),
        argument_str(arguments, "new_text"),
    ) {
        rows = diff_rows(edit_hunks(old_text, new_text), path, call, mode, app);
    }
    // A call with no diff to draw (a failed edit whose replacement was never
    // found) still reports what it said.
    if rows.is_empty() {
        rows = result_rows(call, mode, READ_TRUNCATION, app);
    }
    rows
}

/// The change the harness recorded for this call, parsed. `None` when the row
/// carries no diff or the text is not a diff this build can read.
fn recorded_hunks(call: &ToolCall) -> Option<Vec<Hunk>> {
    let hunks = parse_unified_diff(call.diff.as_deref()?);
    if hunks.is_empty() {
        None
    } else {
        Some(hunks)
    }
}

/// One file change as the rows it draws: the hunks highlighted as the file's
/// language, then the tool's own words when it failed. A finished mutation's
/// confirmation only repeats the path the header already names, so a drawn
/// diff stands without it.
fn diff_rows(
    hunks: Vec<Hunk>,
    path: Option<&str>,
    call: &ToolCall,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let mut rows: Vec<Box<dyn Element>> = Vec::new();
    if !hunks.is_empty() {
        // A diff is drawn whole in both open folds, so it is bounded the same
        // way a read's excerpt is: a write that replaced a whole file would
        // otherwise lay that file out, and colour it, on every frame.
        let (hunks, elided) = capped_hunks(hunks, EXPANDED_LINE_CAP);
        let diff = match path {
            Some(path) => Diff::new(hunks).with_language(path),
            None => Diff::new(hunks),
        };
        rows.push(Box::new(diff) as Box<dyn Element>);
        if elided > 0 {
            rows.push(tool_body_row(
                format!("… {elided} more lines"),
                ColorToken::Muted,
                app,
            ));
        }
    }
    // The result is what a mutation reports when it fails ("old_text not
    // found", "no such file"), so it is shown rather than hidden behind the
    // diff.
    if rows.is_empty() || call.status == ToolCallStatus::Error {
        rows.extend(result_rows(call, mode, READ_TRUNCATION, app));
    }
    rows
}

/// The hunks a diff draws, and the lines it leaves out: at most `cap` body
/// lines, with the hunk straddling the cap cut at it and its counts taken from
/// the lines it still carries. A hunk's own header counts are the only thing
/// the cut has to keep in step; the rows carry their file line numbers.
fn capped_hunks(hunks: Vec<Hunk>, cap: usize) -> (Vec<Hunk>, usize) {
    let mut kept: Vec<Hunk> = Vec::new();
    let mut drawn = 0usize;
    let mut elided = 0usize;
    for mut hunk in hunks {
        let room = cap.saturating_sub(drawn);
        if room == 0 {
            elided += hunk.lines.len();
            continue;
        }
        if hunk.lines.len() > room {
            elided += hunk.lines.len() - room;
            hunk.lines.truncate(room);
            hunk.old_count = hunk
                .lines
                .iter()
                .filter(|line| line.kind != DiffLineKind::Added)
                .count() as u32;
            hunk.new_count = hunk
                .lines
                .iter()
                .filter(|line| line.kind != DiffLineKind::Removed)
                .count() as u32;
        }
        drawn += hunk.lines.len();
        kept.push(hunk);
    }
    (kept, elided)
}

/// The edit as one unified-diff hunk: the replaced text removed, the
/// replacement added. `edit_file` substitutes the first occurrence, so there is
/// no surrounding context to carry and the hunk is numbered from its own start.
pub(super) fn edit_hunks(old_text: &str, new_text: &str) -> Vec<Hunk> {
    let removed: Vec<&str> = old_text.lines().collect();
    let added: Vec<&str> = new_text.lines().collect();
    let mut lines = Vec::new();
    for (index, text) in removed.iter().enumerate() {
        lines.push(DiffLine {
            kind: DiffLineKind::Removed,
            old_line: Some(index as u32 + 1),
            new_line: None,
            text: text.to_string(),
        });
    }
    for (index, text) in added.iter().enumerate() {
        lines.push(DiffLine {
            kind: DiffLineKind::Added,
            old_line: None,
            new_line: Some(index as u32 + 1),
            text: text.to_string(),
        });
    }
    if lines.is_empty() {
        return Vec::new();
    }
    vec![Hunk {
        old_start: 1,
        old_count: removed.len() as u32,
        new_start: 1,
        new_count: added.len() as u32,
        section: String::new(),
        lines,
    }]
}

/// A search: the sources its result names, under the row that already quotes
/// the query it was run for.
pub(super) fn search_body(
    call: &ToolCall,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let mut rows = Vec::new();
    if let Some(result) = call.result.as_deref() {
        let sources = web_search_sources(result);
        if sources.is_empty() {
            // No parsable source (an error, say): show the result itself rather
            // than hiding what the tool said.
            rows.extend(result_rows(call, mode, READ_TRUNCATION, app));
        } else {
            let sources = if mode == ToolDisplayMode::Truncated {
                truncate_items(&sources, READ_TRUNCATION)
            } else {
                sources
            };
            for source in sources {
                rows.push(tool_body_row(
                    elide_line(&source).into_owned(),
                    ColorToken::Accent,
                    app,
                ));
            }
        }
    }
    rows
}

/// What the expanded body says when the record is all the caller has: the
/// child's own conversation is persisted under its id (S2) but is not part of
/// the record the parent's row reads, so the row states that rather than
/// drawing transcript rows it was never given. The child view (S6) is the
/// surface that reads the child's conversation.
pub(crate) const SUB_AGENT_TRANSCRIPT_NOTE: &str =
    "the record carries no rows of the child's own conversation — the child view reads them";

/// A sub-agent: what its live record carries. The child's identity and the
/// counters it has spent, then the outcome it ended with, then the line saying
/// the child's own conversation is not part of the record.
///
/// With no record held for the call — a transcript re-read after a restart, or a
/// sub-agent-shaped tool that spawns no child — the rows are the ones the call's
/// own arguments carry: its id, its input, then its result.
pub(super) fn sub_agent_body(
    arguments: &serde_json::Value,
    call: &ToolCall,
    mode: ToolDisplayMode,
    sub_agent: Option<&SubAgentRow>,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let Some(record) = sub_agent else {
        return sub_agent_argument_rows(arguments, call, mode, app);
    };
    let mut rows = vec![
        tool_body_row(
            format!(
                "sub-agent {} · type {}",
                one_line(&record.child_id),
                one_line(&record.subagent_type)
            ),
            ColorToken::Text,
            app,
        ),
        tool_body_row(record.counters_line(), ColorToken::Muted, app),
        tool_body_row(one_line(&record.status_line()), ColorToken::Muted, app),
    ];
    if let Some(outcome) = record.outcome.as_deref().filter(|text| !text.is_empty()) {
        if record.status == SubAgentRowStatus::Completed {
            // The child's output is a block of text, so it folds the way a tool
            // result does: the head and tail truncated, all of it expanded.
            rows.extend(folded_text_rows(outcome, mode, READ_TRUNCATION, app));
        } else {
            // An error or a cancellation reason is one line of the body.
            rows.push(tool_body_row(
                one_line(outcome),
                if record.status == SubAgentRowStatus::Failed {
                    ColorToken::Error
                } else {
                    ColorToken::Muted
                },
                app,
            ));
        }
    }
    rows.push(tool_body_row(
        SUB_AGENT_TRANSCRIPT_NOTE.to_string(),
        ColorToken::Muted,
        app,
    ));
    rows
}

/// The sub-agent rows a call with no live record still has: its id, the input it
/// was given, then its result.
pub(super) fn sub_agent_argument_rows(
    arguments: &serde_json::Value,
    call: &ToolCall,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let mut rows = Vec::new();
    if let Some(agent) =
        argument_str(arguments, "agent_id").or_else(|| argument_str(arguments, "id"))
    {
        rows.push(tool_body_row(agent.to_string(), ColorToken::Text, app));
    }
    if let Some(input) =
        argument_str(arguments, "input").or_else(|| argument_str(arguments, "prompt"))
    {
        rows.push(tool_body_row(
            elide_line(&one_line(input)).into_owned(),
            ColorToken::Muted,
            app,
        ));
    }
    rows.extend(result_rows(call, mode, READ_TRUNCATION, app));
    rows
}

/// A tool with no declared shape: its named arguments, then its result.
///
/// The argument payload is never printed as JSON. Each named argument is one row
/// reading `key: value` — the shape the reference draws an integration tool's
/// arguments in — with a string as it stands and a nested value as its count. A
/// payload that is empty, null or unparsable names no argument, so the body
/// opens on the result instead.
pub(super) fn generic_body(
    call: &ToolCall,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let arguments: serde_json::Value =
        serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
    let mut rows = argument_rows(&arguments, app);
    rows.extend(result_rows(call, mode, READ_TRUNCATION, app));
    rows
}

/// A tool no family claims: its result alone. The row already names the tool and
/// the one argument it could name, so the body opens on the output rather than
/// repeating the call's arguments.
pub(super) fn other_body(
    call: &ToolCall,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    result_rows(call, mode, READ_TRUNCATION, app)
}

/// One row per named argument, the name quiet and the value plain.
fn argument_rows(
    arguments: &serde_json::Value,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let Some(named) = arguments.as_object() else {
        return Vec::new();
    };
    named
        .iter()
        .map(|(key, value)| {
            InlineText::new(vec![
                TextSpan::plain(format!("{key}: "))
                    .with_family(FontFamily::Mono)
                    .with_color(app.theme.color(ColorToken::Muted)),
                TextSpan::plain(argument_value(value))
                    .with_family(FontFamily::Mono)
                    .with_color(app.theme.color(ColorToken::Text)),
            ])
            .with_font_size(chat_typography::CODE_SIZE)
            .with_line_height(chat_typography::CODE_SMALL_LH)
            .finish()
        })
        .collect()
}

/// One argument value on one line: a string as it stands, a `null`, a boolean or
/// a number as it reads, and a nested payload as how much of it there is. A
/// value is never printed as JSON: the row is the agent's reply, and a payload
/// belongs in the tool's own output, printed there if the tool chose to.
fn argument_value(value: &serde_json::Value) -> String {
    match value {
        // A string argument can be a whole file's contents (a `write_file`), so
        // the row draws its head rather than the payload on every frame.
        serde_json::Value::String(text) => elide_line(&one_line(text)).into_owned(),
        serde_json::Value::Null => "null".to_string(),
        other if other.is_boolean() || other.is_number() => other.to_string(),
        serde_json::Value::Array(items) => count_of(items.len(), "item"),
        other => match other.as_object() {
            Some(fields) => count_of(fields.len(), "field"),
            None => String::new(),
        },
    }
}

/// How many of something a nested argument holds, in the reference's own plural
/// style (`1 field`, `3 items`).
fn count_of(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}
