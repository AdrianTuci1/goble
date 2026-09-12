use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use goble_harness_types::MediumKind;
use goble_ui::elements::{
    AgentCardUi, ChatMessage as UiChatMessage, ConversationEntry, PopupMenuItem, TerminalFilter,
};
use goble_ui::{ScrollState, SettingsPage};

use crate::state::PaneControls;
use crate::terminal::TerminalRegistry;

use super::color_picker;
use super::pane::Space;
use super::types::{
    AppTab, CostEntry, CronEntry, ExecutionEntry, HarnessEntry, LlmFormField, McpSearchEntry,
    McpServerEntry, SettingsCategory, TaskEntry, TimelineEntry, VaultSecretEntry, WorkflowEntry,
    WorkspaceRouting,
};

/// What a pane's sub-agent child view (S6) shows: the child's own conversation,
/// with the live record that titles the view. The rows are the child's, read
/// from the store under the child's chat id and parsed on the app side, so the
/// view draws them through the same message renderer the parent's transcript
/// uses.
#[derive(Clone, Debug)]
pub struct SubAgentViewSnapshot {
    pub child_id: String,
    /// The live record's row: the title's type, description, status and elapsed
    /// time. `None` when the child has no record in this pane anymore, which
    /// leaves the conversation itself on screen.
    pub row: Option<goble_ui::SubAgentRow>,
    pub messages: Vec<UiChatMessage>,
}

/// Per-pane chat data used to render a chat leaf: its own transcript, composer
/// draft, working-directory label, suspended ask and queued prompt. Each chat
/// pane is an independent session, so this is keyed by pane id and never shared.
#[derive(Clone, Debug)]
pub struct PaneChatSnapshot {
    pub conversation_id: String,
    pub messages: Vec<UiChatMessage>,
    pub composer_draft: String,
    pub composer_path: String,
    pub pending_ask: Option<goble_ui::AskUserUi>,
    /// A command the harness suspended on for approval, rendered by the
    /// composer's proposal card (A5).
    pub pending_command: Option<goble_ui::CommandProposalUi>,
    /// The app-owned selected candidate index for `pending_command`, so the
    /// composer's selection survives the per-frame rebuild.
    pub command_selection: Rc<RefCell<usize>>,
    pub queued_prompt: Option<String>,
    pub agent_busy: bool,
    /// The live turn-status footer's state (C2): the activity/elapsed while the
    /// pane's turn is busy, or the still-running kinds once it stops, or
    /// [`goble_ui::TurnStatus::Idle`] when nothing is in flight.
    pub turn_status: goble_ui::TurnStatus,
    /// A live remote-desktop frame the harness handed off, rendered inline.
    pub inline_screen: Option<ScreenFrameSnapshot>,
    /// A detected BYOH handoff URI (`rdp://…` / `goble://desktop?…`), rendered
    /// as a clickable "open remote desktop" affordance when no live frame yet.
    pub screen_link: Option<String>,
    /// The pane's live sub-agent records, keyed by the id of the parent tool
    /// call that spawned each child; the parent transcript's rows read their
    /// status, activity and elapsed time from these.
    pub sub_agents: HashMap<String, goble_ui::SubAgentRow>,
    /// The sub-agent child view this pane shows (S6): the child's own
    /// conversation, entered from the parent transcript's row. `None` when the
    /// pane shows its own transcript.
    pub sub_agent_view: Option<SubAgentViewSnapshot>,
    /// This pane's transcript scroll offset (app-owned, so the scrollback
    /// position and follow-the-stream flag survive the per-frame rebuild).
    pub scroll: Rc<RefCell<ScrollState>>,
    /// The tokens this conversation has spent, summed from the provider-reported
    /// usage of its own model calls. Accounting only, never a price.
    pub usage: goble_ui::TokenUsage,
    /// Whether the usage disclosure is expanded. App-owned so it survives the
    /// per-frame rebuild.
    pub usage_open: Rc<RefCell<bool>>,
}

/// The composer's warp-new style context pills: which harness (the selected
/// environment medium), which working directory (a session's path) and which
/// git branch the next turn is scoped to. The labels/menu items are derived
/// from the environment tree each frame; the menu open flags are app-owned so
/// they survive the per-frame element rebuild.
#[derive(Clone, Debug)]
pub struct ComposerContext {
    pub harness_label: String,
    pub harness_ids: Vec<String>,
    pub harness_items: Vec<PopupMenuItem>,
    pub harness_menu_open: Rc<RefCell<bool>>,
    pub dir_label: String,
    pub dir_ids: Vec<String>,
    pub dir_items: Vec<PopupMenuItem>,
    pub dir_menu_open: Rc<RefCell<bool>>,
    pub branch_label: String,
    pub branch_ids: Vec<String>,
    pub branch_items: Vec<PopupMenuItem>,
    pub branch_menu_open: Rc<RefCell<bool>>,
}

/// Plain snapshot of the main UI state used to build the tree. Owned by the
/// host app; rendered from scratch every frame so state changes show up live.
#[derive(Clone, Debug)]
pub struct UiSnapshot {
    pub current_tab: AppTab,
    pub conversations: Vec<ConversationEntry>,
    pub selected_id: Option<String>,
    pub search_query: String,
    pub search_focused: bool,
    pub new_conversation_draft: String,
    pub create_focused: bool,
    pub chat_messages: Vec<UiChatMessage>,
    /// A suspended agent question shown inline at the end of the transcript.
    pub pending_ask: Option<goble_ui::AskUserUi>,
    /// A prompt the user sent while the agent was busy, shown as a pending block.
    pub queued_prompt: Option<String>,
    pub composer_draft: String,
    pub composer_focused: bool,
    /// Model choices shown in the composer's model dropdown.
    pub models: Vec<String>,
    /// Currently selected model (shown as the composer's model label).
    pub selected_model: String,
    /// Harnesses the composer can route a turn to (native + registered BYOH).
    pub harnesses: Vec<HarnessEntry>,
    /// The harness the active pane routes turns to (default `internal`).
    pub selected_harness: String,
    /// App-owned open flag for the composer model menu, so open state survives
    /// the per-frame element rebuild.
    pub model_menu_open: Rc<RefCell<bool>>,
    pub agent_name: String,
    pub agent_busy: bool,
    /// The topbar live cue (C3): the count of work in flight beside the active
    /// pane's turn (`LiveWork::work_count`) and the spinner phase from the
    /// observed turn start. Zero is the inert state.
    pub live_work_count: usize,
    pub live_work_phase: f32,
    /// Whether the agent auto-approves `ask_user` questions.
    pub auto_approve: bool,
    pub right_sidebar_open: bool,
    /// Whether the agent/window is fullscreen (borderless). Rendered as the
    /// checked state of the agent header menu's fullscreen item.
    pub fullscreen: bool,
    /// App-owned open flag for the agent header's 3-dots menu.
    pub agent_header_menus: HashMap<u64, Rc<RefCell<bool>>>,
    /// Per-terminal-block filter state (open flag + selected filter), keyed by
    /// content; shared with app state so the filter tray persists.
    pub terminal_filters: Rc<RefCell<HashMap<String, TerminalFilter>>>,
    /// Collapsed/expanded state per reasoning row, keyed by the row's
    /// `<conversation>:<step>` key; shared with app state so a row the user
    /// expanded stays expanded.
    pub reasoning_expanded: Rc<RefCell<HashMap<String, bool>>>,
    /// Three-state fold per tool call (`Collapsed -> Truncated -> Expanded`),
    /// keyed by the call's id; shared with app state so a fold survives the
    /// per-frame rebuild.
    pub tool_fold: Rc<RefCell<HashMap<String, goble_ui::ToolDisplayMode>>>,
    /// Whole-transcript terminal filters (open flag + selected filter) keyed by
    /// pane id, so each pty/agent pane keeps its own filter bar.
    pub terminal_global_filters: HashMap<u64, TerminalFilter>,
    /// Per-pane scroll offset of the terminal pane's command sections, shared
    /// with app state so a scrollback position survives the per-frame rebuild.
    pub pane_terminal_scroll: HashMap<u64, Rc<RefCell<goble_ui::ScrollState>>>,
    pub crons_open: bool,
    pub crons: Vec<CronEntry>,
    /// Workflows registered with the embedded daemon (real data).
    pub workflows: Vec<WorkflowEntry>,
    /// Executions from the daemon execution ledger (real data).
    pub executions: Vec<ExecutionEntry>,
    /// Durable tasks from the persistence layer (real data).
    pub tasks: Vec<TaskEntry>,
    /// Chronological record derived from execution/session/task data.
    pub timeline: Vec<TimelineEntry>,
    /// Cost rows derived from real execution/usage records.
    pub costs: Vec<CostEntry>,
    pub settings_page: SettingsPage,
    pub settings_profile_name: String,
    pub settings_profile_email: String,
    pub settings_dark_mode: bool,
    pub settings_llm_provider: String,
    pub settings_llm_model: String,
    pub settings_llm_api_key: String,
    pub settings_llm_base_url: String,
    pub settings_llm_temperature: String,
    pub settings_workers: Vec<(String, String, String, bool)>,
    pub settings_cluster_name: String,
    pub settings_cluster_configured: bool,
    pub settings_authorized_keys: Vec<(String, String, String)>,
    pub settings_vault_unlocked: bool,
    pub settings_overlay_open: bool,
    pub settings_category: SettingsCategory,
    pub settings_invert_scroll: bool,
    pub settings_scroll_speed: i32,
    pub settings_font_size: f32,
    /// Whether the rich input uses modal (vim) editing, and which pane states
    /// to read for the mode badge.
    pub vim_mode: bool,
    pub theme_primary: Option<String>,
    pub theme_secondary: Option<String>,
    pub theme_accent: Option<String>,
    /// Which theme channel the single color wheel in Settings→Appearance edits.
    pub theme_color_target: color_picker::ColorTarget,
    pub theme_color_drag: Rc<RefCell<Option<color_picker::ColorPickerDrag>>>,
    /// First-run: whether the "configure a model key" banner is shown in chat.
    pub show_llm_key_banner: bool,
    /// First-run: whether the "local or remote workspace?" choice is shown.
    pub show_workspace_choice: bool,
    /// First-run routing decision once the user picks Local or Remote.
    pub workspace_routing: Option<WorkspaceRouting>,
    /// First-run: whether the model-provider dialog is open over the chat.
    pub llm_dialog_open: bool,
    /// Editable model-form fields, held in app-owned ref cells so the text and
    /// focus survive the per-frame element rebuild while the dialog is open.
    pub llm_dialog_provider: Rc<RefCell<String>>,
    pub llm_dialog_model: Rc<RefCell<String>>,
    pub llm_dialog_api_key: Rc<RefCell<String>>,
    pub llm_dialog_base_url: Rc<RefCell<String>>,
    pub llm_dialog_temperature: Rc<RefCell<String>>,
    pub llm_dialog_focus: Rc<RefCell<Option<LlmFormField>>>,
    pub sidebar_width: f32,
    pub sidebar_dragging: bool,
    /// Whether the left conversation sidebar is shown.
    pub sidebar_visible: bool,
    /// Whether the sidebar lists every conversation (false shows a few cards
    /// plus a "View all" button).
    pub conversations_expanded: bool,
    /// Scroll offset of the sidebar's conversation list.
    pub sidebar_scroll: Rc<RefCell<ScrollState>>,
    /// Scroll offset of the settings overlay's content pane.
    pub settings_scroll: Rc<RefCell<ScrollState>>,
    /// Whether the topbar workspace frame is in inline-rename mode.
    pub space_rename_editing: bool,
    /// The in-progress space name while renaming.
    pub space_rename_draft: String,
    /// Whether the rename field holds focus.
    pub space_rename_focused: bool,
    /// Per-card interaction state (hover / delete menu), shared with the
    /// card elements so selections and menus persist across frames.
    pub agent_cards: HashMap<String, Rc<RefCell<AgentCardUi>>>,
    /// Hover flag for the sidebar's "New conversation" row, owned here so the row's
    /// highlight survives the per-frame element rebuild.
    pub new_agent_hover: Rc<RefCell<bool>>,
    /// Multiple "spaces": each is a pane tree rendered as a top-bar tab. The
    /// active space's tree is what `build_ui` renders as the main content.
    pub spaces: Vec<Space>,
    pub active_space: usize,
    /// Hovered space tab index (app state so it survives the per-frame rebuild).
    pub space_hover: Option<usize>,
    /// Space tab currently pressed (MouseDown), used to detect a drag start.
    pub space_press: Option<usize>,
    /// Space tab currently being dragged (reordering in progress).
    pub space_drag: Option<usize>,
    /// The focused leaf pane (target for splits/closes, highlighted).
    pub active_pane_id: u64,
    /// The split node currently being dragged, if any.
    pub dragging_pane_id: Option<u64>,
    /// The current working path shown in the composer.
    pub composer_path: String,
    /// Warp-new composer context pills (harness / working dir / git branch),
    /// built per pane id: two pty/agent panes in one workspace each get their
    /// own pills and their own open flags.
    pub composer_context: HashMap<u64, ComposerContext>,
    /// Per-pane hover flags (keyed by pane id) so pane-header hover survives
    /// the per-frame element rebuild.
    pub pane_hover: HashMap<u64, Rc<RefCell<bool>>>,
    /// Per-pane chat data (transcript + draft + path) keyed by pane id, so each
    /// chat pane renders its own independent session.
    pub pane_chat: HashMap<u64, PaneChatSnapshot>,
    /// Per-pane rich-input controls (auto-approve, model, branch, harness mode,
    /// dropdown open flags), keyed by pane id.
    pub pane_controls: HashMap<u64, PaneControls>,
    /// Live terminal sessions + input mirrors, shared with the terminal pane
    /// elements so they can spawn/read/write a PTY without blocking the UI
    /// thread. Cloned by `Rc` each frame (never the sessions themselves).
    pub terminal: Rc<RefCell<TerminalRegistry>>,
    /// Whether the Cmd+K command palette overlay is open.
    pub command_palette_open: bool,
    /// The palette's filter text, used to narrow the command list.
    pub command_palette_query: String,
    /// The palette's selected row index.
    pub command_palette_index: usize,
    /// First run: a compact "getting started" hint shown once the workspace
    /// choice is made, so the first-run flow lands on the chat with a tip.
    pub show_onboarding_tip: bool,
    /// App-owned open flag for the topbar "+" menu (choose an environment to
    /// open a new space, or add a new medium).
    pub add_space_menu_open: Rc<RefCell<bool>>,
    /// App-owned open flag for the topbar compact environment selector.
    pub env_selector_open: Rc<RefCell<bool>>,
    /// Whether the "add a new medium" dialog is open over the app.
    pub add_medium_dialog_open: bool,
    /// The new medium's name as typed in the add-medium dialog.
    pub add_medium_draft: String,
    /// Whether the add-medium dialog's text field is focused.
    pub add_medium_focused: bool,
    /// App-owned open flag for the terminal pane's "Run agent" menu, so the
    /// popup survives the per-frame element rebuild.
    pub terminal_run_agent_menu_open: Rc<RefCell<bool>>,
}

/// Plain snapshot of the AI domain state (vault + MCP connectors) used to
/// build the auxiliary panels. Owned by the host app.
#[derive(Clone, Debug)]
pub struct AiSnapshot {
    pub connectors_open: bool,
    pub vault_open: bool,
    pub vault_unlocked: bool,
    pub vault_secrets: Vec<VaultSecretEntry>,
    pub vault_unlock_draft: String,
    pub vault_new_key: String,
    pub vault_new_value: String,
    pub vault_error: Option<String>,
    pub connector_search: String,
    pub connectors: Vec<McpServerEntry>,
    pub install_open: bool,
    pub install_editing_id: Option<String>,
    pub install_name: String,
    pub install_source: String,
    pub install_source_value: String,
    pub install_search_query: String,
    pub install_search_results: Vec<McpSearchEntry>,
    pub install_selected_secrets: Vec<String>,
    pub install_error: Option<String>,
    pub installing: bool,
}

/// A single session/run shown in the per-project observability panel.
#[derive(Clone, Debug)]
pub struct ProjectSessionEntry {
    pub session_id: String,
    pub medium: String,
    pub created_at: String,
    pub status: String,
}

/// A project group in the projects panel: its sessions and a "what's running"
/// status derived from the project's tasks.
#[derive(Clone, Debug)]
pub struct ProjectEntry {
    pub project_id: String,
    pub directory: String,
    pub status: String,
    pub sessions: Vec<ProjectSessionEntry>,
}

/// Plain snapshot of the per-project observability panel. Owned by the host
/// app and rebuilt from [`crate::projects::ProjectsState`] every frame.
#[derive(Clone, Debug)]
pub struct ProjectsSnapshot {
    pub projects: Vec<ProjectEntry>,
}

/// A session leaf in the environment tree: one run scoped to a project on a
/// medium. `path` is the working directory the turn runs in (and the active
/// pane's cwd is set to it when the session is chosen).
#[derive(Clone, Debug)]
pub struct MediaSession {
    pub id: String,
    pub label: String,
    pub path: String,
}

/// A project branch in the environment tree, holding the sessions that run on
/// that project through the parent medium.
#[derive(Clone, Debug)]
pub struct MediaProject {
    pub id: String,
    pub label: String,
    pub sessions: Vec<MediaSession>,
}

/// A top-level medium node in the environment tree (Local, VM, Remote xrdp,
/// Browser, Container), each expandable to its projects, each project
/// expandable to its sessions. `id` is the [`goble_harness_types::MediumId`]
/// string carried on the turn.
#[derive(Clone, Debug)]
pub struct MediaNode {
    pub kind: MediumKind,
    pub id: String,
    pub label: String,
    pub projects: Vec<MediaProject>,
}

/// Plain snapshot of the environment-tree selector state. Owned by the host
/// app and rebuilt from [`crate::media::MediaState`] every frame.
#[derive(Clone, Debug)]
pub struct MediaSnapshot {
    pub mediums: Vec<MediaNode>,
    /// The id of the currently selected medium.
    pub selected_medium: String,
    /// The id of the currently selected project (under the selected medium).
    pub selected_project: String,
    /// The id of the currently selected session leaf, or empty when the user
    /// has not picked one yet (the turn then runs on the pane's own session).
    pub selected_session: String,
    /// Keys of the tree branches currently expanded (medium / project nodes).
    pub expanded: Vec<String>,
}

/// A capturable/controllable screen source shown in the screen panel. Read live
/// from the backend's [`goble_screen_core::ScreenRegistry`] (exposed via
/// [`goble_desktop_service::DesktopState::screen_registry`]).
#[derive(Clone, Debug)]
pub struct ScreenSourceEntry {
    /// Source identifier, e.g. `local`.
    pub source: String,
    /// Whether the source has a registered [`goble_screen_core::ScreenCapturer`].
    pub capturable: bool,
    /// Whether the source has a registered [`goble_screen_core::ScreenController`].
    pub controllable: bool,
}

/// Read-only mirror of the held live frame, passed to a [`goble_ui::elements::FrameView`].
#[derive(Clone, Debug)]
pub struct ScreenFrameSnapshot {
    /// Monotonic parity used to detect pixel changes.
    pub frame_seq: u64,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// RGBA8 pixel data (`width * height * 4` bytes).
    pub data: Arc<[u8]>,
}

/// Plain snapshot of the screen panel state. Owned by the host app and rebuilt
/// from [`crate::screen::ScreenState`] every frame.
#[derive(Clone, Debug)]
pub struct ScreenSnapshot {
    /// Whether the screen sheet is open.
    pub open: bool,
    /// Live capture: whether broadcast is streaming the selected source.
    pub broadcast: bool,
    /// Computer-use: whether click/type/scroll input is enabled.
    pub computer_use: bool,
    /// The source the panel is operating on.
    pub selected_source: String,
    /// The capturable/controllable sources from the screen registry.
    pub sources: Vec<ScreenSourceEntry>,
    /// Short status of the last broadcast capture (dimensions or an error).
    pub last_capture: Option<String>,
    /// Whether screen-event recording is active.
    pub recording: bool,
    /// Whether a screen-event replay is running.
    pub replaying: bool,
    /// Whether the replay loops instead of running once.
    pub replay_loop: bool,
    /// Number of recorded events.
    pub recorded_count: usize,
    /// Non-empty status from the last replay step (e.g. an input error).
    pub replay_status: Option<String>,
    /// The held live frame, when broadcast is capturing a source.
    pub frame: Option<ScreenFrameSnapshot>,
}
