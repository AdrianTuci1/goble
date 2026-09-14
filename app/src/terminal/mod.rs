//! PTY-backed terminal pane support.
//!
//! One module per surface of the terminal feature: the TUI agent catalogue
//! ([`agents`]), the key and pointer encoding policy ([`keys`]), the input-line
//! model ([`input`]), the shell spawn ([`shell`]), the claim protocol
//! ([`claim`]), the per-pane session ([`session`]) and the pane registry
//! ([`registry`]). The items keep the paths their callers already use: they
//! are re-exported here.

mod agents;
mod claim;
mod input;
mod keys;
mod registry;
mod session;
mod shell;

pub use agents::{TerminalMode, TerminalSnapshot, TuiAgent};
pub use claim::{
    ClaimError, ClaimOutcome, PendingCommandClaim, CLAIM_TIMEOUT, PANE_TOOL_TIMEOUT,
};
pub use input::{classify_input, update_input_mirror, InputClass};
pub use keys::{
    classify_key, is_agent_enter, is_agent_submit, mouse_report, terminal_modifiers,
    TerminalKeyAction,
};
pub use registry::TerminalRegistry;
pub use session::{TerminalSession, TerminalViewState};
pub use shell::{default_shell, resolve_cwd};

#[cfg(test)]
mod tests;
