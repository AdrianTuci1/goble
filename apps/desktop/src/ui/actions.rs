use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goble_ui::elements::LaneEdge;
use goble_ui::geometry::Vector2F;
use goble_ui::SettingsPage;

use super::color_picker;
use super::file_menu::FileMenuAction;
use super::pane::{FloatingDragMode, NavDir, PaneDropTarget, PaneRect};
use super::tab_menu::TabMenuAction;
use super::types::{
    AppTab, ArchivedFilter, OnboardingAction, SettingsCategory, SidebarView, WorkspaceRouting,
};
use crate::state::DiffReviewState;
use crate::state::SideTabAction;

/// Start a floating pane drag: the pane, the mode, the pointer where the press
/// landed, and the space's pane area in pixels.
pub type FloatStartHandler = Rc<RefCell<dyn FnMut(u64, FloatingDragMode, Vector2F, Vector2F)>>;
/// Answer the inline ask-user card, with an optional credential.
pub type AnswerAskHandler = Rc<RefCell<dyn FnMut(String, Option<(String, String)>)>>;
/// Answer a suspended command proposal from a pane.
pub type CommandDecisionHandler =
    Rc<RefCell<dyn FnMut(u64, String, goble_core::harness::CommandDecision)>>;
/// Move a lifted pane with the pointer: the position, the tab-strip insertion
/// point it names (`None` off the strip), and the pane it is over in the pane
/// area (`None` over no other pane).
pub type PaneLiftMoveHandler =
    Rc<RefCell<dyn FnMut(Vector2F, Option<usize>, Option<PaneDropTarget>)>>;
/// Save the LLM form's five fields.
pub type SaveLlmHandler = Rc<RefCell<dyn FnMut(String, String, String, String, String)>>;
/// A callback carrying three strings.
pub type ThreeStringHandler = Rc<RefCell<dyn FnMut(String, String, String)>>;

/// One interaction in the diff review pane.
///
/// The pane is rebuilt every frame, so its view choices (which file is
/// selected, which files are open, whether the sidebar shows) live in the
/// pane's controls and the pane reports each change here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffPanelAction {
    /// Open the review pane for a source pane's directory, optionally on one
    /// file. The composer's diff menu sends this.
    Open(Option<String>),
    /// Show only this file, or every file when `None`.
    Select(Option<String>),
    /// Open or fold one file's diff. The row sends the state it wants rather
    /// than "toggle", because a long or binary file starts folded and a toggle
    /// would fold it again.
    SetFileOpen(String, bool),
    /// Open or close one file's in-place editor. Opening also unfolds the file
    /// and hands it the keyboard; closing returns the block to its diff.
    SetFileEditing(String, bool),
    /// Make this review pane the active one, so a press in an editor can type
    /// into it even when the pane was opened beside a terminal.
    Focus,
    /// Show or hide the file list.
    ToggleSidebar,
    /// Switch the comparison between one column and side by side.
    ToggleView,
    /// Turn long-line wrapping on or off.
    ToggleWrap,
    /// Fold or unfold one directory of the changed-file tree.
    ToggleDir(String),
    /// Copy the current contents of one changed file to the clipboard. The
    /// path is absolute; a file the platform cannot read leaves the clipboard
    /// untouched and reports why in the pane's status line.
    CopyContent(String),
    /// Open a changed file in a file pane that floats over the space.
    OpenFloating(String),
    /// Open or close the commit bar under the topbar.
    ToggleCommit,
    /// Hold the commit bar's message as it is typed.
    SetCommitMessage(String),
    /// Stage every change and commit it with the bar's message.
    Commit,
    /// Push the current branch.
    Push,
    /// Open a pull request for the current branch through `gh`.
    CreatePr,
    /// Copy a changed file's `file://` link.
    CopyLink(String),
    /// Ask the file's header to confirm discarding its changes.
    ConfirmRevert(String),
    /// Dismiss the revert confirmation without touching the file.
    CancelRevert,
    /// Restore one file to `HEAD`, or remove it when it is untracked.
    RevertFile(String),
    /// Open a changed file in the platform's editor.
    OpenExternal(String),
    /// Open a changed file in a file pane.
    OpenFile(String),
    /// Copy these paths to the system clipboard, one per line.
    CopyPaths(Vec<String>),
}

/// One interaction in the agent-management panel (the topbar's dashboard).
///
/// The panel is a roster of conversations with a peek/reply box and a dispatch
/// box; every control it draws sends one of these to the host, which mutates
/// the app-owned [`super::AgentDashboardState`] and the conversation it names.
pub enum AgentDashboardAction {
    /// The topbar control: show the panel, or put it away when it is up.
    Toggle,
    /// Close the panel without touching anything.
    Close,
    /// Move the panel's cursor to this conversation (`None` clears it), so the
    /// peek panel follows the row the user picked.
    Select(Option<String>),
    /// The roster's search text changed.
    Search(String),
    /// Track focus of the roster's search field.
    SearchFocus(bool),
    /// The reply draft changed.
    SetReplyDraft(String),
    ReplyFocus(bool),
    /// Send the reply draft to this conversation. A busy agent queues it.
    Reply(String),
    /// The dispatch draft changed.
    SetDispatchDraft(String),
    DispatchFocus(bool),
    /// Put the panel in dispatch mode: the prompt line takes the caret, or
    /// drops the draft and gives the keyboard back when it already has it. The
    /// line itself is always drawn.
    ToggleDispatch,
    /// The Escape shortcut: step the cursor off whatever it was on and put the
    /// caret in the prompt for a new agent, keeping any draft already typed.
    NewAgent,
    /// Start a new conversation seeded with the dispatch draft, in a tab of
    /// its own.
    Dispatch,
    /// Cancel the named conversation's running turn.
    Stop(String),
    /// Remove this conversation: the first click arms the row's delete, the
    /// second one wipes the conversation from the store.
    Delete(String),
    /// Start renaming this conversation: its row becomes an editable field.
    BeginRename(String),
    /// The rename field's draft changed.
    RenameDraft(String),
    /// Commit the rename field's draft as the conversation's name.
    CommitRename,
    /// Leave rename without writing anything.
    CancelRename,
    /// Switch the roster between grouping by state and by directory.
    ToggleGrouping,
}

/// One gesture the side panel's Agent tab sends to the host.
///
/// The tab is a real conversation agent with its own conversation id, so its
/// composer cannot go through the active-pane callbacks: typing in it must not
/// touch the active pane's draft, and sending must run the side conversation's
/// turn even when the keyboard is in the main pane. The host owns the side
/// pane's session under [`crate::state::SIDE_AGENT_PANE_ID`].
pub enum SideChatAction {
    /// The side composer's draft changed.
    Draft(String),
    /// The side composer gained or lost focus.
    Focus(bool),
    /// Submit the side draft: bind a new conversation on the first send, then
    /// run the turn on the side pane's own conversation.
    Send(String),
    /// The side composer's attach control.
    Attach,
}

/// Callbacks supplied by the host app for the main view. Created fresh on
/// every rebuild; they mutate app-owned state, which is rendered back on the
/// next frame.
#[derive(Clone)]
pub struct UiActions {
    pub on_search_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_search_focus_change: Rc<RefCell<dyn FnMut(bool)>>,
    pub on_create_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_create_focus_change: Rc<RefCell<dyn FnMut(bool)>>,
    pub on_create_submit: Rc<RefCell<dyn FnMut()>>,
    pub on_select_conversation: Rc<RefCell<dyn FnMut(String)>>,
    pub on_select_tab: Rc<RefCell<dyn FnMut(AppTab)>>,
    pub on_composer_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_composer_focus_change: Rc<RefCell<dyn FnMut(bool)>>,
    /// The side panel Agent tab's composer: a conversation of its own, routed to
    /// [`crate::state::SIDE_AGENT_PANE_ID`] instead of the active pane.
    pub on_side_chat: Rc<RefCell<dyn FnMut(SideChatAction)>>,
    pub on_send_message: Rc<RefCell<dyn FnMut(String)>>,
    /// Send a line to the CLI agent running in a pane's pty: `(pane, text)`.
    /// This is the rich input of a TUI harness — the prompt goes to the agent's
    /// own stdin, not to a model turn.
    pub on_agent_prompt: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Cmd/Ctrl+Enter in a chat composer: send the draft to the agent as a NEW
    /// agent conversation (warp-new behavior), binding the pane to a fresh
    /// conversation before the turn.
    pub on_cmd_enter: Rc<RefCell<dyn FnMut(String)>>,
    /// Cmd/Ctrl+Alt+Enter in a chat composer: route the conversation to the
    /// cloud medium and submit the draft (warp-new's `⌘⌥⏎`).
    pub on_send_to_cloud: Rc<RefCell<dyn FnMut(String)>>,
    /// Move the active pane's slash-command menu selection to another row.
    pub on_slash_move: Rc<RefCell<dyn FnMut(usize)>>,
    /// Put the slash-command menu away after a command ran: the draft it was
    /// querying is spent, so the bar is cleared and the menu closes with it.
    pub on_slash_close: Rc<RefCell<dyn FnMut()>>,
    /// Put the slash-command menu away on Escape without touching the draft, so
    /// a command half-typed is not thrown away by closing the menu.
    pub on_slash_dismiss: Rc<RefCell<dyn FnMut()>>,
    /// Take a skill or a path the band offered: the text replaces the trigger
    /// token in the active pane's draft, so what the harness reads is the token
    /// the user picked.
    pub on_composer_insert: Rc<RefCell<dyn FnMut(String)>>,
    pub on_attach: Rc<RefCell<dyn FnMut()>>,
    pub on_voice: Rc<RefCell<dyn FnMut()>>,
    pub on_select_model: Rc<RefCell<dyn FnMut()>>,
    /// Select a specific model from one pane's composer dropdown, by display
    /// name. The pane id keeps the choice local to that pane.
    pub on_model_select: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Set one pane's thinking level from the model tray, by the id the harness
    /// reports (`on` plans the turn first, `off` answers it directly). The
    /// choice is persisted on the pane's conversation.
    pub on_thinking_select: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Select the composer harness (environment medium) by medium id.
    pub on_select_harness: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Select a pane's composer working directory (session) by session id.
    pub on_select_dir: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Select a pane's composer git branch by branch name.
    pub on_select_branch: Rc<RefCell<dyn FnMut(u64, String)>>,
    pub on_copy: Rc<RefCell<dyn FnMut()>>,
    /// Copy a terminal block's text to the clipboard (receives the block text).
    pub on_copy_terminal: Rc<RefCell<dyn FnMut(String)>>,
    /// Clear the active pane's transcript (`/clear` in the palette).
    pub on_clear_transcript: Rc<RefCell<dyn FnMut()>>,
    pub on_stop: Rc<RefCell<dyn FnMut()>>,
    /// Enter a sub-agent child's own conversation in `pane_id`, by the child's
    /// conversation id — the open control on the pane's sub-agent strip (S5).
    /// The child view (S6) puts the child's card in the pane's block list and
    /// shows the child's transcript in its place.
    pub on_open_sub_agent: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Stop the sub-agent child `child_id` running under `pane_id`, by the
    /// child's conversation id — the stop control the pane's sub-agent strip
    /// draws while the child runs.
    pub on_stop_sub_agent: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Leave the child view `pane_id` shows and put the pane back on the
    /// conversation it came from; the child's card stays in the block list.
    pub on_close_sub_agent: Rc<RefCell<dyn FnMut(u64)>>,
    /// Send the text typed in a child view to that child's own conversation:
    /// `(pane_id, text)`. The child id is resolved from the pane's open view.
    pub on_send_sub_agent_message: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Fold or open one group of the work dock under the agent pane's header.
    pub on_toggle_work_section: Rc<RefCell<dyn FnMut(goble_ui::WorkSection)>>,
    /// Reveal the rows one work-dock group holds behind `show N more`.
    pub on_show_all_work_section: Rc<RefCell<dyn FnMut(goble_ui::WorkSection)>>,
    /// Open the workflow-runs view in `pane_id` on the run a work-dock row
    /// stands for, by the run's ledger id.
    pub on_open_workflow_run: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Select a run in the pane's workflow-runs view: `run_id`'s detail, or the
    /// list when `None`. A pane with no view open ignores it.
    pub on_select_workflow_run: Rc<RefCell<dyn FnMut(u64, Option<String>)>>,
    /// Leave the workflow-runs view open in `pane_id`, back to its own surface.
    pub on_close_workflow_view: Rc<RefCell<dyn FnMut(u64)>>,
    /// Submit the composed answer of the inline ask-user card. The optional
    /// `(name, value)` carries a credential entered in the card, threaded out
    /// separately from the answer so the secret never enters the transcript.
    pub on_answer_ask: AnswerAskHandler,
    /// Skip the inline ask-user card.
    pub on_skip_ask: Rc<RefCell<dyn FnMut()>>,
    /// Answer a suspended command proposal from `pane_id`: `(pane, proposal id,
    /// decision)` resumes the turn (approve/edit runs it, reject fails it).
    pub on_command_decision: CommandDecisionHandler,
    /// Toggle one pane's auto-approve (autonomy) switch above its composer.
    pub on_toggle_auto_approve: Rc<RefCell<dyn FnMut(u64, bool)>>,
    /// Set one pane's approval mode from the composer's mode control or the
    /// Shift+Tab cycle: always-approve, manual, plan, auto-review.
    pub on_select_approval_mode: Rc<RefCell<dyn FnMut(u64, goble_ui::ApprovalMode)>>,
    /// Turn modal (vim) editing on or off for the rich input. The choice is
    /// persisted; turning it off returns every pane's editor to insert mode.
    pub on_toggle_vim_mode: Rc<RefCell<dyn FnMut(bool)>>,
    /// Editor & Input: save a dirty file view on close instead of asking.
    pub on_toggle_auto_save_on_close: Rc<RefCell<dyn FnMut(bool)>>,
    /// Fork `pane_id`'s conversation into a new one that inherits its transcript
    /// (the transcript footer's Fork).
    pub on_fork_conversation: Rc<RefCell<dyn FnMut(u64)>>,
    /// The system clipboard the vim registers `"+`/`"*` read and write.
    pub clipboard: Rc<RefCell<dyn goble_ui::vim::Clipboard>>,
    /// Turn one pane's harness mode on/off at the rich input (Warp-new style:
    /// Cmd+Enter activates the harness, Esc returns to the plain pty).
    pub on_set_pane_harness_mode: Rc<RefCell<dyn FnMut(u64, bool)>>,
    /// Esc at a chat pane's agent view: kept for the legacy call sites while
    /// the dual-surface switch is retired.
    pub on_show_pane_terminal: Rc<RefCell<dyn FnMut(u64)>>,
    /// Toggle an agent pane's bottom terminal dock from the header icon.
    pub on_toggle_terminal_dock: Rc<RefCell<dyn FnMut(u64)>>,
    /// Set the bottom terminal dock's height while its resize handle is dragged.
    pub on_terminal_dock_height: Rc<RefCell<dyn FnMut(u64, f32)>>,
    /// Enter a conversation's agent view in a pane by clicking the card it left
    /// behind in the terminal (Warp-new style: the `EnterAgentView` block).
    pub on_open_agent_view: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Send a queued prompt now (interrupting the in-flight turn).
    pub on_send_queued: Rc<RefCell<dyn FnMut()>>,
    /// Dismiss a queued prompt.
    pub on_dismiss_queued: Rc<RefCell<dyn FnMut()>>,
    pub on_menu: Rc<RefCell<dyn FnMut()>>,
    /// Toggle the global inbox overlay (the topbar control and Cmd+Shift+I).
    pub on_inbox: Rc<RefCell<dyn FnMut()>>,
    /// Close the inbox overlay without changing anything.
    pub on_close_inbox: Rc<RefCell<dyn FnMut()>>,
    /// Move the inbox's highlight by `step` rows, clamped to the list.
    pub on_inbox_move: Rc<RefCell<dyn FnMut(i32)>>,
    /// Acknowledge the highlighted inbox item.
    pub on_inbox_ack: Rc<RefCell<dyn FnMut()>>,
    /// Acknowledge every inbox item.
    pub on_inbox_ack_all: Rc<RefCell<dyn FnMut()>>,
    /// Jump to the pane the highlighted inbox item belongs to.
    pub on_inbox_jump: Rc<RefCell<dyn FnMut()>>,
    /// Jump to the pane the row at this index belongs to (a click on the row).
    pub on_inbox_jump_to: Rc<RefCell<dyn FnMut(usize)>>,
    /// Jump to the pane the announced item belongs to.
    pub on_announcement_jump: Rc<RefCell<dyn FnMut()>>,
    /// Put the announcement away without jumping anywhere.
    pub on_announcement_dismiss: Rc<RefCell<dyn FnMut()>>,
    /// One interaction in the agent-management panel: the topbar control opens
    /// it, and every row, field and action inside sends its own variant.
    pub on_agent_dashboard: Rc<RefCell<dyn FnMut(AgentDashboardAction)>>,
    pub on_settings: Rc<RefCell<dyn FnMut()>>,
    pub on_close_settings_overlay: Rc<RefCell<dyn FnMut()>>,
    /// Maximize the right overview panel over the main pane, or restore it.
    pub on_toggle_side_panel_maximized: Rc<RefCell<dyn FnMut()>>,
    /// One interaction in the right side panel's tab strip: select, close, add
    /// or cycle a tab. The app state machine keeps the strip and its selection.
    pub on_side_tab: Rc<RefCell<dyn FnMut(SideTabAction)>>,
    /// The settings tab's one dispatch: every rail row, control, field commit,
    /// dialog button and keyboard gesture the surface makes. The handler in
    /// `actions/settings_actions.rs` is the single call site for the
    /// file-backed settings service.
    pub settings: crate::ui::settings::SettingsActions,
    /// Toggle the left conversation sidebar (topbar sidebar button).
    pub on_toggle_sidebar: Rc<RefCell<dyn FnMut()>>,
    /// One interaction in the diff review pane, naming the pane it happened in.
    pub on_diff_panel: Rc<RefCell<dyn FnMut(u64, DiffPanelAction)>>,
    /// A file row/card was right-clicked: `(path, copy_path, at)`. The app
    /// stores the one open target; the next frame hangs the menu from `at`.
    pub on_file_menu: Rc<RefCell<dyn FnMut(String, String, Vector2F)>>,
    /// A row of the open file context menu was chosen.
    pub on_file_menu_action: Rc<RefCell<dyn FnMut(FileMenuAction)>>,
    /// The file context menu closed itself (outside press, Escape or a row).
    pub on_file_menu_close: Rc<RefCell<dyn FnMut()>>,
    /// Toggle the sidebar conversation list between "a few cards" and the full,
    /// scrollable list.
    pub on_toggle_conversations_expanded: Rc<RefCell<dyn FnMut()>>,
    /// Show one of the sidebar's views (conversations, project explorer, global
    /// search).
    pub on_select_sidebar_view: Rc<RefCell<dyn FnMut(SidebarView)>>,
    /// Collapse or expand one sidebar section, by its key.
    pub on_toggle_section: Rc<RefCell<dyn FnMut(String)>>,
    /// Star or unstar a conversation, from its card's menu.
    pub on_toggle_star: Rc<RefCell<dyn FnMut(String)>>,
    /// Open the sidebar's "Rename conversation" dialog for a conversation.
    pub on_session_rename_start: Rc<RefCell<dyn FnMut(String)>>,
    /// The Rename dialog's draft changed.
    pub on_session_rename_draft: Rc<RefCell<dyn FnMut(String)>>,
    /// Commit the Rename dialog (Enter or its Rename button).
    pub on_session_rename_commit: Rc<RefCell<dyn FnMut()>>,
    /// Cancel the Rename dialog (Escape or its backdrop).
    pub on_session_rename_cancel: Rc<RefCell<dyn FnMut()>>,
    /// Fork a conversation by id from the row's menu, without changing the
    /// selection (DSH forks the session and leaves the current one on screen).
    pub on_fork_conversation_by_id: Rc<RefCell<dyn FnMut(String)>>,
    /// Copy a conversation's raw id to the system clipboard and toast it.
    pub on_copy_conversation_id: Rc<RefCell<dyn FnMut(String)>>,
    /// Copy one labelled value from a menu flyout (conversation name, id,
    /// project name) to the clipboard and toast the result. `(what, value)`.
    pub on_copy_menu_value: Rc<RefCell<dyn FnMut(String, String)>>,
    /// Archive a conversation; live work opens the stop-and-archive confirm.
    pub on_archive_conversation: Rc<RefCell<dyn FnMut(String)>>,
    /// Restore an archived conversation.
    pub on_unarchive_conversation: Rc<RefCell<dyn FnMut(String)>>,
    /// Confirm the stop-and-archive dialog: stop the work, then archive.
    pub on_archive_confirm: Rc<RefCell<dyn FnMut()>>,
    /// Dismiss the stop-and-archive dialog, leaving the work running.
    pub on_archive_cancel: Rc<RefCell<dyn FnMut()>>,
    /// Set the sidebar's archived filter from the view-options menu.
    pub on_archived_filter_change: Rc<RefCell<dyn FnMut(ArchivedFilter)>>,
    /// The Workspaces header's "+": open the platform's directory picker and add
    /// the picked directory as a Workspace (DSH's add-workspace flow). It never
    /// creates a conversation — the separate "New conversation" row does that.
    pub on_add_workspace: Rc<RefCell<dyn FnMut()>>,
    /// A workspace row's New-session control: start a conversation inside that
    /// workspace's directory (DSH's `ProjectRow` create action).
    pub on_sidebar_workspace_new_session: Rc<RefCell<dyn FnMut(String, String)>>,
    /// Open the "Rename workspace" dialog for the workspace over this path,
    /// with the title it currently shows.
    pub on_sidebar_workspace_rename_start: Rc<RefCell<dyn FnMut(String, String)>>,
    /// The workspace Rename dialog's draft changed.
    pub on_sidebar_workspace_rename_draft: Rc<RefCell<dyn FnMut(String)>>,
    /// Commit the workspace Rename dialog (Enter or its Rename button).
    pub on_sidebar_workspace_rename_commit: Rc<RefCell<dyn FnMut()>>,
    /// Cancel the workspace Rename dialog (Escape or its backdrop).
    pub on_sidebar_workspace_rename_cancel: Rc<RefCell<dyn FnMut()>>,
    /// Reveal the workspace's own directory in the platform's file manager —
    /// DSH's "Show in Finder" workspace menu row (`NSWorkspace` selects the
    /// folder). A platform that cannot reveal raises the transient notice
    /// instead of pretending the folder opened.
    pub on_sidebar_workspace_show_in_finder: Rc<RefCell<dyn FnMut(String)>>,
    /// Delete a Workspace row (its list entry only; the folder and its
    /// conversations are kept, DSH's delete contract).
    pub on_sidebar_workspace_delete: Rc<RefCell<dyn FnMut(String)>>,
    /// A workspace row opened its tray (`Some` section) or every menu should
    /// close (`None`), so one menu is open at a time app-wide.
    pub on_sidebar_workspace_menu: Rc<RefCell<dyn FnMut(Option<String>)>>,
    /// Put the transient notice away.
    pub on_toast_dismiss: Rc<RefCell<dyn FnMut()>>,
    /// Expand or collapse a directory in the project explorer, by path.
    pub on_toggle_explorer_dir: Rc<RefCell<dyn FnMut(String)>>,
    /// Hand a file path to the active pane's input, from a click in the project
    /// explorer or on a global-search result.
    pub on_explorer_file_click: Rc<RefCell<dyn FnMut(String)>>,
    /// Open a file in a file pane and arrange the active space as floating
    /// windows (DSH's "Open in floating panel"). The file pane opens first, so
    /// the window the gesture is about is the one on top.
    pub on_open_file_floating: Rc<RefCell<dyn FnMut(String)>>,
    /// Dock one window floated over the active space's layout: the float goes
    /// away and the pane returns to the layout it was opened over.
    pub on_dock_float: Rc<RefCell<dyn FnMut(u64)>>,
    /// Per-pane review view choices, shared with the element tree so unified /
    /// split, wrapping, folded directories and the selector survive the
    /// per-frame rebuild.
    pub review_views: Rc<RefCell<HashMap<u64, DiffReviewState>>>,
    /// Update the global search query and run it.
    pub on_global_search_change: Rc<RefCell<dyn FnMut(String)>>,
    /// Track focus of the global search field.
    pub on_global_search_focus_change: Rc<RefCell<dyn FnMut(bool)>>,
    /// Click on a topbar workspace chip: selects that space, and a double-click
    /// enters inline rename for its name.
    pub on_workspace_click: Rc<RefCell<dyn FnMut(usize)>>,
    /// Right click on a workspace tab: open its menu hung from `at`, the point
    /// the click landed on. A second right click on the same tab closes it.
    pub on_space_menu: Rc<RefCell<dyn FnMut(usize, goble_ui::Vector2F)>>,
    /// The open tab menu closed itself — a press landed outside its panel.
    pub on_space_menu_close: Rc<RefCell<dyn FnMut()>>,
    /// A row of the tab menu was chosen, for the tab at that index.
    pub on_space_menu_action: Rc<RefCell<dyn FnMut(usize, TabMenuAction)>>,
    /// Update the inline space-rename draft.
    pub on_space_rename_change: Rc<RefCell<dyn FnMut(String)>>,
    /// Track focus of the inline space-rename field (blur commits).
    pub on_space_rename_focus: Rc<RefCell<dyn FnMut(bool)>>,
    /// Commit the inline space rename.
    pub on_space_rename_commit: Rc<RefCell<dyn FnMut()>>,
    /// Cancel the inline space rename without changing the name.
    pub on_space_rename_cancel: Rc<RefCell<dyn FnMut()>>,
    /// First-run onboarding: the usage card, the name field, the consent step,
    /// and the step buttons. The action layer persists the answers and finishes
    /// the flow.
    pub on_onboarding: Rc<RefCell<dyn FnMut(OnboardingAction)>>,
    /// Settings → Profile: the friendly name field.
    /// Settings → Profile: the name field gained (`true`) or lost the caret.
    /// Losing it abandons the edit, putting back what the field held.
    /// Settings → Agent & Approval: pick a default model from that menu.
    /// Settings → Profile: allow or refuse anonymous telemetry uploads.
    /// Settings: switch the page the settings tab shows.
    /// Settings: step the active category by `delta` (with wraparound). The
    /// arrow-key path, which must advance from the live category rather than
    /// from the one captured when the tree was built.
    /// Settings: move the keyboard's slot inside the region that holds it —
    /// the rail's category, or the pane's control.
    /// Settings: move the keyboard from the category rail into the content
    /// pane, onto its first control.
    /// Settings: move the keyboard from the content pane back to the rail.
    /// Settings: activate the focused pane control — a switch flips, a stepper
    /// advances, a list row opens, a text field takes the caret.
    /// Settings: adjust the focused pane control by `delta`; only the controls
    /// that hold a discrete value change.
    /// Settings: commit the focused text field's edit — `Enter` again — so what
    /// was typed stays and the caret goes.
    /// Settings: cancel the focused text field's edit, putting back what it
    /// held when the caret went in. This is what `Escape` means in a field
    /// before it means "back to the rail"; the sheet stays open.
    /// Settings → Environment: the new-group name field.
    /// Settings → Environment: create the group the name field holds.
    /// Settings → Environment: open a group (its secrets join the pane).
    /// Settings → Environment: delete a group and its secrets.
    /// Settings → Environment: the secret-name field.
    /// Settings → Environment: the secret-value field.
    /// Settings → Environment: add the secret the fields hold, or save the
    /// entry they were loaded from.
    /// Settings → Environment: load one secret back into the fields to edit.
    /// Settings → Environment: remove one secret.
    /// Settings: toggle mouse scroll-direction inversion.
    /// Settings: set mouse scroll speed (1..=100).
    /// Settings: set editor font-size zoom factor.
    /// Models: reload the model list from the global config file.
    /// Agent & Approval: re-read `[remote_agents.<name>]` from the global
    /// config file and refresh the page's rows.
    /// Agent & Approval: open a conversation that sets Remote up by asking for
    /// what is missing and writing it through `goble-cli setup`.
    pub on_setup_remote_with_agent: Rc<RefCell<dyn FnMut()>>,
    /// Agent & Approval: the same conversation for the hub.
    pub on_setup_hub_with_agent: Rc<RefCell<dyn FnMut()>>,
    /// Open Settings on Agent & Approval, with the keyboard on the Remote
    /// form. Every chooser that names Remote before anything is configured
    /// takes this path.
    pub on_open_remote_settings: Rc<RefCell<dyn FnMut()>>,
    /// Connections: re-read `~/.ssh` and replace the cached read.
    /// Updates: ask the release host now, off the UI thread.
    /// Updates: download, verify, install and restart with the offered release.
    /// Connections: select a connection by alias (the row's own click). It
    /// names a target and connects nothing.
    pub on_projects: Rc<RefCell<dyn FnMut()>>,
    /// Navigate to the harness workflows page (topbar button).
    pub on_workflows: Rc<RefCell<dyn FnMut()>>,
    /// Navigate to the harness tasks/executions page (topbar button).
    pub on_executions: Rc<RefCell<dyn FnMut()>>,
    /// Navigate to the harness timeline page (topbar button).
    pub on_timeline: Rc<RefCell<dyn FnMut()>>,
    /// Navigate to the harness costs page (topbar button).
    pub on_costs: Rc<RefCell<dyn FnMut()>>,
    /// Navigate to the MCP/connectors page (topbar button).
    pub on_mcps: Rc<RefCell<dyn FnMut()>>,
    pub on_plugins: Rc<RefCell<dyn FnMut()>>,
    pub on_open_crons: Rc<RefCell<dyn FnMut()>>,
    pub on_close_crons: Rc<RefCell<dyn FnMut()>>,
    /// Toggle the workflow-runs view in the active pane (a pane surface, not an
    /// overlay): open the run list, or close the view already there.
    pub on_toggle_workflow_view: Rc<RefCell<dyn FnMut()>>,
    /// Toggle the keyboard shortcuts panel (Ctrl+.).
    pub on_toggle_shortcuts_help: Rc<RefCell<dyn FnMut()>>,
    /// Close the keyboard shortcuts panel (its ✕, its backdrop, Escape).
    pub on_close_shortcuts_help: Rc<RefCell<dyn FnMut()>>,
    /// Append a printable character to the shortcuts panel's filter.
    pub on_shortcuts_help_type: Rc<RefCell<dyn FnMut(char)>>,
    /// Delete the filter's last character.
    pub on_shortcuts_help_backspace: Rc<RefCell<dyn FnMut()>>,
    /// Empty the panel's filter (Ctrl/Cmd+/, the footer's chord).
    pub on_shortcuts_help_clear_filter: Rc<RefCell<dyn FnMut()>>,
    /// Escape inside the panel: empty a non-empty filter first, and only close
    /// the panel once the filter is already empty.
    pub on_shortcuts_help_escape: Rc<RefCell<dyn FnMut()>>,
    /// Step the panel's highlighted row by this many rows, clamped to the rows
    /// the filter leaves.
    pub on_shortcuts_help_move: Rc<RefCell<dyn FnMut(i32)>>,
    pub on_toggle_right_sidebar: Rc<RefCell<dyn FnMut()>>,
    /// Toggle the agent/window fullscreen (borderless). Flips app state and
    /// requests the platform window to enter/leave fullscreen.
    pub on_toggle_fullscreen: Rc<RefCell<dyn FnMut()>>,
    /// Expand `pane_id` over the whole panes space, or put it back in its own
    /// place when it is already the expanded one.
    pub on_toggle_pane_maximized: Rc<RefCell<dyn FnMut(u64)>>,
    pub on_cron_create: Rc<RefCell<dyn FnMut()>>,
    pub on_cron_delete: Rc<RefCell<dyn FnMut(String)>>,
    pub on_cron_trigger: Rc<RefCell<dyn FnMut(String)>>,
    /// Begin dragging the sidebar divider at the given pointer x.
    pub on_sidebar_drag_start: Rc<RefCell<dyn FnMut(f32)>>,
    /// Move the divider while dragging, given the pointer x.
    pub on_sidebar_drag_move: Rc<RefCell<dyn FnMut(f32)>>,
    /// Finish a drag; the divider settles at the last width.
    pub on_sidebar_drag_end: Rc<RefCell<dyn FnMut()>>,
    /// Begin dragging the shared main/side-panel boundary.
    pub on_side_split_drag_start: Rc<RefCell<dyn FnMut()>>,
    /// Move the shared boundary: the side panel's new share of the content
    /// width, already clamped by the layout element to `0.25..=0.75` and to
    /// the cards' minimum width.
    pub on_side_split_drag_move: Rc<RefCell<dyn FnMut(f32)>>,
    /// Finish the shared-boundary drag (mouse up anywhere).
    pub on_side_split_drag_end: Rc<RefCell<dyn FnMut()>>,
    /// Begin dragging a pane's content-lane invisible edge: the pane, the
    /// pointer x, the edge grabbed, the content width it started at and the
    /// pane's available column width. Only that pane's width is written.
    pub on_content_lane_drag_start: Rc<RefCell<dyn FnMut(u64, f32, LaneEdge, f32, f32)>>,
    /// Move the dragged lane edge, given the pointer x. The handler resolves
    /// the width through the lane's width axis and stores the preference.
    pub on_content_lane_drag_move: Rc<RefCell<dyn FnMut(f32)>>,
    /// Finish the lane drag; the preference settles at the last resolved width.
    pub on_content_lane_drag_end: Rc<RefCell<dyn FnMut()>>,
    /// Split the active pane side-by-side (Ctrl/Cmd+Space).
    pub on_split_right: Rc<RefCell<dyn FnMut()>>,
    /// Split the active pane downward (Cmd+Shift+D).
    pub on_split_down: Rc<RefCell<dyn FnMut()>>,
    /// Cycle the active space's layout: BSP → master-stack → floating →
    /// carousel (Cmd+Shift+L and the palette).
    pub on_cycle_layout: Rc<RefCell<dyn FnMut()>>,
    /// Start a floating pane drag: the pane, the mode, the pointer where the
    /// press landed and the space's pane area in pixels.
    pub on_float_start: FloatStartHandler,
    /// Move or resize a floating pane after a drag.
    pub on_float_rect: Rc<RefCell<dyn FnMut(u64, PaneRect)>>,
    /// The floating drag ended; persist the rect it settled on.
    pub on_float_rect_end: Rc<RefCell<dyn FnMut()>>,
    /// Put a floating window in the dock (its titlebar's minimize control).
    pub on_float_minimize: Rc<RefCell<dyn FnMut(u64)>>,
    /// Bring a docked window back and raise it (a dock chip's click).
    pub on_float_restore: Rc<RefCell<dyn FnMut(u64)>>,
    /// Open the saved-host picker (Cmd+Shift+N).
    pub on_open_ssh_host_picker: Rc<RefCell<dyn FnMut()>>,
    /// The Workspaces "+" menu's SSH row: open the "Open SSH Host" overlay.
    pub on_open_ssh_target: Rc<RefCell<dyn FnMut()>>,
    pub on_close_ssh_target: Rc<RefCell<dyn FnMut()>>,
    pub on_ssh_target_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_ssh_target_focus: Rc<RefCell<dyn FnMut(bool)>>,
    pub on_ssh_choose_config: Rc<RefCell<dyn FnMut()>>,
    pub on_ssh_target_connect: Rc<RefCell<dyn FnMut()>>,
    pub on_ssh_pick_config_host: Rc<RefCell<dyn FnMut(usize)>>,
    /// Close the saved-host picker without opening anything.
    pub on_close_ssh_host_picker: Rc<RefCell<dyn FnMut()>>,
    /// Move the host picker's selection.
    pub on_ssh_host_move: Rc<RefCell<dyn FnMut(i32)>>,
    /// Open the saved host at this row in a new terminal pane.
    pub on_ssh_host_pick: Rc<RefCell<dyn FnMut(usize)>>,
    /// A line submitted at a shell's own prompt (typed in the grid), so an
    /// `ssh` line binds the pane and `exit` offers the saved host again.
    pub on_shell_line: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Reconnect the pane whose SSH session just ended to the saved host.
    pub on_ssh_reconnect: Rc<RefCell<dyn FnMut()>>,
    /// Dismiss the reconnect offer.
    pub on_ssh_reconnect_dismiss: Rc<RefCell<dyn FnMut()>>,
    /// Close the active pane, focusing the surviving sibling. A file view with
    /// unsaved edits asks first unless auto-save on close is on.
    pub on_close_pane: Rc<RefCell<dyn FnMut()>>,
    /// Close dialog: save the pending file view, then close it.
    pub on_close_unsaved_save: Rc<RefCell<dyn FnMut()>>,
    /// Close dialog: close the pending file view and drop its edits.
    pub on_close_unsaved_discard: Rc<RefCell<dyn FnMut()>>,
    /// Close dialog: keep the pending file view open.
    pub on_close_unsaved_cancel: Rc<RefCell<dyn FnMut()>>,
    /// Create a new terminal pane by splitting the active pane downward.
    pub on_new_terminal: Rc<RefCell<dyn FnMut()>>,
    /// Route a terminal pane's current input to the agent turn path (Cmd+Enter),
    /// using that pane's own conversation + cwd + medium/project scope.
    pub on_terminal_command: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Run a line as a shell command in `pane_id`'s own PTY. This is the
    /// terminal pane's rich input: a draft there is a command, the way the
    /// agent composer's `!cmd` is (warp-new's input model, shell side).
    pub on_run_shell_command: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Launch a real TUI agent (codex/claude/...) inside a pane's PTY, switching
    /// that pane to native-first agent mode. Callable from any surface; the
    /// pane must already be (or become) a terminal pane.
    pub on_launch_tui_agent: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Switch the active space by index.
    pub on_select_space: Rc<RefCell<dyn FnMut(usize)>>,
    /// Move the active space `delta` tabs along the strip (Ctrl+Tab /
    /// Ctrl+Shift+Tab), wrapping at both ends.
    pub on_switch_space: Rc<RefCell<dyn FnMut(i32)>>,
    /// Add a new space (single chat pane) and make it active.
    pub on_add_space: Rc<RefCell<dyn FnMut()>>,
    /// Close the space (tab) at `index`. The last remaining space is never
    /// removed: it resets to a fresh empty chat pane, so closing a tab never
    /// exits the program.
    pub on_close_space: Rc<RefCell<dyn FnMut(usize)>>,
    /// Add a new space whose default environment medium is `medium_id`, making
    /// it active and setting the new thread's workspace routing to that medium.
    pub on_add_space_with_medium: Rc<RefCell<dyn FnMut(String)>>,
    /// Add a new space holding an agent pane — the surface a fresh conversation
    /// starts on — in the environment "+" would use.
    pub on_add_agent_space: Rc<RefCell<dyn FnMut()>>,
    /// Open the cloud agent surface: a fresh conversation on the Remote
    /// (worker) environment, whose pane starts as a viewer and whose landing
    /// invites the first prompt. The environment menu's "Cloud agent" row and
    /// its Remote row both open it.
    pub on_new_cloud_agent: Rc<RefCell<dyn FnMut()>>,
    /// Open the "add a new medium" dialog (from the environment menu's
    /// "New environment…" row).
    pub on_open_add_medium: Rc<RefCell<dyn FnMut()>>,
    /// Update the add-medium dialog's text field.
    pub on_add_medium_draft_change: Rc<RefCell<dyn FnMut(String)>>,
    /// Track whether the add-medium dialog's text field is focused.
    pub on_add_medium_focus_change: Rc<RefCell<dyn FnMut(bool)>>,
    /// Close the add-medium dialog without saving.
    pub on_add_medium_close: Rc<RefCell<dyn FnMut()>>,
    /// Pointer pressed down on a space tab (begins a potential drag), with the
    /// point it landed on.
    pub on_space_press: Rc<RefCell<dyn FnMut(usize, Vector2F)>>,
    /// Pointer moved over the space bar (updates the hovered tab).
    pub on_space_hover: Rc<RefCell<dyn FnMut(Option<usize>)>>,
    /// Drag a space tab to a new position (reorder in the tab list).
    pub on_space_reorder: Rc<RefCell<dyn FnMut(usize, usize)>>,
    /// Pointer released (ends a press/drag and clears the drag state).
    pub on_space_release: Rc<RefCell<dyn FnMut()>>,
    /// Focus a specific leaf pane (e.g. by clicking its header).
    pub on_pane_activate: Rc<RefCell<dyn FnMut(u64)>>,
    /// Move focus to the pane adjacent to the active one (Ctrl/Cmd+Arrow).
    pub on_pane_navigate: Rc<RefCell<dyn FnMut(NavDir)>>,
    /// Begin dragging the split divider for pane `id`.
    pub on_pane_drag_start: Rc<RefCell<dyn FnMut(u64)>>,
    /// Drag the split divider for pane `id` to the given ratio.
    pub on_pane_drag_move: Rc<RefCell<dyn FnMut(u64, f32)>>,
    /// Finish dragging a split divider.
    pub on_pane_drag_end: Rc<RefCell<dyn FnMut()>>,
    /// Lift a pane out of its grid: its header was pressed at this position,
    /// and until the button comes up the pointer carries it toward the tab
    /// strip, where a release gives it a tab of its own.
    pub on_pane_lift: Rc<RefCell<dyn FnMut(u64, Vector2F)>>,
    /// Track the pointer while a pane is lifted, with the tab index the drop
    /// would land at (`None` while the pointer is off the tab strip) and the
    /// pane it is over in the pane area (`None` over no other pane).
    pub on_pane_lift_move: PaneLiftMoveHandler,
    /// Release a lifted pane: `Some(index)` gives it a tab of its own at that
    /// insertion point; `None` with a pane target moves it beside that pane;
    /// neither — a release over no target — cancels the drag with no change to
    /// the workspace.
    pub on_pane_drop: Rc<RefCell<dyn FnMut(Option<usize>, Option<PaneDropTarget>)>>,
    /// Delete a conversation/agent card from the list.
    pub on_agent_delete: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: return to the previous view (chat).
    pub on_settings_back: Rc<RefCell<dyn FnMut()>>,
    /// Settings: switch the active settings page.
    /// Settings: toggle dark mode.
    pub on_toggle_dark_mode: Rc<RefCell<dyn FnMut(bool)>>,
    /// Settings: pick which theme channel the single color wheel edits.
    /// Settings: set the custom theme primary color (as `#rrggbb`).
    pub on_set_theme_primary: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: set the custom theme secondary color (as `#rrggbb`).
    pub on_set_theme_secondary: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: set the custom theme accent color (as `#rrggbb`).
    pub on_set_theme_accent: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: set the custom theme background color (as `#rrggbb`).
    pub on_set_theme_bg: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: set the custom theme surface color (as `#rrggbb`).
    pub on_set_theme_surface: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: pick a preset palette, or `None` for the Default base.
    pub on_set_theme_preset: Rc<RefCell<dyn FnMut(Option<goble_ui::theme::ThemePreset>)>>,
    /// Settings: step the Themes stepper by `delta` bases.
    /// Settings: clear all five colour overrides, keeping the current base.
    /// Settings: save the profile (name, email).
    /// Settings: save the LLM config (provider, model, api_key, base_url, temperature).
    pub on_save_llm: SaveLlmHandler,
    /// Settings: register a worker (name, url).
    pub on_add_worker: Rc<RefCell<dyn FnMut(String, String)>>,
    /// Settings: remove a worker by id.
    pub on_remove_worker: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: unlock the vault with a passphrase.
    /// Settings: create a cluster identity (name, passphrase).
    /// Settings: unlock the cluster identity (passphrase).
    /// Settings: add an authorized key (name, pem, fingerprint).
    /// Settings: remove an authorized key by id.
    /// Open an external URL clicked in the transcript. The app guards it to
    /// `http`/`https` before any OS opener sees it.
    pub on_open_url: Rc<RefCell<dyn FnMut(String)>>,
    /// First-run: open Settings->LLM to configure a model key.
    ///
    /// No surface calls this any more: the notice band's "Edit API keys" opens
    /// the home's `config.toml` in a file pane ([`Self::on_open_config_file`])
    /// instead, because that file is where the keys and the models live. The
    /// action is kept whole — the model form is still the flow's route, and its
    /// own tests drive it — but it has no opener in the tree until one is given
    /// a reason to carry it.
    pub on_config_llm_key: Rc<RefCell<dyn FnMut()>>,
    /// Open `~/.goble/config.toml` — where the API keys and the models live — in
    /// a file pane beside the active pane.
    pub on_open_config_file: Rc<RefCell<dyn FnMut()>>,
    /// Show one of the files a file pane keeps open, named by its chip in the
    /// pane's own topbar.
    pub on_file_chip_select: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Close one file's chip, asking first when it has unsaved edits.
    pub on_file_chip_close: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// First-run: choose the workspace routing (Local or Remote).
    pub on_choose_workspace: Rc<RefCell<dyn FnMut(WorkspaceRouting)>>,
    /// First-run: close the model-provider dialog without saving.
    pub on_close_llm_dialog: Rc<RefCell<dyn FnMut()>>,
    /// First-run: dismiss the model-key banner without configuring a key,
    /// marking onboarding complete so the flow is not a dead end.
    pub on_dismiss_llm_key_banner: Rc<RefCell<dyn FnMut()>>,
    /// First-run: dismiss the workspace choice without deciding, marking
    /// onboarding complete.
    pub on_dismiss_workspace_choice: Rc<RefCell<dyn FnMut()>>,
    /// First-run: dismiss the compact getting-started tip.
    pub on_dismiss_onboarding_tip: Rc<RefCell<dyn FnMut()>>,
    /// Toggle the Cmd+K command palette overlay.
    pub on_toggle_command_palette: Rc<RefCell<dyn FnMut()>>,
    /// Close the Cmd+K command palette overlay.
    pub on_close_command_palette: Rc<RefCell<dyn FnMut()>>,
    /// Update the command palette's filter text, resetting the selection.
    pub on_command_palette_change: Rc<RefCell<dyn FnMut(String)>>,
    /// Set the command palette's selected index (clamped by the palette, which
    /// knows the filtered list length).
    pub on_command_palette_move: Rc<RefCell<dyn FnMut(usize)>>,
}

/// Callbacks supplied by the host app for the AI domain (vault + connectors).
pub struct AiActions {
    pub on_open_connectors: Rc<RefCell<dyn FnMut()>>,
    pub on_close_connectors: Rc<RefCell<dyn FnMut()>>,
    pub on_open_vault: Rc<RefCell<dyn FnMut()>>,
    pub on_close_vault: Rc<RefCell<dyn FnMut()>>,
    pub on_vault_unlock_draft_change: Rc<RefCell<dyn FnMut(String)>>,
    /// Unlock the vault with the passphrase the draft holds (the vault sheet's
    /// button).
    pub on_vault_unlock: Rc<RefCell<dyn FnMut()>>,
    pub on_vault_new_key_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_vault_new_value_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_vault_secret_add: Rc<RefCell<dyn FnMut()>>,
    pub on_vault_secret_delete: Rc<RefCell<dyn FnMut(String)>>,
    pub on_connector_search_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_open: Rc<RefCell<dyn FnMut()>>,
    pub on_install_edit: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_pick: ThreeStringHandler,
    pub on_install_close: Rc<RefCell<dyn FnMut()>>,
    pub on_install_name_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_source_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_source_value_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_search_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_secret_toggle: Rc<RefCell<dyn FnMut(String, bool)>>,
    pub on_install_submit: Rc<RefCell<dyn FnMut()>>,
    pub on_connector_delete: Rc<RefCell<dyn FnMut(String)>>,
    pub on_connector_discover: Rc<RefCell<dyn FnMut(String)>>,
    pub on_connector_toggle: Rc<RefCell<dyn FnMut(String, bool)>>,
}

/// Callbacks supplied by the host app for the projects panel.
pub struct ProjectsActions {
    /// Reload sessions/per-project status from the backend.
    pub on_refresh: Rc<RefCell<dyn FnMut()>>,
}

/// Callbacks supplied by the host app for the environment selector.
pub struct MediaActions {
    /// Expand or collapse a branch node (medium or project) by its key.
    pub on_toggle: Rc<RefCell<dyn FnMut(String)>>,
    /// Select a session leaf: `(medium_id, project_id, session_id)`. Sets the
    /// active medium+project+session for the next turn.
    pub on_select_session: ThreeStringHandler,
    /// Select a work environment (medium) by id; resets the project to that
    /// medium's default and clears the session.
    pub on_select_medium: Rc<RefCell<dyn FnMut(String)>>,
    /// Add a custom environment medium by label (the id is derived from the
    /// label). Selects the new medium, persists it so it reappears, and closes
    /// the add-medium dialog.
    pub on_add_medium: Rc<RefCell<dyn FnMut(String)>>,
}
