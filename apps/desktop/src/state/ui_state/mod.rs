//! One pane-tree state machine: the `UiState` value and its inherent methods.
//!
//! The struct's fields are declared here; its methods live one module per
//! concern ([`refresh`], [`pane_move`], [`panes`], [`agents`], [`session`],
//! [`settings`], [`close`], [`mock`], [`spaces`], [`worker`]) as separate
//! `impl UiState` blocks, which are the same block to the compiler as one.
//! [`title`] is not one of them: the subject a conversation is named with is
//! derived from its first prompt (the model-backed title request included), not
//! held by the state machine.

use super::*;

use goble_ui::elements::conversation_list_item::SidebarGroupUi;
use goble_ui::elements::LaneEdge;

use goble_core::hosts::HostEntry;

use crate::ui::settings::SettingsUiState;

mod agents;
mod close;
mod file_menu;
mod inbox;
mod mock;
mod onboarding;
mod pane_move;
mod panes;
mod refresh;
mod responsive;
mod reviewer;
mod session;
mod session_menu;
mod settings;
mod spaces;
mod title;
mod work_dock;
mod worker;
mod workspace_rows;

pub use agents::{PaneAgentState, TurnOutcome};
pub use file_menu::{FileMenu, FileRevertTarget};
pub use inbox::{InboxItem, InboxKind};
pub use onboarding::{ONBOARDING_STEPS, ONBOARDING_USAGE};
pub use pane_move::{PaneDrag, PaneDrop, PaneMove};
pub use responsive::{
    pane_slot_min_width, Responsive, PANE_MIN_WIDTH, RESPONSIVE_HYSTERESIS,
    SECONDARY_PANEL_BREAKPOINT, SIDEBAR_AUTO_COLLAPSE,
};
pub use reviewer::{review_for, DiffReviewState, DiffViewMode};
pub use session_menu::{ArchiveConfirm, SidebarRename, ToastKind, ToastNotice};
pub use title::{deterministic_subject, generate_and_store_title, spawn_title_generation};
pub use workspace_rows::{SidebarWorkspace, SidebarWorkspaceRename};

/// The title a conversation gets when it is created without a subject of its
/// own. It is a placeholder, not a subject: a tab whose agent holds such a
/// conversation still reads [`NEW_AGENT_TAB_LABEL`], and the conversation's
/// first prompt replaces it with a subject ([`title`]) — the model-generated
/// title when a provider answers, the deterministic topic phrase otherwise.
pub const NEW_CONVERSATION_TITLE: &str = "New conversation";

/// The label a tab whose pane holds an agent draws while that agent's
/// conversation has no subject yet. Once the conversation has one, the tab
/// reads the subject instead (see [`UiState::space_label`]).
pub const NEW_AGENT_TAB_LABEL: &str = "New Agent";

/// The reserved pane id the side panel's Agent tab owns.
///
/// The side panel is a second surface beside the pane tree, but its Agent tab
/// is a real conversation agent: it has its own store conversation (created on
/// its first prompt), its own draft, transcript, model and turn events. A
/// reserved id outside the pane tree gives it exactly the same per-pane
/// machinery ([`UiState::pane_sessions`], `pane_runtime`, `pane_chat`) without
/// appearing as a leaf, so the active pane's chat is never reused.
pub const SIDE_AGENT_PANE_ID: u64 = u64::MAX - 1;
/// The reserved pane id the right panel's Files tab reads a file under, so a
/// file opens in the panel without touching the main pane tree.
pub const SIDE_FILE_PANE_ID: u64 = u64::MAX - 2;

/// The subject a fresh side-panel agent conversation starts under. It is a
/// placeholder like [`NEW_CONVERSATION_TITLE`], replaced by the first prompt's
/// subject through the same title path a pane's conversation uses.
pub const SIDE_AGENT_TITLE: &str = "New side question";

/// The name a fresh cloud agent draws while its conversation has no subject
/// yet: a viewer pane starts on the worker, and the surface that opens for it
/// says what it is about to do.
pub const NEW_CLOUD_AGENT_TITLE: &str = "New cloud agent";

/// What the composer's model control reads while no model is configured: the
/// pane's own model value stays empty — [`UiState::pane_agent_model`] reports
/// nothing chosen, which is what [`UiState::can_run_agent_turn`] decides on —
/// and only the label the view draws says what is missing.
///
/// The element that draws the label owns the words, and this is that very
/// constant re-exported, so the two cannot come to say different things.
pub use goble_ui::elements::chat_composer::MODEL_NOT_CONFIGURED;

/// The window two presses on the same tab are one gesture in, in milliseconds —
/// a double click. warp-new's own value and its own name
/// (`crates/octomusui/src/windowing/winit/event_loop/mod.rs::MULTI_CLICK_INTERVAL`).
pub const MULTI_CLICK_INTERVAL_MS: u128 = 400;

/// A run of presses on one workspace tab, counted the way warp-new counts a
/// multi-click: on the press, against the *previous press* rather than against
/// any release between them.
#[derive(Clone, Copy, Debug)]
pub struct SpacePressRun {
    pub index: usize,
    pub pressed_at: std::time::Instant,
    pub count: u32,
}

/// An open workspace tab menu: the tab it belongs to and the point the right
/// click landed on, which is where the panel hangs from.
#[derive(Clone, Copy, Debug)]
pub struct SpaceMenu {
    pub index: usize,
    pub at: goble_ui::Vector2F,
}

/// One tab in the right side panel. The panel opens on [`Overview`]; the "+"
/// menu adds the other four, and each draws the active pane's own surface.
///
/// [`Overview`]: Self::Overview
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideTabKind {
    /// The original panel: search, the pane's real actions, its sub-agents and
    /// its background work.
    Overview,
    /// The active pane's agent chat, with its transcript and composer.
    Agent,
    /// The file the active pane shows, when that pane is a file view.
    Files,
    /// The active pane's live shell, in the panel instead of a pane leaf.
    Terminal,
    /// The active pane's live work items and their descriptions.
    TaskOutput,
    /// The active pane's working-tree review: the changed-file rail and the
    /// diff, inside the panel like Terminal and Side Question (Q).
    Review,
    /// A freshly opened, not-yet-chosen tab. It shows Overview's launcher and
    /// is replaced by the first kind chosen from it.
    NewTab,
}

impl SideTabKind {
    /// The title the tab chip and the panel header draw.
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Agent => "Side Question",
            Self::Files => "Files",
            Self::Terminal => "Terminal",
            Self::TaskOutput => "Task output",
            Self::Review => "Review",
            Self::NewTab => "New tab",
        }
    }

    /// The tab chip's glyph.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Overview => "message-plus-square",
            Self::Agent => "message-plus-square",
            Self::Files => "file",
            Self::Terminal => "terminal",
            Self::TaskOutput => "terminal-input",
            Self::Review => "file",
            Self::NewTab => "plus",
        }
    }
}

/// One interaction in the right side panel's tab strip. The strip is rebuilt
/// every frame, so every callback sends a value here and the app state machine
/// answers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideTabAction {
    /// Make the tab at this index the panel's surface.
    Select(usize),
    /// Close the tab at this index, selecting a neighbour when it was active.
    Close(usize),
    /// Add this kind (or show it when it is already a tab).
    Add(SideTabKind),
    /// Move by this many tabs, wrapping at both ends. A horizontal swipe sends
    /// `-1` (next) or `1` (previous).
    Cycle(i32),
    /// Show or hide the inner Terminals/Background Tasks rail.
    ToggleRail,
    /// Move the tab at the first index to the second index.
    Reorder(usize, usize),
}

#[derive(Clone)]
pub struct UiState {
    pub current_tab: AppTab,
    pub conversations: Vec<ConversationEntry>,
    pub selected_id: Option<String>,
    pub search_query: String,
    pub search_focused: bool,
    /// The sidebar search field's insertion point. The element tree is rebuilt
    /// every frame, so the caret lives here; it is reset to the end whenever
    /// the app writes a query the field did not type itself.
    pub sidebar_search_caret: Rc<RefCell<usize>>,
    pub new_conversation_draft: String,
    pub create_focused: bool,
    pub chat_messages: Vec<ChatMessage>,
    /// A suspended agent question, rendered inline at the end of the transcript.
    /// Set from the `chat:ask_user` event and re-read from the store so it
    /// survives a refresh or app restart.
    pub pending_ask: Option<AskUserUi>,
    /// A prompt sent while the agent was busy, queued so it does not interrupt
    /// the running turn. Shown as a pending block with "Send now" / dismiss.
    pub queued_prompt: Option<String>,
    pub composer_draft: String,
    pub composer_focused: bool,
    /// AF: the Side Question composer's own focus, independent of the active
    /// pane's composer.
    pub side_composer_focused: bool,
    /// Model choices shown in the composer's model dropdown.
    pub models: Vec<String>,
    /// The same models grouped the way the model tray lists them: one column
    /// per provider the configuration declares models for, each with the
    /// thinking levels its harness runs them at. Read from the desktop's
    /// catalog, so the tray shows what a turn can actually run.
    pub model_groups: Vec<goble_desktop_service::ModelGroup>,
    /// T3-style integration snapshots (Antigravity and the CLI integrations):
    /// detection, auth, models and config path, loaded once per settings/model
    /// refresh rather than per frame.
    pub integrations: Vec<goble_core::integration::IntegrationSnapshot>,
    /// The harness's own composer catalog: the commands and skills a draft can
    /// list, and the models it runs with their thinking levels. Read from the
    /// desktop, so the composer lists what the harness advertises instead of a
    /// table kept here.
    pub composer_catalog: goble_harness_types::HarnessCatalog,
    /// Currently selected model (shown as the composer's model label).
    pub selected_model: String,
    /// Harnesses the composer can route a turn to (native only in this build).
    pub harnesses: Vec<HarnessEntry>,
    /// The harness the active pane routes turns to (default `internal`).
    pub selected_harness: String,
    /// App-owned open flag for the composer model menu.
    pub model_menu_open: Rc<RefCell<bool>>,
    /// App-owned open flags for the composer harness / dir / branch menus.
    pub harness_menu_open: Rc<RefCell<bool>>,
    pub dir_menu_open: Rc<RefCell<bool>>,
    /// The shell-bar composer's directory tray scroll offset, app-owned beside
    /// its open flag so a deep directory keeps the position the wheel left it.
    pub dir_menu_scroll: PanelScroll,
    pub branch_menu_open: Rc<RefCell<bool>>,
    /// The git branch of the active pane's working directory (best-effort,
    /// read from `.git/HEAD`). Shown as the composer's branch pill when set.
    pub composer_branch: String,
    pub agent_name: String,
    pub agent_busy: bool,
    /// Whether the agent auto-approves `ask_user` questions (skip the ask).
    pub auto_approve: bool,
    /// The app-wide command-approval policy. A change here is written to the
    /// store and copied into every pane, so the composer, the proposal path and
    /// a restart all read the same mode.
    pub approval_mode: ApprovalMode,
    /// Whether the first-run onboarding overlay is up.
    pub onboarding_open: bool,
    /// Which of the three onboarding steps is up: usage, name, telemetry.
    pub onboarding_step: usize,
    /// The usage card the onboarding flow has selected (`work`, `personal`,
    /// `learning`), empty until one is picked.
    pub onboarding_usage: String,
    /// The friendly name being typed in onboarding, before it is committed.
    pub onboarding_name: String,
    /// Whether the onboarding consent step is set to share. Applied only when
    /// the flow finishes.
    pub onboarding_share: bool,
    /// The telemetry consent in force, as the profile page draws it.
    pub share_telemetry: bool,
    /// The default model the profile page shows and changes.
    pub settings_default_model: String,
    pub right_sidebar_open: bool,
    /// Whether the agent/window is currently fullscreen (borderless). Lives in
    /// app state so it survives the per-frame element rebuild.
    pub fullscreen: bool,
    /// The sub-agent child views open in a pane, keyed by pane id: the child's
    /// own conversation shown in the pane, entered from the parent transcript's
    /// row ([`UiState::open_sub_agent`]) and left again with Esc
    /// ([`UiState::close_sub_agent_view`]).
    pub sub_agent_views: HashMap<u64, SubAgentChildView>,
    /// Whether each group of an agent pane's work dock is open, keyed by
    /// [`goble_ui::WorkSection::key`]. An absent key is an open group, so a
    /// section the user never folded lists its rows. Shared with the UI so a
    /// fold survives the per-frame element rebuild.
    pub work_dock_open: Rc<RefCell<HashMap<String, bool>>>,
    /// Whether each group of the work dock has revealed the rows behind
    /// `show N more`, keyed the same way.
    pub work_dock_show_all: Rc<RefCell<HashMap<String, bool>>>,
    /// The pane drawn over the whole panes space, if any. Lives in app state so
    /// it survives the per-frame element rebuild; `None` is the tree's own
    /// layout. A pane id from another space simply does not match, so the
    /// expansion is per-space without being stored per-space.
    pub maximized_pane: Option<u64>,
    /// Per-terminal-block filter state (open flag + selected filter), keyed by
    /// the block's content key. Shared with the UI so the filter tray's open
    /// state + selection survive the per-frame element rebuild.
    pub terminal_filters: Rc<RefCell<HashMap<String, TerminalFilter>>>,
    /// Per-reasoning-row collapsed/expanded state, keyed by the row's
    /// `<conversation>:<step>` key. Shared with the UI so a row the user
    /// expanded stays expanded across the per-frame element rebuild.
    pub reasoning_expanded: Rc<RefCell<HashMap<String, bool>>>,
    /// Per-tool-call three-state fold (`Collapsed -> Truncated -> Expanded`),
    /// keyed by the call's id. Shared with the UI next to `reasoning_expanded`
    /// so a call the user folded stays folded across the per-frame rebuild.
    pub tool_fold: Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    /// The tool row (a chain's header, keyed by its group key) the pointer is
    /// over, reported by the row's own paint. The next frame reads it to draw
    /// the row's arrow and its white label without a hover band behind it.
    pub tool_hover: Rc<RefCell<Option<String>>>,
    /// Whole-transcript terminal filter (open flag + selected filter) per pane,
    /// so the filter bar of one pty/agent pane does not open in its sibling.
    pub terminal_global_filters: HashMap<u64, TerminalFilter>,
    pub crons_open: bool,
    /// The workflow-runs views open in a pane, keyed by pane id: the ledger's
    /// runs shown in the pane itself, entered from the work dock's workflow row
    /// or the workflow toggle ([`UiState::open_workflow_view`]) and left with
    /// Esc ([`UiState::close_workflow_view`]).
    pub workflow_views: HashMap<u64, WorkflowView>,
    /// Whether the keyboard shortcuts panel is up.
    pub shortcuts_help_open: bool,
    /// The shortcuts panel's filter text, and the row it highlights — an index
    /// into the rows the filter leaves. Both survive the per-frame tree rebuild
    /// and both reset when the panel is opened.
    pub shortcuts_help_filter: String,
    pub shortcuts_help_index: usize,
    /// Scroll offset of the shortcuts panel's list, so a filtered list keeps
    /// the place the keyboard scrolled it to across the per-frame rebuild.
    pub shortcuts_help_scroll: Rc<RefCell<ScrollState>>,
    pub crons: Vec<CronEntry>,
    /// Harness workflows (real daemon workflow store).
    pub workflows: Vec<WorkflowEntry>,
    /// Workflow runs, newest first (real `workflow_runs` ledger): live runs as
    /// the service records them at start, and settled ones.
    pub workflow_runs: Vec<WorkflowRunEntry>,
    /// Executions from the daemon execution ledger (real data).
    pub executions: Vec<ExecutionEntry>,
    /// Durable tasks from the persistence layer (real data).
    pub tasks: Vec<TaskEntry>,
    /// Chronological records derived from execution/session/task data.
    pub timeline: Vec<TimelineEntry>,
    /// Cost rows derived from real execution/usage records.
    pub costs: Vec<CostEntry>,
    pub settings_profile_name: String,
    pub settings_dark_mode: bool,
    pub settings_llm_provider: String,
    pub settings_llm_model: String,
    pub settings_llm_api_key: String,
    pub settings_llm_base_url: String,
    pub settings_llm_temperature: String,
    pub settings_workers: Vec<(String, String, String, bool)>,
    /// The settings tab's own state: the file projection, the keyboard's
    /// position, the field drafts and the dialogs. One field, not a field per
    /// row; the pages live in `ui::settings`.
    pub settings: SettingsUiState,
    /// The update checker, when this build knows a release host. `None` keeps
    /// the Updates section written but inert: a build with nowhere to check
    /// opens no sockets and offers no install.
    pub update: Option<std::sync::Arc<crate::update::UpdateService>>,
    /// What that checker last published, refreshed from it on every rebuild so
    /// the page reads one frame behind the worker thread.
    pub update_view: crate::update::UpdateView,
    /// Whether the rich input uses modal (vim) editing. Persisted in the store
    /// as `vim_mode`; each pane keeps its own mode/registers in
    /// [`PaneControls::vim`].
    pub vim_mode: bool,
    /// Whether closing a file pane with unsaved edits saves the buffer first.
    /// Persisted as `auto_save_on_close`; off asks the user at the close.
    pub auto_save_on_close: bool,
    /// The pane a close is waiting on: a file view whose buffer is dirty and
    /// the user has not answered "save?" yet. The close dialog reads this.
    pub pending_close_pane: Option<u64>,
    /// The one file chip a close is waiting on, as `(pane, path)`. Closing a
    /// chip asks only about that file, so the pane and its other files stay.
    pub pending_close_file: Option<(u64, String)>,
    /// The tokens each conversation has spent, keyed by conversation id, summed
    /// from the provider-reported usage of its own model calls (`chat:usage`).
    /// Accounting only; nothing here is a price.
    pub conversation_usage: HashMap<String, goble_ui::TokenUsage>,
    /// The most recent model call's reported usage per conversation, and how
    /// many calls have reported one. The usage dialog shows the last call
    /// beside the session total; the host reports per call, not per turn.
    pub conversation_last_usage: HashMap<String, goble_ui::TokenUsage>,
    pub conversation_usage_calls: HashMap<String, u64>,
    /// Custom theme color overrides as `#rrggbb` hex, set via the color wheel
    /// pickers in Settings→Appearance. `None` uses the theme's built-in color.
    pub theme_primary: Option<String>,
    pub theme_secondary: Option<String>,
    pub theme_accent: Option<String>,
    pub theme_bg: Option<String>,
    pub theme_surface: Option<String>,
    /// The preset palette in force (`ThemePreset::key`), or `None` for the
    /// Default base the dark-mode switch styles.
    pub theme_preset: Option<String>,
    /// Which theme channel the single color wheel in Settings→Appearance edits.
    pub theme_color_target: crate::ui::color_picker::ColorTarget,
    /// Active color-wheel drag (which region), owned here so the per-frame
    /// element rebuild keeps the drag alive.
    pub theme_color_drag: Rc<RefCell<Option<crate::ui::color_picker::ColorPickerDrag>>>,
    /// First-run: whether the "configure a model key" banner is shown in chat.
    pub show_llm_key_banner: bool,
    /// First-run: whether the "local or remote workspace?" choice is shown.
    pub show_workspace_choice: bool,
    /// First-run routing decision once the user picks Local or Remote.
    pub workspace_routing: Option<WorkspaceRouting>,
    /// Settings → Agent & Approval: the `[remote_agents.<name>]` tables read
    /// from the config, refreshed at startup and by the page's reload row.
    pub remote_agents: Vec<RemoteAgentEntry>,
    /// `[remote] mode`: a sandbox platform, or a gateway server.
    pub remote_mode: goble_core::config::RemoteMode,
    /// `[remote] gateway_url`, as configured.
    pub remote_gateway_url: String,
    /// `[remote] worker_token`, as configured. Stored beside the URL so the
    /// form on the page can show what is already there.
    pub remote_gateway_token: String,
    /// Settings → Agent & Approval: the `config.toml` that holds `[remote]`
    /// and `[remote_agents.*]`, named on the page so the configuration has a
    /// visible home.
    pub remote_config_path: String,
    /// What the last cloud-agent deploy reported: the row above says a worker
    /// is being shipped, or names the sandbox it landed in, or why it stopped.
    pub remote_deploy: RemoteDeploy,
    /// First-run: whether the model-provider dialog is open over the chat.
    pub llm_dialog_open: bool,
    /// Editable model-provider form values + focus, held here so text/focus
    /// survive the per-frame element rebuild while the dialog is open.
    pub llm_dialog_provider: Rc<RefCell<String>>,
    pub llm_dialog_model: Rc<RefCell<String>>,
    pub llm_dialog_api_key: Rc<RefCell<String>>,
    pub llm_dialog_base_url: Rc<RefCell<String>>,
    pub llm_dialog_temperature: Rc<RefCell<String>>,
    pub llm_dialog_focus: Rc<RefCell<Option<LlmFormField>>>,
    pub sidebar_width: f32,
    pub sidebar_dragging: bool,
    pub sidebar_drag_origin_x: f32,
    pub sidebar_drag_start_width: f32,
    /// The right side panel's share of the content width while it is open and
    /// unmaximized: the panel gets `side_split_ratio` of the width the left
    /// sidebar leaves, the main card gets the rest. `0.5` is the equal split.
    ///
    /// Persisted with the pane layout (`PersistedPaneState::side_split_ratio`,
    /// clamped to `0.25..=0.75` on restore), so the split the user dragged
    /// survives a restart. The value lives in app state (not the frame) so a
    /// drag survives the per-frame element rebuild, and it is restored across
    /// per-frame rebuilds and pane/space switches in one run.
    pub side_split_ratio: f32,
    /// Whether the shared main/side-panel boundary is being dragged. Carried in
    /// app state so the per-frame element rebuild keeps the drag alive.
    pub side_split_dragging: bool,
    /// The user's committed content width, per pane: the pane whose invisible
    /// content-lane edge they dragged lays its content out at that width
    /// (clamped into the pane by the lane's own resolver every frame). A pane
    /// with no entry keeps the lane's aspect-ratio width (64% of the pane's own
    /// width, clamped). Entries are pruned with the pane (`forget_pane`).
    pub content_lane_prefs: HashMap<u64, f32>,
    /// Which pane owns the live content-lane drag, if one is in flight. Only
    /// that pane's lane draws the accent handle line and takes the captured
    /// moves; the move/end handlers write only its preference, so dragging one
    /// pane never resizes its siblings.
    pub content_lane_drag_pane: Option<u64>,
    /// Whether the content lane's invisible edge is being dragged. Carried in
    /// app state so the per-frame element rebuild keeps the drag alive.
    pub content_lane_dragging: bool,
    /// The edge being dragged, so the lane draws its accent handle line there.
    pub content_lane_drag_edge: Option<LaneEdge>,
    /// Where that drag's pointer press landed, the content width and the
    /// available column it started from: the move handler widens the lane by
    /// twice the pointer's travel from here, so a press on the strip does not
    /// jump the width. The drag is symmetric about the centre, so the edge's
    /// direction is the pointer-delta sign.
    pub content_lane_drag_origin_x: f32,
    pub content_lane_drag_start_width: f32,
    pub content_lane_drag_column: f32,
    /// The preference the drag started from, and whether the pointer actually
    /// moved: a click that grabs a handle without dragging must leave the
    /// width axis exactly as it was instead of committing a preference.
    pub content_lane_drag_start_pref: Option<f32>,
    pub content_lane_drag_moved: bool,
    /// Whether the left conversation sidebar is shown. Toggled by the topbar's
    /// sidebar button; the main area fills the window when it is hidden.
    pub sidebar_visible: bool,
    /// The responsive layout's decisions for the frame being built (see
    /// [`responsive`]): the sidebar auto-collapse, the secondary panels leaving
    /// the view, and the focused-pane fallback. Recomputed every frame from the
    /// measured window width before the snapshot is taken.
    pub responsive: Responsive,
    /// The narrow side of the sidebar auto-collapse breakpoint on the last
    /// frame: the hysteresis memory, and what detects the crossing that drops a
    /// manual re-expand override.
    pub responsive_narrow: bool,
    /// The user re-expanded the auto-collapsed sidebar below the breakpoint
    /// (DeepSeek Harness's `narrowExpanded`): the preference is untouched, so
    /// growing the window restores the pre-squeeze layout. Cleared at the next
    /// breakpoint crossing.
    pub sidebar_narrow_expanded: bool,
    /// The window width the responsive decisions were last computed from, so a
    /// manual sidebar toggle inside a breakpoint can re-solve them immediately
    /// (the next frame re-measures anyway).
    pub responsive_width: f32,
    /// Whether the sidebar's conversation list shows every conversation. While
    /// false only a few cards are drawn, followed by a "View all" button.
    pub conversations_expanded: bool,
    /// Which of the sidebar's views is showing: the conversations list, the
    /// project explorer or global search.
    pub sidebar_view: SidebarView,
    /// Conversations the user starred, by id. They are collected at the top of
    /// the sidebar and marked on their own cards.
    pub starred_conversations: HashSet<String>,
    /// The sidebar sections the user collapsed, by key: `starred`, or
    /// `folder:<name>` for one of the conversation folders.
    pub collapsed_sections: HashSet<String>,
    /// The directories the project explorer has open, by absolute path.
    pub explorer_expanded: HashSet<String>,
    /// The explorer row under the pointer, by path, shared with the tree so its
    /// hover-dependent colour survives the per-frame element rebuild — hover
    /// itself is read at paint time, so the row styles one frame after the
    /// pointer reaches it.
    pub explorer_hover: Rc<RefCell<Option<String>>>,
    /// The Workspaces row under the pointer on the last frame, by key:
    /// `header:settings`, `header:new`, `action:new-conversation`,
    /// `group:<folder>` or `session:<id>`. The rows swap their trailing cluster
    /// on hover, which is a *layout* change, so hover must be known while the
    /// next frame is built — the same phase-lag contract as `explorer_hover`.
    pub sidebar_hover_row: Rc<RefCell<Option<String>>>,
    /// The Workspaces row the sidebar's keyboard focus is on, by key, or
    /// `None` when the sidebar does not hold the keyboard.
    pub sidebar_focus_row: Rc<RefCell<Option<String>>>,
    /// Whether the Workspaces header's view-options menu is open (the sliders
    /// button). Shared with the popup so it survives the rebuild.
    pub sidebar_menu_open: Rc<RefCell<bool>>,
    /// The Workspaces list's grouping choice: one flat list (`true`) or one
    /// block per project/folder (`false`, the default).
    pub sidebar_flat_list: Rc<RefCell<bool>>,
    /// The Workspaces list's order: most recent first (the default) or the
    /// user's manual order, which drag reordering writes.
    pub sidebar_recent_first: Rc<RefCell<bool>>,
    /// The Workspaces list's archived filter (hide / show / only archived
    /// rows), owned here because the view-options popup's selection must
    /// survive the per-frame rebuild.
    pub sidebar_archived_filter: Rc<RefCell<crate::ui::ArchivedFilter>>,
    /// The open "Rename conversation" dialog, if any.
    pub sidebar_rename: Option<SidebarRename>,
    /// The sidebar's durable Workspaces rows: the directories the user added
    /// with the Workspaces header's "+" (DSH's workspace list). A row that has
    /// no conversations yet is listed from here; the folder groups the sessions
    /// imply are matched to these rows by the directory's last segment.
    pub sidebar_workspaces: Vec<SidebarWorkspace>,
    /// The open "Rename workspace" dialog, if any.
    pub sidebar_workspace_rename: Option<SidebarWorkspaceRename>,
    /// Per-workspace-group tray state, shared with the rows so an open menu
    /// survives the per-frame element rebuild (the session rows' `agent_cards`
    /// is the same contract).
    pub sidebar_group_ui: Rc<RefCell<HashMap<String, Rc<RefCell<SidebarGroupUi>>>>>,
    /// The open stop-and-archive confirmation, if any.
    pub archive_confirm: Option<ArchiveConfirm>,
    /// The transient notice at the top of the window, if one is up.
    pub toast: Option<ToastNotice>,
    /// When the notice above stops being drawn on its own.
    pub toast_until: Option<std::time::Instant>,
    /// Monotonic id source for the toasts, so the surface can tell one from the
    /// next.
    pub next_toast_seq: u64,
    /// The sidebar's drag-reorder gesture: the press, whether it became a drag
    /// and which row currently carries the insert marker.
    pub sidebar_drag: Rc<RefCell<goble_ui::elements::conversation_list_item::SidebarDrag>>,
    /// The user's manual order of conversations inside each group (group label
    /// to conversation ids). Session-scoped: reordering is not persisted.
    pub sidebar_session_order: Rc<RefCell<std::collections::HashMap<String, Vec<String>>>>,
    /// Global search: the query, the rows it produced and whether it has run at
    /// all. The search is run when the query changes, never per frame.
    pub global_search_query: String,
    pub global_search_rows: Vec<SearchRow>,
    pub global_search_searched: bool,
    /// Why the search has nothing to show, when that is not simply an empty
    /// result: no ripgrep on this machine, or a query ripgrep refused.
    pub global_search_error: Option<String>,
    pub global_search_focused: bool,
    /// The global search field's insertion point, held the way the sidebar
    /// search's is.
    pub global_search_caret: Rc<RefCell<usize>>,
    /// The walk behind the query, on a thread of its own: reading every file
    /// under the root is far too slow to run between two keystrokes. The field
    /// hands it the query and the frame collects the answer.
    pub global_search_worker: Rc<SearchWorker>,
    /// Scroll offset of the sidebar's conversation list (owned here so it
    /// survives the per-frame element rebuild).
    pub sidebar_scroll: Rc<RefCell<ScrollState>>,
    /// Scroll offsets of the project explorer's and the global search's lists.
    pub explorer_scroll: Rc<RefCell<ScrollState>>,
    pub global_search_scroll: Rc<RefCell<ScrollState>>,
    /// Q: the centered spotlight window opened by the topbar Search.
    pub spotlight: crate::ui::spotlight::SpotlightState,
    /// Whether the topbar workspace frame is in inline-rename mode (entered by
    /// double-clicking the tab, or from its menu's "Rename tab" row).
    pub space_rename_editing: bool,
    /// Whether the platform's chord modifier — Command on macOS, Ctrl elsewhere
    /// — is held down. While it is, every workspace tab draws its own shortcut
    /// number in its trailing slot (see `WorkspaceChip`). Not persisted: it
    /// describes the keyboard this instant, and a restart has no key down.
    pub command_held: bool,
    /// The run of presses on a workspace tab that the strip is counting: which
    /// tab, when it was pressed, and how many presses in a row this is.
    ///
    /// warp-new's own rule (`crates/octomusui/src/windowing/winit/event_loop/
    /// mod.rs::determine_click_count_and_update_button_state`): the count is
    /// read on the *press* and acted on at the release, so the time a press is
    /// held does not come out of the window between two presses. App-owned
    /// rather than a local of the per-frame actions: the element tree — and the
    /// callbacks in it — is rebuilt every frame, so a cell made with them would
    /// forget the first press before the second one arrives and the rename
    /// could never open.
    pub space_press_run: Rc<RefCell<Option<SpacePressRun>>>,
    /// The in-progress space name while renaming.
    pub space_rename_draft: String,
    /// Whether the rename field holds focus.
    pub space_rename_focused: bool,
    /// The workspace tab whose right-click menu is open, and the point the
    /// pointer was at when it opened — warp-new hangs that menu from the
    /// pointer itself (`TabContextMenuAnchor::Pointer`). `None` when no menu is
    /// open; the menu element is built for the frame this names, and a press
    /// outside its panel clears it (`on_space_menu_close`), because only the app
    /// outlives the frame that press happened in.
    pub space_menu: Rc<RefCell<Option<SpaceMenu>>>,
    /// The file row/card context menu that is open, if any: the path it acts
    /// on and the point it hangs from. App-owned because the tree is rebuilt
    /// every frame.
    pub file_menu: Rc<RefCell<Option<FileMenu>>>,
    /// Per-card interaction state (hover / delete menu), owned here so it
    /// survives the per-frame element rebuild. Keyed by conversation id.
    pub agent_cards: HashMap<String, Rc<RefCell<AgentCardUi>>>,
    /// Hover flag for the sidebar's "New conversation" row, owned here so the row
    /// highlight survives the per-frame element rebuild.
    pub new_agent_hover: Rc<RefCell<bool>>,
    /// Multiple "spaces" (warp-new style): each is a pane tree shown as a tab
    /// in the top bar. `active_space` selects the one currently rendered.
    pub spaces: Vec<Space>,
    pub active_space: usize,
    /// Hovered space tab index (app state so the highlight survives the
    /// per-frame element rebuild).
    pub space_hover: Option<usize>,
    /// Space tab currently pressed (MouseDown), used to detect a drag start.
    pub space_press: Option<usize>,
    /// Where that press landed, in window coordinates: the drag starts only
    /// once the pointer has travelled away from it, so a click that wobbles a
    /// pixel still selects.
    pub space_press_at: Option<goble_ui::Vector2F>,
    /// Space tab currently being dragged (reordering in progress).
    pub space_drag: Option<usize>,
    /// The pane lifted out of its grid toward the topbar's tab strip (dropped
    /// there, it becomes a tab of its own). Owned here because the element tree
    /// is rebuilt every frame, so the drag has to survive the rebuild.
    pub pane_drag: Option<PaneDrag>,
    /// The bounds each pane leaf was drawn at on the last frame, in window
    /// coordinates, keyed by pane id. Written while the panes paint; read by
    /// the drag handlers, which have to know what pane the pointer is over and
    /// where the drop ghost belongs. A pane the last frame did not draw has no
    /// entry, so it cannot be a drop target.
    pub pane_bounds: Rc<RefCell<HashMap<u64, goble_ui::geometry::RectF>>>,
    /// Monotonic id counter so new panes never collision with existing ones.
    pub next_pane_id: u64,
    /// The leaf pane currently focused (target for splits/closes, highlighted).
    pub active_pane_id: u64,
    /// The split node currently being dragged, if any.
    pub dragging_pane_id: Option<u64>,
    /// The current working path shown in the composer.
    pub composer_path: String,
    /// Per-pane hover flags (keyed by pane id) so pane-header hover survives
    /// the per-frame element rebuild.
    pub pane_hover: HashMap<u64, Rc<RefCell<bool>>>,
    /// Per-pane conversation identity + composer draft + cwd. Keyed by pane id;
    /// every chat leaf pane has an entry so it is an independent session.
    pub pane_sessions: HashMap<u64, PaneSession>,
    /// Runtime per-pane transcript/ask/queued/busy state (not persisted).
    pub pane_runtime: HashMap<u64, PaneRuntime>,
    /// Per-pane viewer sessions, keyed by pane id: which worker the pane's
    /// conversation runs on and how that connection stands. An entry exists for
    /// a viewer pane (a [`crate::ui::PaneKind::Worker`] leaf) and for no other
    /// kind — a pane that leaves viewer shape drops its entry, because it no
    /// longer has a connection to report. Not persisted: which worker a routing
    /// resolves to is the worker pool's answer, re-resolved when the pane adopts
    /// the routing again.
    pub pane_workers: HashMap<u64, WorkerSession>,
    /// Per-pane SSH bindings, keyed by pane id and persisted with the layout.
    /// A saved-host pane keeps its host name here so its session chip and the
    /// reconnect offer survive a restart; a pane the user moved with a typed
    /// `ssh` line keeps the resolved session.
    pub pane_ssh: HashMap<u64, SshPaneBinding>,
    /// The sidebar chat a pane's CLI harness conversation was registered under,
    /// keyed by pane id and harness id: a harness launched in a pane opens one
    /// conversation in the sidebar, and launching the same harness there again
    /// returns to that conversation instead of stacking a duplicate.
    pub harness_chats: HashMap<(u64, String), String>,
    /// What the harness's own session store answered when a conversation still
    /// named for its harness was looked up, keyed by conversation id: the title
    /// it offered, or the empty string when it offered none. The store is the
    /// filesystem, so a conversation is looked at once per run rather than on
    /// every sidebar rebuild; see [`UiState::refresh_conversations`].
    pub harness_title_attempts: HashMap<String, String>,
    /// The harness conversations whose card was named from the session the
    /// conversation owns, by conversation id. A conversation with a session is
    /// named from that session once per run; the set keeps the store from being
    /// read again on every sidebar rebuild.
    pub harness_session_titles: std::collections::HashSet<String>,
    /// The saved hosts read from `~/.goble/hosts.toml`, in file order. Filled
    /// when the host picker opens, never while the window draws.
    pub ssh_host_entries: Vec<HostEntry>,
    /// Whether the Cmd+Shift+N host picker is up and the row it highlights.
    pub ssh_host_picker_open: bool,
    pub ssh_host_picker_index: usize,
    /// The "Open SSH Host" overlay opened from the Workspaces "+" menu: its
    /// draft target, focus, chosen config, parsed hosts and honest error.
    pub ssh_target_open: bool,
    pub ssh_target_draft: String,
    pub ssh_target_focused: bool,
    pub ssh_config_path: String,
    pub ssh_config_hosts: Vec<goble_core::ssh_hosts::SshHost>,
    pub ssh_target_error: Option<String>,
    /// Whether the Workspaces header's add menu is up.
    pub sidebar_add_menu_open: Rc<RefCell<bool>>,
    /// Whether the Conversations header's add menu is up.
    pub conversations_add_menu_open: Rc<RefCell<bool>>,
    /// The sidebar Automations popover.
    pub automations_open: bool,
    /// Keyboard row in the Automations popover (`crons.len()` = New automation).
    pub automations_highlight: usize,
    /// Whether the composer's project pill menu is up.
    pub project_menu_open: Rc<RefCell<bool>>,
    /// The chat transcript's changed-files card fold, shared across rebuilds.
    pub changed_files_open: Rc<RefCell<bool>>,
    /// The settings overlay (not a pane): open flag and the hamburger's state
    /// in the narrow window.
    pub settings_overlay_open: bool,
    pub settings_overlay_hamburger: Rc<RefCell<bool>>,
    /// Whether the settings sheet floats (wide) or fills the window (narrow).
    pub settings_overlay_wide: bool,
    /// The chrome row's Overview popover.
    pub chrome_overview_open: Rc<RefCell<bool>>,
    pub chrome_more_open: Rc<RefCell<bool>>,
    /// Whether the right overview panel is maximized over the main pane.
    pub side_panel_maximized: bool,
    /// The tabs the right side panel shows, in strip order. It always starts on
    /// [`SideTabKind::Overview`]; the "+" menu appends the active pane's other
    /// surfaces. The app machine keeps it non-empty (see
    /// [`UiState::ensure_side_tabs`]).
    pub side_tabs: Vec<SideTabKind>,
    /// The inner Terminals/Background Tasks rail is open in Terminal and Task
    /// Output. AF: the per-tab topbar toggles it.
    pub side_rail_open: bool,
    /// The pane + working directory a Review side tab shows. The tab reads the
    /// pane's own diff controls and `pane_diffs[pane]`, so opening Review does
    /// not split a diff pane into the layout.
    pub side_review: Option<(u64, String)>,
    /// The file the side panel's Files tab shows, independent of the active
    /// pane's kind. A click anywhere that names a file sets this and activates
    /// the Files tab.
    pub side_file: Option<String>,
    /// The most recently opened file anywhere (a chat link, the explorer, the
    /// review list). The Files tab opens onto this file instead of its empty
    /// state — no extra "open a file" press needed.
    pub last_opened_file: Option<String>,
    /// The Files tab's explorer view: true shows the workspace tree inside the
    /// side panel, false shows the file the tab opened.
    pub side_files_show_explorer: bool,
    /// The index in [`Self::side_tabs`] the panel is showing. Kept in range by
    /// the side-tab methods; a stale value falls back to Overview.
    pub active_side_tab: usize,
    /// Whether the side panel's "+" menu is open. Shared with the popup so its
    /// open state survives the per-frame element rebuild.
    pub side_tab_add_menu_open: Rc<RefCell<bool>>,
    /// Horizontal swipe travel accumulated by the side panel's content wrapper
    /// (in logical points). Owned here because the wrapper is rebuilt every
    /// frame; one thresholded swipe moves exactly one tab.
    pub side_swipe_accum: Rc<RefCell<f32>>,
    /// Send a prompt immediately while a turn runs instead of queueing it.
    pub send_immediately: bool,
    /// A saved host whose session just ended, offered for reconnect: the pane
    /// it ran in and the entry's name. Cleared by reconnecting or dismissing.
    pub ssh_reconnect_prompt: Option<(u64, String)>,
    /// The floating pane drag in progress, shared with the pane elements so it
    /// survives the per-frame tree rebuild.
    pub floating_drag: Rc<RefCell<Option<FloatingDrag>>>,
    /// Worker agent executions observed live from the `agent:*` events, keyed
    /// by trace id. `agent:started` inserts, `agent:finished` removes (it is no
    /// longer in flight); the product pages are still re-read from the service
    /// by `executions:updated`.
    pub live_executions: HashMap<String, LiveExecution>,
    /// Per-pane rich-input controls (auto-approve, model, branch, harness mode,
    /// dropdown open flags), keyed by pane id so two pty/agent panes in the
    /// same workspace never share them.
    pub pane_controls: HashMap<u64, PaneControls>,
    /// Per-pane review view choices (unified/split, wrapping, the folded
    /// directories of the changed-file tree, and the header selector's state),
    /// keyed by pane id. Shared with the UI so they survive the per-frame
    /// element rebuild.
    pub diff_reviews: Rc<RefCell<HashMap<u64, DiffReviewState>>>,
    /// Per-pane transcript scroll offset, keyed by pane id. Owned here (not on
    /// the element, which is rebuilt every frame) so a pane's scrollback
    /// position and its follow-the-stream state survive the rebuild.
    pub pane_chat_scroll: HashMap<u64, Rc<RefCell<ScrollState>>>,
    /// Per-pane transcript drag selection, keyed by pane id. Owned here so the
    /// highlight and the frozen text survive the per-frame rebuild, and so two
    /// chat panes never share a selection.
    pub pane_transcript_selection: HashMap<u64, Rc<RefCell<goble_ui::TranscriptSelection>>>,
    /// Per-pane expanded state of the transcript's usage disclosure, keyed by
    /// pane id. Owned here for the same reason as the scroll offset: the element
    /// tree is rebuilt every frame.
    pub pane_usage_open: HashMap<u64, Rc<RefCell<bool>>>,
    /// Files the pane's rich input carries, keyed by pane id: what the attach
    /// control picked and the chips over the editor draw. Owned here (the
    /// element is rebuilt every frame) and spent with the draft they belong to
    /// (see `send_agent_prompt`).
    pub pane_attachments: HashMap<u64, Vec<String>>,
    /// Per-pane scroll offset of the terminal pane's own command sections (the
    /// block view), keyed by pane id. Owned here for the same reason as the
    /// transcript scroll: the pane's element tree is rebuilt every frame.
    pub pane_terminal_scroll: HashMap<u64, Rc<RefCell<ScrollState>>>,
    /// Per-pane scroll offset of a pane's file view, keyed by pane id. A file
    /// view opens at its first line and keeps the user's place, so it is a plain
    /// offset rather than the terminal's follow-the-end state.
    pub file_scroll: HashMap<u64, Rc<RefCell<ScrollState>>>,
    /// Per-pane horizontal offset of a pane's file view, keyed by pane id. The
    /// lines do not wrap, so the view scrolls sideways to the end of the
    /// longest one. Kept beside the vertical offset for the same reason.
    pub file_hscroll: HashMap<u64, Rc<RefCell<ScrollState>>>,
    /// The files each pane holds open as chips, in the order they were opened.
    /// A pane keeps one `PaneKind::File` path — the one it shows — and the
    /// list here names the files it can switch between without leaving the
    /// pane.
    pub pane_open_files: HashMap<u64, Vec<String>>,
    /// The editor buffer of every file a pane keeps open, keyed by pane and
    /// file, mirrored from the frame's `FileCache` so the close action can see
    /// whether any of them has unsaved edits (and save them) without the file
    /// cache.
    pub file_buffers: HashMap<(u64, String), Rc<RefCell<crate::ui::file_view::FileBuffer>>>,
    /// Live per-pane terminal sessions (PTY child + output buffer) and the local
    /// input mirrors used to route `Cmd+Enter` to the agent. Not persisted; a
    /// terminal pane is re-spawned in its cwd on next render.
    pub terminal: Rc<RefCell<TerminalRegistry>>,
    /// Whether the Cmd+K command palette overlay is open.
    pub command_palette_open: bool,
    /// The palette's filter text, used to narrow the command list.
    pub command_palette_query: String,
    /// The palette's selected row index.
    pub command_palette_index: usize,
    /// The global inbox: approvals, questions and terminal reports from every
    /// pane, newest first. Owned here because the inbox is app-wide, not a
    /// property of the pane on screen.
    pub inbox: Vec<InboxItem>,
    /// Whether the inbox overlay is up.
    pub inbox_open: bool,
    /// The row the inbox overlay highlights (an index into [`Self::inbox`]).
    pub inbox_index: usize,
    /// Why the last jump did not land (its pane closed, or nothing was
    /// selected), shown by the overlay instead of failing silently.
    pub inbox_error: Option<String>,
    /// Scroll offset of the inbox overlay's list.
    pub inbox_scroll: Rc<RefCell<ScrollState>>,
    /// The rect the topbar's inbox control painted at, recorded by the control
    /// itself. The panel is a window-level popover hung from that rect, one
    /// frame behind it — the same seam the settings model menu uses.
    pub inbox_anchor: Rc<RefCell<Option<goble_ui::RectF>>>,
    /// The id of the inbox item being announced at the top of the window, and
    /// when that banner stops being drawn on its own. Set where the item is
    /// raised, so it names a report this run produced and never one read back
    /// from the store at startup.
    pub announcement: Option<String>,
    pub announcement_until: Option<std::time::Instant>,
    /// Monotonic id source for event-derived inbox items.
    pub next_inbox_id: u64,
    /// The agent-management panel (the topbar's dashboard): whether it is up,
    /// what narrows the roster, and its drafts.
    pub agent_dashboard: AgentDashboardState,
    /// First run: a compact "getting started" hint shown once the workspace
    /// choice is made.
    pub show_onboarding_tip: bool,
    /// App-owned open flag for the topbar "+" menu (choose an environment to
    /// open a new space, or add a new medium), so it survives the per-frame
    /// element rebuild.
    pub add_space_menu_open: Rc<RefCell<bool>>,
    /// App-owned open flag for the topbar compact environment selector, so it
    /// survives the per-frame element rebuild.
    pub env_selector_open: Rc<RefCell<bool>>,
    /// The environment menu's hovered row (an item index), shared with the menu
    /// so its hover tray survives the per-frame element rebuild — hover itself
    /// is read at paint time.
    pub env_selector_hover: Rc<RefCell<Option<usize>>>,
    /// The one menu the window recognised as open on the last frame, mirrored
    /// from the UI's shared open-menu registry (`AppContext::open_menus`) after
    /// the tree painted: a `None` means no menu is up. The registry stays the
    /// authority (it owns the close handle and the surface the pointer belongs
    /// to); this is the app-level view of the same state, so a test or a
    /// reader of the state machine can see which menu the window is drawing.
    pub open_menu: Option<goble_ui::elements::MenuId>,
    /// Whether the "add a new medium" dialog is open over the app.
    pub add_medium_dialog_open: bool,
    /// The new medium's name as typed in the add-medium dialog.
    pub add_medium_draft: String,
    /// Whether the add-medium dialog's text field is focused.
    pub add_medium_focused: bool,
    /// App-owned open flag for the terminal pane's "Run agent" menu.
    pub terminal_run_agent_menu_open: Rc<RefCell<bool>>,
    /// Whether the user has completed (or dismissed) the first-run flow. Kept
    /// in the backend store so a returning run skips the onboarding overlays
    /// and the getting-started tip.
    pub onboarding_done: bool,
}
