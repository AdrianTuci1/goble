//! App-owned UI state.
//!
//! The element tree is rebuilt from this state on every frame, so state lives
//! here in the executable alongside the UI builder. That keeps text input
//! focus/value across rebuilds.
//!
//! This module owns the data only — the callbacks that mutate it live in
//! [`crate::actions`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use goble_core::agent::Trigger;
use goble_core::harness::{tool_kind_for, ToolCallStatus, ToolKind};
use goble_desktop_service::DesktopState;
use goble_ui::{
    AgentCardUi, AskUserUi, ChatFragment, ChatMessage, ChatRole, CommandProposalUi,
    ConversationEntry, ConversationStatus, ScrollState, SettingsPage, SubAgentRow,
    SubAgentRowStatus, TerminalData, TerminalFilter, TerminalLine, TerminalStatus, ToolCall,
    ToolDisplayMode, TurnActivity, TurnStatus, WorkKind, WorkKindCount,
};
use goble_ui::vim::VimState;

use goble_terminal::blocks::{BlockId, BlockView};

use crate::emulator::VisibleBlock;
use crate::terminal::TerminalRegistry;
use crate::ui::{
    AppTab, CostEntry, CronEntry, ExecutionEntry, HarnessEntry, LlmFormField, Pane,
    PaneChatSnapshot, PaneKind, SettingsCategory, Space, TaskEntry, TimelineEntry, WorkflowEntry,
    WorkspaceRouting, SIDEBAR_WIDTH,
};


mod routing;
mod links;
mod transcript;
mod types;
mod ui_state;
mod paths;

pub(crate) use routing::*;
pub(crate) use links::*;
pub use transcript::*;
pub use types::*;
pub use ui_state::*;
pub use paths::*;

#[cfg(test)]
mod pane_session_tests;
#[cfg(test)]
mod space_reorder_tests;
#[cfg(test)]
mod screen_link_tests;
#[cfg(test)]
mod external_url_tests;
