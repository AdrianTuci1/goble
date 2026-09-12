use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::SettingsPage;

use super::color_picker;
use super::pane::NavDir;
use super::types::{AppTab, SettingsCategory, WorkspaceRouting};

/// Callbacks supplied by the host app for the main view. Created fresh on
/// every rebuild; they mutate app-owned state, which is rendered back on the
/// next frame.
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
    pub on_send_message: Rc<RefCell<dyn FnMut(String)>>,
    /// Cmd/Ctrl+Enter in a chat composer: send the draft to the agent as a NEW
    /// agent conversation (warp-new behavior), binding the pane to a fresh
    /// conversation before the turn.
    pub on_cmd_enter: Rc<RefCell<dyn FnMut(String)>>,
    /// Fire when the composer draft starts with `/` (slash command): the host
    /// opens the command palette.
    pub on_composer_slash: Rc<RefCell<dyn FnMut()>>,
    pub on_attach: Rc<RefCell<dyn FnMut()>>,
    pub on_voice: Rc<RefCell<dyn FnMut()>>,
    pub on_select_model: Rc<RefCell<dyn FnMut()>>,
    /// Select a specific model from one pane's composer dropdown, by display
    /// name. The pane id keeps the choice local to that pane.
    pub on_model_select: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Select the composer harness (environment medium) by medium id.
    pub on_select_harness: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Select a pane's composer working directory (session) by session id.
    pub on_select_dir: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Select a pane's composer git branch by branch name.
    pub on_select_branch: Rc<RefCell<dyn FnMut(u64, String)>>,
    pub on_copy: Rc<RefCell<dyn FnMut()>>,
    /// Copy a terminal block's text to the clipboard (receives the block text).
    pub on_copy_terminal: Rc<RefCell<dyn FnMut(String)>>,
    pub on_restart: Rc<RefCell<dyn FnMut()>>,
    /// Rename the current agent/conversation (agent-header 3-dots menu).
    pub on_rename_agent: Rc<RefCell<dyn FnMut()>>,
    /// Clear the active pane's transcript (agent-header 3-dots menu).
    pub on_clear_transcript: Rc<RefCell<dyn FnMut()>>,
    pub on_stop: Rc<RefCell<dyn FnMut()>>,
    /// Enter a sub-agent child's own conversation in `pane_id`, by the child's
    /// conversation id — the row's click in the parent transcript (S5). The
    /// child view (S6) puts the child's card in the pane's block list and shows
    /// the child's transcript in its place.
    pub on_open_sub_agent: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Leave the child view `pane_id` shows and put the pane back on the
    /// conversation it came from; the child's card stays in the block list.
    pub on_close_sub_agent: Rc<RefCell<dyn FnMut(u64)>>,
    /// Submit the composed answer of the inline ask-user card. The optional
    /// `(name, value)` carries a credential entered in the card, threaded out
    /// separately from the answer so the secret never enters the transcript.
    pub on_answer_ask: Rc<RefCell<dyn FnMut(String, Option<(String, String)>)>>,
    /// Skip the inline ask-user card.
    pub on_skip_ask: Rc<RefCell<dyn FnMut()>>,
    /// Answer a suspended command proposal from `pane_id`: `(pane, proposal id,
    /// decision)` resumes the turn (approve/edit runs it, reject fails it).
    pub on_command_decision:
        Rc<RefCell<dyn FnMut(u64, String, goble_core::harness::CommandDecision)>>,
    /// Toggle one pane's auto-approve (autonomy) switch above its composer.
    pub on_toggle_auto_approve: Rc<RefCell<dyn FnMut(u64, bool)>>,
    /// Turn modal (vim) editing on or off for the rich input. The choice is
    /// persisted; turning it off returns every pane's editor to insert mode.
    pub on_toggle_vim_mode: Rc<RefCell<dyn FnMut(bool)>>,
    /// Fork `pane_id`'s conversation into a new one that inherits its transcript
    /// (the transcript footer's Fork).
    pub on_fork_conversation: Rc<RefCell<dyn FnMut(u64)>>,
    /// The system clipboard the vim registers `"+`/`"*` read and write.
    pub clipboard: Rc<RefCell<dyn goble_ui::vim::Clipboard>>,
    /// Turn one pane's harness mode on/off at the rich input (Warp-new style:
    /// Cmd+Enter activates the harness, Esc returns to the plain pty).
    pub on_set_pane_harness_mode: Rc<RefCell<dyn FnMut(u64, bool)>>,
    /// Enter a conversation's agent view in a pane by clicking the card it left
    /// behind in the terminal (Warp-new style: the `EnterAgentView` block).
    pub on_open_agent_view: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Send a queued prompt now (interrupting the in-flight turn).
    pub on_send_queued: Rc<RefCell<dyn FnMut()>>,
    /// Dismiss a queued prompt.
    pub on_dismiss_queued: Rc<RefCell<dyn FnMut()>>,
    pub on_menu: Rc<RefCell<dyn FnMut()>>,
    pub on_inbox: Rc<RefCell<dyn FnMut()>>,
    pub on_settings: Rc<RefCell<dyn FnMut()>>,
    /// Toggle the left conversation sidebar (topbar sidebar button).
    pub on_toggle_sidebar: Rc<RefCell<dyn FnMut()>>,
    /// Toggle the sidebar conversation list between "a few cards" and the full,
    /// scrollable list.
    pub on_toggle_conversations_expanded: Rc<RefCell<dyn FnMut()>>,
    /// Click on a topbar workspace chip: selects that space, and a double-click
    /// enters inline rename for its name.
    pub on_workspace_click: Rc<RefCell<dyn FnMut(usize)>>,
    /// Update the inline space-rename draft.
    pub on_space_rename_change: Rc<RefCell<dyn FnMut(String)>>,
    /// Track focus of the inline space-rename field (blur commits).
    pub on_space_rename_focus: Rc<RefCell<dyn FnMut(bool)>>,
    /// Commit the inline space rename.
    pub on_space_rename_commit: Rc<RefCell<dyn FnMut()>>,
    /// Cancel the inline space rename without changing the name.
    pub on_space_rename_cancel: Rc<RefCell<dyn FnMut()>>,
    /// Settings: close the Settings overlay.
    pub on_settings_close: Rc<RefCell<dyn FnMut()>>,
    /// Settings: switch the active settings category.
    pub on_settings_category: Rc<RefCell<dyn FnMut(SettingsCategory)>>,
    /// Settings: step the active category by `delta` (with wraparound). The
    /// arrow-key path, which must advance from the live category rather than
    /// from the one captured when the tree was built.
    pub on_settings_category_step: Rc<RefCell<dyn FnMut(i32)>>,
    /// Settings: toggle mouse scroll-direction inversion.
    pub on_toggle_invert_scroll: Rc<RefCell<dyn FnMut(bool)>>,
    /// Settings: set mouse scroll speed (1..=100).
    pub on_set_scroll_speed: Rc<RefCell<dyn FnMut(i32)>>,
    /// Settings: set editor font-size zoom factor.
    pub on_set_font_size: Rc<RefCell<dyn FnMut(f32)>>,
    /// Models: reload the model list from the global config file.
    pub on_reload_model_config: Rc<RefCell<dyn FnMut()>>,
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
    pub on_toggle_right_sidebar: Rc<RefCell<dyn FnMut()>>,
    /// Toggle the agent/window fullscreen (borderless). Flips app state and
    /// requests the platform window to enter/leave fullscreen.
    pub on_toggle_fullscreen: Rc<RefCell<dyn FnMut()>>,
    pub on_cron_create: Rc<RefCell<dyn FnMut()>>,
    pub on_cron_delete: Rc<RefCell<dyn FnMut(String)>>,
    pub on_cron_trigger: Rc<RefCell<dyn FnMut(String)>>,
    /// Begin dragging the sidebar divider at the given pointer x.
    pub on_sidebar_drag_start: Rc<RefCell<dyn FnMut(f32)>>,
    /// Move the divider while dragging, given the pointer x.
    pub on_sidebar_drag_move: Rc<RefCell<dyn FnMut(f32)>>,
    /// Finish a drag; the divider settles at the last width.
    pub on_sidebar_drag_end: Rc<RefCell<dyn FnMut()>>,
    /// Split the active pane side-by-side (Ctrl/Cmd+Space).
    pub on_split_right: Rc<RefCell<dyn FnMut()>>,
    /// Split the active pane downward (Cmd+Shift+D).
    pub on_split_down: Rc<RefCell<dyn FnMut()>>,
    /// Close the active pane, focusing the surviving sibling.
    pub on_close_pane: Rc<RefCell<dyn FnMut()>>,
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
    /// Add a new space (single chat pane) and make it active.
    pub on_add_space: Rc<RefCell<dyn FnMut()>>,
    /// Close the space (tab) at `index`. The last remaining space is never
    /// removed: it resets to a fresh empty chat pane, so closing a tab never
    /// exits the program.
    pub on_close_space: Rc<RefCell<dyn FnMut(usize)>>,
    /// Add a new space whose default environment medium is `medium_id`, making
    /// it active and setting the new thread's workspace routing to that medium.
    pub on_add_space_with_medium: Rc<RefCell<dyn FnMut(String)>>,
    /// Open the "add a new medium" dialog (from the topbar "+" menu).
    pub on_open_add_medium: Rc<RefCell<dyn FnMut()>>,
    /// Update the add-medium dialog's text field.
    pub on_add_medium_draft_change: Rc<RefCell<dyn FnMut(String)>>,
    /// Track whether the add-medium dialog's text field is focused.
    pub on_add_medium_focus_change: Rc<RefCell<dyn FnMut(bool)>>,
    /// Close the add-medium dialog without saving.
    pub on_add_medium_close: Rc<RefCell<dyn FnMut()>>,
    /// Pointer pressed down on a space tab (begins a potential drag).
    pub on_space_press: Rc<RefCell<dyn FnMut(usize)>>,
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
    /// Delete a conversation/agent card from the list.
    pub on_agent_delete: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: return to the previous view (chat).
    pub on_settings_back: Rc<RefCell<dyn FnMut()>>,
    /// Settings: switch the active settings page.
    pub on_settings_navigate: Rc<RefCell<dyn FnMut(SettingsPage)>>,
    /// Settings: toggle dark mode.
    pub on_toggle_dark_mode: Rc<RefCell<dyn FnMut(bool)>>,
    /// Settings: pick which theme channel the single color wheel edits.
    pub on_set_theme_target: Rc<RefCell<dyn FnMut(color_picker::ColorTarget)>>,
    /// Settings: set the custom theme primary color (as `#rrggbb`).
    pub on_set_theme_primary: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: set the custom theme secondary color (as `#rrggbb`).
    pub on_set_theme_secondary: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: set the custom theme accent color (as `#rrggbb`).
    pub on_set_theme_accent: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: save the profile (name, email).
    pub on_save_profile: Rc<RefCell<dyn FnMut(String, String)>>,
    /// Settings: save the LLM config (provider, model, api_key, base_url, temperature).
    pub on_save_llm: Rc<RefCell<dyn FnMut(String, String, String, String, String)>>,
    /// Settings: register a worker (name, url).
    pub on_add_worker: Rc<RefCell<dyn FnMut(String, String)>>,
    /// Settings: remove a worker by id.
    pub on_remove_worker: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: unlock the vault with a passphrase.
    pub on_vault_unlock: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: create a cluster identity (name, passphrase).
    pub on_create_cluster: Rc<RefCell<dyn FnMut(String, String)>>,
    /// Settings: unlock the cluster identity (passphrase).
    pub on_unlock_cluster: Rc<RefCell<dyn FnMut(String)>>,
    /// Settings: add an authorized key (name, pem, fingerprint).
    pub on_add_authorized_key: Rc<RefCell<dyn FnMut(String, String, String)>>,
    /// Settings: remove an authorized key by id.
    pub on_remove_authorized_key: Rc<RefCell<dyn FnMut(String)>>,
    /// Close the inline remote-desktop block the harness handed off to.
    pub on_close_inline_screen: Rc<RefCell<dyn FnMut()>>,
    /// Open a remote desktop from a detected BYOH handoff URI.
    pub on_open_screen_link: Rc<RefCell<dyn FnMut(String)>>,
    /// Open an external URL clicked in the transcript. The app guards it to
    /// `http`/`https` before any OS opener sees it.
    pub on_open_url: Rc<RefCell<dyn FnMut(String)>>,
    /// First-run: open Settings->LLM to configure a model key (banner click).
    pub on_config_llm_key: Rc<RefCell<dyn FnMut()>>,
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
    pub on_vault_unlock: Rc<RefCell<dyn FnMut()>>,
    pub on_vault_new_key_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_vault_new_value_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_vault_secret_add: Rc<RefCell<dyn FnMut()>>,
    pub on_vault_secret_delete: Rc<RefCell<dyn FnMut(String)>>,
    pub on_connector_search_change: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_open: Rc<RefCell<dyn FnMut()>>,
    pub on_install_edit: Rc<RefCell<dyn FnMut(String)>>,
    pub on_install_pick: Rc<RefCell<dyn FnMut(String, String, String)>>,
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
    pub on_select_session: Rc<RefCell<dyn FnMut(String, String, String)>>,
    /// Select a work environment (medium) by id; resets the project to that
    /// medium's default and clears the session.
    pub on_select_medium: Rc<RefCell<dyn FnMut(String)>>,
    /// Add a custom environment medium by label (the id is derived from the
    /// label). Selects the new medium, persists it so it reappears, and closes
    /// the add-medium dialog.
    pub on_add_medium: Rc<RefCell<dyn FnMut(String)>>,
}

/// Callbacks supplied by the host app for the screen panel.
pub struct ScreenActions {
    /// Open the screen sheet.
    pub on_open: Rc<RefCell<dyn FnMut()>>,
    /// Close the screen sheet.
    pub on_close: Rc<RefCell<dyn FnMut()>>,
    /// Toggle live capture (broadcast) of the selected source.
    pub on_toggle_broadcast: Rc<RefCell<dyn FnMut(bool)>>,
    /// Toggle computer-use (click/type/scroll) input.
    pub on_toggle_computer_use: Rc<RefCell<dyn FnMut(bool)>>,
    /// Select a source by its id.
    pub on_select_source: Rc<RefCell<dyn FnMut(String)>>,
    /// Reload the capturable/controllable source list from the registry.
    pub on_refresh_sources: Rc<RefCell<dyn FnMut()>>,
    /// Start recording screen observation + input events.
    pub on_record_start: Rc<RefCell<dyn FnMut()>>,
    /// Stop recording (the events remain available for replay).
    pub on_record_stop: Rc<RefCell<dyn FnMut()>>,
    /// Replay the recorded events once, scheduling them by their timing.
    pub on_replay_once: Rc<RefCell<dyn FnMut()>>,
    /// Replay the recorded events in a loop.
    pub on_replay_loop: Rc<RefCell<dyn FnMut()>>,
    /// Stop a running replay.
    pub on_replay_stop: Rc<RefCell<dyn FnMut()>>,
    /// Discard the recorded events.
    pub on_clear_recording: Rc<RefCell<dyn FnMut()>>,
    /// A pointer click at source pixel `(x, y)` on the selected source.
    pub on_screen_click: Rc<RefCell<dyn FnMut(u32, u32)>>,
    /// Typed text forwarded to the selected source's focused field.
    pub on_screen_type: Rc<RefCell<dyn FnMut(String)>>,
    /// A scroll of `(dx, dy)` ticks on the selected source.
    pub on_screen_scroll: Rc<RefCell<dyn FnMut(i32, i32)>>,
}
