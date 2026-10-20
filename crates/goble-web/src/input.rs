//! Translate browser key events into bytes for the pty.
//!
//! The browser reports a key plus modifiers; the pty wants the byte sequence a
//! real terminal would send. This module is deliberately DOM-free so the
//! mapping is covered by native unit tests.
//!
//! Application shortcuts (`Ctrl`/`Cmd` + `Shift` + key) are reserved: the DOM
//! layer consumes them before the encoder ever sees them, and [`KeyEvent::encode`]
//! returns `None` for the whole family so a missed reservation can never leak a
//! raw control byte into the shell.

/// One key press as reported by the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    /// `KeyboardEvent::key`: `"a"`, `"Enter"`, `"ArrowUp"`, … .
    pub key: String,
    /// `KeyboardEvent::code`: physical key, e.g. `"KeyA"`.
    pub code: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    /// Auto-repeat. Repeated keys still reach the pty; kept for callers that
    /// want to suppress repeat-driven UI work.
    pub repeat: bool,
}

impl KeyEvent {
    #[must_use]
    pub fn new(key: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            code: code.into(),
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
            repeat: false,
        }
    }

    #[must_use]
    pub fn ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    #[must_use]
    pub fn alt(mut self) -> Self {
        self.alt = true;
        self
    }

    #[must_use]
    pub fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    #[must_use]
    pub fn meta(mut self) -> Self {
        self.meta = true;
        self
    }

    #[must_use]
    pub fn repeat(mut self) -> Self {
        self.repeat = true;
        self
    }

    /// True for the `Ctrl`/`Cmd` + `Shift` family the UI reserves for itself.
    #[must_use]
    pub fn is_reserved_shortcut(&self) -> bool {
        (self.ctrl || self.meta) && self.shift
    }

    /// Bytes to send to the pty, or `None` when the key is a reserved shortcut
    /// or has no terminal meaning.
    #[must_use]
    pub fn encode(&self) -> Option<Vec<u8>> {
        if self.is_reserved_shortcut() {
            return None;
        }
        // A bare Command press is for the OS/browser; macOS users do not expect
        // Cmd+C to reach the shell as a control byte.
        if self.meta && !self.ctrl {
            return None;
        }

        if let Some(sequence) = self.named_key() {
            return Some(with_alt(sequence, self.alt));
        }

        let mut chars = self.key.chars();
        let character = chars.next()?;
        if chars.next().is_some() || character.is_control() {
            return None;
        }

        let byte = if self.ctrl {
            control_byte(character)?
        } else if character.is_ascii() {
            character as u8
        } else {
            let mut buffer = [0u8; 4];
            return Some(with_alt(
                character.encode_utf8(&mut buffer).as_bytes().to_vec(),
                self.alt,
            ));
        };

        Some(with_alt(vec![byte], self.alt))
    }

    fn named_key(&self) -> Option<Vec<u8>> {
        let sequence: &[u8] = match self.key.as_str() {
            "Enter" => b"\r",
            "Backspace" => b"\x7f",
            "Tab" if self.shift => b"\x1b[Z",
            "Tab" => b"\t",
            "Escape" => b"\x1b",
            "Delete" => b"\x1b[3~",
            "Insert" => b"\x1b[2~",
            "Home" => b"\x1b[H",
            "End" => b"\x1b[F",
            "PageUp" => b"\x1b[5~",
            "PageDown" => b"\x1b[6~",
            "ArrowUp" => b"\x1b[A",
            "ArrowDown" => b"\x1b[B",
            "ArrowRight" => b"\x1b[C",
            "ArrowLeft" => b"\x1b[D",
            "F1" => b"\x1bOP",
            "F2" => b"\x1bOQ",
            "F3" => b"\x1bOR",
            "F4" => b"\x1bOS",
            "F5" => b"\x1b[15~",
            "F6" => b"\x1b[17~",
            "F7" => b"\x1b[18~",
            "F8" => b"\x1b[19~",
            "F9" => b"\x1b[20~",
            "F10" => b"\x1b[21~",
            "F11" => b"\x1b[23~",
            "F12" => b"\x1b[24~",
            // A name, not a character: the browser reports this for keys it
            // cannot map, and sending anything would be a guess.
            _ => return None,
        };
        Some(sequence.to_vec())
    }
}

fn with_alt(mut bytes: Vec<u8>, alt: bool) -> Vec<u8> {
    if alt {
        bytes.insert(0, 0x1b);
    }
    bytes
}

/// C0 byte for `Ctrl` + a single character, following xterm conventions.
fn control_byte(character: char) -> Option<u8> {
    let byte = match character {
        'a'..='z' => character as u8 - b'a' + 1,
        'A'..='Z' => character as u8 - b'A' + 1,
        ' ' | '@' | '2' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '7' | '/' => 0x1f,
        '?' | '8' => 0x7f,
        _ => return None,
    };
    Some(byte)
}

/// Raw UTF-8 for composed text (IME, mobile keyboards). Empty input produces
/// no bytes.
#[must_use]
pub fn text_bytes(text: &str) -> Option<Vec<u8>> {
    (!text.is_empty()).then(|| text.as_bytes().to_vec())
}

/// Bytes for a paste, with bracketed-paste handling.
///
/// Browsers hand over rich text; a terminal wants `\r` line endings. Bracketed
/// paste markers in the pasted text itself are stripped so a nested paste does
/// not terminate the shell's paste mode early. When the shell has enabled
/// bracketed paste, the payload is wrapped in the mode's markers.
#[must_use]
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let stripped = text.replace("\x1b[200~", "").replace("\x1b[201~", "");
    let normalized = stripped
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r");
    if bracketed {
        let mut bytes = Vec::with_capacity(normalized.len() + 12);
        bytes.extend_from_slice(b"\x1b[200~");
        bytes.extend_from_slice(normalized.as_bytes());
        bytes.extend_from_slice(b"\x1b[201~");
        bytes
    } else {
        normalized.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: &str) -> KeyEvent {
        KeyEvent::new(key, "")
    }

    #[test]
    fn printable_ascii() {
        assert_eq!(press("a").encode(), Some(b"a".to_vec()));
        assert_eq!(press("Z").shift().encode(), Some(b"Z".to_vec()));
        assert_eq!(press(" ").encode(), Some(b" ".to_vec()));
    }

    #[test]
    fn printable_unicode_is_utf8() {
        assert_eq!(press("é").encode(), Some("é".as_bytes().to_vec()));
        assert_eq!(press("界").encode(), Some("界".as_bytes().to_vec()));
    }

    #[test]
    fn ctrl_letters_are_control_bytes() {
        assert_eq!(press("a").ctrl().encode(), Some(vec![0x01]));
        assert_eq!(press("A").ctrl().encode(), Some(vec![0x01]));
        assert_eq!(press("c").ctrl().encode(), Some(vec![0x03]));
        assert_eq!(press("z").ctrl().encode(), Some(vec![0x1a]));
    }

    #[test]
    fn ctrl_specials() {
        assert_eq!(press("[").ctrl().encode(), Some(vec![0x1b]));
        assert_eq!(press("\\").ctrl().encode(), Some(vec![0x1c]));
        assert_eq!(press(" ").ctrl().encode(), Some(vec![0x00]));
        assert_eq!(press("?").ctrl().encode(), Some(vec![0x7f]));
    }

    #[test]
    fn alt_prefixes_escape() {
        assert_eq!(press("x").alt().encode(), Some(b"\x1bx".to_vec()));
        assert_eq!(press("Enter").alt().encode(), Some(b"\x1b\r".to_vec()));
        assert_eq!(
            press("ArrowUp").alt().encode(),
            Some(b"\x1b\x1b[A".to_vec())
        );
    }

    #[test]
    fn enter_backspace_tab() {
        assert_eq!(press("Enter").encode(), Some(b"\r".to_vec()));
        assert_eq!(press("Backspace").encode(), Some(b"\x7f".to_vec()));
        assert_eq!(press("Tab").encode(), Some(b"\t".to_vec()));
        assert_eq!(press("Tab").shift().encode(), Some(b"\x1b[Z".to_vec()));
    }

    #[test]
    fn escape_is_single_byte() {
        assert_eq!(press("Escape").encode(), Some(b"\x1b".to_vec()));
    }

    #[test]
    fn arrows_and_editing_keys() {
        assert_eq!(press("ArrowUp").encode(), Some(b"\x1b[A".to_vec()));
        assert_eq!(press("ArrowDown").encode(), Some(b"\x1b[B".to_vec()));
        assert_eq!(press("ArrowRight").encode(), Some(b"\x1b[C".to_vec()));
        assert_eq!(press("ArrowLeft").encode(), Some(b"\x1b[D".to_vec()));
        assert_eq!(press("Home").encode(), Some(b"\x1b[H".to_vec()));
        assert_eq!(press("End").encode(), Some(b"\x1b[F".to_vec()));
        assert_eq!(press("Insert").encode(), Some(b"\x1b[2~".to_vec()));
        assert_eq!(press("Delete").encode(), Some(b"\x1b[3~".to_vec()));
        assert_eq!(press("PageUp").encode(), Some(b"\x1b[5~".to_vec()));
        assert_eq!(press("PageDown").encode(), Some(b"\x1b[6~".to_vec()));
    }

    #[test]
    fn function_keys() {
        assert_eq!(press("F1").encode(), Some(b"\x1bOP".to_vec()));
        assert_eq!(press("F4").encode(), Some(b"\x1bOS".to_vec()));
        assert_eq!(press("F5").encode(), Some(b"\x1b[15~".to_vec()));
        assert_eq!(press("F6").encode(), Some(b"\x1b[17~".to_vec()));
        assert_eq!(press("F9").encode(), Some(b"\x1b[20~".to_vec()));
        assert_eq!(press("F12").encode(), Some(b"\x1b[24~".to_vec()));
    }

    #[test]
    fn app_shortcuts_are_reserved() {
        for key in ["t", "w", "r", "c", "v"] {
            assert_eq!(press(key).ctrl().shift().encode(), None, "{key}");
            assert_eq!(press(key).meta().shift().encode(), None, "{key}");
        }
        assert_eq!(press("Tab").ctrl().shift().encode(), None);
    }

    #[test]
    fn meta_alone_is_not_sent() {
        assert_eq!(press("a").meta().encode(), None);
        assert_eq!(press("ArrowLeft").meta().encode(), None);
    }

    #[test]
    fn unnamed_keys_are_dropped() {
        assert_eq!(press("Unidentified").encode(), None);
        assert_eq!(press("Dead").encode(), None);
        assert_eq!(press("Shift").encode(), None);
    }

    #[test]
    fn ctrl_without_a_control_byte_is_dropped() {
        assert_eq!(press("é").ctrl().encode(), None);
    }

    #[test]
    fn repeat_still_encodes() {
        assert_eq!(press("x").repeat().encode(), Some(b"x".to_vec()));
    }

    #[test]
    fn ime_text_is_raw_utf8() {
        assert_eq!(
            text_bytes("こんにちは"),
            Some("こんにちは".as_bytes().to_vec())
        );
        assert_eq!(text_bytes(""), None);
    }

    #[test]
    fn paste_normalizes_newlines() {
        assert_eq!(paste_bytes("a\nb", false), b"a\rb".to_vec());
        assert_eq!(paste_bytes("a\r\nb", false), b"a\rb".to_vec());
        assert_eq!(paste_bytes("a\rb", false), b"a\rb".to_vec());
    }

    #[test]
    fn paste_wraps_when_bracketed() {
        assert_eq!(paste_bytes("hi", true), b"\x1b[200~hi\x1b[201~".to_vec());
        assert_eq!(paste_bytes("hi", false), b"hi".to_vec());
    }

    #[test]
    fn paste_strips_nested_bracketed_markers() {
        assert_eq!(paste_bytes("\x1b[200~x\x1b[201~", false), b"x".to_vec());
    }
}
