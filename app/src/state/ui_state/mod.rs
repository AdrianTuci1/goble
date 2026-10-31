//! One pane-tree state machine: the `UiState` value and its inherent methods.
//!
//! The struct's fields are declared here; its methods live one module per
//! concern ([`refresh`], [`pane_move`], [`panes`], [`agents`], [`session`],
//! [`settings`], [`close`], [`mock`], [`spaces`], [`worker`]) as separate
//! `impl UiState` blocks, which are the same block to the compiler as one.
//! [`title`] is not one of them: the subject a conversation is named with is
//! derived from the conversation, not held by the state machine.

use super::*;

use goble_core::hosts::HostEntry;
use goble_core::ssh_hosts::SshHosts;

mod agents;
mod close;
mod inbox;
mod mock;
mod pane_move;
mod panes;
mod refresh;
mod session;
mod settings;
mod spaces;
mod title;
mod worker;

pub use agents::{PaneAgentState, TurnOutcome};
pub use inbox::{InboxItem, InboxKind};
pub use pane_move::{PaneDrag, PaneDrop, PaneMove};

/// The title a conversation gets when it is created without a subject of its
/// own. It is a placeholder, not a subject: a tab whose agent holds such a
/// conversation still reads [`NEW_AGENT_TAB_LABEL`], and the agent replaces it
/// with a subject derived from the conversation ([`title`]) once it has
/// answered in it.
pub const NEW_CONVERSATION_TITLE: &str = "New conversation";

/// The label a tab whose pane holds an agent draws while that agent's
/// conversation has no subject yet. Once the conversation has one, the tab
/// reads the subject instead (see [`UiState::space_label`]).
pub const NEW_AGENT_TAB_LABEL: &str = "New Agent";

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


#[derive(Clone)]
pub struct UiState {
    pub current_tab: AppTab,
    pub conversations: Vec<ConversationEntry>,
    pub selected_id: Option<String>,
    pub search_query: String,
    pub search_focused: bool,
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
    /// Model choices shown in the composer's model dropdown.
    pub models: Vec<String>,
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
    pub right_sidebar_open: bool,
    /// Whether the agent/window is currently fullscreen (borderless). Lives in
    /// app state so it survives the per-frame element rebuild.
    pub fullscreen: bool,
    /// The sub-agent child views open in a pane, keyed by pane id: the child's
    /// own conversation shown in the pane, entered from the parent transcript's
    /// row ([`UiState::open_sub_agent`]) and left again with Esc
    /// ([`UiState::close_sub_agent_view`]).
    pub sub_agent_views: HashMap<u64, SubAgentChildView>,
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
    /// Whole-transcript terminal filter (open flag + selected filter) per pane,
    /// so the filter bar of one pty/agent pane does not open in its sibling.
    pub terminal_global_filters: HashMap<u64, TerminalFilter>,
    pub crons_open: bool,
    /// Whether the workflow-runs overlay is up (it covers the workspace without
    /// replacing it).
    pub task_workflow_open: bool,
    /// The workflow the overlay's run list has selected, and the phase — one of
    /// that workflow's steps — selected inside its detail. Shared cells because
    /// the element tree is rebuilt every frame: a row the user clicked writes
    /// its index there and the next frame's rows read it back.
    pub task_workflow_selected: Rc<RefCell<usize>>,
    pub task_workflow_phase: Rc<RefCell<usize>>,
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
    /// Settled workflow runs, newest first (real `workflow_runs` ledger).
    pub workflow_runs: Vec<WorkflowRunEntry>,
    /// Executions from the daemon execution ledger (real data).
    pub executions: Vec<ExecutionEntry>,
    /// Durable tasks from the persistence layer (real data).
    pub tasks: Vec<TaskEntry>,
    /// Chronological records derived from execution/session/task data.
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
    /// Which of the settings tab's two regions (the page rail, the content
    /// pane) holds the keyboard. Opening it starts in the rail.
    pub settings_focus: SettingsFocus,
    /// The pane control the keyboard is on: an index into the order
    /// [`crate::ui::settings::pane_controls`] answers for the active category.
    pub settings_pane_focus: usize,
    /// Whether the focused pane control is a text field that holds the caret.
    /// While it does, the overlay reserves `Enter` (commit the edit) and
    /// `Escape` (cancel it) and every other key reaches the field.
    pub settings_pane_field_active: bool,
    /// What the focused text field held when the caret was put into it.
    /// `Enter` commits what was typed and clears this; `Escape` puts the value
    /// back, so a cancelled edit leaves nothing behind.
    pub settings_field_edit_start: Option<String>,
    /// Settings → Environment: the persisted groups of secrets and their
    /// entries, reloaded from the store whenever one changes.
    pub settings_environment_groups: Vec<SecretGroup>,
    /// Settings → Environment: the group whose secrets the pane is showing.
    pub settings_environment_open_group: Option<String>,
    /// Settings → Environment: the new-group name field.
    pub settings_environment_group_draft: String,
    /// Settings → Environment: the secret-name field.
    pub settings_environment_secret_name: String,
    /// Settings → Environment: the secret-value field. Held here, like every
    /// other settings field, because the element tree is rebuilt every frame.
    pub settings_environment_secret_value: String,
    /// Settings → Environment: the name the entry being edited carried before
    /// the fields were filled in, so an edit that renames removes the old one.
    pub settings_environment_editing: Option<String>,
    /// Settings → Connections: the `~/.ssh` read, filled when the page is
    /// first shown and kept until the reload row re-reads it. The element tree
    /// is rebuilt every frame, so the directory is read here and never while
    /// the page draws; `None` means there was no home directory to read.
    pub settings_ssh_hosts: Option<SshHosts>,
    /// Whether [`Self::settings_ssh_hosts`] has been filled. A cache that has
    /// been read is not read again for the same home, however many frames the
    /// page is drawn on.
    pub settings_ssh_read: bool,
    /// The home directory `~/.ssh` is read under. `None` is the user's own
    /// home; tests point it at a fixture so no real home is ever read.
    pub settings_ssh_home: Option<std::path::PathBuf>,
    /// Settings → Connections: the alias `Enter` last selected, drawn as the
    /// page's prefill line. It names a target for whatever consumes it next.
    pub settings_ssh_selected: Option<String>,
    /// Mouse: invert the wheel/scroll direction.
    pub settings_invert_scroll: bool,
    /// Mouse: scroll speed multiplier (1..=100, default 50).
    pub settings_scroll_speed: i32,
    /// Editor: font size as a whole-app zoom factor (mirrors `ui_zoom`).
    pub settings_font_size: f32,
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
    /// Custom theme color overrides as `#rrggbb` hex, set via the color wheel
    /// pickers in Settings→Appearance. `None` uses the built-in theme color.
    pub theme_primary: Option<String>,
    pub theme_secondary: Option<String>,
    pub theme_accent: Option<String>,
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
    /// Whether the left conversation sidebar is shown. Toggled by the topbar's
    /// sidebar button; the main area fills the window when it is hidden.
    pub sidebar_visible: bool,
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
    /// Global search: the query, the rows it produced and whether it has run at
    /// all. The search is run when the query changes, never per frame.
    pub global_search_query: String,
    pub global_search_rows: Vec<SearchRow>,
    pub global_search_searched: bool,
    /// Why the search has nothing to show, when that is not simply an empty
    /// result: no ripgrep on this machine, or a query ripgrep refused.
    pub global_search_error: Option<String>,
    pub global_search_focused: bool,
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
    /// Scroll offset of the settings overlay's content pane.
    pub settings_scroll: Rc<RefCell<ScrollState>>,
    /// Whether the topbar workspace frame is in inline-rename mode (entered by
    /// double-clicking the tab, or from its menu's "Rename tab" row).
    pub space_rename_editing: bool,
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
    /// The saved hosts read from `~/.goble/hosts.toml`, in file order. Filled
    /// when the host picker opens, never while the window draws.
    pub ssh_host_entries: Vec<HostEntry>,
    /// Whether the Cmd+Shift+N host picker is up and the row it highlights.
    pub ssh_host_picker_open: bool,
    pub ssh_host_picker_index: usize,
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
    /// Monotonic id source for event-derived inbox items.
    pub next_inbox_id: u64,
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
