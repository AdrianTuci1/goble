//! Vim's text objects: the ranges `i` and `a` hand to an operator.
//!
//! Every function takes the buffer as a slice of characters and works in
//! character indices, which is what the editor's own insertion point uses, and
//! returns a half-open range.

use std::ops::Range;

use super::words::{CharKind, WordKind};

/// A bracket pair a block text object selects around (`i(`, `a{`, `i[`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bracket {
    Parenthesis,
    CurlyBrace,
    SquareBracket,
}

impl Bracket {
    /// The bracket `c` names, and whether it opens its pair.
    pub fn of(c: char) -> Option<(Self, bool)> {
        match c {
            '(' | ')' => Some((Self::Parenthesis, c == '(')),
            '{' | '}' => Some((Self::CurlyBrace, c == '{')),
            '[' | ']' => Some((Self::SquareBracket, c == '[')),
            _ => None,
        }
    }

    fn chars(self) -> (char, char) {
        match self {
            Self::Parenthesis => ('(', ')'),
            Self::CurlyBrace => ('{', '}'),
            Self::SquareBracket => ('[', ']'),
        }
    }
}

/// A quote character a quote text object selects around (`i"`, `a'`, `` i` ``).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quote {
    Single,
    Double,
    Backtick,
}

impl Quote {
    /// The quote `c` names, if it is one.
    pub fn of(c: char) -> Option<Self> {
        match c {
            '\'' => Some(Self::Single),
            '"' => Some(Self::Double),
            '`' => Some(Self::Backtick),
            _ => None,
        }
    }

    fn is_char(self, c: char) -> bool {
        c == self.char()
    }

    fn char(self) -> char {
        match self {
            Self::Single => '\'',
            Self::Double => '"',
            Self::Backtick => '`',
        }
    }
}

/// The index of the bracket that pairs with the one at `at` (`%`, and the
/// partner search behind `i(`/`a(`).
///
/// `from_opening` says the bracket at `at` opens a pair, so the search runs
/// forward; otherwise it runs backward over what precedes `at`. Nested pairs of
/// the same kind are counted, so `(a (b) c)` on the first `(` pairs with the
/// last `)`.
pub fn matching_bracket(
    chars: &[char],
    at: usize,
    bracket: Bracket,
    from_opening: bool,
) -> Option<usize> {
    let (open, close) = bracket.chars();
    let mut depth = 0u32;
    if from_opening {
        let mut i = at + 1;
        while i < chars.len() {
            if chars[i] == open {
                depth += 1;
            } else if chars[i] == close {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            i += 1;
        }
    } else {
        let mut i = at;
        while i > 0 {
            let c = chars[i - 1];
            if c == close {
                depth += 1;
            } else if c == open {
                if depth == 0 {
                    return Some(i - 1);
                }
                depth -= 1;
            }
            i -= 1;
        }
    }
    None
}

/// Vim's `a(`-family object: the text enclosed by a bracket pair, brackets
/// included.
pub fn a_block(chars: &[char], at: usize, bracket: Bracket) -> Option<Range<usize>> {
    let c = *chars.get(at)?;
    match Bracket::of(c) {
        // The cursor already sits on a bracket of the right kind: pair it.
        Some((found, opens)) if found == bracket => {
            let other = matching_bracket(chars, at, bracket, opens)?;
            let (start, end) = if other > at { (at, other) } else { (other, at) };
            Some(start..end + 1)
        }
        // Otherwise look for the nearest enclosing pair, opening first.
        _ => {
            let end = matching_bracket(chars, at, bracket, true)?;
            let start = matching_bracket(chars, end, bracket, false)?;
            Some(start..end + 1)
        }
    }
}

/// Vim's `i(`-family object: the text inside a bracket pair, brackets excluded.
///
/// Whitespace between the opening bracket and the first line of content is
/// trimmed when `preserve_leading_padding` is false — `d` takes it, `c` keeps
/// it, which is how vim itself differs between the two.
pub fn inner_block(
    chars: &[char],
    at: usize,
    bracket: Bracket,
    preserve_leading_padding: bool,
) -> Option<Range<usize>> {
    let block = a_block(chars, at, bracket)?;
    let mut start = block.start + 1;
    let mut end = block.end - 1;

    // Trailing padding: whitespace that runs back to a newline is not content.
    let mut i = end;
    while i > start && chars[i - 1].is_whitespace() {
        if chars[i - 1] == '\n' {
            end = i - 1;
            break;
        }
        i -= 1;
    }

    if preserve_leading_padding {
        let mut i = start;
        while i < end && chars[i].is_whitespace() {
            if chars[i] == '\n' {
                start = i + 1;
                break;
            }
            i += 1;
        }
    }

    Some(start..end)
}

/// Vim's `i` word object (`diw`): the run of one character class around `at`.
pub fn inner_word(chars: &[char], at: usize, kind: WordKind) -> Option<Range<usize>> {
    let context = CharKind::of(*chars.get(at)?);
    let mut start = at;
    while start > 0 && CharKind::of(chars[start - 1]).continues(context, kind) {
        start -= 1;
    }
    let mut end = at;
    while end < chars.len() && CharKind::of(chars[end]).continues(context, kind) {
        end += 1;
    }
    Some(start..end)
}

/// Vim's `a` word object (`daw`): the word plus the whitespace that follows it,
/// or the whitespace before it when it ends the buffer.
pub fn a_word(chars: &[char], at: usize, kind: WordKind) -> Option<Range<usize>> {
    let context = CharKind::of(*chars.get(at)?);
    let mut start = at;
    let mut end = at;
    while start > 0 && CharKind::of(chars[start - 1]).continues(context, kind) {
        start -= 1;
    }
    while end < chars.len() && CharKind::of(chars[end]).continues(context, kind) {
        end += 1;
    }

    if context == CharKind::Whitespace {
        // On whitespace the object takes the word that follows it.
        let Some(&next) = chars.get(end) else {
            return Some(start..end);
        };
        let next_context = CharKind::of(next);
        while end < chars.len() && CharKind::of(chars[end]).continues(next_context, kind) {
            end += 1;
        }
        return Some(start..end);
    }

    match chars.get(end) {
        // Content follows: take the whitespace after the word.
        Some(&c) if c.is_whitespace() => {
            while end < chars.len() && chars[end].is_whitespace() {
                end += 1;
            }
        }
        // No content follows: take the whitespace before it instead.
        None => {
            while start > 0 && chars[start - 1].is_whitespace() {
                start -= 1;
            }
        }
        // Content, but not whitespace, follows: take the whitespace before.
        Some(_) => {
            while start > 0 && chars[start - 1].is_whitespace() {
                start -= 1;
            }
        }
    }
    Some(start..end)
}

/// Vim's `i"`-family object: the text inside a pair of quotes on the line.
pub fn inner_quote(chars: &[char], at: usize, quote: Quote) -> Option<Range<usize>> {
    let range = a_quote(chars, at, quote)?;
    Some(range.start + 1..range.end - 1)
}

/// Vim's `a"`-family object: the text inside a pair of quotes, quotes included.
pub fn a_quote(chars: &[char], at: usize, quote: Quote) -> Option<Range<usize>> {
    let line = line_range(chars, at);

    // A quote on the cursor decides the pair by parity of the quotes before it.
    if chars.get(at).is_some_and(|c| quote.is_char(*c)) {
        let mut before = 0usize;
        let mut i = at;
        while i > line.start {
            if quote.is_char(chars[i - 1]) {
                before += 1;
            }
            i -= 1;
        }
        if before % 2 == 1 {
            // An odd number before it makes this one the closing quote.
            let mut i = at;
            while i > line.start {
                if quote.is_char(chars[i - 1]) {
                    return Some(i - 1..at + 1);
                }
                i -= 1;
            }
            return None;
        }
        // Even: it opens the pair, so take the next quote as the close.
        let mut i = at + 1;
        while i < line.end {
            if quote.is_char(chars[i]) {
                return Some(at..i + 1);
            }
            i += 1;
        }
        return None;
    }

    let behind = {
        let mut found = None;
        let mut i = at;
        while i > line.start {
            if quote.is_char(chars[i - 1]) {
                found = Some(i - 1);
                break;
            }
            i -= 1;
        }
        found
    };
    let ahead = {
        let mut found = None;
        let mut i = at;
        while i < line.end {
            if quote.is_char(chars[i]) {
                found = Some(i);
                break;
            }
            i += 1;
        }
        found
    };

    match (behind, ahead) {
        // Quotes on both sides: that is the pair.
        (Some(start), Some(end)) => Some(start..end + 1),
        // Only one ahead: treat it as the opening quote and look for a close
        // after it.
        (None, Some(start)) => {
            let mut i = start + 1;
            while i < line.end {
                if quote.is_char(chars[i]) {
                    return Some(start..i + 1);
                }
                i += 1;
            }
            None
        }
        // A quote behind with none ahead is not a pair.
        (_, None) => None,
    }
}

/// Vim's `ip` object: the run of non-blank lines around `at`, blank lines
/// excluded.
pub fn inner_paragraph(chars: &[char], at: usize) -> Option<Range<usize>> {
    let lines = line_ranges(chars);
    let index = line_index(&lines, at)?;
    let (mut first, mut last) = (index, index);
    while first > 0 && !line_is_blank(chars, &lines[first - 1]) {
        first -= 1;
    }
    while last + 1 < lines.len() && !line_is_blank(chars, &lines[last + 1]) {
        last += 1;
    }
    Some(lines[first].start..lines[last].end)
}

/// Vim's `ap` object: the paragraph plus one blank line, below it when there is
/// one and above it otherwise.
pub fn a_paragraph(chars: &[char], at: usize) -> Option<Range<usize>> {
    let text = inner_paragraph(chars, at)?;
    let lines = line_ranges(chars);
    if let Some(next) = lines.iter().find(|line| line.start == text.end + 1) {
        if line_is_blank(chars, next) {
            return Some(text.start..next.end);
        }
    }
    let above = lines
        .iter()
        .rev()
        .find(|line| line.end + 1 == text.start && line_is_blank(chars, line));
    match above {
        Some(line) => Some(line.start..text.end),
        None => Some(text),
    }
}

/// The half-open range of the line holding `at`.
fn line_range(chars: &[char], at: usize) -> Range<usize> {
    let start = chars[..at.min(chars.len())]
        .iter()
        .rposition(|c| *c == '\n')
        .map_or(0, |i| i + 1);
    let end = chars[at.min(chars.len())..]
        .iter()
        .position(|c| *c == '\n')
        .map_or(chars.len(), |i| at.min(chars.len()) + i);
    start..end
}

/// Every line's half-open range, in order, without the newline itself.
fn line_ranges(chars: &[char]) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    for (i, c) in chars.iter().enumerate() {
        if *c == '\n' {
            lines.push(start..i);
            start = i + 1;
        }
    }
    lines.push(start..chars.len());
    lines
}

/// Which of `lines` holds `at`.
fn line_index(lines: &[Range<usize>], at: usize) -> Option<usize> {
    lines
        .iter()
        .position(|line| line.contains(&at))
        .or_else(|| lines.len().checked_sub(1))
}

/// Whether a line holds only whitespace.
fn line_is_blank(chars: &[char], line: &Range<usize>) -> bool {
    chars[line.clone()].iter().all(|c| c.is_whitespace())
}
