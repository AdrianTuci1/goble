//! The warp-new *input-line* model — an input is a terminal command by default,
//! an agent prompt in agent mode, and a `!`-prefixed command forces a shell
//! command from agent mode — is implemented by [`classify_input`]/[`InputClass`].

use goble_ui::event::ModifiersState;

use super::keys::named_key;

// ---------------------------------------------------------------------------
// Input-line classification (terminal command vs. agent prompt)
// ---------------------------------------------------------------------------

/// How a submitted input line should be handled.
///
/// This is the warp-new input model: the bottom prompt line is a terminal
/// command by default, and only sends to the agent on the submit keybinding
/// (Cmd/Ctrl+Enter) or in agent mode (an active agent conversation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputClass {
    /// Run this as a shell command in the pane's PTY session.
    TerminalCommand(String),
    /// Send this to the agent (start/continue the pane's agent conversation).
    AgentPrompt(String),
}

/// Classify a submitted input line.
///
/// A real parser for the warp-new input model, not a stub:
///
/// - In **terminal mode** (`agent_mode == false`) every non-empty input is a
///   terminal command, so `ls` / `git status` run in the pane's shell.
/// - In **agent mode** (`agent_mode == true`, i.e. the pane has an active
///   agent conversation) an input is an agent prompt unless it begins with
///   `!`, in which case it is a terminal command with the leading `!` stripped
///   (so `!foo` runs `foo`).
///
/// An input that is empty or whitespace-only yields `None` (nothing to do).
///
/// The classification is deliberately mode-driven rather than keyword-driven:
/// the only way to run a shell command from agent mode is the explicit `!`
/// prefix, matching the warp-new terminal command parser.
pub fn classify_input(input: &str, agent_mode: bool) -> Option<InputClass> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if agent_mode {
        if let Some(rest) = trimmed.strip_prefix('!') {
            let command = rest.trim();
            if command.is_empty() {
                None
            } else {
                Some(InputClass::TerminalCommand(command.to_string()))
            }
        } else {
            Some(InputClass::AgentPrompt(input.to_string()))
        }
    } else {
        Some(InputClass::TerminalCommand(input.to_string()))
    }
}

/// Apply a forwarded keystroke to the per-pane local input mirror.
///
/// This is the input that `Cmd+Enter` routes to the harness; it mirrors what
/// the user typed at the shell so the agent sees the command line as typed.
/// Only plain typing is mirrored: an escape sequence, an arrow key or a Ctrl
/// combination changes the shell's line in ways this mirror cannot follow, and
/// guessing would put text in the agent's mouth that the user never typed.
pub fn update_input_mirror(mirror: &mut String, key: &str, modifiers: ModifiersState) {
    if key == "Backspace" && !modifiers.command && !modifiers.ctrl && !modifiers.alt {
        mirror.pop();
        return;
    }
    if key.eq_ignore_ascii_case("enter") || key.eq_ignore_ascii_case("return") {
        mirror.clear();
        return;
    }
    if modifiers.command || modifiers.ctrl || modifiers.alt {
        return;
    }
    // A named key is not text, however printable its name is.
    if named_key(key).is_some() {
        return;
    }
    if !key.is_empty() && key.chars().all(|c| !c.is_control()) {
        mirror.push_str(key);
    }
}
