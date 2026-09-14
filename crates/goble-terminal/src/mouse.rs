//! Host → PTY mouse encoding.
//!
//! Mouse reporting is negotiated, never assumed. The application turns on a
//! reporting mode — `1000` press/release, `1002` also motion while a button is
//! held, `1003` also motion with no button — and may additionally ask for the
//! SGR form (`1006`). A report carries the button, the modifiers held, and the
//! cell under the pointer. Without `1006` the legacy X10 form is produced,
//! which cannot express releases or coordinates past column 223.
//!
//! Three deliberate differences from the reference terminal, called out in the
//! design doc: the middle button is reported, modifiers are encoded, and the
//! wheel scrolls the application's viewport rather than being interpreted here.

use crate::keys::{Modifiers, TermMode};

/// The button a report is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// What happened at the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseAction {
    Press(MouseButton),
    Release(MouseButton),
    /// The pointer moved with this button held.
    Drag(MouseButton),
    /// The pointer moved with no button held.
    Move,
    WheelUp,
    WheelDown,
}

impl MouseAction {
    /// The button code before the 32 offset xterm applies to it.
    const fn code(self) -> u8 {
        match self {
            MouseAction::Press(button)
            | MouseAction::Release(button)
            | MouseAction::Drag(button) => match button {
                MouseButton::Left => 0,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
            },
            // A motion report with no button is button code 3.
            MouseAction::Move => 3,
            MouseAction::WheelUp => 64,
            MouseAction::WheelDown => 65,
        }
    }

    const fn is_release(self) -> bool {
        matches!(self, MouseAction::Release(_))
    }

    const fn is_motion(self) -> bool {
        matches!(self, MouseAction::Drag(_) | MouseAction::Move)
    }
}

/// The largest one-based coordinate the legacy form can carry: past this the
/// value plus the 32 offset would overrun the byte.
const LEGACY_MAX: usize = 223;

/// Encode one pointer report, or `None` when the application is not listening
/// or has not asked for that kind of report.
///
/// `column` and `line` are zero-based cell coordinates; the wire form is
/// one-based, which is applied here.
pub fn encode_mouse(
    action: MouseAction,
    column: usize,
    line: usize,
    modifiers: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if !allowed(action, mode) {
        return None;
    }

    let mut code = action.code();
    // Motion reports carry the 32 bit that marks them as motion.
    if action.is_motion() {
        code += 32;
    }
    if modifiers.shift {
        code += 4;
    }
    if modifiers.alt {
        code += 8;
    }
    if modifiers.ctrl {
        code += 16;
    }

    let column = column + 1;
    let line = line + 1;

    if mode.sgr_mouse {
        // The SGR form carries the button code as a decimal number, so only
        // the motion bit and the modifiers are added to it; the 32 offset
        // belongs to the legacy form alone.
        let mut out = Vec::with_capacity(16);
        out.extend_from_slice(b"\x1b[<");
        push_number(&mut out, code.into());
        out.push(b';');
        push_number(&mut out, column as u32);
        out.push(b';');
        push_number(&mut out, line as u32);
        // SGR distinguishes a release by the final byte, so the button itself
        // survives; the legacy form cannot do this.
        out.push(if action.is_release() { b'm' } else { b'M' });
        return Some(out);
    }

    // Legacy X10 form: three bytes after the introducer, each offset by 32.
    // A release has no encoding of its own, so it is reported as button 3 —
    // the reason SGR exists.
    let legacy_code = if action.is_release() { 3 } else { code };
    let column = column.min(LEGACY_MAX);
    let line = line.min(LEGACY_MAX);
    let mut out = Vec::with_capacity(6);
    out.extend_from_slice(b"\x1b[M");
    out.push(32 + legacy_code);
    out.push(32 + column as u8);
    out.push(32 + line as u8);
    Some(out)
}

/// Whether the negotiated mode wants this report at all.
fn allowed(action: MouseAction, mode: TermMode) -> bool {
    if !mode.mouse_reported() {
        return false;
    }
    match action {
        MouseAction::Move => mode.mouse_motion,
        MouseAction::Drag(_) => mode.mouse_drag || mode.mouse_motion,
        MouseAction::Press(_)
        | MouseAction::Release(_)
        | MouseAction::WheelUp
        | MouseAction::WheelDown => true,
    }
}

fn push_number(out: &mut Vec<u8>, value: u32) {
    if value >= 1000 {
        out.push(b'0' + ((value / 1000) % 10) as u8);
    }
    if value >= 100 {
        out.push(b'0' + ((value / 100) % 10) as u8);
    }
    if value >= 10 {
        out.push(b'0' + ((value / 10) % 10) as u8);
    }
    out.push(b'0' + (value % 10) as u8);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SGR: TermMode = TermMode {
        mouse_click: true,
        sgr_mouse: true,
        ..TermMode::NONE
    };
    const LEGACY: TermMode = TermMode {
        mouse_click: true,
        ..TermMode::NONE
    };

    fn sgr(action: MouseAction, column: usize, line: usize) -> String {
        String::from_utf8(encode_mouse(action, column, line, Modifiers::NONE, SGR).unwrap())
            .unwrap()
    }

    fn legacy(action: MouseAction, column: usize, line: usize) -> Vec<u8> {
        encode_mouse(action, column, line, Modifiers::NONE, LEGACY).unwrap()
    }

    #[test]
    fn nothing_is_reported_until_the_application_asks() {
        for action in [
            MouseAction::Press(MouseButton::Left),
            MouseAction::Release(MouseButton::Left),
            MouseAction::WheelUp,
            MouseAction::Move,
        ] {
            assert_eq!(
                encode_mouse(action, 0, 0, Modifiers::NONE, TermMode::NONE),
                None
            );
        }
    }

    #[test]
    fn sgr_press_release_and_wheel() {
        assert_eq!(
            sgr(MouseAction::Press(MouseButton::Left), 0, 0),
            "\x1b[<0;1;1M"
        );
        assert_eq!(
            sgr(MouseAction::Release(MouseButton::Left), 0, 0),
            "\x1b[<0;1;1m"
        );
        assert_eq!(
            sgr(MouseAction::Press(MouseButton::Middle), 4, 2),
            "\x1b[<1;5;3M"
        );
        assert_eq!(
            sgr(MouseAction::Press(MouseButton::Right), 9, 3),
            "\x1b[<2;10;4M"
        );
        assert_eq!(sgr(MouseAction::WheelUp, 0, 0), "\x1b[<64;1;1M");
        assert_eq!(sgr(MouseAction::WheelDown, 0, 0), "\x1b[<65;1;1M");
    }

    #[test]
    fn sgr_drag_adds_the_motion_bit() {
        let mode = TermMode {
            mouse_drag: true,
            sgr_mouse: true,
            ..TermMode::NONE
        };
        let bytes = encode_mouse(
            MouseAction::Drag(MouseButton::Left),
            3,
            1,
            Modifiers::NONE,
            mode,
        )
        .unwrap();
        // 0 (left) + 32 (motion): the SGR form carries no offset of its own.
        assert_eq!(String::from_utf8(bytes).unwrap(), "\x1b[<32;4;2M");
    }

    #[test]
    fn modifiers_are_encoded() {
        let shift = encode_mouse(
            MouseAction::Press(MouseButton::Left),
            0,
            0,
            Modifiers::new(true, false, false),
            SGR,
        )
        .unwrap();
        assert_eq!(String::from_utf8(shift).unwrap(), "\x1b[<4;1;1M");
        let all = encode_mouse(
            MouseAction::Press(MouseButton::Left),
            0,
            0,
            Modifiers::new(true, true, true),
            SGR,
        )
        .unwrap();
        assert_eq!(String::from_utf8(all).unwrap(), "\x1b[<28;1;1M");
    }

    #[test]
    fn motion_needs_the_matching_mode() {
        let click_only = TermMode {
            mouse_click: true,
            sgr_mouse: true,
            ..TermMode::NONE
        };
        assert_eq!(
            encode_mouse(MouseAction::Move, 0, 0, Modifiers::NONE, click_only),
            None
        );
        let motion = TermMode {
            mouse_motion: true,
            sgr_mouse: true,
            ..TermMode::NONE
        };
        assert!(
            encode_mouse(MouseAction::Move, 0, 0, Modifiers::NONE, motion).is_some(),
            "1003 reports motion with no button"
        );
    }

    #[test]
    fn legacy_form_offsets_every_byte_by_32() {
        assert_eq!(
            legacy(MouseAction::Press(MouseButton::Left), 0, 0),
            b"\x1b[M\x20\x21\x21"
        );
        assert_eq!(
            legacy(MouseAction::Press(MouseButton::Right), 1, 2),
            b"\x1b[M\x22\x22\x23"
        );
        assert_eq!(legacy(MouseAction::WheelUp, 0, 0), b"\x1b[M\x60\x21\x21");
    }

    #[test]
    fn legacy_release_becomes_button_three() {
        // X10 cannot express a release; 3 is what every terminal sends.
        assert_eq!(
            legacy(MouseAction::Release(MouseButton::Left), 0, 0),
            b"\x1b[M\x23\x21\x21"
        );
    }

    #[test]
    fn legacy_clamps_coordinates_that_would_overrun_a_byte() {
        let bytes = legacy(MouseAction::Press(MouseButton::Left), 300, 300);
        assert_eq!(bytes, b"\x1b[M\x20\xff\xff");
    }

    #[test]
    fn wide_coordinates_survive_in_sgr() {
        assert_eq!(
            sgr(MouseAction::Press(MouseButton::Left), 299, 99),
            "\x1b[<0;300;100M"
        );
    }
}
