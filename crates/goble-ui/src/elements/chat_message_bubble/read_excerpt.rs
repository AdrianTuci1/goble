use crate::elements::chat_content::{ToolCall, ToolDisplayMode};
use crate::elements::{Container, CrossAxisAlignment, Element, Fill, Flex, InlineText, TextSpan};
use crate::theme::{ColorToken, FontFamily};
use goble_core::harness::ToolCallStatus;
use super::tool_body::READ_TRUNCATION;

/// The lines a read result is addressable as: the absolute number its first
/// line carries and the content lines themselves.
pub(crate) struct ReadExcerpt {
    pub(super) base_line: usize,
    pub(super) lines: Vec<String>,
}

impl ReadExcerpt {
    /// The content as one string. The highlighter takes whole text so its
    /// per-line state carries across the excerpt's lines.
    pub(super) fn text(&self) -> String {
        self.lines.join("\n")
    }
}

/// Address a read's result as file lines, or `None` when it carries no usable
/// line information.
///
/// The base number is derived from what the call actually returned. The
/// harness's `read_file` declares no offset and returns `read_to_string`'s raw
/// content, so: an explicit `offset`/`start_line` argument is used when a
/// definition provides one, a `cat -n`-style gutter already in the result is
/// kept (never re-numbered), and otherwise a whole-file read starts at line 1.
/// A failed or empty result, or a body whose gutter does not parse
/// consistently, is not addressable — the caller falls back to plain rows
/// rather than drawing wrong line numbers.
pub(crate) fn read_excerpt(arguments: &serde_json::Value, call: &ToolCall) -> Option<ReadExcerpt> {
    if call.status == ToolCallStatus::Error {
        return None;
    }
    let result = call.result.as_deref()?;
    if result.is_empty() {
        return None;
    }

    let numbered: Vec<Option<(usize, String)>> = result
        .lines()
        .map(|line| split_line_number(line).map(|(number, text)| (number, text.to_string())))
        .collect();
    let numbered_count = numbered.iter().filter(|entry| entry.is_some()).count();
    if numbered_count > 0 {
        // Either every line carries a number (a `cat -n`-style body, whose own
        // numbers are the file's) or the result is not cleanly numbered and
        // its numbers cannot be trusted.
        if numbered_count != numbered.len() {
            return None;
        }
        let mut numbers = Vec::with_capacity(numbered.len());
        let mut lines = Vec::with_capacity(numbered.len());
        for entry in numbered {
            let (number, text) = entry?;
            numbers.push(number);
            lines.push(text);
        }
        let first = numbers[0];
        if numbers.iter().enumerate().any(|(i, n)| *n != first + i) {
            return None;
        }
        return Some(ReadExcerpt {
            base_line: first,
            lines,
        });
    }

    Some(ReadExcerpt {
        base_line: read_line_offset(arguments).unwrap_or(1),
        lines: result.lines().map(str::to_string).collect(),
    })
}

/// The explicit first-line number a call declares when its definition takes a
/// range. The harness's `read_file` declares none, so a whole-file read's base
/// is 1.
pub(super) fn read_line_offset(arguments: &serde_json::Value) -> Option<usize> {
    ["offset", "start_line", "line"]
        .iter()
        .find_map(|key| arguments.get(*key).and_then(serde_json::Value::as_u64))
        .filter(|offset| *offset >= 1)
        .map(|offset| offset as usize)
}

/// Split a `cat -n`-style gutter off a line: leading spaces, a number, then a
/// tab. `None` when the line is not numbered.
pub(super) fn split_line_number(line: &str) -> Option<(usize, &str)> {
    let digits_start = line.find(|c: char| c != ' ')?;
    let rest = &line[digits_start..];
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    let number = rest[..digits_end].parse().ok()?;
    rest[digits_end..]
        .strip_prefix('\t')
        .map(|text| (number, text))
}

/// The width of a line-number gutter: the number of decimal digits in the
/// largest number the excerpt carries.
pub(super) fn digit_count(mut number: usize) -> usize {
    let mut digits = 1;
    while number >= 10 {
        number /= 10;
        digits += 1;
    }
    digits
}

/// The read drawn as an editor excerpt: a column of lines, each a right-aligned
/// number in its gutter and the line's content beside it, on the darkest
/// background band. Truncated draws the head and tail with the middle elided.
pub(super) fn read_excerpt_column(
    excerpt: &ReadExcerpt,
    mode: ToolDisplayMode,
    highlighted: Option<&[crate::syntax::HighlightedLine]>,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let gutter_width = digit_count(excerpt.base_line + excerpt.lines.len().saturating_sub(1));
    let total = excerpt.lines.len();
    let (first, last) = READ_TRUNCATION;

    let mut indices: Vec<Option<usize>> = Vec::new();
    if mode == ToolDisplayMode::Truncated && total > first + last {
        indices.extend((0..first).map(Some));
        indices.push(None);
        indices.extend((total - last..total).map(Some));
    } else {
        indices.extend((0..total).map(Some));
    }

    let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    for index in indices {
        let line = match index {
            Some(index) => read_excerpt_line(
                excerpt.base_line + index,
                gutter_width,
                &excerpt.lines[index],
                highlighted.and_then(|lines| lines.get(index)),
                app,
            ),
            None => read_excerpt_band(
                vec![TextSpan::plain("…")
                    .with_family(FontFamily::Mono)
                    .with_color(app.theme.color(ColorToken::Muted))],
                app,
            ),
        };
        column = column.with_child(line);
    }
    column.finish()
}

/// One excerpt line: the right-aligned number in the gutter, then the line's
/// content — highlighted when the language resolved, plain mono otherwise — on
/// the band.
pub(super) fn read_excerpt_line(
    number: usize,
    gutter_width: usize,
    text: &str,
    runs: Option<&crate::syntax::HighlightedLine>,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let mut spans = vec![
        TextSpan::plain(format!("{:>w$}  ", number, w = gutter_width))
            .with_family(FontFamily::Mono)
            .with_color(app.theme.color(ColorToken::Muted)),
    ];
    match runs {
        Some(runs) if !runs.is_empty() => spans.extend(runs.iter().cloned()),
        _ => spans.push(
            TextSpan::plain(text.to_string())
                .with_family(FontFamily::Mono)
                .with_color(app.theme.color(ColorToken::Text)),
        ),
    }
    read_excerpt_band(spans, app)
}

/// A band row of the excerpt: the runs drawn on the darkest background, so the
/// file reads as its own surface within the bubble.
pub(super) fn read_excerpt_band(spans: Vec<TextSpan>, app: &crate::elements::AppContext) -> Box<dyn Element> {
    Container::new(
        InlineText::new(spans)
            .with_font_size(12.0)
            .with_line_height(1.4)
            .finish(),
    )
    .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
    .finish()
}
