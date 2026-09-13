//! One pane-tree state machine: the `UiState` value and its inherent methods.
//!
//! The struct's fields are declared here; its methods live one module per
//! concern ([`refresh`], [`panes`], [`agents`], [`session`], [`settings`],
//! [`mock`], [`spaces`]) as separate `impl UiState` blocks, which are the same
//! block to the compiler as one.

use super::*;

mod agents;
mod mock;
mod panes;
mod refresh;
mod session;
mod settings;
mod spaces;


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
    /// A detected BYOH handoff URI the user clicked to open; the root view
    /// performs the open (screen sheet) then clears this.
    pub pending_screen_link_open: Option<String>,
    /// App-owned open flag for the composer model menu.
    pub model_menu_open: Rc<RefCell<bool>>,
    /// App-owned open flags for the composer harness / dir / branch menus.
    pub harness_menu_open: Rc<RefCell<bool>>,
    pub dir_menu_open: Rc<RefCell<bool>>,
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
    /// App-owned open flags for each pane's agent-header 3-dots menu, keyed by
    /// pane id. Per-pane (rather than a single shared flag) so opening the tray
    /// in one split pane does not open it in the other panes sharing the view.
    pub agent_header_menus: HashMap<u64, Rc<RefCell<bool>>>,
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
    /// Whether the tasks & workflows overlay is up (it covers the workspace
    /// without replacing it).
    pub task_workflow_open: bool,
    /// Whether the keyboard shortcuts panel is up.
    pub shortcuts_help_open: bool,
    pub crons: Vec<CronEntry>,
    /// Harness workflows (real daemon workflow store).
    pub workflows: Vec<WorkflowEntry>,
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
    /// Whether the Settings overlay panel is open (replaces the old Settings tab).
    pub settings_overlay_open: bool,
    /// Active settings category in the overlay.
    pub settings_category: SettingsCategory,
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
    /// double-clicking the active space's name).
    pub space_rename_editing: bool,
    /// The in-progress space name while renaming.
    pub space_rename_draft: String,
    /// Whether the rename field holds focus.
    pub space_rename_focused: bool,
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
    /// Space tab currently being dragged (reordering in progress).
    pub space_drag: Option<usize>,
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
    /// Per-pane expanded state of the transcript's usage disclosure, keyed by
    /// pane id. Owned here for the same reason as the scroll offset: the element
    /// tree is rebuilt every frame.
    pub pane_usage_open: HashMap<u64, Rc<RefCell<bool>>>,
    /// Per-pane scroll offset of the terminal pane's own command sections (the
    /// block view), keyed by pane id. Owned here for the same reason as the
    /// transcript scroll: the pane's element tree is rebuilt every frame.
    pub pane_terminal_scroll: HashMap<u64, Rc<RefCell<ScrollState>>>,
    /// Per-pane scroll offset of a pane's file view, keyed by pane id. A file
    /// view opens at its first line and keeps the user's place, so it is a plain
    /// offset rather than the terminal's follow-the-end state.
    pub file_scroll: HashMap<u64, Rc<RefCell<ScrollState>>>,
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
