use crate::elements::chat_content::{SubAgentRow, SubAgentRowStatus, ToolCall, ToolDisplayMode};
use crate::elements::{Diff, DiffLine, DiffLineKind, Element, Hunk, TerminalData, TerminalStatus};
use crate::theme::ColorToken;
use goble_core::harness::{one_line, tool_kind_for, web_search_sources, ToolCallStatus, ToolKind};
use super::read_excerpt::{read_excerpt, read_excerpt_column};
use crate::elements::terminal_block::TerminalBlockPlumbing;
use super::tool_call::{argument_str, tool_body_row};

/// First / last body lines a truncated read draws, and the fewer lines a
/// truncated command's output draws — grok-build's per-block truncation counts.
pub(super) const READ_TRUNCATION: (usize, usize) = (5, 3);

pub(super) const COMMAND_TRUNCATION: (usize, usize) = (2, 3);

/// The head and tail of `text`'s lines with an ellipsis between them, or all of
/// them when there are no more than `first + last`.
pub(super) fn truncate_lines(text: &str, truncation: (usize, usize)) -> Vec<String> {
    let (first, last) = truncation;
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= first + last {
        return lines.into_iter().map(str::to_string).collect();
    }
    let mut truncated: Vec<String> = lines[..first].iter().map(|line| line.to_string()).collect();
    truncated.push("…".to_string());
    for line in &lines[lines.len() - last..] {
        truncated.push(line.to_string());
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
/// truncated, the whole thing as one row expanded. The sub-agent record's own
/// outcome folds the same way a tool result does.
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
        ToolDisplayMode::Expanded => vec![tool_body_row(text.to_string(), ColorToken::Muted, app)],
    }
}

/// The body a tool call shows, shaped by the family its name parses into. The
/// header already names the call and its operand, so a body opens with the
/// result itself: a read's excerpt, a command's block, an edit's hunks. A
/// folded call has no body; the fold is the only thing this adds.
pub(super) fn build_tool_call_body(
    call: &ToolCall,
    mode: ToolDisplayMode,
    sub_agent: Option<&SubAgentRow>,
    plumbing: &TerminalBlockPlumbing,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    if mode == ToolDisplayMode::Collapsed {
        return Vec::new();
    }
    let arguments: serde_json::Value =
        serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
    match tool_kind_for(&call.name) {
        ToolKind::Execute => command_body(&arguments, call, mode, plumbing, app),
        ToolKind::Read => read_body(&arguments, call, mode, app),
        ToolKind::Create => path_body(call, mode, app),
        ToolKind::Edit => diff_body(&arguments, call, mode, app),
        ToolKind::Search | ToolKind::WebSearch => search_body(call, mode, app),
        // A fetch, a listing, a memory read and a tool search all draw what the
        // tool returned and nothing else: their row already names the operand.
        ToolKind::WebFetch
        | ToolKind::List
        | ToolKind::MemorySearch
        | ToolKind::SearchTools
        | ToolKind::Skill => result_rows(call, mode, READ_TRUNCATION, app),
        ToolKind::SubAgent => sub_agent_body(&arguments, call, mode, sub_agent, app),
        ToolKind::UseTool | ToolKind::Other => generic_body(call, mode, app),
    }
}

/// A command: the terminal block its segment is, built from the command and its
/// output. It is the same block terminal mode draws, not a re-drawn agent-output
/// row, so one command is one block wherever it appears. Truncated, the block
/// carries the head and tail of the output; expanded, all of it.
pub(super) fn command_body(
    arguments: &serde_json::Value,
    call: &ToolCall,
    mode: ToolDisplayMode,
    plumbing: &TerminalBlockPlumbing,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let Some(command) = argument_str(arguments, "command") else {
        return result_rows(call, mode, READ_TRUNCATION, app);
    };
    let output = call.result.as_deref().unwrap_or("");
    let output = match mode {
        ToolDisplayMode::Truncated => truncate_lines(output, COMMAND_TRUNCATION).join("\n"),
        _ => output.to_string(),
    };
    let data = TerminalData::for_command(command, &output, command_status(call.status));
    vec![plumbing.element(&data)]
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
/// A read with no usable line information falls back to plain rows rather than
/// drawing a number it cannot justify.
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
    let highlighted = crate::syntax::highlight(&excerpt.text(), path);
    vec![read_excerpt_column(
        &excerpt,
        mode,
        highlighted.as_deref(),
        app,
    )]
}

/// A file being written: what the tool returned — its confirmation or its
/// error — under the row that already names the path. Its result is not a
/// file's contents, so it is never numbered.
pub(super) fn path_body(
    call: &ToolCall,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    result_rows(call, mode, READ_TRUNCATION, app)
}

/// An edit: the replaced text as diff rows, under the row that already names
/// the path. The diff is highlighted as the edited file's language, resolved
/// from its path.
pub(super) fn diff_body(
    arguments: &serde_json::Value,
    call: &ToolCall,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let mut rows = Vec::new();
    let path = argument_str(arguments, "path");
    if let (Some(old_text), Some(new_text)) = (
        argument_str(arguments, "old_text"),
        argument_str(arguments, "new_text"),
    ) {
        let hunks = edit_hunks(old_text, new_text);
        if !hunks.is_empty() {
            let diff = match path {
                Some(path) => Diff::new(hunks).with_language(path),
                None => Diff::new(hunks),
            };
            rows.push(Box::new(diff) as Box<dyn Element>);
        }
    }
    // The result is what an edit reports when it fails ("old_text not found"),
    // so it is shown rather than hidden behind the diff.
    rows.extend(result_rows(call, mode, READ_TRUNCATION, app));
    rows
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
                rows.push(tool_body_row(source, ColorToken::Accent, app));
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
        rows.push(tool_body_row(input.to_string(), ColorToken::Muted, app));
    }
    rows.extend(result_rows(call, mode, READ_TRUNCATION, app));
    rows
}

/// A tool with no declared shape: its arguments, then its result.
pub(super) fn generic_body(
    call: &ToolCall,
    mode: ToolDisplayMode,
    app: &crate::elements::AppContext,
) -> Vec<Box<dyn Element>> {
    let mut rows = Vec::new();
    if !call.arguments.is_empty() && call.arguments != "{}" {
        rows.push(tool_body_row(
            call.arguments.clone(),
            ColorToken::Muted,
            app,
        ));
    }
    rows.extend(result_rows(call, mode, READ_TRUNCATION, app));
    rows
}
