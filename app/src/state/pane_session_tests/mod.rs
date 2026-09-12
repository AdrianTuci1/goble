//! Per-pane session behaviour: what each pane's own conversation, runtime and
//! controls do, one module per surface.
//!
//! The cases are grouped the way the state they exercise is: [`binding`]
//! (pane <-> conversation), [`live_events`] (tool/sub-agent/turn events),
//! [`transcript`] (the rows a refresh builds), [`sessions`] (active-pane
//! fallbacks and rebinding), [`proposals`] (a suspended command) and
//! [`snapshots`] (the per-pane chat snapshot and controls).

use super::*;
use crate::emulator::Emulator;
use crate::terminal::TerminalSession;

mod binding;
mod live_events;
mod proposals;
mod sessions;
mod snapshots;
mod transcript;
