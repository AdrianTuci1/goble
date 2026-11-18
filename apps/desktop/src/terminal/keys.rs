//! Keys are encoded for the program that is running (cursor-key style,
//! bracketed paste, mouse reporting), which is why [`classify_key`] needs the
//! session's [`TermMode`](goble_terminal::TermMode) and not just the keystroke.
//! The harness vs. shell key distinction is captured by [`classify_key`] /
//! [`is_agent_enter`]: plain Enter is forwarded to the pty (so it runs as a
//! terminal command in the shell), while Cmd/Ctrl+Enter is routed through the
//! agent turn path (see `crate::actions`).

use goble_terminal::{encode_mouse, Key, KeyEncoder, Modifiers, MouseAction, TermMode};
use goble_ui::event::ModifiersState;

// ---------------------------------------------------------------------------
// Enter vs Cmd+Enter routing
// ---------------------------------------------------------------------------

/// What the terminal should do with a (key, modifier) pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalKeyAction {
    /// Forward these bytes to the pty (a normal shell keystroke).
    Forward(Vec<u8>),
    /// Cmd/Ctrl+Enter: route the pane's current input to the agent harness and
    /// do NOT write anything to the shell.
    RouteToAgent,
    /// Ignore (e.g. unresolvable or control keys we don't thread through).
    Ignore,
}

/// True when the modifiers are the submit-to-agent keybinding (Cmd or Ctrl, no
/// shift, not alt). This is the warp-new submit-to-local-agent binding: the
/// input is sent to the agent instead of being run as a shell command.
pub fn is_agent_submit(modifiers: ModifiersState) -> bool {
    (modifiers.command || modifiers.ctrl) && !modifiers.shift
}

/// True when the event is Cmd/Ctrl+Enter (command or ctrl, no shift, not alt).
pub fn is_agent_enter(key: &str, modifiers: ModifiersState) -> bool {
    let enter = key.eq_ignore_ascii_case("enter") || key.eq_ignore_ascii_case("return");
    enter && is_agent_submit(modifiers)
}

/// Classify a single key event into a terminal action.
///
/// The bytes for the running program are produced by the emulator crate's key
/// encoder, which knows the modes the program negotiated (application cursor
/// keys, bracketed paste, focus reporting). What is decided here is the policy
/// around it: Cmd/Ctrl+Enter becomes an agent turn instead of a keystroke, and
/// Command+anything is an OS shortcut rather than input to the pty.
///
/// The local input buffer mirror (what `Cmd+Enter` routes to the harness) is
/// maintained by the caller from the forwarded keys; `RouteToAgent` consumes it
/// without touching the pty.
pub fn classify_key(key: &str, modifiers: ModifiersState, mode: TermMode) -> TerminalKeyAction {
    // Cmd/Ctrl+Enter must NOT reach the shell: it is an agent turn instead.
    if is_agent_enter(key, modifiers) {
        return TerminalKeyAction::RouteToAgent;
    }

    // Command is the platform modifier: every other combination with it belongs
    // to a menu item or the system, never to a program in the pane.
    if modifiers.command {
        return TerminalKeyAction::Ignore;
    }

    let modifiers = terminal_modifiers(modifiers);
    if let Some(named) = named_key(key) {
        return TerminalKeyAction::Forward(KeyEncoder::encode(named, modifiers, mode));
    }

    // Text: a printable character, a chunk from an input method, or a paste
    // that arrived as one string. Control characters never travel this way.
    if !key.is_empty() && key.chars().all(|c| !c.is_control()) {
        let mut out = Vec::with_capacity(key.len());
        for c in key.chars() {
            KeyEncoder::encode_into(&mut out, Key::Char(c), modifiers, mode);
        }
        return TerminalKeyAction::Forward(out);
    }

    TerminalKeyAction::Ignore
}

/// The emulator's encoding of the modifiers attached to a key event.
pub fn terminal_modifiers(modifiers: ModifiersState) -> Modifiers {
    Modifiers::new(modifiers.shift, modifiers.alt, modifiers.ctrl)
}

/// The keys that are not plain text, in the string form the platform layer
/// delivers. `None` means the key is text (or is not a key we encode).
pub(super) fn named_key(key: &str) -> Option<Key> {
    let named = match key {
        "Enter" | "Return" => Key::Enter,
        "Escape" => Key::Escape,
        "Backspace" => Key::Backspace,
        "Tab" => Key::Tab,
        "Delete" => Key::Delete,
        "Insert" => Key::Insert,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "ArrowUp" => Key::Up,
        "ArrowDown" => Key::Down,
        "ArrowLeft" => Key::Left,
        "ArrowRight" => Key::Right,
        _ => return key.strip_prefix('F')?.parse().ok().map(Key::Function),
    };
    Some(named)
}

/// The report for a pointer event, in the form the program negotiated: `None`
/// when it is not listening for that kind of report.
///
/// Modifiers are not carried: the platform layer does not put them on mouse
/// events, so a report cannot claim a modifier that was not observed.
pub fn mouse_report(action: MouseAction, cell: (usize, usize), mode: TermMode) -> Option<Vec<u8>> {
    let (row, column) = cell;
    encode_mouse(action, column, row, Modifiers::NONE, mode)
}
