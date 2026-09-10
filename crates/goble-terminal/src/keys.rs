//! Host → PTY key encoding.
//!
//! The encoder is a pure function of `(key, modifiers, terminal mode)` → bytes.
//! Application-level shortcuts (Cmd on macOS) are resolved before this point and
//! never reach the PTY, so [`Modifiers`] only carries the three modifiers a
//! terminal reports to the shell: Shift, Alt (Meta) and Control.
//!
//! Encoding is the legacy xterm form, which is what shell line editors and
//! full-screen TUIs expect. The Kitty keyboard protocol (`CSI … u`) is a
//! separate, application-negotiated encoding and is not produced here.

use std::fmt;

/// Modifiers reported to the PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        shift: false,
        alt: false,
        ctrl: false,
    };

    pub const fn new(shift: bool, alt: bool, ctrl: bool) -> Self {
        Self { shift, alt, ctrl }
    }

    pub const fn any(self) -> bool {
        self.shift || self.alt || self.ctrl
    }

    /// The xterm modifier parameter: 1 + Shift(1) + Alt(2) + Ctrl(4).
    /// Only meaningful when [`Modifiers::any`] is true.
    pub const fn xterm(self) -> u8 {
        1 + self.shift as u8 + 2 * self.alt as u8 + 4 * self.ctrl as u8
    }
}

/// The parts of the terminal mode that change key encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct TermMode {
    /// DECCKM: cursor keys send `SS3` instead of `CSI`.
    pub app_cursor: bool,
}

/// A logical key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// A printable character, or the text produced by the key (Ctrl letters
    /// arrive here as their letter and are mapped to C0 bytes by the encoder).
    Char(char),
    Enter,
    Tab,
    BackTab,
    Backspace,
    Escape,
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1..=F20.
    Function(u8),
}

const ESC: u8 = 0x1b;
const CSI: &[u8] = b"\x1b[";
const SS3: &[u8] = b"\x1bO";

/// Stateless key encoder.
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyEncoder;

impl KeyEncoder {
    /// Encode a key press into the bytes to write to the PTY.
    pub fn encode(key: Key, modifiers: Modifiers, mode: TermMode) -> Vec<u8> {
        let mut out = Vec::with_capacity(8);
        Self::encode_into(&mut out, key, modifiers, mode);
        out
    }

    /// Encode into an existing buffer.
    pub fn encode_into(out: &mut Vec<u8>, key: Key, modifiers: Modifiers, mode: TermMode) {
        match key {
            Key::Char(c) => encode_char(out, c, modifiers),
            Key::Enter => {
                if modifiers.alt {
                    out.push(ESC);
                }
                out.push(b'\r');
            }
            Key::Escape => {
                out.push(ESC);
                if modifiers.alt {
                    out.push(ESC);
                }
            }
            Key::Backspace => {
                if modifiers.alt {
                    out.push(ESC);
                }
                out.push(0x7f);
            }
            Key::Tab => {
                if modifiers.shift {
                    // Shift+Tab is back-tab, which has no modifier parameter.
                    out.extend_from_slice(b"\x1b[Z");
                } else {
                    if modifiers.alt {
                        out.push(ESC);
                    }
                    out.push(b'\t');
                }
            }
            Key::BackTab => out.extend_from_slice(b"\x1b[Z"),
            Key::Up => cursor_key(out, b'A', modifiers, mode),
            Key::Down => cursor_key(out, b'B', modifiers, mode),
            Key::Right => cursor_key(out, b'C', modifiers, mode),
            Key::Left => cursor_key(out, b'D', modifiers, mode),
            Key::Home => cursor_key(out, b'H', modifiers, mode),
            Key::End => cursor_key(out, b'F', modifiers, mode),
            Key::PageUp => tilde_key(out, 5, modifiers),
            Key::PageDown => tilde_key(out, 6, modifiers),
            Key::Insert => tilde_key(out, 2, modifiers),
            Key::Delete => tilde_key(out, 3, modifiers),
            Key::Function(n) => function_key(out, n, modifiers),
        }
    }
}

/// Cursor keys: `CSI A`, `SS3 A` in application-cursor mode, and `CSI 1;<mod>A`
/// as soon as any modifier is held.
fn cursor_key(out: &mut Vec<u8>, code: u8, modifiers: Modifiers, mode: TermMode) {
    if modifiers.any() {
        out.extend_from_slice(CSI);
        push_modifier(out, modifiers);
        out.push(code);
    } else if mode.app_cursor {
        out.extend_from_slice(SS3);
        out.push(code);
    } else {
        out.extend_from_slice(CSI);
        out.push(code);
    }
}

/// Keys in the `CSI <n> ~` family.
fn tilde_key(out: &mut Vec<u8>, number: u8, modifiers: Modifiers) {
    out.extend_from_slice(CSI);
    push_number(out, number);
    if modifiers.any() {
        out.push(b';');
        push_number(out, modifiers.xterm());
    }
    out.push(b'~');
}

fn function_key(out: &mut Vec<u8>, n: u8, modifiers: Modifiers) {
    match n {
        1..=4 => {
            let code = b'P' + (n - 1);
            if modifiers.any() {
                out.extend_from_slice(CSI);
                push_modifier(out, modifiers);
                out.push(code);
            } else {
                out.extend_from_slice(SS3);
                out.push(code);
            }
        }
        5..=20 => {
            let number = match n {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                12 => 24,
                13 => 25,
                14 => 26,
                15 => 28,
                16 => 29,
                17 => 31,
                18 => 32,
                19 => 33,
                _ => 34,
            };
            tilde_key(out, number, modifiers);
        }
        _ => {}
    }
}

/// `CSI 1;<mod>` — the shared prefix of every modified key.
fn push_modifier(out: &mut Vec<u8>, modifiers: Modifiers) {
    out.extend_from_slice(b"1;");
    push_number(out, modifiers.xterm());
}

fn push_number(out: &mut Vec<u8>, value: u8) {
    if value >= 100 {
        out.push(b'0' + value / 100);
    }
    if value >= 10 {
        out.push(b'0' + (value / 10) % 10);
    }
    out.push(b'0' + value % 10);
}

/// Text keys. Ctrl with no other modifier is mapped to a C0 byte when one
/// exists; otherwise the character's own bytes are sent, prefixed with `ESC`
/// when Alt is held.
fn encode_char(out: &mut Vec<u8>, c: char, modifiers: Modifiers) {
    if modifiers.ctrl && !modifiers.alt && !modifiers.shift {
        if let Some(byte) = c0_for(c) {
            out.push(byte);
            return;
        }
    }

    if modifiers.alt {
        out.push(ESC);
    }

    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

/// The C0 control produced by Ctrl + `c`, when the combination has one.
fn c0_for(c: char) -> Option<u8> {
    let upper = c.to_ascii_uppercase();
    let byte = match upper {
        'A'..='Z' => upper as u8 - b'A' + 1,
        '@' | ' ' => 0x00,
        '[' => 0x1b,
        '\\' => 0x1c,
        ']' => 0x1d,
        '^' => 0x1e,
        '_' => 0x1f,
        '?' => 0x7f,
        _ => return None,
    };
    Some(byte)
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Char(c) => write!(f, "{c:?}"),
            other => write!(f, "{other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: TermMode = TermMode { app_cursor: false };
    const APP: TermMode = TermMode { app_cursor: true };

    fn enc(key: Key, m: Modifiers) -> Vec<u8> {
        KeyEncoder::encode(key, m, BASE)
    }

    fn enc_mode(key: Key, m: Modifiers, mode: TermMode) -> Vec<u8> {
        KeyEncoder::encode(key, m, mode)
    }

    #[test]
    fn text_keys_send_utf8() {
        assert_eq!(enc(Key::Char('a'), Modifiers::NONE), b"a");
        assert_eq!(
            enc(Key::Char('A'), Modifiers::new(true, false, false)),
            b"A"
        );
        assert_eq!(enc(Key::Char('ș'), Modifiers::NONE), "ș".as_bytes());
    }

    #[test]
    fn alt_prefixes_escape() {
        let alt = Modifiers::new(false, true, false);
        assert_eq!(enc(Key::Char('x'), alt), b"\x1bx");
        assert_eq!(enc(Key::Enter, alt), b"\x1b\r");
        assert_eq!(enc(Key::Backspace, alt), b"\x1b\x7f");
    }

    #[test]
    fn ctrl_letters_map_to_c0() {
        let ctrl = Modifiers::new(false, false, true);
        assert_eq!(enc(Key::Char('c'), ctrl), vec![0x03]);
        assert_eq!(enc(Key::Char(' '), ctrl), vec![0x00]);
        assert_eq!(enc(Key::Char('['), ctrl), vec![0x1b]);
        assert_eq!(enc(Key::Char('\\'), ctrl), vec![0x1c]);
        assert_eq!(enc(Key::Char(']'), ctrl), vec![0x1d]);
        assert_eq!(enc(Key::Char('_'), ctrl), vec![0x1f]);
        // No C0 mapping: fall back to the raw bytes.
        assert_eq!(enc(Key::Char('1'), ctrl), b"1");
    }

    #[test]
    fn ctrl_with_alt_does_not_use_c0_table() {
        assert_eq!(
            enc(Key::Char('c'), Modifiers::new(false, true, true)),
            b"\x1bc"
        );
    }

    #[test]
    fn cursor_keys_plain_and_application_mode() {
        assert_eq!(enc(Key::Up, Modifiers::NONE), b"\x1b[A");
        assert_eq!(enc(Key::Down, Modifiers::NONE), b"\x1b[B");
        assert_eq!(enc(Key::Right, Modifiers::NONE), b"\x1b[C");
        assert_eq!(enc(Key::Left, Modifiers::NONE), b"\x1b[D");
        assert_eq!(enc(Key::Home, Modifiers::NONE), b"\x1b[H");
        assert_eq!(enc(Key::End, Modifiers::NONE), b"\x1b[F");

        assert_eq!(enc_mode(Key::Up, Modifiers::NONE, APP), b"\x1bOA");
        assert_eq!(enc_mode(Key::Home, Modifiers::NONE, APP), b"\x1bOH");
    }

    #[test]
    fn any_modifier_forces_csi_form() {
        let shift = Modifiers::new(true, false, false);
        let ctrl = Modifiers::new(false, false, true);
        let all = Modifiers::new(true, true, true);
        assert_eq!(enc_mode(Key::Up, shift, APP), b"\x1b[1;2A");
        assert_eq!(enc_mode(Key::Left, ctrl, BASE), b"\x1b[1;5D");
        assert_eq!(enc_mode(Key::Right, all, BASE), b"\x1b[1;8C");
    }

    #[test]
    fn tilde_family() {
        assert_eq!(enc(Key::Insert, Modifiers::NONE), b"\x1b[2~");
        assert_eq!(enc(Key::Delete, Modifiers::NONE), b"\x1b[3~");
        assert_eq!(enc(Key::PageUp, Modifiers::NONE), b"\x1b[5~");
        assert_eq!(enc(Key::PageDown, Modifiers::NONE), b"\x1b[6~");
        assert_eq!(
            enc(Key::PageUp, Modifiers::new(true, false, false)),
            b"\x1b[5;2~"
        );
        assert_eq!(
            enc(Key::Delete, Modifiers::new(true, true, true)),
            b"\x1b[3;8~"
        );
    }

    #[test]
    fn function_keys() {
        assert_eq!(enc(Key::Function(1), Modifiers::NONE), b"\x1bOP");
        assert_eq!(enc(Key::Function(4), Modifiers::NONE), b"\x1bOS");
        assert_eq!(enc(Key::Function(5), Modifiers::NONE), b"\x1b[15~");
        assert_eq!(enc(Key::Function(10), Modifiers::NONE), b"\x1b[21~");
        assert_eq!(enc(Key::Function(11), Modifiers::NONE), b"\x1b[23~");
        assert_eq!(enc(Key::Function(12), Modifiers::NONE), b"\x1b[24~");
        assert_eq!(enc(Key::Function(13), Modifiers::NONE), b"\x1b[25~");
        assert_eq!(enc(Key::Function(20), Modifiers::NONE), b"\x1b[34~");
        // Modified: F1..F4 switch to the CSI form, the rest gain a parameter.
        assert_eq!(
            enc(Key::Function(2), Modifiers::new(true, false, false)),
            b"\x1b[1;2Q"
        );
        assert_eq!(
            enc(Key::Function(5), Modifiers::new(false, false, true)),
            b"\x1b[15;5~"
        );
        // Out of range encodes nothing.
        assert_eq!(enc(Key::Function(0), Modifiers::NONE), b"");
        assert_eq!(enc(Key::Function(21), Modifiers::NONE), b"");
    }

    #[test]
    fn tab_enter_backspace_escape() {
        assert_eq!(enc(Key::Tab, Modifiers::NONE), b"\t");
        assert_eq!(enc(Key::Tab, Modifiers::new(true, false, false)), b"\x1b[Z");
        assert_eq!(enc(Key::BackTab, Modifiers::NONE), b"\x1b[Z");
        assert_eq!(enc(Key::Enter, Modifiers::NONE), b"\r");
        assert_eq!(enc(Key::Backspace, Modifiers::NONE), b"\x7f");
        assert_eq!(enc(Key::Escape, Modifiers::NONE), b"\x1b");
    }

    #[test]
    fn modifier_parameter_matches_xterm() {
        assert_eq!(Modifiers::new(true, false, false).xterm(), 2);
        assert_eq!(Modifiers::new(false, true, false).xterm(), 3);
        assert_eq!(Modifiers::new(true, true, false).xterm(), 4);
        assert_eq!(Modifiers::new(false, false, true).xterm(), 5);
        assert_eq!(Modifiers::new(true, false, true).xterm(), 6);
        assert_eq!(Modifiers::new(false, true, true).xterm(), 7);
        assert_eq!(Modifiers::new(true, true, true).xterm(), 8);
    }
}
