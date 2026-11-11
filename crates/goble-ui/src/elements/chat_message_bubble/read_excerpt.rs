use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use crate::elements::chat_content::{ToolCall, ToolDisplayMode};
use crate::elements::{Container, CrossAxisAlignment, Element, Fill, Flex, InlineText, TextSpan};
use crate::syntax::HighlightedLine;
use crate::theme::{ColorToken, FontFamily};
use goble_core::harness::ToolCallStatus;
use super::tool_body::READ_TRUNCATION;

/// Lines an expanded read draws before the rest is elided. The element tree is
/// rebuilt on every frame, so the body has to cost what is on screen and not
/// what the file holds: without a cap a five-thousand-line read is five
/// thousand bands laid out, and the whole file tokenised, on each frame.
pub(super) const EXPANDED_LINE_CAP: usize = 200;

/// The blocks this thread's highlight memo keeps, and the largest block it
/// keeps. A drawn block is at most [`EXPANDED_LINE_CAP`] lines, so the memo
/// holds drawn excerpts and never a whole file.
const MEMO_ENTRIES: usize = 4;

const MEMO_MAX_BYTES: usize = 64 * 1024;

thread_local! {
    /// The last few blocks highlighted on this thread, keyed by language and
    /// text. The runs are shared, so a frame that rebuilds the same excerpt
    /// reuses its colouring instead of tokenising it again.
    static MEMO: RefCell<Vec<((String, String), Rc<Vec<HighlightedLine>>)>> =
        RefCell::new(Vec::new());
}

/// The lines a read result is addressable as: the absolute number its first
/// line carries and the content lines themselves, borrowed from the result so
/// a line costs a slice rather than a copy.
pub(crate) struct ReadExcerpt<'a> {
    pub(super) base_line: usize,
    pub(super) lines: Vec<&'a str>,
}

impl ReadExcerpt<'_> {
    /// The text of `range`'s lines, joined so the highlighter's per-line state
    /// carries across them. Only a block the fold draws is ever joined.
    fn range_text(&self, range: Range<usize>) -> String {
        self.lines[range].join("\n")
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
///
/// A failed or empty result, or a body whose gutter does not parse
/// consistently, is not addressable — the caller falls back to plain rows
/// rather than drawing wrong line numbers.
pub(crate) fn read_excerpt<'a>(
    arguments: &serde_json::Value,
    call: &'a ToolCall,
) -> Option<ReadExcerpt<'a>> {
    if call.status == ToolCallStatus::Error {
        return None;
    }
    let result = call.result.as_deref()?;
    if result.is_empty() {
        return None;
    }

    // One pass over the borrowed result: a numbered line contributes its
    // content past the gutter, an unnumbered one the line itself. Nothing is
    // copied, so a body the fold does not draw never becomes a `String`.
    let mut base_line: Option<usize> = None;
    let mut numbered = 0usize;
    let mut lines: Vec<&str> = Vec::new();
    for line in result.lines() {
        match split_line_number(line) {
            Some((number, content)) => {
                // Either every line carries a number (a `cat -n`-style body,
                // whose own numbers are the file's) or the result is not
                // cleanly numbered and its numbers cannot be trusted.
                if let Some(first) = base_line {
                    if number != first + lines.len() {
                        return None;
                    }
                } else {
                    base_line = Some(number);
                }
                numbered += 1;
                lines.push(content);
            }
            None => lines.push(line),
        }
    }

    if numbered == 0 {
        let first = read_line_offset(arguments).unwrap_or(1);
        return Some(ReadExcerpt {
            base_line: first,
            lines,
        });
    }
    if numbered != lines.len() {
        return None;
    }
    Some(ReadExcerpt {
        base_line: base_line?,
        lines,
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

/// The lines a fold puts on screen, and the text each block is coloured from.
/// Everything the fold elides is neither joined nor tokenised.
pub(super) struct DrawnLines {
    /// The head block's excerpt indices, and the text its lines join to.
    pub(super) head: Range<usize>,
    pub(super) head_text: String,
    /// The tail block a truncated fold draws, and the text its lines join to.
    pub(super) tail: Option<Range<usize>>,
    pub(super) tail_text: Option<String>,
    /// The residue between the blocks: how many lines it stands for and the
    /// label it draws. `None` when the fold leaves nothing out.
    pub(super) residue: Option<(usize, String)>,
}

/// The excerpt lines a fold draws. Truncated draws [`READ_TRUNCATION`]'s head
/// and tail; open, it draws the head up to [`EXPANDED_LINE_CAP`] lines and
/// elides the rest, so neither fold's cost follows the file's length.
pub(super) fn drawn_lines(excerpt: &ReadExcerpt<'_>, mode: ToolDisplayMode) -> DrawnLines {
    let total = excerpt.lines.len();
    let (first, last) = READ_TRUNCATION;
    if mode == ToolDisplayMode::Truncated && total > first + last {
        let head = 0..first;
        let tail = total - last..total;
        return DrawnLines {
            head_text: excerpt.range_text(head.clone()),
            tail_text: Some(excerpt.range_text(tail.clone())),
            head,
            tail: Some(tail),
            residue: Some((total - first - last, "…".to_string())),
        };
    }

    let drawn = total.min(EXPANDED_LINE_CAP);
    let head = 0..drawn;
    let elided = total - drawn;
    DrawnLines {
        head_text: excerpt.range_text(head.clone()),
        tail_text: None,
        head,
        tail: None,
        residue: (elided > 0).then(|| (elided, format!("… {elided} more lines"))),
    }
}

/// The runs for one drawn block, or `None` when the block's language does not
/// resolve and the block is drawn plain mono.
///
/// The memo is what keeps a still transcript cheap: the tree is rebuilt every
/// frame, so the same block would otherwise be tokenised again on each one. It
/// is per-thread state on the render path — no lock and no worker. A block past
/// [`MEMO_MAX_BYTES`] is coloured without being kept.
fn highlight_block(text: &str, path: &str) -> Option<Rc<Vec<HighlightedLine>>> {
    if text.len() > MEMO_MAX_BYTES {
        return crate::syntax::highlight(text, path).map(Rc::new);
    }
    let hit = MEMO.with(|memo| {
        memo.borrow()
            .iter()
            .find(|((language, body), _)| language == path && body == text)
            .map(|(_, runs)| Rc::clone(runs))
    });
    if hit.is_some() {
        return hit;
    }
    let runs = Rc::new(crate::syntax::highlight(text, path)?);
    MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        if memo.len() >= MEMO_ENTRIES {
            memo.remove(0);
        }
        memo.push(((path.to_string(), text.to_string()), Rc::clone(&runs)));
    });
    Some(runs)
}

/// The read drawn as an editor excerpt: a column of lines, each a right-aligned
/// number in its gutter and the line's content beside it, on the darkest
/// background band. Truncated draws the head and tail with the middle elided;
/// open, it draws the head and names the lines it leaves out.
pub(super) fn read_excerpt_column(
    excerpt: &ReadExcerpt<'_>,
    mode: ToolDisplayMode,
    path: &str,
    app: &crate::elements::AppContext,
) -> Box<dyn Element> {
    let drawn = drawn_lines(excerpt, mode);
    let gutter_width = digit_count(excerpt.base_line + excerpt.lines.len().saturating_sub(1));
    // Only the drawn blocks are handed to the highlighter, and each block's
    // runs are indexed from the block's own first line.
    let head_runs = highlight_block(&drawn.head_text, path);
    let tail_runs = drawn
        .tail_text
        .as_deref()
        .and_then(|text| highlight_block(text, path));

    let mut column = Flex::column().with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    for index in drawn.head.clone() {
        column = column.with_child(read_excerpt_line(
            excerpt.base_line + index,
            gutter_width,
            excerpt.lines[index],
            head_runs
                .as_deref()
                .and_then(|lines| lines.get(index - drawn.head.start)),
            app,
        ));
    }
    if let Some((_, label)) = &drawn.residue {
        column = column.with_child(read_excerpt_band(
            vec![TextSpan::plain(label.clone())
                .with_family(FontFamily::Mono)
                .with_color(app.theme.color(ColorToken::Muted))],
            app,
        ));
    }
    if let Some(tail) = &drawn.tail {
        for index in tail.clone() {
            column = column.with_child(read_excerpt_line(
                excerpt.base_line + index,
                gutter_width,
                excerpt.lines[index],
                tail_runs
                    .as_deref()
                    .and_then(|lines| lines.get(index - tail.start)),
                app,
            ));
        }
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
