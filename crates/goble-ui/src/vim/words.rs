//! Character classes and vim's word motions (`w W b B e E ge gE`).
//!
//! Vim moves by "words" in two flavours: a word runs over one class of
//! characters at a time — word characters, a run of punctuation, or whitespace
//! — while a "big word" treats word characters and punctuation as one class,
//! so `W` steps over `crates/goble-ui` at once where `w` stops twice.

/// Characters vim treats as word-breaking: a run of them is its own word.
const WORD_BOUNDARY_CHARS: [char; 33] = [
    '`', '~', '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '-', '=', '+', '[', '{', ']', '}',
    '\\', '|', ';', ':', '\'', '"', ',', '.', '<', '>', '/', '?', '«', '»',
];

/// Which class a character belongs to, for word motions and text objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharKind {
    Word,
    Symbol,
    Whitespace,
}

impl CharKind {
    pub(super) fn of(c: char) -> Self {
        if c.is_whitespace() {
            Self::Whitespace
        } else if WORD_BOUNDARY_CHARS.contains(&c) {
            Self::Symbol
        } else {
            Self::Word
        }
    }

    /// Whether a motion that crossed from `self` to `other` stayed inside one
    /// word. A big word draws no distinction between word characters and
    /// punctuation; whitespace always ends a word.
    pub(super) fn continues(self, other: Self, kind: WordKind) -> bool {
        match kind {
            WordKind::Word => self == other,
            WordKind::BigWord => {
                self == other || (self != Self::Whitespace && other != Self::Whitespace)
            }
        }
    }
}

/// Which flavour of word a motion steps by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordKind {
    Word,
    BigWord,
}

/// Where the next word starts (`w`/`W`), scanning forward from `caret`.
///
/// A run of whitespace is skipped and a run of punctuation counts as its own
/// word, so `w` on `foo(bar)` stops at `(` and then at `bar`.
pub fn word_start_forward(chars: &[char], caret: usize, kind: WordKind) -> usize {
    let len = chars.len();
    if caret >= len {
        return caret;
    }
    let mut context = CharKind::of(chars[caret]);
    let mut i = caret;
    loop {
        i += 1;
        if i >= len {
            return len;
        }
        let here = CharKind::of(chars[i]);
        if !context.continues(here, kind) && here != CharKind::Whitespace {
            return i;
        }
        context = here;
    }
}

/// Where the previous word starts (`b`/`B`), scanning backward from `caret`.
///
/// Whitespace between the words is skipped first, so `b` from the first
/// character of a word reaches the start of the word before it, while `b` from
/// the middle of a word reaches the start of the one it is in.
pub fn word_start_backward(chars: &[char], caret: usize, kind: WordKind) -> usize {
    if chars.is_empty() {
        return 0;
    }
    let caret = caret.min(chars.len() - 1);
    if caret == 0 {
        return 0;
    }
    let mut i = caret - 1;
    while i > 0 && CharKind::of(chars[i]) == CharKind::Whitespace {
        i -= 1;
    }
    let class = CharKind::of(chars[i]);
    while i > 0 && CharKind::of(chars[i - 1]).continues(class, kind) {
        i -= 1;
    }
    i
}

/// The end of the word at or after `caret` (`e`/`E`), scanning forward.
///
/// The result is the index of the last character of that word, never past the
/// end of the buffer.
pub fn word_end_forward(chars: &[char], caret: usize, kind: WordKind) -> usize {
    let len = chars.len();
    if len == 0 {
        return 0;
    }
    let caret = caret.min(len - 1);
    let mut i = caret + 1;
    if i >= len {
        return caret;
    }
    while i + 1 < len {
        let here = CharKind::of(chars[i]);
        let next = CharKind::of(chars[i + 1]);
        if !here.continues(next, kind) && here != CharKind::Whitespace {
            return i;
        }
        i += 1;
    }
    len - 1
}

/// The end of the word before `caret` (`ge`/`gE`), scanning backward.
///
/// The word the insertion point is in is stepped over first, so `ge` reaches
/// the end of the word *before* this one whether the insertion point sits at
/// its start or in the middle of it.
pub fn word_end_backward(chars: &[char], caret: usize, kind: WordKind) -> usize {
    if chars.is_empty() {
        return 0;
    }
    let caret = caret.min(chars.len() - 1);
    let mut i = caret;
    let class = CharKind::of(chars[i]);
    if class != CharKind::Whitespace {
        while i > 0 && CharKind::of(chars[i - 1]).continues(class, kind) {
            i -= 1;
        }
    }
    if i == 0 {
        return 0;
    }
    i -= 1;
    while i > 0 && CharKind::of(chars[i]) == CharKind::Whitespace {
        i -= 1;
    }
    let previous = CharKind::of(chars[i]);
    while i + 1 < chars.len() && CharKind::of(chars[i + 1]).continues(previous, kind) {
        i += 1;
    }
    i
}
