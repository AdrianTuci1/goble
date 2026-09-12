//! UI builder for the native Goble app.
//!
//! Builds the complete element tree from a [`UiSnapshot`] / [`AiSnapshot`]
//! every frame. Lives in the executable (matching warp-new's "thick app"
//! model) so the app owns both the state and the tree; there is no separate
//! hot-reusable dylib or ABI boundary anymore.
//!
//! One module per surface: the pane-tree model ([`pane`]), the plain snapshots
//! the tree is built from ([`snapshot`]), the host callbacks that mutate app
//! state ([`actions`]), the value types the panels render ([`types`]) and the
//! builder itself ([`build`]). Their items are re-exported here, so
//! `crate::ui::X` keeps naming them.

pub mod chat;
pub mod color_picker;
pub mod connectors;
pub mod crons;
pub mod harness;
pub mod media;
pub mod model_form;
pub mod palette;
pub mod panes;
pub mod projects;
pub mod screen;
pub mod settings;
pub mod shell;
pub mod sidebar;
pub mod space_bar;
pub mod terminal;
pub mod vault;

mod actions;
mod build;
mod pane;
mod snapshot;
mod types;

pub use actions::{AiActions, MediaActions, ProjectsActions, ScreenActions, UiActions};
pub use build::{build_ui, SETTINGS_OVERLAY_INSET};
pub use pane::{NavDir, Pane, PaneKind, Space, SplitDir};
pub use snapshot::{
    AiSnapshot, ComposerContext, MediaNode, MediaProject, MediaSession, MediaSnapshot,
    PaneChatSnapshot, ProjectEntry, ProjectSessionEntry, ProjectsSnapshot, ScreenFrameSnapshot,
    ScreenSnapshot, ScreenSourceEntry, SubAgentViewSnapshot, UiSnapshot,
};
pub use types::{
    AppTab, CostEntry, CronEntry, ExecutionEntry, HarnessEntry, HarnessKind, LlmFormField,
    McpSearchEntry, McpServerEntry, SettingsCategory, TaskEntry, TimelineEntry, VaultSecretEntry,
    WorkflowEntry, WorkspaceRouting,
};

/// Width of the left conversation sidebar.
pub const SIDEBAR_WIDTH: f32 = 300.0;

/// Width of the connectors sheet (wider than the default panels).
pub const CONNECTORS_WIDTH: f32 = 480.0;

#[cfg(test)]
mod pane_tests;
#[cfg(test)]
mod settings_category_tests;
