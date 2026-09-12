//! Vim modal editing for the rich input, ported from the engine the reference
//! terminal (warp-new) uses.
//!
//! The engine owns the modal state — the mode, the half-typed command, counts,
//! registers, undo history — and edits a [`VimBuffer`] the host hands it for the
//! length of one keypress. The host keeps ownership of the text: it builds a
//! buffer from its value and insertion point, and writes them back when the
//! engine reports [`VimOutcome::Moved`] or [`VimOutcome::Edited`]. That is what
//! makes the state survive the per-frame rebuild of the element tree — it lives
//! beside the host's own state instead of inside the element.
//!
//! Not carried over are the parts that need a host surface this app does not
//! have: in-editor search (`/ ? n N * #`), the ex command line (`:`), and macros
//! (`q`/`@`) — the reference's generic editor view implements none of them
//! either. `j`/`k`, `o`/`O` and `J` are no-ops for the mirror-image reason: they
//! act on lines, and the rich input is one.

use std::collections::HashMap;
use std::ops::Range;

use crate::event::ModifiersState;

pub mod objects;
pub mod words;

#[cfg(test)]
mod tests;

use objects::{Bracket, Quote};
use words::{WordKind, WordKind::BigWord, WordKind::Word};

/// How many edits one input's undo history holds.
const UNDO_LIMIT: usize = 100;

/// Which of vim's modes the editor is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VimMode {
    Normal,
    #[default]
    Insert,
    Visual(MotionType),
    Replace,
}

impl VimMode {
    /// The mode's name, as the composer's badge shows it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual(MotionType::Charwise) => "VISUAL",
            Self::Visual(MotionType::Linewise) => "V-LINE",
            Self::Replace => "REPLACE",
        }
    }

    /// Whether typing goes into the text rather than being read as a command.
    pub fn is_insert(self) -> bool {
        matches!(self, Self::Insert)
    }
}

/// Whether a visual selection (and the range an operator takes from a linewise
/// motion) covers characters or whole lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionType {
    Charwise,
    Linewise,
}

/// What the host should do with the key it handed to the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VimOutcome {
    /// Not a vim key: handle it the way the input does without vim mode.
    Passthrough,
    /// Vim's key, and nothing the host can observe changed.
    Consumed,
    /// The insertion point moved: write `buffer.caret` back, without reporting
    /// a change (the text is the same).
    Moved,
    /// The text changed: write the value and the insertion point back, and
    /// report the change.
    Edited,
}

/// The system clipboard, so `"+`/`"*` (and the unnamed register when the host
/// asks for it) can reach outside the process.
pub trait Clipboard {
    fn read(&mut self) -> Option<String>;
    fn write(&mut self, text: &str);
}

/// The text being edited and the insertion point, both in character indices.
///
/// The host builds one of these per keypress, so the engine never holds a copy
/// of the text between events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VimBuffer {
    pub text: String,
    pub caret: usize,
}

impl VimBuffer {
    pub fn new(text: impl Into<String>, caret: usize) -> Self {
        let text = text.into();
        let caret = caret.min(text.chars().count());
        Self { text, caret }
    }

    /// The text as characters: every edit below works in character indices, so
    /// a multi-byte character counts as one position.
    fn chars(&self) -> Vec<char> {
        self.text.chars().collect()
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn write_chars(&mut self, chars: &[char]) {
        self.text = chars.iter().collect();
    }

    /// Remove `range`, leaving the insertion point at its start.
    fn delete(&mut self, range: Range<usize>) -> bool {
        let mut chars = self.chars();
        let start = range.start.min(chars.len());
        let end = range.end.min(chars.len());
        if start >= end {
            return false;
        }
        chars.drain(start..end);
        self.write_chars(&chars);
        self.caret = start;
        true
    }

    /// Insert `text` at `at`, leaving the insertion point after it.
    fn insert(&mut self, at: usize, text: &str) -> usize {
        let mut chars = self.chars();
        let at = at.min(chars.len());
        let inserted: Vec<char> = text.chars().collect();
        for (i, c) in inserted.iter().enumerate() {
            chars.insert(at + i, *c);
        }
        self.write_chars(&chars);
        self.caret = at + inserted.len();
        self.caret
    }

    /// Overwrite `count` characters from `at` with `c`, as `r` does.
    fn replace_with(&mut self, at: usize, count: usize, c: char) -> bool {
        let mut chars = self.chars();
        if at >= chars.len() {
            return false;
        }
        let end = (at + count).min(chars.len());
        for slot in chars.iter_mut().take(end).skip(at) {
            *slot = c;
        }
        self.write_chars(&chars);
        self.caret = at;
        true
    }

    /// Change the case of every character in `range`.
    fn change_case(&mut self, range: Range<usize>, case: Case) -> bool {
        let mut chars = self.chars();
        let start = range.start.min(chars.len());
        let end = range.end.min(chars.len());
        if start >= end {
            return false;
        }
        for slot in chars.iter_mut().take(end).skip(start) {
            *slot = match case {
                Case::Toggle if slot.is_uppercase() => slot.to_lowercase().next().unwrap_or(*slot),
                Case::Toggle if slot.is_lowercase() => slot.to_uppercase().next().unwrap_or(*slot),
                Case::Toggle => *slot,
                Case::Lower => slot.to_lowercase().next().unwrap_or(*slot),
                Case::Upper => slot.to_uppercase().next().unwrap_or(*slot),
            };
        }
        self.write_chars(&chars);
        self.caret = start;
        true
    }

    /// The slice of the text a range names.
    fn slice(&self, range: Range<usize>) -> String {
        let chars = self.chars();
        let start = range.start.min(chars.len());
        let end = range.end.min(chars.len());
        if start >= end {
            return String::new();
        }
        chars[start..end].iter().collect()
    }
}

/// Which way a case change goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    Toggle,
    Lower,
    Upper,
}

/// A register's contents, and whether it was taken linewise.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Register {
    text: String,
    linewise: bool,
}

/// The operator half of a command, waiting for a motion or a text object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operator {
    Delete,
    Change,
    Yank,
    ToggleCase,
    Lowercase,
    Uppercase,
}

impl Operator {
    fn char(self) -> char {
        match self {
            Self::Delete => 'd',
            Self::Change => 'c',
            Self::Yank => 'y',
            Self::ToggleCase => '~',
            Self::Lowercase => 'u',
            Self::Uppercase => 'U',
        }
    }

    fn from_char(c: char) -> Option<Self> {
        match c {
            'd' => Some(Self::Delete),
            'c' => Some(Self::Change),
            'y' => Some(Self::Yank),
            _ => None,
        }
    }
}

/// A `f`/`F`/`t`/`T` motion, kept so `;` and `,` can repeat it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FindMotion {
    /// Whether the search runs forward.
    forward: bool,
    /// Whether the cursor stops one short of the target (`t`/`T`).
    before: bool,
    target: char,
}

/// A command that is only half typed, and what it is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    /// An operator, waiting for a motion or a text object.
    Operator(Operator),
    /// An operator after `i`/`a`, waiting for the object's own character.
    TextObject(Operator, bool),
    /// After `g`.
    G,
    /// After `f`/`F`/`t`/`T`, waiting for the character to find.
    Find(FindMotion),
    /// After an operator and then `f`/`F`/`t`/`T`, waiting for the character.
    OperatorFind(Operator, FindMotion),
    /// After `r`, waiting for the replacement character.
    Replace,
    /// After `"`, waiting for a register name.
    Register,
    /// In visual mode, after `i`/`a`.
    VisualObject(bool),
}

/// The change `.` repeats: the keys that started it and the text an insert
/// session typed.
#[derive(Clone, Debug, Default)]
struct LastChange {
    keys: Vec<String>,
    insert: String,
}

/// The composer's vim state: everything the modal editing keeps between keys.
#[derive(Debug)]
pub struct VimState {
    mode: VimMode,
    /// Where a visual selection or an operator started.
    anchor: usize,
    pending: Option<Pending>,
    /// The count typed before an operator (`2` in `2d3w`).
    count: Option<u32>,
    /// The count typed after an operator (`3` in `2d3w`).
    count2: Option<u32>,
    register: char,
    registers: HashMap<char, Register>,
    last_find: Option<FindMotion>,
    last_change: Option<LastChange>,
    /// The keys of the command being typed, for dot-repeat.
    command: Vec<String>,
    /// The keys that opened the insert session being recorded.
    insert_keys: Vec<String>,
    insert_text: String,
    undo: Vec<(String, usize)>,
    redo: Vec<(String, usize)>,
    /// Whether the unnamed register follows the system clipboard.
    unnamed_clipboard: bool,
    /// Depth of a `.` replay, which must not overwrite what it is replaying.
    replaying: u32,
}

impl Default for VimState {
    fn default() -> Self {
        Self::new()
    }
}

impl VimState {
    pub fn new() -> Self {
        Self {
            mode: VimMode::Insert,
            anchor: 0,
            pending: None,
            count: None,
            count2: None,
            register: '"',
            registers: HashMap::new(),
            last_find: None,
            last_change: None,
            command: Vec::new(),
            insert_keys: Vec::new(),
            insert_text: String::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            unnamed_clipboard: false,
            replaying: 0,
        }
    }

    pub fn mode(&self) -> VimMode {
        self.mode
    }

    /// Whether a command is half typed. The host keeps Escape for vim while
    /// this holds, so the app's own Escape (leaving the harness) only runs when
    /// there is nothing pending.
    pub fn is_pending(&self) -> bool {
        self.pending.is_some() || self.count.is_some() || self.count2.is_some()
    }

    /// The text of the half-typed command, for the composer's badge.
    pub fn showcmd(&self) -> String {
        let mut text = String::new();
        if let Some(count) = self.count {
            text.push_str(&count.to_string());
        }
        match self.pending {
            Some(Pending::Operator(operator)) => text.push(operator.char()),
            Some(Pending::TextObject(operator, around)) => {
                text.push(operator.char());
                text.push(if around { 'a' } else { 'i' });
            }
            Some(Pending::G) => text.push('g'),
            Some(Pending::OperatorFind(operator, motion)) => {
                text.push(operator.char());
                text.push(find_char(motion));
            }
            Some(Pending::Find(motion)) => text.push(find_char(motion)),
            Some(Pending::Replace) => text.push('r'),
            Some(Pending::Register) => text.push('"'),
            Some(Pending::VisualObject(around)) => text.push(if around { 'a' } else { 'i' }),
            None => {}
        }
        if let Some(count) = self.count2 {
            text.push_str(&count.to_string());
        }
        text
    }

    /// Whether the unnamed register follows the system clipboard.
    pub fn set_unnamed_clipboard(&mut self, on: bool) {
        self.unnamed_clipboard = on;
    }

    /// The characters a visual selection covers, given the host's insertion
    /// point (the selection's moving end lives there).
    pub fn selection(&self, caret: usize, len: usize) -> Option<Range<usize>> {
        let VimMode::Visual(kind) = self.mode else {
            return None;
        };
        if len == 0 {
            return None;
        }
        match kind {
            MotionType::Linewise => Some(0..len),
            MotionType::Charwise => {
                let start = self.anchor.min(caret).min(len - 1);
                let end = self.anchor.max(caret).min(len - 1) + 1;
                Some(start..end)
            }
        }
    }

    /// Drop a half-typed command and go back to insert mode. The host calls
    /// this when the input loses focus or a click moves the insertion point.
    pub fn reset(&mut self) {
        self.pending = None;
        self.count = None;
        self.count2 = None;
        self.command.clear();
        self.insert_keys.clear();
        self.insert_text.clear();
        self.mode = VimMode::Insert;
    }

    // -- recording and history ------------------------------------------------

    /// Remember the keys of the command in progress, for `.`.
    fn record(&mut self, key: &str) {
        if self.replaying == 0 {
            self.command.push(key.to_string());
        }
    }

    /// Close the command in progress as a change `.` can repeat.
    fn remember_change(&mut self, insert: Option<String>) {
        if self.replaying > 0 {
            return;
        }
        let keys = std::mem::take(&mut self.command);
        let insert = match insert {
            Some(text) => text,
            None => std::mem::take(&mut self.insert_text),
        };
        self.last_change = Some(LastChange { keys, insert });
        self.insert_keys.clear();
    }

    fn snapshot(&mut self, buffer: &VimBuffer) {
        self.undo.push((buffer.text.clone(), buffer.caret));
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self, buffer: &mut VimBuffer) -> VimOutcome {
        let Some((text, caret)) = self.undo.pop() else {
            return VimOutcome::Consumed;
        };
        self.redo.push((buffer.text.clone(), buffer.caret));
        buffer.text = text;
        buffer.caret = caret;
        self.after_undo(buffer)
    }

    fn redo(&mut self, buffer: &mut VimBuffer) -> VimOutcome {
        let Some((text, caret)) = self.redo.pop() else {
            return VimOutcome::Consumed;
        };
        self.undo.push((buffer.text.clone(), buffer.caret));
        buffer.text = text;
        buffer.caret = caret;
        self.after_undo(buffer)
    }

    fn after_undo(&mut self, buffer: &mut VimBuffer) -> VimOutcome {
        self.mode = VimMode::Normal;
        self.pending = None;
        self.count = None;
        self.count2 = None;
        buffer.caret = normal_caret(buffer.caret, buffer.len());
        VimOutcome::Edited
    }

    // -- counts and registers -------------------------------------------------

    fn push_count(&mut self, c: char) {
        let digit = c.to_digit(10).unwrap_or(0);
        let slot = if self.pending.is_some() {
            &mut self.count2
        } else {
            &mut self.count
        };
        let current = slot.unwrap_or(0);
        *slot = Some(current.saturating_mul(10).saturating_add(digit));
    }

    /// The counts typed so far, multiplied: `2d3w` is six words.
    fn take_counts(&mut self) -> usize {
        let count = self.count.take().unwrap_or(1).max(1) as usize;
        let count2 = self.count2.take().unwrap_or(1).max(1) as usize;
        count.saturating_mul(count2)
    }

    fn clear_pending(&mut self) {
        self.pending = None;
        self.count = None;
        self.count2 = None;
        self.command.clear();
    }

    /// Store `text` under `name`, and in the system clipboard when the name
    /// asks for it. Every write also fills the unnamed register, as vim's does.
    fn write_register(
        &mut self,
        name: char,
        text: String,
        linewise: bool,
        clipboard: Option<&mut dyn Clipboard>,
    ) {
        if name == '_' {
            // The black-hole register takes the text and keeps nothing.
            return;
        }
        // An uppercase name appends to the register of the same lowercase name.
        let (key, append) = if name.is_ascii_uppercase() {
            (name.to_ascii_lowercase(), true)
        } else {
            (name, false)
        };
        self.store(key, &text, linewise, append);
        if key != '"' {
            self.store('"', &text, linewise, false);
        }
        if matches!(name, '+' | '*') || (self.unnamed_clipboard && name == '"') {
            if let Some(clipboard) = clipboard {
                clipboard.write(&text);
            }
        }
    }

    fn store(&mut self, key: char, text: &str, linewise: bool, append: bool) {
        let entry = self.registers.entry(key).or_default();
        if append {
            if linewise && !entry.text.ends_with('\n') && !entry.text.is_empty() {
                entry.text.push('\n');
            }
            entry.text.push_str(text);
            entry.linewise = linewise;
        } else {
            entry.text = text.to_string();
            entry.linewise = linewise;
        }
    }

    fn read_register(&mut self, name: char, clipboard: Option<&mut dyn Clipboard>) -> Register {
        if name == '_' {
            return Register::default();
        }
        if matches!(name, '+' | '*') {
            let text = clipboard.and_then(|clipboard| clipboard.read()).unwrap_or_default();
            return Register { text, linewise: false };
        }
        let stored = self.registers.get(&name).cloned().unwrap_or_default();
        if name == '"' && self.unnamed_clipboard {
            // The clipboard wins when it holds something the registers do not.
            if let Some(text) = clipboard.and_then(|clipboard| clipboard.read()) {
                if text != stored.text {
                    return Register { text, linewise: false };
                }
            }
        }
        stored
    }

    // -- key handling ---------------------------------------------------------

    /// Handle one key. `buffer` carries the host's value and insertion point in;
    /// the host writes them back for [`VimOutcome::Moved`]/[`VimOutcome::Edited`].
    pub fn handle(
        &mut self,
        key: &str,
        modifiers: &ModifiersState,
        buffer: &mut VimBuffer,
        clipboard: Option<&mut dyn Clipboard>,
    ) -> VimOutcome {
        if key == "Escape" || (modifiers.ctrl && key == "[") {
            return self.escape(buffer);
        }
        if modifiers.command {
            // Cmd shortcuts belong to the app.
            return VimOutcome::Passthrough;
        }
        if modifiers.ctrl {
            if self.mode == VimMode::Normal && key.eq_ignore_ascii_case("r") {
                return self.redo(buffer);
            }
            return VimOutcome::Passthrough;
        }
        if modifiers.alt {
            return if self.mode.is_insert() {
                VimOutcome::Passthrough
            } else {
                VimOutcome::Consumed
            };
        }
        match self.mode {
            VimMode::Insert => self.insert(key, buffer),
            VimMode::Replace => self.replace(key, buffer),
            VimMode::Normal => self.normal(key, buffer, clipboard),
            VimMode::Visual(_) => self.visual(key, buffer, clipboard),
        }
    }

    /// Escape leaves insert mode or drops a half-typed command. In normal mode
    /// with nothing pending it hands the key back, so the app's own Escape
    /// (leaving the harness) still runs.
    fn escape(&mut self, buffer: &mut VimBuffer) -> VimOutcome {
        match self.mode {
            VimMode::Insert => {
                self.finish_insert();
                self.mode = VimMode::Normal;
                self.clear_pending();
                buffer.caret = normal_caret(buffer.caret, buffer.len());
                VimOutcome::Consumed
            }
            VimMode::Visual(_) | VimMode::Replace => {
                self.mode = VimMode::Normal;
                self.clear_pending();
                buffer.caret = normal_caret(buffer.caret, buffer.len());
                VimOutcome::Consumed
            }
            VimMode::Normal => {
                if self.is_pending() {
                    self.clear_pending();
                    VimOutcome::Consumed
                } else {
                    VimOutcome::Passthrough
                }
            }
        }
    }

    /// Close an insert session as one change, so `.` can type it again.
    fn finish_insert(&mut self) {
        let keys = std::mem::take(&mut self.insert_keys);
        let insert = std::mem::take(&mut self.insert_text);
        if keys.is_empty() {
            self.command.clear();
            return;
        }
        if self.replaying == 0 {
            self.last_change = Some(LastChange { keys, insert });
        }
        self.command.clear();
    }

    /// Insert mode: typed characters land in the text; Enter and the arrow keys
    /// stay the host's, so Enter still submits and the arrows still move.
    fn insert(&mut self, key: &str, buffer: &mut VimBuffer) -> VimOutcome {
        match key {
            "Backspace" => {
                if buffer.caret == 0 {
                    return VimOutcome::Consumed;
                }
                self.snapshot(buffer);
                buffer.delete(buffer.caret - 1..buffer.caret);
                self.insert_text.pop();
                VimOutcome::Edited
            }
            "Delete" => {
                if buffer.caret >= buffer.len() {
                    return VimOutcome::Consumed;
                }
                self.snapshot(buffer);
                buffer.delete(buffer.caret..buffer.caret + 1);
                VimOutcome::Edited
            }
            _ => {
                let mut chars = key.chars();
                let (Some(c), None) = (chars.next(), chars.next()) else {
                    return VimOutcome::Passthrough;
                };
                self.snapshot(buffer);
                buffer.insert(buffer.caret, &c.to_string());
                self.insert_text.push(c);
                VimOutcome::Edited
            }
        }
    }

    /// Replace mode: one character deep, as in the reference engine — the typed
    /// character overwrites the one under the insertion point and vim is back in
    /// normal mode.
    fn replace(&mut self, key: &str, buffer: &mut VimBuffer) -> VimOutcome {
        let mut chars = key.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            return VimOutcome::Passthrough;
        };
        self.mode = VimMode::Normal;
        self.clear_pending();
        self.snapshot(buffer);
        if buffer.replace_with(buffer.caret, 1, c) {
            buffer.caret = (buffer.caret + 1).min(buffer.len().saturating_sub(1));
            VimOutcome::Edited
        } else {
            VimOutcome::Consumed
        }
    }

    /// Normal mode: the command table.
    fn normal(
        &mut self,
        key: &str,
        buffer: &mut VimBuffer,
        clipboard: Option<&mut dyn Clipboard>,
    ) -> VimOutcome {
        let key = named_key(key).unwrap_or(key);
        if let Some(pending) = self.pending {
            self.record(key);
            return self.normal_pending(pending, key, buffer, clipboard);
        }
        let mut chars = key.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else {
            // A named key no command answers to: the host keeps it.
            return VimOutcome::Passthrough;
        };
        // Digits build a count, except that a leading `0` is the line motion.
        if c.is_ascii_digit() && (c != '0' || self.count.is_some()) {
            self.record(key);
            self.push_count(c);
            return VimOutcome::Consumed;
        }
        self.record(key);
        self.normal_command(c, buffer, clipboard)
    }

    /// One normal-mode command, with the counts it was given.
    fn normal_command(
        &mut self,
        c: char,
        buffer: &mut VimBuffer,
        clipboard: Option<&mut dyn Clipboard>,
    ) -> VimOutcome {
        let len = buffer.len();
        match c {
            'h' => {
                let count = self.take_counts();
                self.move_caret(buffer, count, |_, at| at.saturating_sub(1))
            }
            'l' | ' ' => {
                let count = self.take_counts();
                self.move_caret(buffer, count, |chars, at| {
                    (at + 1).min(chars.len().saturating_sub(1))
                })
            }
            // One line: there is no line below or above to walk to.
            'j' | 'k' => {
                self.take_counts();
                VimOutcome::Consumed
            }
            'w' | 'W' | 'b' | 'B' | 'e' | 'E' => {
                let count = self.take_counts();
                let kind = word_kind(c);
                self.move_caret(buffer, count, move |chars, at| {
                    word_motion(c, chars, at, kind)
                })
            }
            '0' => self.move_caret(buffer, 1, |_, _| 0),
            '^' => self.move_caret(buffer, 1, |chars, _| first_non_blank(chars)),
            '$' => self.move_caret(buffer, 1, |chars, _| chars.len().saturating_sub(1)),
            'G' => self.move_caret(buffer, 1, |chars, _| first_non_blank(chars)),
            '%' => {
                let count = self.take_counts();
                self.move_caret(buffer, count, |chars, at| {
                    matching_bracket_index(chars, at).unwrap_or(at)
                })
            }
            'f' | 'F' | 't' | 'T' => {
                self.pending = Some(Pending::Find(FindMotion {
                    forward: matches!(c, 'f' | 't'),
                    before: matches!(c, 't' | 'T'),
                    target: ' ',
                }));
                VimOutcome::Consumed
            }
            ';' | ',' => {
                let count = self.take_counts();
                let Some(mut motion) = self.last_find else {
                    return VimOutcome::Consumed;
                };
                if c == ',' {
                    motion.forward = !motion.forward;
                }
                self.find_move(motion, count, buffer)
            }
            'g' => {
                self.pending = Some(Pending::G);
                VimOutcome::Consumed
            }
            'i' | 'a' | 'I' | 'A' => {
                let at = match c {
                    'i' => InsertAt::Here,
                    'a' => InsertAt::After,
                    'I' => InsertAt::FirstNonBlank,
                    _ => InsertAt::End,
                };
                self.enter_insert(buffer, at)
            }
            // There is no second line to open above or below.
            'o' | 'O' | 'J' => VimOutcome::Consumed,
            'x' => {
                let count = self.take_counts();
                let range = buffer.caret..(buffer.caret + count).min(len);
                self.apply_operator(Operator::Delete, range, false, buffer, clipboard)
            }
            'X' => {
                let count = self.take_counts();
                let range = buffer.caret.saturating_sub(count)..buffer.caret;
                self.apply_operator(Operator::Delete, range, false, buffer, clipboard)
            }
            'D' => {
                let range = buffer.caret..len;
                self.apply_operator(Operator::Delete, range, false, buffer, clipboard)
            }
            'C' => {
                let range = buffer.caret..len;
                self.apply_operator(Operator::Change, range, false, buffer, clipboard)
            }
            's' => {
                let count = self.take_counts();
                let range = buffer.caret..(buffer.caret + count).min(len);
                self.apply_operator(Operator::Change, range, false, buffer, clipboard)
            }
            'S' => self.apply_operator(Operator::Change, 0..len, true, buffer, clipboard),
            'Y' => self.apply_operator(Operator::Yank, 0..len, true, buffer, clipboard),
            '~' => {
                let count = self.take_counts();
                let range = buffer.caret..(buffer.caret + count).min(len);
                let outcome =
                    self.apply_operator(Operator::ToggleCase, range, false, buffer, clipboard);
                if outcome == VimOutcome::Edited {
                    // `~` walks forward over what it changed.
                    buffer.caret = buffer.caret.saturating_add(count).min(len.saturating_sub(1));
                }
                outcome
            }
            'r' => {
                self.pending = Some(Pending::Replace);
                VimOutcome::Consumed
            }
            'R' => {
                self.clear_pending();
                self.mode = VimMode::Replace;
                VimOutcome::Consumed
            }
            'u' => {
                self.take_counts();
                self.undo(buffer)
            }
            'p' | 'P' => {
                let count = self.take_counts();
                self.paste(count, c == 'P', buffer, clipboard)
            }
            'd' | 'c' | 'y' => {
                self.pending = Some(Pending::Operator(
                    Operator::from_char(c).unwrap_or(Operator::Delete),
                ));
                VimOutcome::Consumed
            }
            '"' => {
                self.pending = Some(Pending::Register);
                VimOutcome::Consumed
            }
            'v' => {
                self.anchor = normal_caret(buffer.caret, len);
                self.mode = VimMode::Visual(MotionType::Charwise);
                VimOutcome::Consumed
            }
            'V' => {
                self.anchor = 0;
                self.mode = VimMode::Visual(MotionType::Linewise);
                VimOutcome::Consumed
            }
            '.' => self.dot(buffer, clipboard),
            // Every other key is vim's, with nothing bound to it here: it is
            // swallowed rather than typed into the text.
            _ => VimOutcome::Consumed,
        }
    }

    /// What a half-typed command does with the key that completes it.
    fn normal_pending(
        &mut self,
        pending: Pending,
        key: &str,
        buffer: &mut VimBuffer,
        clipboard: Option<&mut dyn Clipboard>,
    ) -> VimOutcome {
        match pending {
            Pending::Register => {
                self.pending = None;
                if let (Some(c), None) = (key.chars().next(), key.chars().nth(1)) {
                    if is_register_name(c) {
                        self.register = c;
                    }
                }
                VimOutcome::Consumed
            }
            Pending::Replace => {
                self.pending = None;
                let count = self.take_counts();
                let (Some(c), None) = (key.chars().next(), key.chars().nth(1)) else {
                    return VimOutcome::Consumed;
                };
                self.snapshot(buffer);
                if buffer.replace_with(buffer.caret, count, c) {
                    self.remember_change(None);
                    VimOutcome::Edited
                } else {
                    VimOutcome::Consumed
                }
            }
            Pending::Find(mut motion) => {
                self.pending = None;
                let count = self.take_counts();
                let (Some(c), None) = (key.chars().next(), key.chars().nth(1)) else {
                    return VimOutcome::Consumed;
                };
                motion.target = c;
                self.last_find = Some(motion);
                self.find_move(motion, count, buffer)
            }
            Pending::OperatorFind(operator, mut motion) => {
                self.pending = None;
                let count = self.take_counts();
                let (Some(c), None) = (key.chars().next(), key.chars().nth(1)) else {
                    return VimOutcome::Consumed;
                };
                motion.target = c;
                self.last_find = Some(motion);
                let chars = buffer.chars();
                if chars.is_empty() {
                    return VimOutcome::Consumed;
                }
                let len = chars.len();
                let caret = normal_caret(buffer.caret, len);
                match find_index(&chars, caret, motion, count) {
                    Some(found) => {
                        let range = find_operator_range(caret, found, motion, len);
                        self.apply_operator(operator, range, false, buffer, clipboard)
                    }
                    None => VimOutcome::Consumed,
                }
            }
            Pending::G => {
                self.pending = None;
                self.g_command(key, buffer, clipboard)
            }
            Pending::VisualObject(around) => {
                self.pending = None;
                let (Some(c), None) = (key.chars().next(), key.chars().nth(1)) else {
                    return VimOutcome::Consumed;
                };
                match self.text_object(c, around, false, buffer) {
                    Some(range) => {
                        self.anchor = range.start;
                        buffer.caret = range.end.saturating_sub(1);
                        VimOutcome::Moved
                    }
                    None => VimOutcome::Consumed,
                }
            }
            Pending::TextObject(operator, around) => {
                self.pending = None;
                self.take_counts();
                let (Some(c), None) = (key.chars().next(), key.chars().nth(1)) else {
                    return VimOutcome::Consumed;
                };
                let change = operator == Operator::Change;
                match self.text_object(c, around, change, buffer) {
                    Some(range) => self.apply_operator(operator, range, false, buffer, clipboard),
                    None => VimOutcome::Consumed,
                }
            }
            Pending::Operator(operator) => {
                let (Some(c), None) = (key.chars().next(), key.chars().nth(1)) else {
                    self.pending = None;
                    return VimOutcome::Consumed;
                };
                if c.is_ascii_digit() && (c != '0' || self.count2.is_some()) {
                    self.push_count(c);
                    self.pending = Some(Pending::Operator(operator));
                    return VimOutcome::Consumed;
                }
                if c == 'i' || c == 'a' {
                    self.pending = Some(Pending::TextObject(operator, c == 'a'));
                    return VimOutcome::Consumed;
                }
                if matches!(c, 'f' | 'F' | 't' | 'T') {
                    self.pending = Some(Pending::OperatorFind(
                        operator,
                        FindMotion {
                            forward: matches!(c, 'f' | 't'),
                            before: matches!(c, 't' | 'T'),
                            target: ' ',
                        },
                    ));
                    return VimOutcome::Consumed;
                }
                let count = self.take_counts();
                self.pending = None;
                if Operator::from_char(c) == Some(operator) {
                    // `dd`/`cc`/`yy`: the whole line.
                    let len = buffer.len();
                    return self.apply_operator(operator, 0..len, true, buffer, clipboard);
                }
                match self.motion_range(c, count, buffer, operator == Operator::Change) {
                    Some(range) => self.apply_operator(operator, range, false, buffer, clipboard),
                    None => VimOutcome::Consumed,
                }
            }
        }
    }

    /// Apply a completed operator to a range.
    fn apply_operator(
        &mut self,
        operator: Operator,
        range: Range<usize>,
        linewise: bool,
        buffer: &mut VimBuffer,
        clipboard: Option<&mut dyn Clipboard>,
    ) -> VimOutcome {
        let register = self.used_register();
        let text = buffer.slice(range.clone());
        if text.is_empty() {
            return VimOutcome::Consumed;
        }
        match operator {
            Operator::Yank => {
                self.write_register(register, text, linewise, clipboard);
                buffer.caret = range.start;
                VimOutcome::Moved
            }
            Operator::Delete => {
                self.write_register(register, text, linewise, clipboard);
                self.snapshot(buffer);
                buffer.delete(range);
                self.remember_change(None);
                VimOutcome::Edited
            }
            Operator::Change => {
                self.write_register(register, text, linewise, clipboard);
                self.snapshot(buffer);
                buffer.delete(range);
                // What gets typed next is what `.` types again.
                self.insert_keys = std::mem::take(&mut self.command);
                self.insert_text.clear();
                self.mode = VimMode::Insert;
                VimOutcome::Edited
            }
            Operator::ToggleCase | Operator::Lowercase | Operator::Uppercase => {
                let case = match operator {
                    Operator::Lowercase => Case::Lower,
                    Operator::Uppercase => Case::Upper,
                    _ => Case::Toggle,
                };
                self.snapshot(buffer);
                if buffer.change_case(range, case) {
                    self.remember_change(None);
                    VimOutcome::Edited
                } else {
                    VimOutcome::Consumed
                }
            }
        }
    }

    /// Take the register a command reads or writes, leaving the unnamed one
    /// selected for the next command.
    fn used_register(&mut self) -> char {
        let name = self.register;
        self.register = '"';
        name
    }

    /// Move the insertion point by a motion, `count` times, clamped to the text.
    fn move_caret(
        &mut self,
        buffer: &mut VimBuffer,
        count: usize,
        step: impl Fn(&[char], usize) -> usize,
    ) -> VimOutcome {
        self.count = None;
        self.count2 = None;
        let chars = buffer.chars();
        if chars.is_empty() {
            buffer.caret = 0;
            return VimOutcome::Consumed;
        }
        let mut at = normal_caret(buffer.caret, chars.len());
        for _ in 0..count.max(1) {
            at = step(&chars, at);
        }
        buffer.caret = at.min(chars.len() - 1);
        VimOutcome::Moved
    }

    /// Move to where `f`/`F`/`t`/`T` lands.
    fn find_move(
        &mut self,
        motion: FindMotion,
        count: usize,
        buffer: &mut VimBuffer,
    ) -> VimOutcome {
        let chars = buffer.chars();
        if chars.is_empty() {
            return VimOutcome::Consumed;
        }
        let caret = normal_caret(buffer.caret, chars.len());
        match find_caret_index(&chars, caret, motion, count) {
            Some(at) => {
                buffer.caret = at;
                VimOutcome::Moved
            }
            None => VimOutcome::Consumed,
        }
    }

    /// Open an insert session, remembering the keys that led to it so `.` can
    /// replay the whole change.
    fn enter_insert(&mut self, buffer: &mut VimBuffer, at: InsertAt) -> VimOutcome {
        let len = buffer.len();
        let moved = match at {
            InsertAt::Here => {
                buffer.caret = normal_caret(buffer.caret, len);
                false
            }
            InsertAt::After => {
                buffer.caret = (normal_caret(buffer.caret, len) + 1).min(len);
                true
            }
            InsertAt::FirstNonBlank => {
                buffer.caret = first_non_blank(&buffer.chars());
                true
            }
            InsertAt::End => {
                buffer.caret = len;
                true
            }
        };
        self.insert_keys = std::mem::take(&mut self.command);
        self.insert_text.clear();
        self.mode = VimMode::Insert;
        if moved {
            VimOutcome::Moved
        } else {
            VimOutcome::Consumed
        }
    }

    /// The range a text object names.
    fn text_object(
        &self,
        c: char,
        around: bool,
        change: bool,
        buffer: &VimBuffer,
    ) -> Option<Range<usize>> {
        let chars = buffer.chars();
        if chars.is_empty() {
            return None;
        }
        let at = normal_caret(buffer.caret, chars.len());
        match c {
            'w' => {
                if around {
                    objects::a_word(&chars, at, Word)
                } else {
                    objects::inner_word(&chars, at, Word)
                }
            }
            'W' => {
                if around {
                    objects::a_word(&chars, at, BigWord)
                } else {
                    objects::inner_word(&chars, at, BigWord)
                }
            }
            'p' => {
                if around {
                    objects::a_paragraph(&chars, at)
                } else {
                    objects::inner_paragraph(&chars, at)
                }
            }
            '\'' | '"' | '`' => {
                let quote = Quote::of(c)?;
                if around {
                    objects::a_quote(&chars, at, quote)
                } else {
                    objects::inner_quote(&chars, at, quote)
                }
            }
            _ => {
                let (bracket, _) = Bracket::of(c)?;
                if around {
                    objects::a_block(&chars, at, bracket)
                } else {
                    objects::inner_block(&chars, at, bracket, change)
                }
            }
        }
    }

    /// The range an operator takes from a motion. `change` is set when the
    /// operator is `c`, whose `cw` behaves like `ce`.
    fn motion_range(
        &mut self,
        c: char,
        count: usize,
        buffer: &VimBuffer,
        change: bool,
    ) -> Option<Range<usize>> {
        let chars = buffer.chars();
        if chars.is_empty() {
            return None;
        }
        let len = chars.len();
        let caret = normal_caret(buffer.caret, len);
        match c {
            'h' => Some(caret.saturating_sub(count)..caret),
            'l' | ' ' => Some(caret..(caret + count).min(len)),
            'w' | 'W' => {
                let kind = word_kind(c);
                if change {
                    // `cw` takes the word without the whitespace after it.
                    let target = repeat_motion(count, caret, |at| {
                        words::word_end_forward(&chars, at, kind)
                    });
                    return Some(caret..(target + 1).min(len));
                }
                let target = repeat_motion(count, caret, |at| {
                    words::word_start_forward(&chars, at, kind)
                });
                Some(caret.min(target)..caret.max(target))
            }
            'b' | 'B' => {
                let kind = word_kind(c);
                let target = repeat_motion(count, caret, |at| {
                    words::word_start_backward(&chars, at, kind)
                });
                Some(caret.min(target)..caret.max(target))
            }
            'e' | 'E' => {
                let kind = word_kind(c);
                let target = repeat_motion(count, caret, |at| {
                    words::word_end_forward(&chars, at, kind)
                });
                // `e` is inclusive of the word's last character.
                Some(caret..(target + 1).min(len))
            }
            '0' => Some(0..caret),
            '^' => {
                let blank = first_non_blank(&chars);
                Some(blank.min(caret)..blank.max(caret))
            }
            '$' => Some(caret..len),
            '%' => {
                let other = matching_bracket_index(&chars, caret)?;
                Some(caret.min(other)..caret.max(other) + 1)
            }
            ';' | ',' => {
                let mut motion = self.last_find?;
                if c == ',' {
                    motion.forward = !motion.forward;
                }
                let found = find_index(&chars, caret, motion, count)?;
                Some(find_operator_range(caret, found, motion, len))
            }
            _ => None,
        }
    }

    /// The commands behind `g`.
    fn g_command(
        &mut self,
        key: &str,
        buffer: &mut VimBuffer,
        clipboard: Option<&mut dyn Clipboard>,
    ) -> VimOutcome {
        let (Some(c), None) = (key.chars().next(), key.chars().nth(1)) else {
            return VimOutcome::Consumed;
        };
        match c {
            // `gg` is the first line, and the whole text is one line.
            'g' => self.move_caret(buffer, 1, |chars, _| first_non_blank(chars)),
            'e' | 'E' => {
                let count = self.take_counts();
                let kind = if c == 'e' { Word } else { BigWord };
                self.move_caret(buffer, count, move |chars, at| {
                    words::word_end_backward(chars, at, kind)
                })
            }
            '~' | 'u' | 'U' => {
                let count = self.take_counts();
                let operator = match c {
                    'u' => Operator::Lowercase,
                    'U' => Operator::Uppercase,
                    _ => Operator::ToggleCase,
                };
                let len = buffer.len();
                let range = buffer.caret..(buffer.caret + count).min(len);
                self.apply_operator(operator, range, false, buffer, clipboard)
            }
            _ => VimOutcome::Consumed,
        }
    }

    /// `p`/`P`: put the register's text next to the insertion point.
    fn paste(
        &mut self,
        count: usize,
        before: bool,
        buffer: &mut VimBuffer,
        clipboard: Option<&mut dyn Clipboard>,
    ) -> VimOutcome {
        let register = self.used_register();
        let content = self.read_register(register, clipboard);
        if content.text.is_empty() {
            return VimOutcome::Consumed;
        }
        let text = content.text.repeat(count.max(1));
        let caret = normal_caret(buffer.caret, buffer.len());
        let at = if before { caret } else { (caret + 1).min(buffer.len()) };
        self.snapshot(buffer);
        let after = buffer.insert(at, &text);
        // The insertion point lands on the last pasted character.
        buffer.caret = normal_caret(after.saturating_sub(1), buffer.len());
        self.remember_change(None);
        VimOutcome::Edited
    }

    /// `.`: make the last change again.
    ///
    /// The replay runs without the system clipboard: a change repeated through
    /// `"+`/`"*` still uses vim's own registers, but does not read or write the
    /// clipboard again.
    fn dot(
        &mut self,
        buffer: &mut VimBuffer,
        _clipboard: Option<&mut dyn Clipboard>,
    ) -> VimOutcome {
        let Some(change) = self.last_change.clone() else {
            return VimOutcome::Consumed;
        };
        let repeats = self.take_counts();
        let mut outcome = VimOutcome::Consumed;
        self.replaying += 1;
        for _ in 0..repeats.max(1) {
            for key in change.keys.clone() {
                outcome = self.normal(&key, buffer, None);
            }
            for c in change.insert.chars() {
                outcome = self.insert(&c.to_string(), buffer);
            }
            if self.mode == VimMode::Insert {
                self.escape(buffer);
            }
        }
        self.replaying -= 1;
        // The replay must not replace the change it is repeating.
        self.last_change = Some(change);
        outcome
    }

    /// Visual mode: motions extend the selection, operators take it.
    fn visual(
        &mut self,
        key: &str,
        buffer: &mut VimBuffer,
        clipboard: Option<&mut dyn Clipboard>,
    ) -> VimOutcome {
        let key = named_key(key).unwrap_or(key);
        if let Some(Pending::VisualObject(around)) = self.pending {
            self.record(key);
            return self.normal_pending(Pending::VisualObject(around), key, buffer, clipboard);
        }
        let (Some(c), None) = (key.chars().next(), key.chars().nth(1)) else {
            return VimOutcome::Passthrough;
        };
        self.record(key);
        match c {
            'h' => {
                let count = self.take_counts();
                self.move_caret(buffer, count, |_, at| at.saturating_sub(1))
            }
            'l' | ' ' => {
                let count = self.take_counts();
                self.move_caret(buffer, count, |chars, at| {
                    (at + 1).min(chars.len().saturating_sub(1))
                })
            }
            'j' | 'k' => {
                self.take_counts();
                VimOutcome::Consumed
            }
            'w' | 'W' | 'b' | 'B' | 'e' | 'E' => {
                let count = self.take_counts();
                let kind = word_kind(c);
                self.move_caret(buffer, count, move |chars, at| {
                    word_motion(c, chars, at, kind)
                })
            }
            '0' => self.move_caret(buffer, 1, |_, _| 0),
            '^' => self.move_caret(buffer, 1, |chars, _| first_non_blank(chars)),
            '$' | 'G' => self.move_caret(buffer, 1, |chars, _| chars.len().saturating_sub(1)),
            '%' => {
                let count = self.take_counts();
                self.move_caret(buffer, count, |chars, at| {
                    matching_bracket_index(chars, at).unwrap_or(at)
                })
            }
            'f' | 'F' | 't' | 'T' => {
                self.pending = Some(Pending::Find(FindMotion {
                    forward: matches!(c, 'f' | 't'),
                    before: matches!(c, 't' | 'T'),
                    target: ' ',
                }));
                VimOutcome::Consumed
            }
            ';' | ',' => {
                let count = self.take_counts();
                let Some(mut motion) = self.last_find else {
                    return VimOutcome::Consumed;
                };
                if c == ',' {
                    motion.forward = !motion.forward;
                }
                self.find_move(motion, count, buffer)
            }
            'i' | 'a' => {
                self.pending = Some(Pending::VisualObject(c == 'a'));
                VimOutcome::Consumed
            }
            // Swap which end of the selection moves.
            'o' => {
                std::mem::swap(&mut self.anchor, &mut buffer.caret);
                VimOutcome::Moved
            }
            'v' => {
                self.mode = VimMode::Normal;
                buffer.caret = normal_caret(buffer.caret, buffer.len());
                VimOutcome::Consumed
            }
            'V' => {
                self.mode = VimMode::Visual(MotionType::Linewise);
                self.anchor = 0;
                VimOutcome::Consumed
            }
            'd' | 'x' | 'c' | 's' | 'y' | 'Y' | '~' | 'u' | 'U' => {
                let operator = match c {
                    'd' | 'x' => Operator::Delete,
                    'c' | 's' => Operator::Change,
                    'y' | 'Y' => Operator::Yank,
                    '~' => Operator::ToggleCase,
                    'u' => Operator::Lowercase,
                    _ => Operator::Uppercase,
                };
                let linewise =
                    matches!(self.mode, VimMode::Visual(MotionType::Linewise)) || c == 'Y';
                let Some(range) = self.selection(buffer.caret, buffer.len()) else {
                    self.mode = VimMode::Normal;
                    return VimOutcome::Consumed;
                };
                self.mode = VimMode::Normal;
                self.apply_operator(operator, range, linewise, buffer, clipboard)
            }
            'p' | 'P' => {
                let register = self.used_register();
                let content = self.read_register(register, clipboard);
                let Some(range) = self.selection(buffer.caret, buffer.len()) else {
                    self.mode = VimMode::Normal;
                    return VimOutcome::Consumed;
                };
                let count = self.take_counts();
                self.mode = VimMode::Normal;
                self.snapshot(buffer);
                buffer.delete(range);
                let at = buffer.caret;
                buffer.insert(at, &content.text.repeat(count.max(1)));
                self.remember_change(None);
                VimOutcome::Edited
            }
            _ => VimOutcome::Consumed,
        }
    }
}

/// Where an insert session opens the insertion point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InsertAt {
    Here,
    After,
    FirstNonBlank,
    End,
}

/// Where a normal-mode insertion point may sit: on a character.
fn normal_caret(caret: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        caret.min(len - 1)
    }
}

/// The first non-blank character's index, or 0.
fn first_non_blank(chars: &[char]) -> usize {
    chars
        .iter()
        .position(|c| !c.is_whitespace())
        .unwrap_or(0)
}

/// The single-character command a named key stands for, if any.
fn named_key(key: &str) -> Option<&'static str> {
    match key {
        "Backspace" | "ArrowLeft" => Some("h"),
        "ArrowRight" => Some("l"),
        "ArrowUp" | "ArrowDown" => Some("k"),
        "Home" => Some("0"),
        "End" => Some("$"),
        "Delete" => Some("x"),
        _ => None,
    }
}

/// The key that names a find motion.
fn find_char(motion: FindMotion) -> char {
    match (motion.forward, motion.before) {
        (true, false) => 'f',
        (true, true) => 't',
        (false, false) => 'F',
        (false, true) => 'T',
    }
}

/// Whether `c` can name a register.
fn is_register_name(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '+' | '*' | '"' | '_')
}

/// Which flavour of word a motion key steps by.
fn word_kind(c: char) -> WordKind {
    if c.is_ascii_uppercase() {
        BigWord
    } else {
        Word
    }
}

/// The motion a word key makes.
fn word_motion(c: char, chars: &[char], at: usize, kind: WordKind) -> usize {
    match c {
        'w' | 'W' => words::word_start_forward(chars, at, kind),
        'b' | 'B' => words::word_start_backward(chars, at, kind),
        'e' | 'E' => words::word_end_forward(chars, at, kind),
        _ => at,
    }
}

/// The index of the bracket matching the one under `at`, for `%`.
fn matching_bracket_index(chars: &[char], at: usize) -> Option<usize> {
    let (bracket, opening) = Bracket::of(*chars.get(at)?)?;
    objects::matching_bracket(chars, at, bracket, opening)
}

/// Repeat a motion `count` times.
fn repeat_motion(count: usize, caret: usize, step: impl Fn(usize) -> usize) -> usize {
    let mut at = caret;
    for _ in 0..count.max(1) {
        at = step(at);
    }
    at
}

/// The index of the `count`-th match of a find motion, searching from `caret`.
fn find_index(chars: &[char], caret: usize, motion: FindMotion, count: usize) -> Option<usize> {
    let mut seen = 0usize;
    if motion.forward {
        let mut i = caret + 1;
        while i < chars.len() {
            if chars[i] == motion.target {
                seen += 1;
                if seen == count.max(1) {
                    return Some(i);
                }
            }
            i += 1;
        }
    } else {
        let mut i = caret;
        while i > 0 {
            if chars[i - 1] == motion.target {
                seen += 1;
                if seen == count.max(1) {
                    return Some(i - 1);
                }
            }
            i -= 1;
        }
    }
    None
}

/// Where the insertion point lands for a find motion (`t`/`T` stop one short).
fn find_caret_index(
    chars: &[char],
    caret: usize,
    motion: FindMotion,
    count: usize,
) -> Option<usize> {
    let found = find_index(chars, caret, motion, count)?;
    Some(if motion.before {
        if motion.forward {
            found.saturating_sub(1)
        } else {
            (found + 1).min(chars.len().saturating_sub(1))
        }
    } else {
        found
    })
}

/// The range an operator takes for a find motion.
fn find_operator_range(
    caret: usize,
    found: usize,
    motion: FindMotion,
    len: usize,
) -> Range<usize> {
    if motion.forward {
        if motion.before {
            // `t`: up to, but not including, the character found.
            caret..found
        } else {
            caret..(found + 1).min(len)
        }
    } else if motion.before {
        // `T`: from just after the character found, back to the cursor.
        (found + 1).min(len)..caret
    } else {
        found..caret
    }
}
