use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::agent::Trigger;
use goble_core::worker::WorkerId;
use goble_core::workflow::WorkflowId;
use goble_desktop_service::DesktopState;
use goble_terminal::blocks::BlockView;
use goble_ui::platform::WindowControl;
use goble_ui::geometry::Vector2F;
use goble_ui::{ChatMessage, ChatRole, SettingsPage};

use crate::media::MediaState;
use crate::state::{
    default_pane_path, routing_to_str, DiffGitResult, SpaceMenu, SpacePressRun, UiState,
    MULTI_CLICK_INTERVAL_MS, NEW_CONVERSATION_TITLE,
};
use crate::terminal::{classify_input, InputClass};
use crate::ui::{
    AppTab, CronEntry, DiffPanelAction, FloatingDrag, FloatingDragMode, LayoutKind, NavDir, Pane,
    PaneKind, SettingsCategory, SettingsControl, SidebarView, Space, SplitDir, TabMenuAction,
    UiActions, WorkspaceRouting,
};
use crate::ui::color_picker::ColorTarget;

use super::pane_ops::{
    close_space_at, ensure_pane_hover, open_diff_pane, open_file_pane, open_pane_harness,
    open_ssh_host_pane, reconnect_ssh_host, split_active_pane,
};
use super::prompt::{run_terminal_command, send_agent_prompt};
use crate::ui::pickers::{change_directory_command, checkout_branch_command};

/// Submit the prompt a running turn has queued, right now.
///
/// This is the second Enter on a composer whose first Enter queued a message.
/// A turn that is only busy is cancelled and the prompt starts fresh; a turn
/// suspended on a command approval is settled with a rejection first, and the
/// queued prompt goes out when that turn's `finished` event lands — starting a
/// second turn over the suspended one would race the harness session.
fn flush_queued_prompt(
    state: &mut UiState,
    desktop: Option<&Arc<DesktopState>>,
    media: &Rc<RefCell<MediaState>>,
) {
    let pane_id = state.active_pane_id;
    let model = if state.selected_model.trim().is_empty() {
        state.settings_llm_model.clone()
    } else {
        state.selected_model.clone()
    };
    // Nothing can run without a runtime and a conversation, so the prompt is
    // only taken once both are known: an empty submit must not eat the queue.
    let Some(desktop) = desktop else {
        return;
    };
    let Some(chat_id) = state.pane_conversation_id(pane_id) else {
        return;
    };
    let Some(prompt) = state
        .pane_runtime
        .get_mut(&pane_id)
        .and_then(|rt| rt.queued_prompt.take())
    else {
        return;
    };
    state.queued_prompt = None;

    // A command the harness is suspended on has no running future to cancel:
    // reject it, which resumes the turn, and let the finish event submit the
    // prompt through the queue.
    let pending = state
        .pane_runtime
        .get(&pane_id)
        .and_then(|rt| rt.pending_command.clone());
    if let Some(proposal) = pending {
        let decision = goble_core::harness::CommandDecision::Reject(
            "superseded by a new message".to_string(),
        );
        match desktop.resume_command_chat_turn(&chat_id, decision) {
            Ok(_) => {
                state.resume_command_decision(pane_id, &proposal.id);
                state.agent_busy = true;
                if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                    rt.queued_prompt = Some(prompt.clone());
                }
                state.queued_prompt = Some(prompt);
            }
            Err(e) => {
                log::warn!("resume_command_chat_turn (send now) failed: {e}");
            }
        }
        state.refresh_messages(desktop);
        return;
    }

    let _ = desktop.cancel_chat_turn(&chat_id);
    let (medium_id, project_id, session_id) = {
        let media = media.borrow();
        let session_id = if media.selected_session_id().is_empty() {
            chat_id.clone()
        } else {
            media.selected_session_id().to_string()
        };
        (
            media.selected_medium_id().to_string(),
            media.selected_project_id().to_string(),
            session_id,
        )
    };
    let pane_session = state.pane_session(pane_id, &chat_id);
    if let Err(e) = crate::runtime::run_turn(
        desktop,
        &chat_id,
        &prompt,
        &state.settings_llm_provider,
        &model,
        state.workspace_routing,
        &medium_id,
        &project_id,
        &session_id,
        &state.composer_path,
        Some(state.selected_harness.as_str()),
        pane_session,
    ) {
        log::warn!("run_chat_turn (queued) failed: {e}");
    } else {
        state.begin_turn(pane_id);
        state.agent_busy = true;
    }
    state.refresh_messages(desktop);
}

/// The body a "New cron" starts from: one phase and one agent call, so the row
/// it creates is runnable immediately instead of being an empty definition.
const NEW_CRON_SCRIPT: &str = r#"let meta = #{
    name: "New scheduled task",
    description: "Created from the crons drawer",
    phases: [#{ title: "Run" }]
};

phase("Run");
let result = agent("Carry out this scheduled task and report what you did.");
complete(#{ output: result.output });
"#;

pub fn make_actions(
    state: Rc<RefCell<UiState>>,
    desktop: Option<Arc<DesktopState>>,
    media: Rc<RefCell<MediaState>>,
    window_control: WindowControl,
    ui_zoom: Rc<RefCell<f32>>,
) -> UiActions {
    let on_search_change = Rc::clone(&state);
    let on_search_focus_change = Rc::clone(&state);
    let on_create_change = Rc::clone(&state);
    let on_create_focus_change = Rc::clone(&state);
    let on_create_submit = Rc::clone(&state);
    let on_toggle_sidebar = Rc::clone(&state);
    let on_diff_panel = Rc::clone(&state);
    let desktop_diff_panel = desktop.clone();
    let on_toggle_conversations_expanded = Rc::clone(&state);
    let on_select_sidebar_view = Rc::clone(&state);
    let on_toggle_section = Rc::clone(&state);
    let on_toggle_star = Rc::clone(&state);
    let on_toggle_explorer_dir = Rc::clone(&state);
    let on_explorer_file_click = Rc::clone(&state);
    let on_global_search_change = Rc::clone(&state);
    let on_global_search_focus_change = Rc::clone(&state);
    let on_workspace_click = Rc::clone(&state);
    let on_space_rename_change = Rc::clone(&state);
    let on_space_rename_focus = Rc::clone(&state);
    let on_space_rename_commit = Rc::clone(&state);
    let on_space_rename_cancel = Rc::clone(&state);
    let desktop_space_rename_focus = desktop.clone();
    let desktop_space_rename_commit = desktop.clone();
    let on_select_conversation = Rc::clone(&state);
    let on_select_tab = Rc::clone(&state);
    let on_composer_change = Rc::clone(&state);
    let on_slash_move = Rc::clone(&state);
    let on_slash_close = Rc::clone(&state);
    let on_slash_dismiss = Rc::clone(&state);
    let on_composer_focus_change = Rc::clone(&state);
    let on_send_message = Rc::clone(&state);
    let on_run_shell_command = Rc::clone(&state);
    let on_cmd_enter = Rc::clone(&state);
    let on_send_to_cloud = Rc::clone(&state);
    let on_attach = Rc::clone(&state);
    let on_voice = Rc::clone(&state);
    let on_model_select = Rc::clone(&state);
    let on_select_harness_state = Rc::clone(&state);
    let desktop_select_harness = desktop.clone();
    let on_select_dir = Rc::clone(&state);
    let on_select_medium = Rc::clone(&media);
    let on_create_medium = Rc::clone(&media);
    let on_select_branch = Rc::clone(&state);
    let on_stop = Rc::clone(&state);
    let on_open_sub_agent = Rc::clone(&state);
    let on_close_sub_agent = Rc::clone(&state);
    let desktop_open_sub_agent = desktop.clone();
    let on_answer_ask = Rc::clone(&state);
    let on_skip_ask = Rc::clone(&state);
    let on_command_decision = Rc::clone(&state);
    let on_toggle_auto_approve = Rc::clone(&state);
    let on_select_approval_mode_state = Rc::clone(&state);
    let on_toggle_vim_mode = Rc::clone(&state);
    let on_toggle_auto_save_on_close = Rc::clone(&state);
    let on_fork_conversation = Rc::clone(&state);
    let on_set_harness_mode = Rc::clone(&state);
    let on_open_agent_view = Rc::clone(&state);
    let on_send_queued = Rc::clone(&state);
    let on_dismiss_queued = Rc::clone(&state);
    let on_settings = Rc::clone(&state);
    let on_inbox = Rc::clone(&state);
    let on_close_inbox = Rc::clone(&state);
    let on_inbox_move = Rc::clone(&state);
    let on_inbox_ack = Rc::clone(&state);
    let on_inbox_ack_all = Rc::clone(&state);
    let on_inbox_jump = Rc::clone(&state);
    let on_settings_category = Rc::clone(&state);
    let on_settings_category_step = Rc::clone(&state);
    let on_settings_focus_move = Rc::clone(&state);
    let on_settings_focus_into_pane = Rc::clone(&state);
    let on_settings_focus_out_of_pane = Rc::clone(&state);
    let on_settings_activate = Rc::clone(&state);
    let on_settings_commit_field = Rc::clone(&state);
    let on_settings_cancel_field = Rc::clone(&state);
    let on_environment_group_draft_change = Rc::clone(&state);
    let on_environment_create_group = Rc::clone(&state);
    let on_environment_open_group = Rc::clone(&state);
    let on_environment_delete_group = Rc::clone(&state);
    let on_environment_secret_name_change = Rc::clone(&state);
    let on_environment_secret_value_change = Rc::clone(&state);
    let on_environment_save_secret = Rc::clone(&state);
    let on_environment_edit_secret = Rc::clone(&state);
    let on_environment_delete_secret = Rc::clone(&state);
    // The handle the Settings -> Environment rows persist through. One clone
    // per closure: several callbacks need it at once.
    let desktop_environment_create = desktop.clone();
    let desktop_environment_delete = desktop.clone();
    let desktop_environment_save = desktop.clone();
    let desktop_environment_secret_delete = desktop.clone();
    let on_set_theme_target = Rc::clone(&state);
    let on_toggle_invert_scroll = Rc::clone(&state);
    let on_set_scroll_speed = Rc::clone(&state);
    let on_set_font_size = Rc::clone(&state);
    let on_reload_model_config = Rc::clone(&state);
    let on_reload_remote_agents = Rc::clone(&state);
    let on_reload_ssh_hosts = Rc::clone(&state);
    let on_projects = Rc::clone(&state);
    let on_workflows = Rc::clone(&state);
    let on_executions = Rc::clone(&state);
    let on_timeline = Rc::clone(&state);
    let on_costs = Rc::clone(&state);
    let on_mcps = Rc::clone(&state);
    let on_open_crons = Rc::clone(&state);
    let on_close_crons = Rc::clone(&state);
    let on_toggle_task_workflow = Rc::clone(&state);
    let on_close_task_workflow = Rc::clone(&state);
    let on_toggle_shortcuts_help = Rc::clone(&state);
    let on_close_shortcuts_help = Rc::clone(&state);
    let on_shortcuts_help_type = Rc::clone(&state);
    let on_shortcuts_help_backspace = Rc::clone(&state);
    let on_shortcuts_help_clear_filter = Rc::clone(&state);
    let on_shortcuts_help_escape = Rc::clone(&state);
    let on_shortcuts_help_move = Rc::clone(&state);
    let on_toggle_right_sidebar = Rc::clone(&state);
    let on_toggle_fullscreen = Rc::clone(&state);
    let on_toggle_pane_maximized = Rc::clone(&state);
    let on_clear_transcript = Rc::clone(&state);
    let on_cron_create = Rc::clone(&state);
    let on_cron_delete = Rc::clone(&state);
    let on_cron_trigger = Rc::clone(&state);
    let on_sidebar_drag_start = Rc::clone(&state);
    let on_sidebar_drag_move = Rc::clone(&state);
    let on_sidebar_drag_end = Rc::clone(&state);
    let on_split_right = Rc::clone(&state);
    let on_split_down = Rc::clone(&state);
    let on_cycle_layout = Rc::clone(&state);
    let on_float_start = Rc::clone(&state);
    let on_float_rect = Rc::clone(&state);
    let on_float_rect_end = Rc::clone(&state);
    let on_float_minimize = Rc::clone(&state);
    let on_float_restore = Rc::clone(&state);
    let on_open_ssh_host_picker = Rc::clone(&state);
    let on_close_ssh_host_picker = Rc::clone(&state);
    let on_ssh_host_move = Rc::clone(&state);
    let on_ssh_host_pick = Rc::clone(&state);
    let on_ssh_reconnect = Rc::clone(&state);
    let on_ssh_reconnect_dismiss = Rc::clone(&state);
    let on_shell_line = Rc::clone(&state);
    let on_new_terminal = Rc::clone(&state);
    let on_close_pane = Rc::clone(&state);
    let on_close_unsaved_save = Rc::clone(&state);
    let on_close_unsaved_discard = Rc::clone(&state);
    let on_close_unsaved_cancel = Rc::clone(&state);
    let on_terminal_cmd = Rc::clone(&state);
    let on_launch_tui_agent = Rc::clone(&state);
    let on_select_space = Rc::clone(&state);
    let on_switch_space = Rc::clone(&state);
    let on_add_space = Rc::clone(&state);
    let on_add_space_with_medium = Rc::clone(&state);
    let on_open_add_medium = Rc::clone(&state);
    let on_add_medium_draft_change = Rc::clone(&state);
    let on_add_medium_focus_change = Rc::clone(&state);
    let on_add_medium_close = Rc::clone(&state);
    let on_space_press = Rc::clone(&state);
    let on_space_menu = Rc::clone(&state);
    let on_space_menu_close = Rc::clone(&state);
    let on_space_menu_action = Rc::clone(&state);
    let on_space_hover = Rc::clone(&state);
    let on_space_reorder = Rc::clone(&state);
    let on_space_release = Rc::clone(&state);
    let on_close_space = Rc::clone(&state);
    let on_pane_activate = Rc::clone(&state);
    let on_pane_navigate = Rc::clone(&state);
    let on_pane_drag_start = Rc::clone(&state);
    let on_pane_drag_move = Rc::clone(&state);
    let on_pane_drag_end = Rc::clone(&state);
    let on_pane_lift = Rc::clone(&state);
    let on_pane_lift_move = Rc::clone(&state);
    let on_pane_drop = Rc::clone(&state);
    let on_agent_delete = Rc::clone(&state);
    let on_settings_back = Rc::clone(&state);
    let on_settings_navigate = Rc::clone(&state);
    let on_toggle_dark_mode = Rc::clone(&state);
    let on_set_theme_primary = Rc::clone(&state);
    let on_set_theme_secondary = Rc::clone(&state);
    let on_set_theme_accent = Rc::clone(&state);
    let on_save_profile = Rc::clone(&state);
    let on_save_llm = Rc::clone(&state);
    let on_add_worker = Rc::clone(&state);
    let on_remove_worker = Rc::clone(&state);
    let on_vault_unlock = Rc::clone(&state);
    let on_create_cluster = Rc::clone(&state);
    let on_unlock_cluster = Rc::clone(&state);
    let on_add_authorized_key = Rc::clone(&state);
    let on_remove_authorized_key = Rc::clone(&state);
    let on_config_llm_key = Rc::clone(&state);
    let on_open_config_file = Rc::clone(&state);
    let on_choose_workspace = Rc::clone(&state);
    let on_open_remote_settings = Rc::clone(&state);
    let on_close_llm_dialog = Rc::clone(&state);
    let on_dismiss_llm_key_banner = Rc::clone(&state);
    let on_dismiss_workspace_choice = Rc::clone(&state);
    let on_dismiss_onboarding_tip = Rc::clone(&state);
    let on_toggle_command_palette = Rc::clone(&state);
    let on_close_command_palette = Rc::clone(&state);
    let on_command_palette_change = Rc::clone(&state);
    let on_command_palette_move = Rc::clone(&state);

    let desktop_create = desktop.clone();
    let desktop_select = desktop.clone();
    let desktop_send = desktop.clone();
    let desktop_cmd_enter = desktop.clone();
    let desktop_send_to_cloud = desktop.clone();
    let desktop_stop = desktop.clone();
    let desktop_answer = desktop.clone();
    let desktop_skip = desktop.clone();
    let desktop_command = desktop.clone();
    let desktop_auto = desktop.clone();
    let desktop_vim = desktop.clone();
    let desktop_auto_save = desktop.clone();
    let desktop_fork = desktop.clone();
    let desktop_set_harness = desktop.clone();
    let desktop_send_queued = desktop.clone();
    let desktop_cron_create = desktop.clone();
    let desktop_cron_delete = desktop.clone();
    let desktop_cron_trigger = desktop.clone();
    let desktop_save_llm = desktop.clone();
    let desktop_dark_mode = desktop.clone();
    let desktop_theme_primary = desktop.clone();
    let desktop_theme_secondary = desktop.clone();
    let desktop_theme_accent = desktop.clone();
    let desktop_reload_model = desktop.clone();
    let desktop_reload_remote_agents = desktop.clone();
    let ui_zoom_font = ui_zoom.clone();
    let desktop_add_worker = desktop.clone();
    let desktop_remove_worker = desktop.clone();
    let desktop_unlock_vault = desktop.clone();
    let desktop_create_cluster = desktop.clone();
    let desktop_unlock_cluster = desktop.clone();
    let desktop_choose_workspace = desktop.clone();
    let desktop_open_remote_settings = desktop.clone();
    let desktop_dismiss_key = desktop.clone();
    let desktop_dismiss_workspace = desktop.clone();
    let desktop_split_right = desktop.clone();
    let desktop_split_down = desktop.clone();
    let desktop_cycle_layout = desktop.clone();
    let desktop_float_rect_end = desktop.clone();
    let desktop_float_minimize = desktop.clone();
    let desktop_float_restore = desktop.clone();
    let desktop_ssh_host = desktop.clone();
    let desktop_ssh_reconnect = desktop.clone();
    let desktop_file_view = desktop.clone();
    let desktop_open_config = desktop.clone();
    // Opening `~/.goble/config.toml` is both a `UiActions` field and a settings
    // row, so it is built once and shared: the button, `Enter` and the notice
    // band all run this closure.
    let action_open_config_file: Rc<RefCell<dyn FnMut()>> = {
        let state = Rc::clone(&on_open_config_file);
        let desktop = desktop_open_config.clone();
        Rc::new(RefCell::new(move || {
            // The API keys and the models live in the home's `config.toml`, so
            // the notice band opens that file in a pane beside the pane that
            // noticed the missing key. The home is the one the app itself
            // reads: `GobleHome::locate()`, the same path `DesktopState` loads
            // at startup and the model dialog saves to.
            let Ok(home) = goble_core::app_home::GobleHome::locate() else {
                log::warn!("cannot locate the Goble home to open config.toml");
                return;
            };
            let mut state = state.borrow_mut();
            open_file_pane(
                &mut state,
                home.config_path().to_string_lossy().to_string(),
                desktop.as_ref(),
            );
        }))
    };
    // One chip in a file pane's topbar shows the file it names. The pane keeps
    // every file it opened, so this only moves the one it shows; the buffers
    // are keyed by file, so an unsaved edit on the one left behind survives.
    let on_file_chip_select = Rc::clone(&state);
    let desktop_file_chip = desktop.clone();
    let action_file_chip_select: Rc<RefCell<dyn FnMut(u64, String)>> = {
        let state = Rc::clone(&on_file_chip_select);
        let desktop = desktop_file_chip.clone();
        Rc::new(RefCell::new(move |pane_id: u64, path: String| {
            let mut state = state.borrow_mut();
            if state.show_file_in_pane(pane_id, path) {
                state.focus_pane(pane_id);
                state.sync_active_view();
                if let Some(desktop) = desktop.as_deref() {
                    state.save_panes(desktop);
                }
            }
        }))
    };
    // The chip's own "x" closes that one file. A dirty buffer asks first, and
    // the pane stays as long as it holds another file.
    let on_file_chip_close = Rc::clone(&state);
    let desktop_file_chip_close = desktop.clone();
    let action_file_chip_close: Rc<RefCell<dyn FnMut(u64, String)>> = {
        let state = Rc::clone(&on_file_chip_close);
        let desktop = desktop_file_chip_close.clone();
        Rc::new(RefCell::new(move |pane_id: u64, path: String| {
            state
                .borrow_mut()
                .request_close_file(pane_id, path, desktop.as_deref());
        }))
    };
    let desktop_new_terminal = desktop.clone();
    let desktop_close_pane = desktop.clone();
    let desktop_close_unsaved_save = desktop.clone();
    let desktop_close_unsaved_discard = desktop.clone();
    let desktop_term_cmd = desktop.clone();
    let desktop_select_space = desktop.clone();
    let desktop_settings = desktop.clone();
    let desktop_switch_space = desktop.clone();
    let desktop_close_space = desktop.clone();
    let desktop_add_space = desktop.clone();
    let desktop_add_space_with_medium = desktop.clone();
    let desktop_space_reorder = desktop.clone();
    let desktop_workspace_click = desktop.clone();
    let desktop_space_menu_action = desktop.clone();
    let desktop_pane_activate = desktop.clone();
    let desktop_pane_navigate = desktop.clone();
    let desktop_pane_drag = desktop.clone();
    let desktop_pane_drop = desktop.clone();
    let desktop_agent_delete = desktop.clone();
    let media_send = Rc::clone(&media);
    let media_cmd_enter = Rc::clone(&media);
    let media_send_to_cloud = Rc::clone(&media);
    let media_send_queued = Rc::clone(&media);
    let media_split_right = Rc::clone(&media);
    let media_split_down = Rc::clone(&media);
    let media_ssh_host = Rc::clone(&media);
    let media_new_terminal = Rc::clone(&media);
    let media_term_cmd = Rc::clone(&media);
    let media_add_space = Rc::clone(&media);
    let media_add_space_with_medium = Rc::clone(&media);
    let media_select_space = Rc::clone(&media);
    let media_switch_space = Rc::clone(&media);
    let media_close_space = Rc::clone(&media);
    let media_workspace_click = Rc::clone(&media);

    // The settings overlay runs these through `with_settings_key_activation`
    // as well as from the controls themselves, so the behaviour that already
    // lives in an action is created once and shared by both paths.
    let action_toggle_dark_mode: Rc<RefCell<dyn FnMut(bool)>> =
        Rc::new(RefCell::new(move |enabled: bool| {
            on_toggle_dark_mode.borrow_mut().settings_dark_mode = enabled;
            if let Some(desktop) = &desktop_dark_mode {
                let mut config = desktop.config();
                config.theme.dark = enabled;
                if let Err(e) = desktop.save_config(&config) {
                    log::warn!("save_config (theme) failed: {e}");
                }
            }
        }));
    let action_set_theme_target: Rc<RefCell<dyn FnMut(ColorTarget)>> =
        Rc::new(RefCell::new(move |target: ColorTarget| {
            on_set_theme_target.borrow_mut().theme_color_target = target;
        }));
    let action_toggle_invert_scroll: Rc<RefCell<dyn FnMut(bool)>> =
        Rc::new(RefCell::new(move |enabled: bool| {
            on_toggle_invert_scroll.borrow_mut().settings_invert_scroll = enabled;
        }));
    let action_reload_model_config: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(move || {
        let mut state = on_reload_model_config.borrow_mut();
        if let Some(desktop) = &desktop_reload_model {
            if let Ok(home) = goble_core::app_home::GobleHome::locate() {
                if let Err(e) = desktop.reload_config(&home.config_path()) {
                    log::error!("reload_config failed: {e}");
                }
            }
            state.models = desktop.available_models(&state.settings_llm_provider);
        }
    }));
    // Remote agents are read by the same config reader the models use: reload
    // the file into the desktop state, then copy its `[remote_agents.*]` map
    // into the page's rows. The state's own pinned path wins, so a test or a
    // tool that names a file is read instead of the user's home.
    let action_reload_remote_agents: Rc<RefCell<dyn FnMut()>> =
        Rc::new(RefCell::new(move || {
            let mut state = on_reload_remote_agents.borrow_mut();
            let Some(desktop) = &desktop_reload_remote_agents else {
                return;
            };
            let path = desktop.config_path().or_else(|| {
                goble_core::app_home::GobleHome::locate()
                    .ok()
                    .map(|home| home.config_path())
            });
            if let Some(path) = path {
                if let Err(e) = desktop.reload_config(&path) {
                    log::error!("reload_config (remote agents) failed: {e}");
                }
            }
            state.refresh_remote_agents(Some(desktop));
        }));
    // Remote is configured on the Settings page it belongs to. Every chooser
    // that names it before anything is configured opens that page with the
    // keyboard on the form, so there is one place to configure and one shape
    // to learn.
    let action_open_remote_settings: Rc<RefCell<dyn FnMut()>> =
        Rc::new(RefCell::new(move || {
            on_open_remote_settings
                .borrow_mut()
                .open_remote_settings_tab(desktop_open_remote_settings.as_deref());
        }));
    // `~/.ssh` is read by the state (which owns the Connections page's cache),
    // so the row and `Enter` on it take one path.
    let action_reload_ssh_hosts: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new({
        let state = Rc::clone(&on_reload_ssh_hosts);
        move || state.borrow_mut().reload_ssh_hosts()
    }));
    let action_ssh_select_host: Rc<RefCell<dyn FnMut(String)>> = Rc::new(RefCell::new({
        let state = Rc::clone(&on_reload_ssh_hosts);
        move |alias: String| state.borrow_mut().ssh_select_host(&alias)
    }));
    let action_toggle_auto_approve: Rc<RefCell<dyn FnMut(u64, bool)>> =
        Rc::new(RefCell::new(move |pane_id: u64, enabled: bool| {
            let mut state = on_toggle_auto_approve.borrow_mut();
            state.pane_controls_mut(pane_id).auto_approve = enabled;
            if pane_id == state.active_pane_id {
                state.auto_approve = enabled;
            }
            if let Some(desktop) = &desktop_auto {
                if let Err(e) = desktop.set_auto_approve(enabled) {
                    log::warn!("set_auto_approve failed: {e}");
                }
            }
        }));
    // The pane's approval mode: how a proposed command reaches the shell. The
    // control and Shift+Tab both land here, so the pane's stored mode is the
    // one policy the turn and the composer read.
    let action_select_approval_mode: Rc<RefCell<dyn FnMut(u64, goble_ui::ApprovalMode)>> =
        Rc::new(RefCell::new(move |pane_id: u64, mode| {
            let mut state = on_select_approval_mode_state.borrow_mut();
            let controls = state.pane_controls_mut(pane_id);
            controls.approval_mode = mode;
            *controls.approval_menu_open.borrow_mut() = false;
        }));
    // Modal (vim) editing for the rich input, off by default. The choice is
    // persisted; turning it off drops every pane's half-typed command and
    // returns its editor to insert mode, so no pane is left in a mode whose
    // keys the editor no longer reads.
    let action_toggle_vim_mode: Rc<RefCell<dyn FnMut(bool)>> =
        Rc::new(RefCell::new(move |enabled: bool| {
            let mut state = on_toggle_vim_mode.borrow_mut();
            state.vim_mode = enabled;
            for controls in state.pane_controls.values_mut() {
                controls.vim.borrow_mut().reset();
            }
            if let Some(desktop) = &desktop_vim {
                if let Err(e) = desktop.set_vim_mode(enabled) {
                    log::warn!("set_vim_mode failed: {e}");
                }
            }
        }));
    // A dirty file view asks before its pane closes unless this is on; then
    // the close saves it first. The choice persists across restarts.
    let action_toggle_auto_save_on_close: Rc<RefCell<dyn FnMut(bool)>> =
        Rc::new(RefCell::new(move |enabled: bool| {
            let mut state = on_toggle_auto_save_on_close.borrow_mut();
            state.auto_save_on_close = enabled;
            if let Some(desktop) = &desktop_auto_save {
                if let Err(e) = desktop.set_auto_save_on_close(enabled) {
                    log::warn!("set_auto_save_on_close failed: {e}");
                }
            }
        }));
    let action_choose_workspace: Rc<RefCell<dyn FnMut(WorkspaceRouting)>> =
        Rc::new(RefCell::new(move |routing: WorkspaceRouting| {
            let mut state = on_choose_workspace.borrow_mut();
            // Remote with nowhere to run asks for the configuration first: a
            // pane opened now could only report "no paired worker".
            if routing == WorkspaceRouting::Remote && !state.remote_configured() {
                state.open_remote_settings_tab(desktop_choose_workspace.as_deref());
                return;
            }
            state.workspace_routing = Some(routing);
            state.show_workspace_choice = false;
            state.show_llm_key_banner = false;
            // Persist the decision on the selected conversation so it survives
            // restarts and is tracked per-conversation.
            if let Some(desktop) = &desktop_choose_workspace {
                if let Some(chat_id) = state.selected_id.clone() {
                    if let Err(e) =
                        desktop.set_chat_workspace_routing(&chat_id, Some(routing_to_str(routing)))
                    {
                        log::warn!("set_chat_workspace_routing failed: {e}");
                    }
                }
            }
            // The first-run flow is complete: show a short getting-started tip
            // over the conversation and mark the run as done so a returning
            // run skips the onboarding overlays entirely.
            state.show_onboarding_tip = true;
            state.onboarding_done = true;
            if let Some(desktop) = &desktop_choose_workspace {
                if let Err(e) = desktop.set_onboarding_done() {
                    log::warn!("set_onboarding_done failed: {e}");
                }
            }
            // Returning to chat lets the conversation continue in the chosen
            // workspace.
            state.current_tab = AppTab::Chat;
        }));
    let action_set_scroll_speed: Rc<RefCell<dyn FnMut(i32)>> =
        Rc::new(RefCell::new(move |speed: i32| {
            on_set_scroll_speed.borrow_mut().settings_scroll_speed = speed.clamp(1, 100);
        }));
    let action_set_font_size: Rc<RefCell<dyn FnMut(f32)>> =
        Rc::new(RefCell::new(move |size: f32| {
            let mut state = on_set_font_size.borrow_mut();
            state.settings_font_size = size.clamp(0.5, 2.0);
            // Drive the real window zoom from the same shared cell the keyboard
            // shortcut and menubar use, so the settings control reflects 1:1.
            *ui_zoom_font.borrow_mut() = state.settings_font_size;
        }));
    // The focused row's own value step, shared by the arrow keys and by `Enter`
    // on a row that holds one (a stepper advances, the channel column moves on).
    // The steppers run through the actions that own their bounds and, for the
    // font, the shared zoom cell; the rest belongs to the state.
    let action_settings_step: Rc<RefCell<dyn FnMut(i32)>> = {
        let step_state = Rc::clone(&state);
        let on_speed = action_set_scroll_speed.clone();
        let on_font = action_set_font_size.clone();
        Rc::new(RefCell::new(move |delta: i32| {
            // Read the focused control into a local first: the arms below run
            // the actions that own the value, and those borrow the same state,
            // so the match must not hold a borrow of its own.
            let control = step_state.borrow().settings_focused_control();
            match control {
                Some(SettingsControl::ScrollSpeed) => {
                    let next = step_state.borrow().settings_scroll_speed + delta;
                    (on_speed.borrow_mut())(next);
                }
                Some(SettingsControl::FontSize) => {
                    let next = step_state.borrow().settings_font_size + delta as f32 * 0.1;
                    (on_font.borrow_mut())(next);
                }
                _ => step_state.borrow_mut().settings_adjust_control(delta),
            }
        }))
    };

    UiActions {
        on_search_change: Rc::new(RefCell::new(move |value: String| {
            on_search_change.borrow_mut().search_query = value;
        })),
        on_search_focus_change: Rc::new(RefCell::new(move |focused: bool| {
            on_search_focus_change.borrow_mut().search_focused = focused;
        })),
        on_create_change: Rc::new(RefCell::new(move |value: String| {
            on_create_change.borrow_mut().new_conversation_draft = value;
        })),
        on_create_focus_change: Rc::new(RefCell::new(move |focused: bool| {
            on_create_focus_change.borrow_mut().create_focused = focused;
        })),
        // "New conversation" — the sidebar's row and the palette's entry — opens
        // a tab of its own: a fresh space holding one agent leaf over a
        // conversation of its own. The tab on screen is left alone, so the row
        // never disturbs the pane the user is working in.
        //
        // The row carries no model check: with nothing runnable the tab still
        // opens and the agent view's notice band names what is missing, which is
        // the only surface that can explain it.
        on_create_submit: Rc::new(RefCell::new(move || {
            let mut state = on_create_submit.borrow_mut();
            let medium_id = on_create_medium.borrow().selected_medium_id().to_string();
            let routing = crate::media::medium_routing(&medium_id);
            let id = state.next_pane_id;
            state.next_pane_id += 1;
            // An unnamed space: its tab derives its label from what it holds —
            // here the conversation bound to its pane, so the tab reads `New
            // Agent` until that conversation has a subject.
            state.spaces.push(
                Space::unnamed(Pane::Leaf { id, kind: PaneKind::Chat })
                    .with_medium(medium_id.clone()),
            );
            state.active_space = state.spaces.len() - 1;
            state.active_pane_id = id;
            state.workspace_routing = Some(WorkspaceRouting::from_routing(routing));
            ensure_pane_hover(&mut state, id);
            let project_id = on_create_medium.borrow().default_project_for_medium(&medium_id);
            let path = default_pane_path(&project_id, desktop_create.as_deref());
            state.set_active_pane_path(path.clone());
            // The subject the row was given names the conversation from the
            // start; a blank one means the user clicked the row directly, so the
            // conversation opens on the placeholder, which the agent replaces
            // with a subject of its own once it has answered in it.
            let title = if state.new_conversation_draft.trim().is_empty() {
                NEW_CONVERSATION_TITLE.to_string()
            } else {
                state.new_conversation_draft.trim().to_string()
            };
            state.bind_pane_new_conversation_titled(id, &title, desktop_create.as_deref());
            state.new_conversation_draft.clear();
            if let Some(conversation_id) = state.pane_conversation_id(id) {
                // The conversation belongs to the selected environment — so its
                // card lands in the matching sidebar folder — and starts where
                // its pane does, which is what the card's directory says.
                if let Some(desktop) = &desktop_create {
                    if let Err(e) =
                        desktop.set_chat_workspace_routing(&conversation_id, Some(routing))
                    {
                        log::warn!("set_chat_workspace_routing failed: {e}");
                    }
                    if !path.trim().is_empty() {
                        if let Err(e) = desktop.set_chat_working_dir(&conversation_id, &path) {
                            log::warn!("set_chat_working_dir failed: {e}");
                        }
                    }
                }
                // Bind it to the active pane (the new one) and load its empty
                // transcript, so the tab starts on a clean thread.
                state.bind_active_pane_conversation(conversation_id, desktop_create.as_deref());
            }
            // A space opened on an environment whose conversations run on a
            // worker holds a viewer pane from the start: the conversation's
            // shell is the worker's, so the pane is never given a local
            // terminal it would only have to be torn out of (and never a pty
            // process it should not have spawned). A local environment keeps
            // the agent pane it always had.
            if let Some(desktop) = &desktop_create {
                state.adopt_routing(
                    id,
                    Some(WorkspaceRouting::from_routing(routing)),
                    desktop,
                );
            }
            open_pane_harness(&mut state, id, desktop_create.as_ref());
            state.refresh_space_labels();
            state.sync_active_view();
            if let Some(desktop) = &desktop_create {
                state.refresh_from_desktop(desktop);
                state.save_panes(desktop);
            }
        })),
        on_select_conversation: Rc::new(RefCell::new(move |id: String| {
            let mut state = on_select_conversation.borrow_mut();
            // A conversation opened from the sidebar gets a tab of its own in
            // the top bar, rather than replacing the thread the focused pane
            // showed. One already on screen is brought forward.
            state.open_conversation_tab(id, desktop_select.as_deref());
        })),
        on_select_tab: Rc::new(RefCell::new(move |tab: AppTab| {
            on_select_tab.borrow_mut().current_tab = tab;
        })),
        on_composer_change: Rc::new(RefCell::new(move |value: String| {
            let mut state = on_composer_change.borrow_mut();
            state.set_active_pane_draft(value);
            // The draft is the slash menu's query, so a new query starts the
            // menu's selection over at the first row — and typing brings the
            // menu back after Escape put it away.
            let pane_id = state.active_pane_id;
            *state.pane_controls_mut(pane_id).slash_index.borrow_mut() = 0;
            *state.pane_controls_mut(pane_id).slash_dismissed.borrow_mut() = false;
        })),
        on_slash_move: Rc::new(RefCell::new(move |index: usize| {
            let mut state = on_slash_move.borrow_mut();
            let pane_id = state.active_pane_id;
            *state.pane_controls_mut(pane_id).slash_index.borrow_mut() = index;
        })),
        on_slash_close: Rc::new(RefCell::new(move || {
            let mut state = on_slash_close.borrow_mut();
            // A command ran, so the bar it was typed in is spent: clearing the
            // draft empties the editor and closes the menu with it.
            state.set_active_pane_draft(String::new());
            let pane_id = state.active_pane_id;
            *state.pane_controls_mut(pane_id).slash_index.borrow_mut() = 0;
        })),
        on_slash_dismiss: Rc::new(RefCell::new(move || {
            let mut state = on_slash_dismiss.borrow_mut();
            // Escape: the menu goes away and the draft stays as typed, so the
            // flag — not the draft — is what closes it.
            let pane_id = state.active_pane_id;
            *state.pane_controls_mut(pane_id).slash_dismissed.borrow_mut() = true;
            *state.pane_controls_mut(pane_id).slash_index.borrow_mut() = 0;
        })),
        on_composer_focus_change: Rc::new(RefCell::new(move |focused: bool| {
            on_composer_focus_change.borrow_mut().composer_focused = focused;
        })),
        on_send_message: Rc::new(RefCell::new(move |text: String| {
            let mut state = on_send_message.borrow_mut();
            // The active pane owns the composer, so route the turn to its own
            // conversation (an independent session per pane).
            let pane_id = state.active_pane_id;
            // An empty submit is the second Enter: the prompt the first one
            // queued is sent now instead of waiting for the turn to finish.
            if text.trim().is_empty() {
                let queued = state
                    .pane_runtime
                    .get(&pane_id)
                    .and_then(|rt| rt.queued_prompt.as_ref())
                    .is_some();
                if queued {
                    flush_queued_prompt(&mut state, desktop_send.as_ref(), &media_send);
                }
                return;
            }
            // The chat composer is a prompt line in agent mode (an active agent
            // conversation): an input is an agent prompt unless it starts with
            // `!`, which forces a terminal command (warp-new input model).
            match classify_input(&text, true) {
                Some(InputClass::TerminalCommand(cmd)) => {
                    // A `!` line typed in an agent view is that conversation's
                    // command, so the conversation draws it: the block belongs to
                    // the conversation and not to the shell behind Esc. The pane's
                    // view names it, and it is read the one way the app reads a
                    // pane's surface (`pane_view`), so the conversation the
                    // command is filed under is the one the pane is drawing.
                    let conversation = match state.pane_view(pane_id) {
                        BlockView::Agent { conversation_id } => Some(conversation_id),
                        BlockView::Terminal => None,
                    };
                    run_terminal_command(&mut state, &cmd, pane_id, conversation.as_deref());
                    // The line ran, so the input it was typed in is spent; the
                    // app-owned draft is what the next frame rebuilds that
                    // editor from.
                    state.set_active_pane_draft(String::new());
                }
                Some(InputClass::AgentPrompt(prompt)) => {
                    let pane_busy = state
                        .pane_runtime
                        .get(&pane_id)
                        .map(|r| r.busy)
                        .unwrap_or(false);
                    // While a turn is running, don't start a second one (that
                    // would interrupt the agent). Queue the prompt and render it
                    // as a pending block; sent when the turn finishes or via
                    // "Send now".
                    if pane_busy {
                        let rt = state.pane_runtime.entry(pane_id).or_default();
                        rt.queued_prompt = Some(prompt.clone());
                        state.queued_prompt = Some(prompt);
                        state.set_active_pane_draft(String::new());
                        return;
                    }
                    send_agent_prompt(
                        &mut state,
                        desktop_send.as_ref(),
                        &media_send,
                        &prompt,
                        pane_id,
                    );
                }
                None => {}
            }
        })),
        // Cmd/Ctrl+Enter in a chat composer: run the turn on the active pane's
        // own conversation, creating it when the pane has none. A conversation
        // the pane already owns is reused, so subsequent messages keep appending
        // to the thread the user is on (no accidental new conversation each
        // send) — but a pane that owns none starts one here rather than
        // adopting the conversation the sidebar last selected.
        on_cmd_enter: Rc::new(RefCell::new(move |text: String| {
            // Cmd+Enter with nothing typed is the gesture that opens the pane's
            // agent view; the switch itself belongs to the caller, so there is
            // no prompt to send here.
            if text.trim().is_empty() {
                return;
            }
            let mut state = on_cmd_enter.borrow_mut();
            let pane_id = state.active_pane_id;
            // A turn that cannot run opens no conversation either: with no model
            // there is nothing to answer the prompt, so the pane keeps whatever
            // conversation it had (usually none) and the composer shows the
            // notice band.
            //
            // The pane's *own* conversation is what counts, not
            // `pane_conversation_id`: that one falls back to the sidebar's
            // selection for a pane with nothing bound, and binding by it would
            // send this gesture into the conversation that was active before.
            if state.can_run_agent_turn(pane_id) && !state.pane_owns_conversation(pane_id) {
                state.bind_pane_new_conversation(pane_id, desktop_cmd_enter.as_deref());
            }
            send_agent_prompt(
                &mut state,
                desktop_cmd_enter.as_ref(),
                &media_cmd_enter,
                &text,
                pane_id,
            );
        })),
        // Cmd/Ctrl+Alt+Enter (warp-new's `⌘⌥⏎`): "send to cloud" is the routing
        // choice plus the ordinary submit. The conversation is pointed at the
        // cloud medium — the same write `on_choose_workspace` makes — and the
        // draft runs through the shared send path. There is no remote transport
        // in this tree, so nothing here reports a remote run as if it happened:
        // the routing is the whole gesture.
        on_send_to_cloud: Rc::new(RefCell::new(move |text: String| {
            let mut state = on_send_to_cloud.borrow_mut();
            let pane_id = state.active_pane_id;
            // Nothing typed is nothing to route: the composer's chord fires
            // either way, and an empty draft is the host's no-op (the same rule
            // `on_cmd_enter` keeps).
            if text.trim().is_empty() {
                return;
            }
            state.workspace_routing = Some(WorkspaceRouting::Remote);
            // The same rule as the plain chord: a pane that owns no conversation
            // starts one here instead of routing the previously selected one.
            if state.can_run_agent_turn(pane_id) && !state.pane_owns_conversation(pane_id) {
                state.bind_pane_new_conversation(pane_id, desktop_send_to_cloud.as_deref());
            }
            // Persist the choice on the conversation the turn will run on, so
            // it survives a restart and is tracked per conversation.
            if let (Some(desktop), Some(chat_id)) = (
                &desktop_send_to_cloud,
                state.pane_conversation_id(pane_id),
            ) {
                if let Err(e) = desktop.set_chat_workspace_routing(
                    &chat_id,
                    Some(routing_to_str(WorkspaceRouting::Remote)),
                ) {
                    log::warn!("set_chat_workspace_routing failed: {e}");
                }
            }
            send_agent_prompt(
                &mut state,
                desktop_send_to_cloud.as_ref(),
                &media_send_to_cloud,
                &text,
                pane_id,
            );
        })),
        // The attach control: open the system's own file picker and carry the
        // chosen path as an attachment of the pane's rich input. The panel is
        // modal and this handler runs on the UI thread, so the answer is here
        // before the next frame; no answer (cancelled, or a platform with no
        // picker) adds nothing.
        on_attach: Rc::new(RefCell::new(move || {
            let mut state = on_attach.borrow_mut();
            let pane_id = state.active_pane_id;
            if let Some(path) = goble_ui::platform::current::pick_file() {
                state.attach_pane_file(pane_id, path);
            }
        })),
        on_voice: Rc::new(RefCell::new(move || {
            on_voice.borrow_mut().push_active_message(ChatMessage::from_markdown(
                ChatRole::Assistant,
                "(voice input coming soon)",
            ));
        })),
        on_select_model: Rc::new(RefCell::new(move || {
            log::info!("model selector pressed");
        })),
        on_model_select: Rc::new(RefCell::new(move |pane_id: u64, model: String| {
            let mut state = on_model_select.borrow_mut();
            {
                let controls = state.pane_controls_mut(pane_id);
                controls.model = model.clone();
                let flag = controls.model_menu_open.clone();
                *flag.borrow_mut() = false;
            }
            if pane_id == state.active_pane_id {
                state.selected_model = model;
                *state.model_menu_open.borrow_mut() = false;
            }
        })),
        // The environment control selects the medium the conversation's turns
        // run on, not a registered harness; an unknown medium id is ignored. A
        // conversation running on a worker keeps its own choice: it is persisted
        // on that conversation (A3), so the pair of panes showing two
        // conversations never shares one environment, and the turn the pane
        // submits carries it. A pane with no environment control — every local
        // conversation — keeps selecting the window's environment.
        on_select_harness: Rc::new(RefCell::new(move |pane_id: u64, medium_id: String| {
            let viewer = on_select_harness_state.borrow().pane_is_viewer(pane_id);
            let known = on_select_medium.borrow().has_medium(&medium_id);
            // A local pane picking Remote before anything is configured gets
            // the setup dialog; the menu closes and nothing else moves.
            if known && !viewer && medium_id == "remote" {
                let mut state = on_select_harness_state.borrow_mut();
                if !state.remote_configured() {
                    state.open_remote_settings_tab(desktop_select_harness.as_deref());
                    let flag = state.pane_controls_mut(pane_id).harness_menu_open.clone();
                    *flag.borrow_mut() = false;
                    *state.harness_menu_open.borrow_mut() = false;
                    return;
                }
            }
            if known && viewer {
                let mut state = on_select_harness_state.borrow_mut();
                let conversation = state.pane_conversation_id(pane_id).unwrap_or_default();
                if let (Some(desktop), false) = (&desktop_select_harness, conversation.is_empty()) {
                    if let Err(e) = desktop.set_chat_medium(&conversation, Some(&medium_id)) {
                        log::warn!("set_chat_medium failed: {e}");
                    }
                }
                // The pane shows the choice on the next frame, whether or not the
                // conversation's row could be written.
                state.pane_controls_mut(pane_id).environment = medium_id.clone();
            } else if known {
                // A pane with no environment of its own — every local pane — keeps
                // selecting the window's environment, which is what its turns run
                // on. An unknown id changes nothing; the menu still closes.
                let _ = on_select_medium.borrow_mut().select_medium(&medium_id);
            }
            let mut state = on_select_harness_state.borrow_mut();
            {
                let controls = state.pane_controls_mut(pane_id);
                let flag = controls.harness_menu_open.clone();
                *flag.borrow_mut() = false;
            }
            *state.harness_menu_open.borrow_mut() = false;
        })),
        // The working-directory pill's selection, warp-new's model: the menu
        // browses the machine and the choice moves the pane. A pane that owns a
        // shell is told to `cd` — the very command the user would type, run in
        // that pane's own pty — and the pane's recorded directory follows, so
        // the pill and the branch under it describe where the shell now is. A
        // pane that has no shell yet has nowhere to run it, so its directory is
        // recorded for the shell it will start and for its composer.
        on_select_dir: Rc::new(RefCell::new(move |pane_id: u64, path: String| {
            let mut state = on_select_dir.borrow_mut();
            let has_shell = state.terminal.borrow().sessions.contains_key(&pane_id);
            if has_shell {
                run_terminal_command(&mut state, &change_directory_command(&path), pane_id, None);
            }
            state.set_pane_path(pane_id, path);
        })),
        // The branch pill's selection: `git checkout '<branch>'` in the pane's
        // own shell, the command warp-new runs. The pill moves to the chosen
        // branch with it, and only when there was a shell to check it out in.
        on_select_branch: Rc::new(RefCell::new(move |pane_id: u64, branch: String| {
            let mut state = on_select_branch.borrow_mut();
            let has_shell = state.terminal.borrow().sessions.contains_key(&pane_id);
            if !has_shell {
                return;
            }
            run_terminal_command(
                &mut state,
                &checkout_branch_command(&branch),
                pane_id,
                None,
            );
            {
                let controls = state.pane_controls_mut(pane_id);
                controls.branch = branch.clone();
                let flag = controls.branch_menu_open.clone();
                *flag.borrow_mut() = false;
            }
            if pane_id == state.active_pane_id {
                state.composer_branch = branch;
            }
        })),
        on_copy: Rc::new(RefCell::new(move || {
            log::info!("copy transcript pressed");
        })),
        // Copy a terminal block's text to the system clipboard. The clipboard
        // helper chooses the platform's tool and keeps an in-process copy when
        // none is installed, so the block copy works everywhere.
        on_copy_terminal: Rc::new(RefCell::new(move |text: String| {
            crate::clipboard::copy_text(&text);
        })),
        // Clear the active pane's in-memory transcript
        // (messages + suspended ask + queued prompt). The backend store is left
        // intact so the conversation can be recovered by restarting the session.
        on_clear_transcript: Rc::new(RefCell::new(move || {
            let mut state = on_clear_transcript.borrow_mut();
            let pane_id = state.active_pane_id;
            if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                rt.messages.clear();
                rt.pending_ask = None;
                rt.reset_command_edit();
                rt.queued_prompt = None;
            }
            state.pending_ask = None;
            state.queued_prompt = None;
            state.sync_active_view();
        })),
        on_stop: Rc::new(RefCell::new(move || {
            let mut state = on_stop.borrow_mut();
            let pane_id = state.active_pane_id;
            // Cancellation is the pane's turn, not whatever the sidebar last
            // selected: the conversation is the pane's own binding.
            if let (Some(desktop), Some(chat_id)) =
                (&desktop_stop, state.pane_conversation_id(pane_id))
            {
                let _ = desktop.cancel_chat_turn(&chat_id);
            }
            state.stop_turn(pane_id);
            state.agent_busy = false;
        })),
        // A sub-agent row's click enters the child's own conversation in the
        // pane the row was drawn in (S6): the child's card goes into the pane's
        // block list, the pane's filter points at the child, and the child's
        // rows are read from the store under the child's chat id.
        on_open_sub_agent: Rc::new(RefCell::new(move |pane_id: u64, child_id: String| {
            on_open_sub_agent.borrow_mut().open_sub_agent(
                pane_id,
                &child_id,
                desktop_open_sub_agent.as_deref(),
            );
        })),
        // Esc from the child view puts the pane back on the conversation it came
        // from; the child's card stays in the block list.
        on_close_sub_agent: Rc::new(RefCell::new(move |pane_id: u64| {
            on_close_sub_agent
                .borrow_mut()
                .close_sub_agent_view(pane_id);
        })),
        on_answer_ask: Rc::new(RefCell::new(
            move |response: String, credential: Option<(String, String)>| {
                let mut state = on_answer_ask.borrow_mut();
                let model = if state.selected_model.trim().is_empty() {
                    state.settings_llm_model.clone()
                } else {
                    state.selected_model.clone()
                };
                let pane_id = state.active_pane_id;
                if let (Some(desktop), Some(chat_id)) =
                    (&desktop_answer, state.pane_conversation_id(pane_id))
                {
                    if let Err(e) = desktop.resume_chat_turn(
                        &chat_id,
                        &response,
                        credential,
                        &state.settings_llm_provider,
                        &model,
                    ) {
                        log::warn!("resume_chat_turn failed: {e}");
                    } else {
                        if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                            rt.busy = true;
                            rt.pending_ask = None;
                            rt.agent_state = crate::state::PaneAgentState::Working;
                        }
                        state.agent_busy = true;
                        state.pending_ask = None;
                    }
                    state.refresh_messages(desktop);
                } else {
                    if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                        rt.pending_ask = None;
                    }
                    state.pending_ask = None;
                }
            },
        )),
        on_skip_ask: Rc::new(RefCell::new(move || {
            let mut state = on_skip_ask.borrow_mut();
            let model = if state.selected_model.trim().is_empty() {
                state.settings_llm_model.clone()
            } else {
                state.selected_model.clone()
            };
            let pane_id = state.active_pane_id;
            if let (Some(desktop), Some(chat_id)) =
                (&desktop_skip, state.pane_conversation_id(pane_id))
            {
                if let Err(e) = desktop.resume_chat_turn(
                    &chat_id,
                    "(skipped)",
                    None,
                    &state.settings_llm_provider,
                    &model,
                ) {
                    log::warn!("resume_chat_turn (skip) failed: {e}");
                } else {
                    // The skipped question resumes the turn, so the pane reports
                    // Working at once rather than waiting for its next event.
                    state.skip_pending_ask(pane_id);
                    state.agent_busy = true;
                    state.pending_ask = None;
                }
                state.refresh_messages(desktop);
            } else {
                if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                    rt.pending_ask = None;
                }
                state.pending_ask = None;
            }
        })),
        // Answer a suspended command proposal from the composer's card: approve
        // or edit runs the chosen text through the harness's resume path, reject
        // fails the tool call. Either way the turn resumes and settles, so the
        // pane cannot be left busy on a suspension with no path back.
        on_command_decision: Rc::new(RefCell::new(
            move |pane_id: u64,
                  proposal_id: String,
                  decision: goble_core::harness::CommandDecision| {
                let mut state = on_command_decision.borrow_mut();
                let chat_id = state.pane_conversation_id(pane_id);
                if let (Some(desktop), Some(chat_id)) = (&desktop_command, chat_id) {
                    match desktop.resume_command_chat_turn(&chat_id, decision) {
                        Ok(_) => {
                            // The decision resumes the suspended turn: record
                            // that it is running before the card clears, so the
                            // pane's derived state is Working immediately.
                            state.resume_command_decision(pane_id, &proposal_id);
                            state.agent_busy = true;
                        }
                        Err(e) => log::warn!("resume_command_chat_turn failed: {e}"),
                    }
                    state.refresh_messages(desktop);
                }
            },
        )),
        on_toggle_auto_approve: action_toggle_auto_approve.clone(),
        on_select_approval_mode: action_select_approval_mode.clone(),
        on_toggle_vim_mode: action_toggle_vim_mode.clone(),
        on_toggle_auto_save_on_close: action_toggle_auto_save_on_close.clone(),
        clipboard: crate::clipboard::system_clipboard(),
        // The transcript footer's Fork: a new conversation inheriting this
        // pane's transcript, bound to the pane that asked for it.
        on_fork_conversation: Rc::new(RefCell::new(move |pane_id: u64| {
            let mut state = on_fork_conversation.borrow_mut();
            if state.fork_pane_conversation(pane_id, desktop_fork.as_deref()).is_some()
                && pane_id == state.active_pane_id
            {
                state.sync_active_view();
            }
        })),
        // Cmd+Enter at a terminal pane's rich input activates the harness for
        // that pane; Esc turns it back off (plain pty). Per pane, so one pane
        // can run the harness while its sibling stays a plain shell.
        //
        // Activating the harness is entering that pane's agent view: the pane
        // needs a conversation of its own to show, so one is bound on the first
        // entry, its card is pushed into the block list (the way back at the
        // terminal) and the pane's filter points at the conversation. Esc is
        // the reverse: the pane's filter returns to the terminal, and the card
        // stays in the list.
        on_set_pane_harness_mode: Rc::new(RefCell::new(move |pane_id: u64, on: bool| {
            let mut state = on_set_harness_mode.borrow_mut();
            if on {
                open_pane_harness(&mut state, pane_id, desktop_set_harness.as_ref());
            } else {
                state.pane_controls_mut(pane_id).harness_mode = false;
                state.leave_agent_view(pane_id);
            }
        })),
        // A click on the card a conversation left behind in the terminal enters
        // that conversation's agent view; the harness input comes with it, the
        // same way Cmd+Enter does.
        on_open_agent_view: Rc::new(RefCell::new(
            move |pane_id: u64, conversation_id: String| {
                let mut state = on_open_agent_view.borrow_mut();
                state.pane_controls_mut(pane_id).harness_mode = true;
                let label = state.conversation_name(&conversation_id);
                state.enter_agent_view(pane_id, &conversation_id, &label);
            },
        )),
        on_send_queued: Rc::new(RefCell::new(move || {
            let mut state = on_send_queued.borrow_mut();
            flush_queued_prompt(&mut state, desktop_send_queued.as_ref(), &media_send_queued);
        })),
        on_dismiss_queued: Rc::new(RefCell::new(move || {
            let mut state = on_dismiss_queued.borrow_mut();
            let pane_id = state.active_pane_id;
            if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                rt.queued_prompt = None;
            }
            state.queued_prompt = None;
        })),
        on_menu: Rc::new(RefCell::new(move || {
            log::info!("menu pressed");
        })),
        on_inbox: Rc::new(RefCell::new(move || {
            let mut state = on_inbox.borrow_mut();
            let opening = !state.inbox_open;
            state.inbox_open = opening;
            if opening {
                state.clamp_inbox_index();
                state.inbox_error = None;
                state.inbox_scroll.borrow_mut().reset();
            }
        })),
        on_close_inbox: Rc::new(RefCell::new(move || {
            let mut state = on_close_inbox.borrow_mut();
            state.inbox_open = false;
            state.inbox_error = None;
        })),
        on_inbox_move: Rc::new(RefCell::new(move |step: i32| {
            let mut state = on_inbox_move.borrow_mut();
            let len = state.inbox.len();
            state.inbox_index = if len == 0 {
                0
            } else {
                (state.inbox_index as i32 + step).clamp(0, len as i32 - 1) as usize
            };
            state.inbox_error = None;
        })),
        on_inbox_ack: Rc::new(RefCell::new(move || {
            let mut state = on_inbox_ack.borrow_mut();
            let index = state.inbox_index;
            if let Some(id) = state.inbox.get(index).map(|item| item.id.clone()) {
                state.ack_inbox(&id);
            }
        })),
        on_inbox_ack_all: Rc::new(RefCell::new(move || {
            on_inbox_ack_all.borrow_mut().ack_all();
        })),
        on_inbox_jump: Rc::new(RefCell::new(move || {
            let mut state = on_inbox_jump.borrow_mut();
            let index = state.inbox_index;
            if state.jump_to_inbox_item(index) {
                state.inbox_open = false;
            }
        })),
        // The review pane's own interactions. Every view choice lives in the
        // pane's controls, so the per-frame rebuild keeps it; the two app
        // effects are opening the pane (and a file from it) and the clipboard.
        on_diff_panel: Rc::new(RefCell::new(
            move |pane_id: u64, action: DiffPanelAction| {
                let mut state = on_diff_panel.borrow_mut();
                match action {
                    DiffPanelAction::Open(file) => {
                        open_diff_pane(&mut state, pane_id, file, desktop_diff_panel.as_ref());
                    }
                    DiffPanelAction::Select(file) => {
                        let controls = state.pane_controls_mut(pane_id);
                        *controls.diff_selected.borrow_mut() = file;
                        *controls.diff_edit_focus.borrow_mut() = None;
                        controls.diff_scroll.borrow_mut().reset();
                    }
                    DiffPanelAction::SetFileOpen(path, open) => {
                        let controls = state.pane_controls_mut(pane_id);
                        controls.diff_expanded.borrow_mut().insert(path, open);
                    }
                    DiffPanelAction::SetFileEditing(path, edit) => {
                        // The press is in this pane whichever body it asks
                        // for: the editor left behind must not keep taking
                        // keys once the pane is not the active one.
                        state.focus_pane(pane_id);
                        let controls = state.pane_controls_mut(pane_id);
                        if edit {
                            // The body is the file's own text, so the block is
                            // unfolded before it is shown.
                            controls
                                .diff_expanded
                                .borrow_mut()
                                .insert(path.clone(), true);
                        } else if controls.diff_edit_focus.borrow().as_deref()
                            == Some(path.as_str())
                        {
                            *controls.diff_edit_focus.borrow_mut() = None;
                        }
                        controls.diff_editing.borrow_mut().insert(path, edit);
                    }
                    DiffPanelAction::Focus => {
                        state.focus_pane(pane_id);
                    }
                    DiffPanelAction::ToggleSidebar => {
                        let controls = state.pane_controls_mut(pane_id);
                        let sidebar = controls.diff_sidebar.clone();
                        let open = *sidebar.borrow();
                        *sidebar.borrow_mut() = !open;
                    }
                    DiffPanelAction::ToggleCommit => {
                        state.focus_pane(pane_id);
                        let controls = state.pane_controls_mut(pane_id);
                        let open = controls.diff_commit_open.clone();
                        let showing = *open.borrow();
                        *open.borrow_mut() = !showing;
                        *controls.diff_commit_focus.borrow_mut() = !showing;
                        *controls.diff_commit_menu_open.borrow_mut() = false;
                    }
                    DiffPanelAction::SetCommitMessage(message) => {
                        let controls = state.pane_controls_mut(pane_id);
                        *controls.diff_commit_message.borrow_mut() = message;
                        *controls.diff_commit_focus.borrow_mut() = true;
                    }
                    DiffPanelAction::Commit => {
                        state.focus_pane(pane_id);
                        let cwd = state.pane_cwd(pane_id);
                        let controls = state.pane_controls_mut(pane_id);
                        let message = controls.diff_commit_message.borrow().trim().to_string();
                        if message.is_empty() {
                            // A commit needs its message: the bar opens on the
                            // field the user has to fill, with the reason in the
                            // status line instead of a silent no-op.
                            *controls.diff_commit_open.borrow_mut() = true;
                            *controls.diff_commit_focus.borrow_mut() = true;
                            *controls.diff_commit_menu_open.borrow_mut() = false;
                            *controls.diff_git_status.borrow_mut() =
                                Some(("Write a commit message first".to_string(), false));
                            return;
                        }
                        let Some(cwd) = cwd else {
                            *controls.diff_git_status.borrow_mut() =
                                Some(("No working tree here".to_string(), false));
                            return;
                        };
                        *controls.diff_commit_open.borrow_mut() = false;
                        *controls.diff_commit_focus.borrow_mut() = false;
                        *controls.diff_commit_menu_open.borrow_mut() = false;
                        *controls.diff_git_status.borrow_mut() =
                            Some(("Committing…".to_string(), true));
                        spawn_git_job(
                            cwd,
                            GitJob::Commit(message),
                            Arc::clone(&controls.diff_git_result),
                        );
                    }
                    DiffPanelAction::Push => {
                        let cwd = state.pane_cwd(pane_id);
                        let controls = state.pane_controls_mut(pane_id);
                        *controls.diff_commit_menu_open.borrow_mut() = false;
                        let Some(cwd) = cwd else {
                            *controls.diff_git_status.borrow_mut() =
                                Some(("No working tree here".to_string(), false));
                            return;
                        };
                        *controls.diff_git_status.borrow_mut() =
                            Some(("Pushing…".to_string(), true));
                        spawn_git_job(cwd, GitJob::Push, Arc::clone(&controls.diff_git_result));
                    }
                    DiffPanelAction::CreatePr => {
                        let cwd = state.pane_cwd(pane_id);
                        let controls = state.pane_controls_mut(pane_id);
                        *controls.diff_commit_menu_open.borrow_mut() = false;
                        let Some(cwd) = cwd else {
                            *controls.diff_git_status.borrow_mut() =
                                Some(("No working tree here".to_string(), false));
                            return;
                        };
                        *controls.diff_git_status.borrow_mut() =
                            Some(("Opening a pull request…".to_string(), true));
                        spawn_git_job(cwd, GitJob::CreatePr, Arc::clone(&controls.diff_git_result));
                    }
                    DiffPanelAction::CopyLink(path) => {
                        crate::clipboard::copy_text(&format!("file://{path}"));
                    }
                    DiffPanelAction::ConfirmRevert(path) => {
                        let controls = state.pane_controls_mut(pane_id);
                        *controls.diff_revert_confirm.borrow_mut() = Some(path);
                    }
                    DiffPanelAction::CancelRevert => {
                        let controls = state.pane_controls_mut(pane_id);
                        *controls.diff_revert_confirm.borrow_mut() = None;
                    }
                    DiffPanelAction::RevertFile(path) => {
                        let cwd = state.pane_cwd(pane_id);
                        let controls = state.pane_controls_mut(pane_id);
                        *controls.diff_revert_confirm.borrow_mut() = None;
                        let Some(cwd) = cwd else {
                            *controls.diff_git_status.borrow_mut() =
                                Some(("No working tree here".to_string(), false));
                            return;
                        };
                        *controls.diff_git_status.borrow_mut() =
                            Some((format!("Reverting {path}…"), true));
                        spawn_git_job(
                            cwd,
                            GitJob::Revert(path),
                            Arc::clone(&controls.diff_git_result),
                        );
                    }
                    DiffPanelAction::OpenExternal(path) => {
                        let controls = state.pane_controls_mut(pane_id);
                        if let Err(error) = crate::state::open_local_path(&path) {
                            *controls.diff_git_status.borrow_mut() = Some((error, false));
                        }
                    }
                    DiffPanelAction::OpenFile(path) => {
                        state.focus_pane(pane_id);
                        open_file_pane(&mut state, path, desktop_diff_panel.as_ref());
                    }
                    DiffPanelAction::CopyPaths(paths) => {
                        crate::clipboard::copy_text(&paths.join("\n"));
                    }
                }
            },
        )),
        on_toggle_sidebar: Rc::new(RefCell::new(move || {
            let mut state = on_toggle_sidebar.borrow_mut();
            state.sidebar_visible = !state.sidebar_visible;
        })),
        on_toggle_conversations_expanded: Rc::new(RefCell::new(move || {
            let mut state = on_toggle_conversations_expanded.borrow_mut();
            state.conversations_expanded = !state.conversations_expanded;
            // Entering the list always starts at the top, and a collapsed list
            // has nothing to scroll.
            state.sidebar_scroll.borrow_mut().reset();
        })),
        on_select_sidebar_view: Rc::new(RefCell::new(move |view: SidebarView| {
            let mut state = on_select_sidebar_view.borrow_mut();
            state.sidebar_view = view;
            // Each view has its own list and its own offset, so switching views
            // starts that list at the top.
            match view {
                SidebarView::Agents => state.sidebar_scroll.borrow_mut().reset(),
                SidebarView::Explorer => state.explorer_scroll.borrow_mut().reset(),
                SidebarView::Search => state.global_search_scroll.borrow_mut().reset(),
            }
        })),
        on_toggle_section: Rc::new(RefCell::new(move |key: String| {
            let mut state = on_toggle_section.borrow_mut();
            if !state.collapsed_sections.remove(&key) {
                state.collapsed_sections.insert(key);
            }
        })),
        on_toggle_star: Rc::new(RefCell::new(move |id: String| {
            let mut state = on_toggle_star.borrow_mut();
            if !state.starred_conversations.remove(&id) {
                state.starred_conversations.insert(id);
            }
        })),
        on_toggle_explorer_dir: Rc::new(RefCell::new(move |path: String| {
            let mut state = on_toggle_explorer_dir.borrow_mut();
            if !state.explorer_expanded.remove(&path) {
                state.explorer_expanded.insert(path);
            }
        })),
        on_explorer_file_click: Rc::new(RefCell::new(move |path: String| {
            let mut state = on_explorer_file_click.borrow_mut();
            open_file_pane(&mut state, path, desktop_file_view.as_ref());
        })),
        on_global_search_change: Rc::new(RefCell::new(move |value: String| {
            let mut state = on_global_search_change.borrow_mut();
            state.global_search_query = value;
            run_global_search(&mut state);
        })),
        on_global_search_focus_change: Rc::new(RefCell::new(move |focused: bool| {
            on_global_search_focus_change.borrow_mut().global_search_focused = focused;
        })),
        on_workspace_click: Rc::new(RefCell::new(move |index: usize| {
            let mut state = on_workspace_click.borrow_mut();
            if index >= state.spaces.len() {
                return;
            }
            // The press this release ends is what the gesture is counted from
            // (see `on_space_press`), not the time of the release: `count` names
            // the tab too, so clicking two different tabs quickly is not a
            // double click, and a press held long does not eat the window.
            let count = state
                .space_press_run
                .borrow()
                .filter(|run| run.index == index)
                .map(|run| run.count)
                .unwrap_or(1);
            if count == 2 {
                state.begin_space_rename(index);
                return;
            }
            // A single click selects the clicked workspace (Warp-style chip row).
            if state.active_space != index {
                state.active_space = index;
                state.spaces[index].recompute_layout();
                state.active_pane_id = state.spaces[index].root.first_leaf_id();
                state.workspace_routing = None;
                adopt_space_medium(&mut state, &media_workspace_click, index);
                state.sync_active_view();
                if let Some(desktop) = &desktop_workspace_click {
                    state.refresh_messages(desktop);
                    state.save_panes(desktop);
                }
                adopt_space_routing(&mut state, index);
            }
        })),
        // The menu a right click on a tab opens: hung from the point the click
        // landed on, which is where the bar paints it.
        on_space_menu: Rc::new(RefCell::new(move |index: usize, at: Vector2F| {
            let state = on_space_menu.borrow_mut();
            if index >= state.spaces.len() {
                return;
            }
            // A second right click on the tab the menu is already open on closes
            // it again, the way warp-new's toggle does.
            let open_here = state
                .space_menu
                .borrow()
                .map(|menu| menu.index == index)
                .unwrap_or(false);
            if open_here {
                *state.space_menu.borrow_mut() = None;
                return;
            }
            *state.space_menu.borrow_mut() = Some(SpaceMenu { index, at });
        })),
        // The menu closed itself: a press landed outside the panel, so the tab
        // it belongs to is dropped here — otherwise the next frame would build
        // the same menu open again.
        on_space_menu_close: Rc::new(RefCell::new(move || {
            let state = on_space_menu_close.borrow();
            *state.space_menu.borrow_mut() = None;
        })),
        on_space_menu_action: Rc::new(RefCell::new(move |index: usize, action: TabMenuAction| {
            let mut state = on_space_menu_action.borrow_mut();
            // Choosing a row closes the menu whether or not the row does
            // anything, which is what a menu does.
            *state.space_menu.borrow_mut() = None;
            if index >= state.spaces.len() {
                return;
            }
            match action {
                TabMenuAction::Rename => state.begin_space_rename(index),
                // The typed name is given up, which is the whole of it: the
                // label is derived from what the tab holds again the next time
                // the tree is rebuilt (`refresh_space_labels` runs before every
                // frame's snapshot), so a terminal tab reads its directory once
                // more and an agent tab its conversation's subject.
                TabMenuAction::ResetName => {
                    state.spaces[index].named = false;
                    if let Some(desktop) = &desktop_space_menu_action {
                        state.save_panes(desktop);
                    }
                }
                TabMenuAction::MoveLeft => {
                    if index > 0 {
                        state.reorder_space(index, index - 1);
                        if let Some(desktop) = &desktop_space_menu_action {
                            state.save_panes(desktop);
                        }
                    }
                }
                TabMenuAction::Close => {
                    close_space_at(&mut state, index, desktop_space_menu_action.as_deref());
                }
                // Every other tab goes, from the far end inwards, so the index
                // of the tab being kept never shifts under the removals.
                TabMenuAction::CloseOthers => {
                    for other in (index + 1..state.spaces.len()).rev() {
                        close_space_at(&mut state, other, desktop_space_menu_action.as_deref());
                    }
                    for other in (0..index).rev() {
                        close_space_at(&mut state, other, desktop_space_menu_action.as_deref());
                    }
                    // What is left is the tab the menu was opened on, and it is
                    // the one on screen.
                    if let Some(desktop) = &desktop_space_menu_action {
                        state.save_panes(desktop);
                    }
                }
                TabMenuAction::SetColor(color) => {
                    state.spaces[index].color = color;
                    if let Some(desktop) = &desktop_space_menu_action {
                        state.save_panes(desktop);
                    }
                }
            }
        })),
        on_space_rename_change: Rc::new(RefCell::new(move |value: String| {
            on_space_rename_change.borrow_mut().space_rename_draft = value;
        })),
        on_space_rename_focus: Rc::new(RefCell::new(move |focused: bool| {
            let mut state = on_space_rename_focus.borrow_mut();
            state.space_rename_focused = focused;
            // Blur commits the rename (Enter commits via `on_space_rename_commit`).
            if !focused && state.space_rename_editing {
                let draft = state.space_rename_draft.clone();
                state.rename_active_space(draft, desktop_space_rename_focus.as_deref());
                state.space_rename_editing = false;
            }
        })),
        on_space_rename_commit: Rc::new(RefCell::new(move || {
            let mut state = on_space_rename_commit.borrow_mut();
            let draft = state.space_rename_draft.clone();
            state.rename_active_space(draft, desktop_space_rename_commit.as_deref());
            state.space_rename_editing = false;
            state.space_rename_focused = false;
        })),
        on_space_rename_cancel: Rc::new(RefCell::new(move || {
            let mut state = on_space_rename_cancel.borrow_mut();
            state.space_rename_editing = false;
            state.space_rename_focused = false;
            state.space_rename_draft.clear();
        })),
        // The settings tab: one space whose root is a settings leaf, opened
        // once and brought to the front on every later press. It is a tab in
        // `AppTab::Chat`, so the press comes back to the chat mode the strip is
        // drawn in.
        on_settings: Rc::new(RefCell::new(move || {
            on_settings
                .borrow_mut()
                .open_settings_tab(desktop_settings.as_deref());
        })),
        on_settings_category: Rc::new(RefCell::new(move |category: SettingsCategory| {
            on_settings_category.borrow_mut().settings_select_category(category);
        })),
        on_settings_category_step: Rc::new(RefCell::new(move |delta: i32| {
            on_settings_category_step.borrow_mut().settings_category_step(delta);
        })),
        on_settings_focus_move: Rc::new(RefCell::new(move |delta: i32| {
            on_settings_focus_move.borrow_mut().settings_focus_step(delta);
        })),
        on_settings_focus_into_pane: Rc::new(RefCell::new(move || {
            on_settings_focus_into_pane.borrow_mut().settings_focus_into_pane();
        })),
        on_settings_focus_out_of_pane: Rc::new(RefCell::new(move || {
            on_settings_focus_out_of_pane.borrow_mut().settings_focus_out_of_pane();
        })),
        on_settings_activate: Rc::new(RefCell::new({
            let activate_state = Rc::clone(&on_settings_activate);
            let on_dark = action_toggle_dark_mode.clone();
            let on_invert = action_toggle_invert_scroll.clone();
            let on_theme = action_set_theme_target.clone();
            let on_auto = action_toggle_auto_approve.clone();
            let on_vim = action_toggle_vim_mode.clone();
            let on_auto_save = action_toggle_auto_save_on_close.clone();
            let on_reload = action_reload_model_config.clone();
            let on_reload_agents = action_reload_remote_agents.clone();
            let on_open_config = Rc::clone(&action_open_config_file);
            let on_step = action_settings_step.clone();
            let desktop_activate = desktop.clone();
            move || {
                let control = activate_state.borrow().settings_focused_control();
                match control {
                    // The controls whose behaviour is already an action run
                    // through it, so a click and the Enter key share one path.
                    Some(SettingsControl::DarkMode) => {
                        let next = !activate_state.borrow().settings_dark_mode;
                        (on_dark.borrow_mut())(next);
                    }
                    Some(SettingsControl::InvertScroll) => {
                        let next = !activate_state.borrow().settings_invert_scroll;
                        (on_invert.borrow_mut())(next);
                    }
                    Some(SettingsControl::ThemeChannel(target)) => {
                        (on_theme.borrow_mut())(target);
                    }
                    // A stepper advances: the same step the arrow keys take.
                    Some(SettingsControl::ScrollSpeed) | Some(SettingsControl::FontSize) => {
                        (on_step.borrow_mut())(1);
                    }
                    Some(SettingsControl::AutoApprove) => {
                        let (pane, next) = {
                            let state = activate_state.borrow();
                            (state.active_pane_id, !state.auto_approve)
                        };
                        (on_auto.borrow_mut())(pane, next);
                    }
                    Some(SettingsControl::VimMode) => {
                        let next = !activate_state.borrow().vim_mode;
                        (on_vim.borrow_mut())(next);
                    }
                    Some(SettingsControl::AutoSaveOnClose) => {
                        let next = !activate_state.borrow().auto_save_on_close;
                        (on_auto_save.borrow_mut())(next);
                    }
                    Some(SettingsControl::OpenConfigFile) => (on_open_config.borrow_mut())(),
                    Some(SettingsControl::ReloadModels) => (on_reload.borrow_mut())(),
                    Some(SettingsControl::ReloadRemoteAgents) => {
                        (on_reload_agents.borrow_mut())()
                    }
                    // Everything else belongs to the state: the environment
                    // rows, and a text field taking the caret or committing it.
                    _ => activate_state
                        .borrow_mut()
                        .settings_activate_control(desktop_activate.as_deref()),
                }
            }
        })),
        on_settings_adjust: Rc::new(RefCell::new({
            let step = action_settings_step.clone();
            move |delta: i32| (step.borrow_mut())(delta)
        })),
        on_settings_commit_field: Rc::new(RefCell::new(move || {
            on_settings_commit_field.borrow_mut().settings_commit_field();
        })),
        on_settings_cancel_field: Rc::new(RefCell::new(move || {
            on_settings_cancel_field.borrow_mut().settings_cancel_field();
        })),
        on_environment_group_draft_change: Rc::new(RefCell::new(move |value: String| {
            on_environment_group_draft_change
                .borrow_mut()
                .settings_environment_group_draft = value;
        })),
        on_environment_create_group: Rc::new(RefCell::new(move || {
            on_environment_create_group
                .borrow_mut()
                .environment_create_group(desktop_environment_create.as_deref());
        })),
        on_environment_open_group: Rc::new(RefCell::new(move |id: String| {
            on_environment_open_group.borrow_mut().environment_open_group(&id);
        })),
        on_environment_delete_group: Rc::new(RefCell::new(move |id: String| {
            on_environment_delete_group
                .borrow_mut()
                .environment_delete_group(&id, desktop_environment_delete.as_deref());
        })),
        on_environment_secret_name_change: Rc::new(RefCell::new(move |value: String| {
            on_environment_secret_name_change
                .borrow_mut()
                .settings_environment_secret_name = value;
        })),
        on_environment_secret_value_change: Rc::new(RefCell::new(move |value: String| {
            on_environment_secret_value_change
                .borrow_mut()
                .settings_environment_secret_value = value;
        })),
        on_environment_save_secret: Rc::new(RefCell::new(move || {
            on_environment_save_secret
                .borrow_mut()
                .environment_save_secret(desktop_environment_save.as_deref());
        })),
        on_environment_edit_secret: Rc::new(RefCell::new(move |id: String| {
            on_environment_edit_secret.borrow_mut().environment_edit_secret(&id);
        })),
        on_environment_delete_secret: Rc::new(RefCell::new(move |id: String| {
            on_environment_delete_secret
                .borrow_mut()
                .environment_delete_secret(&id, desktop_environment_secret_delete.as_deref());
        })),
        on_set_theme_target: action_set_theme_target.clone(),
        on_toggle_invert_scroll: action_toggle_invert_scroll.clone(),
        on_set_scroll_speed: action_set_scroll_speed.clone(),
        on_set_font_size: action_set_font_size.clone(),
        on_reload_model_config: action_reload_model_config.clone(),
        on_reload_remote_agents: action_reload_remote_agents.clone(),
        // Settings → Agent & Approval, and every place that chooses Remote:
        // one page holds the form that configures how Remote is reached.
        on_open_remote_settings: action_open_remote_settings.clone(),
        on_reload_ssh_hosts: action_reload_ssh_hosts.clone(),
        on_ssh_select_host: action_ssh_select_host.clone(),
        on_projects: Rc::new(RefCell::new(move || {
            on_projects.borrow_mut().current_tab = AppTab::Projects;
        })),
        // Harness observability navigation. Each of these toggles its page:
        // clicking the currently-open page returns to the chat workspace, so
        // the user always has an obvious path back.
        on_workflows: Rc::new(RefCell::new(move || {
            let mut state = on_workflows.borrow_mut();
            state.current_tab = if state.current_tab == AppTab::Workflows {
                AppTab::Chat
            } else {
                AppTab::Workflows
            };
        })),
        on_executions: Rc::new(RefCell::new(move || {
            let mut state = on_executions.borrow_mut();
            state.current_tab = if state.current_tab == AppTab::Executions {
                AppTab::Chat
            } else {
                AppTab::Executions
            };
        })),
        on_timeline: Rc::new(RefCell::new(move || {
            let mut state = on_timeline.borrow_mut();
            state.current_tab = if state.current_tab == AppTab::Timeline {
                AppTab::Chat
            } else {
                AppTab::Timeline
            };
        })),
        on_costs: Rc::new(RefCell::new(move || {
            let mut state = on_costs.borrow_mut();
            state.current_tab = if state.current_tab == AppTab::Costs {
                AppTab::Chat
            } else {
                AppTab::Costs
            };
        })),
        on_mcps: Rc::new(RefCell::new(move || {
            let mut state = on_mcps.borrow_mut();
            state.current_tab = if state.current_tab == AppTab::Mcps {
                AppTab::Chat
            } else {
                AppTab::Mcps
            };
        })),
        on_plugins: Rc::new(RefCell::new(move || {
            log::info!("plugins pressed (coming soon)");
        })),
        on_open_crons: Rc::new(RefCell::new(move || {
            on_open_crons.borrow_mut().crons_open = true;
        })),
        on_close_crons: Rc::new(RefCell::new(move || {
            on_close_crons.borrow_mut().crons_open = false;
        })),
        on_toggle_task_workflow: Rc::new(RefCell::new(move || {
            let mut state = on_toggle_task_workflow.borrow_mut();
            state.task_workflow_open = !state.task_workflow_open;
        })),
        on_close_task_workflow: Rc::new(RefCell::new(move || {
            on_close_task_workflow.borrow_mut().task_workflow_open = false;
        })),
        on_toggle_shortcuts_help: Rc::new(RefCell::new(move || {
            let mut state = on_toggle_shortcuts_help.borrow_mut();
            let opening = !state.shortcuts_help_open;
            state.shortcuts_help_open = opening;
            if opening {
                // The panel opens on the whole table: the last filter, the last
                // highlight and the list's scroll position do not outlive it.
                state.shortcuts_help_filter.clear();
                state.shortcuts_help_index = 0;
                state.shortcuts_help_scroll.borrow_mut().reset();
            }
        })),
        on_close_shortcuts_help: Rc::new(RefCell::new(move || {
            on_close_shortcuts_help.borrow_mut().shortcuts_help_open = false;
        })),
        on_shortcuts_help_type: Rc::new(RefCell::new(move |ch: char| {
            let mut state = on_shortcuts_help_type.borrow_mut();
            state.shortcuts_help_filter.push(ch);
            // A narrower list starts at its first row.
            state.shortcuts_help_index = 0;
        })),
        on_shortcuts_help_backspace: Rc::new(RefCell::new(move || {
            let mut state = on_shortcuts_help_backspace.borrow_mut();
            state.shortcuts_help_filter.pop();
            state.shortcuts_help_index = 0;
        })),
        on_shortcuts_help_clear_filter: Rc::new(RefCell::new(move || {
            let mut state = on_shortcuts_help_clear_filter.borrow_mut();
            state.shortcuts_help_filter.clear();
            state.shortcuts_help_index = 0;
        })),
        on_shortcuts_help_escape: Rc::new(RefCell::new(move || {
            let mut state = on_shortcuts_help_escape.borrow_mut();
            if state.shortcuts_help_filter.is_empty() {
                state.shortcuts_help_open = false;
            } else {
                state.shortcuts_help_filter.clear();
                state.shortcuts_help_index = 0;
            }
        })),
        on_shortcuts_help_move: Rc::new(RefCell::new(move |step: i32| {
            let mut state = on_shortcuts_help_move.borrow_mut();
            // The rows the current filter leaves are the list the highlight
            // moves through, so it can never point at a hidden row.
            let rows = crate::ui::shortcuts_help::visible_row_count(&state.shortcuts_help_filter);
            state.shortcuts_help_index = if rows == 0 {
                0
            } else {
                (state.shortcuts_help_index as i32 + step).clamp(0, rows as i32 - 1) as usize
            };
        })),
        on_toggle_right_sidebar: Rc::new(RefCell::new(move || {
            let mut state = on_toggle_right_sidebar.borrow_mut();
            state.right_sidebar_open = !state.right_sidebar_open;
        })),
        // Toggle the agent/window borderless fullscreen. The fullscreen flag is
        // app-owned so it survives the per-frame rebuild; the platform window
        // follows via `window_control`.
        on_toggle_fullscreen: Rc::new(RefCell::new(move || {
            let mut state = on_toggle_fullscreen.borrow_mut();
            state.fullscreen = !state.fullscreen;
            window_control.set_fullscreen(state.fullscreen);
        })),
        // The pane header's expand control: one pane at a time is drawn over the
        // whole panes space. A second press on the same pane puts the tree back,
        // so the pane returns to the place it had — the layout is untouched.
        on_toggle_pane_maximized: Rc::new(RefCell::new(move |pane_id: u64| {
            let mut state = on_toggle_pane_maximized.borrow_mut();
            let retract = state.maximized_pane == Some(pane_id);
            // A pane has nothing to expand over while its space holds one: the
            // tree already is that pane. warp-new's toggle refuses the same case
            // (`pane_count() > 1`). Taking the expansion back is never refused,
            // so a pane left expanded by an edit that removed its neighbour
            // still has a way out.
            let expandable = state
                .spaces
                .get(state.active_space)
                .is_some_and(|space| space.root.leaf_count() > 1);
            if !retract && !expandable {
                return;
            }
            // The pane that takes the space is the pane the app works in, so the
            // expansion moves the focus with it.
            state.focus_pane(pane_id);
            state.maximized_pane = if retract { None } else { Some(pane_id) };
            state.sync_active_view();
        })),
        on_cron_create: Rc::new(RefCell::new(move || {
            let mut state = on_cron_create.borrow_mut();
            if let Some(desktop) = &desktop_cron_create {
                match desktop.create_workflow_with_script(
                    "New scheduled task",
                    "Created from the crons drawer",
                    NEW_CRON_SCRIPT,
                    Trigger::Cron {
                        expression: "0 12 * * *".to_string(),
                    },
                ) {
                    Ok(_) => state.refresh_crons(desktop),
                    Err(e) => log::warn!("create_workflow failed: {e}"),
                }
            } else {
                let id = format!("cr-{}", state.crons.len() + 1);
                state.crons.push(CronEntry::new(
                    id,
                    "New scheduled task",
                    "0 12 * * *",
                    "never",
                ));
            }
        })),
        on_cron_delete: Rc::new(RefCell::new(move |id: String| {
            let mut state = on_cron_delete.borrow_mut();
            if let Some(desktop) = &desktop_cron_delete {
                if let Err(e) = desktop.delete_workflow(&WorkflowId(id.clone())) {
                    log::warn!("delete_workflow failed: {e}");
                }
                state.refresh_crons(desktop);
            } else {
                state.crons.retain(|cron| cron.id != id);
            }
        })),
        on_cron_trigger: Rc::new(RefCell::new(move |id: String| {
            let mut state = on_cron_trigger.borrow_mut();
            let name = state
                .crons
                .iter()
                .find(|cron| cron.id == id)
                .map(|cron| cron.name.clone())
                .unwrap_or_else(|| id.clone());
            match &desktop_cron_trigger {
                // A trigger starts a real run on the workflow's body; the run
                // ledger and the run overlay are what report its progress.
                Some(desktop) => match desktop.run_workflow(&id) {
                    Ok(()) => {
                        state.refresh_crons(desktop);
                        state.push_active_message(ChatMessage::from_markdown(
                            ChatRole::Assistant,
                            format!("Started “{name}”. Its run appears in Workflow Runs."),
                        ));
                    }
                    Err(e) => state.push_active_message(ChatMessage::from_markdown(
                        ChatRole::Assistant,
                        format!("Could not start “{name}”: {e}"),
                    )),
                },
                None => {
                    state.push_active_message(ChatMessage::from_markdown(
                        ChatRole::Assistant,
                        format!("Started “{name}”."),
                    ));
                    let pane_id = state.active_pane_id;
                    if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                        rt.busy = true;
                    }
                    state.agent_busy = true;
                }
            }
        })),
        on_sidebar_drag_start: Rc::new(RefCell::new(move |pointer_x: f32| {
            let mut state = on_sidebar_drag_start.borrow_mut();
            state.sidebar_drag_origin_x = pointer_x;
            state.sidebar_drag_start_width = state.sidebar_width;
            state.sidebar_dragging = true;
        })),
        on_sidebar_drag_move: Rc::new(RefCell::new(move |pointer_x: f32| {
            let mut state = on_sidebar_drag_move.borrow_mut();
            let delta = pointer_x - state.sidebar_drag_origin_x;
            state.sidebar_width = (state.sidebar_drag_start_width + delta).clamp(200.0, 480.0);
        })),
        on_sidebar_drag_end: Rc::new(RefCell::new(move || {
            on_sidebar_drag_end.borrow_mut().sidebar_dragging = false;
        })),
        on_split_right: Rc::new(RefCell::new(move || {
            let (mut state, media) = (on_split_right.borrow_mut(), media_split_right.borrow());
            // The new pane is an agent *and* a terminal: a terminal leaf whose
            // harness is open. A chat leaf has no shell to return to, so the
            // pane would have no way back to the terminal mode (Esc, and the
            // pane's own `esc for terminal` chip, both need one).
            split_active_pane(
                &mut state,
                SplitDir::Horizontal,
                desktop_split_right.as_ref(),
                &media,
                PaneKind::Terminal,
            );
            drop(media);
            let pane_id = state.active_pane_id;
            open_pane_harness(&mut state, pane_id, desktop_split_right.as_ref());
            if let Some(desktop) = &desktop_split_right {
                state.save_panes(desktop);
            }
        })),
        on_split_down: Rc::new(RefCell::new(move || {
            let (mut state, media) = (on_split_down.borrow_mut(), media_split_down.borrow());
            split_active_pane(
                &mut state,
                SplitDir::Vertical,
                desktop_split_down.as_ref(),
                &media,
                PaneKind::Terminal,
            );
            drop(media);
            let pane_id = state.active_pane_id;
            open_pane_harness(&mut state, pane_id, desktop_split_down.as_ref());
            if let Some(desktop) = &desktop_split_down {
                state.save_panes(desktop);
            }
        })),
        // Cmd+Shift+L and the palette: BSP → master-stack → floating →
        // carousel. The space keeps its tree, so switching back never loses a
        // pane.
        on_cycle_layout: Rc::new(RefCell::new(move || {
            let mut state = on_cycle_layout.borrow_mut();
            state.cycle_space_layout(desktop_cycle_layout.as_deref());
            state.sync_active_view();
        })),
        // A press on a floating pane's header or its bottom-right grip. The
        // pane is raised as it starts moving, the way clicking a window does.
        on_float_start: Rc::new(RefCell::new(
            move |pane_id: u64, mode: FloatingDragMode, position: Vector2F, area: Vector2F| {
                let mut state = on_float_start.borrow_mut();
                let space_idx = state.active_space;
                let original = state
                    .spaces
                    .get(space_idx)
                    .and_then(|space| space.floating_rect(pane_id));
                let Some(original) = original else {
                    return;
                };
                *state.floating_drag.borrow_mut() = Some(FloatingDrag {
                    pane_id,
                    mode,
                    start: position,
                    original,
                    area,
                });
                state.focus_pane(pane_id);
                if let Some(space) = state.spaces.get_mut(space_idx) {
                    space.raise_pane(pane_id);
                }
            },
        )),
        on_float_rect: Rc::new(RefCell::new(move |pane_id: u64, rect: crate::ui::PaneRect| {
            let mut state = on_float_rect.borrow_mut();
            let space_idx = state.active_space;
            if let Some(space) = state.spaces.get_mut(space_idx) {
                space.set_floating_rect(pane_id, rect);
            }
        })),
        on_float_rect_end: Rc::new(RefCell::new(move || {
            let state = on_float_rect_end.borrow_mut();
            state.floating_drag.borrow_mut().take();
            if let Some(desktop) = &desktop_float_rect_end {
                state.save_panes(desktop);
            }
        })),
        // A window's minimize control: the window keeps its rect and its place
        // in the z-order and becomes a chip in the dock. When it was the pane
        // the keyboard was in, focus moves to the next visible window, so the
        // keys never stay on a window nobody can see.
        on_float_minimize: Rc::new(RefCell::new(move |pane_id: u64| {
            let mut state = on_float_minimize.borrow_mut();
            let space_idx = state.active_space;
            let active_id = state.active_pane_id;
            let mut next = None;
            if let Some(space) = state.spaces.get_mut(space_idx) {
                space.minimize_pane(pane_id);
                if active_id == pane_id {
                    next = space.visible_leaf_ids().first().copied();
                }
            }
            let dragged = state
                .floating_drag
                .borrow()
                .as_ref()
                .is_some_and(|drag| drag.pane_id == pane_id);
            if dragged {
                state.floating_drag.borrow_mut().take();
            }
            if let Some(next) = next {
                state.focus_pane(next);
                state.sync_active_view();
            }
            if let Some(desktop) = &desktop_float_minimize {
                state.save_panes(desktop);
            }
        })),
        // A dock chip: the window comes back at the rect it was docked with and
        // takes the focus, exactly as clicking an OS window's taskbar button
        // does.
        on_float_restore: Rc::new(RefCell::new(move |pane_id: u64| {
            let mut state = on_float_restore.borrow_mut();
            let space_idx = state.active_space;
            if let Some(space) = state.spaces.get_mut(space_idx) {
                space.restore_pane(pane_id);
            }
            state.focus_pane(pane_id);
            state.sync_active_view();
            if let Some(desktop) = &desktop_float_restore {
                state.save_panes(desktop);
            }
        })),
        // Cmd+Shift+N: read the saved hosts, then show the picker. The read
        // happens on open so an edit to hosts.toml is seen without a restart.
        on_open_ssh_host_picker: Rc::new(RefCell::new(move || {
            let mut state = on_open_ssh_host_picker.borrow_mut();
            state.reload_saved_hosts();
            state.ssh_host_picker_index = 0;
            state.ssh_host_picker_open = true;
            state.command_palette_open = false;
        })),
        on_close_ssh_host_picker: Rc::new(RefCell::new(move || {
            on_close_ssh_host_picker.borrow_mut().ssh_host_picker_open = false;
        })),
        on_ssh_host_move: Rc::new(RefCell::new(move |step: i32| {
            let mut state = on_ssh_host_move.borrow_mut();
            let len = state.ssh_host_entries.len();
            state.ssh_host_picker_index = if len == 0 {
                0
            } else {
                (state.ssh_host_picker_index as i32 + step).clamp(0, len as i32 - 1) as usize
            };
        })),
        on_ssh_host_pick: Rc::new(RefCell::new(move |index: usize| {
            let mut state = on_ssh_host_pick.borrow_mut();
            let host = state.ssh_host_entries.get(index).cloned();
            state.ssh_host_picker_open = false;
            if let Some(host) = host {
                let media = media_ssh_host.borrow();
                open_ssh_host_pane(&mut state, desktop_ssh_host.as_ref(), &media, &host);
            }
        })),
        // The reconnect offer: restart the pane's dead shell and run its host's
        // line again. The saved host list is read here when the picker has not
        // filled it yet (a pane restored after a restart).
        on_ssh_reconnect: Rc::new(RefCell::new(move || {
            let mut state = on_ssh_reconnect.borrow_mut();
            let Some((pane_id, name)) = state.ssh_reconnect_prompt.take() else {
                return;
            };
            if state.ssh_host_entries.is_empty() {
                state.reload_saved_hosts();
            }
            if !reconnect_ssh_host(&mut state, desktop_ssh_reconnect.as_ref(), pane_id, &name) {
                log::warn!("saved host `{name}` is no longer in hosts.toml");
            }
        })),
        on_ssh_reconnect_dismiss: Rc::new(RefCell::new(move || {
            on_ssh_reconnect_dismiss.borrow_mut().ssh_reconnect_prompt = None;
        })),
        // A line typed straight into the grid reaches the shell without the
        // rich input bar, so the SSH binding is read here too.
        on_shell_line: Rc::new(RefCell::new(move |pane_id: u64, line: String| {
            on_shell_line.borrow_mut().note_shell_line(pane_id, &line);
        })),
        on_new_terminal: Rc::new(RefCell::new(move || {
            let (mut state, media) = (on_new_terminal.borrow_mut(), media_new_terminal.borrow());
            split_active_pane(
                &mut state,
                SplitDir::Vertical,
                desktop_new_terminal.as_ref(),
                &media,
                PaneKind::Terminal,
            );
            drop(media);
            if let Some(desktop) = &desktop_new_terminal {
                state.save_panes(desktop);
            }
        })),
        on_close_pane: Rc::new(RefCell::new(move || {
            let mut state = on_close_pane.borrow_mut();
            let pane_id = state.active_pane_id;
            state.request_close_pane(pane_id, desktop_close_pane.as_deref());
        })),
        on_close_unsaved_save: Rc::new(RefCell::new(move || {
            let mut state = on_close_unsaved_save.borrow_mut();
            // Only one of the two can be pending: a file chip or a whole pane.
            state.close_pending_file_save(desktop_close_unsaved_save.as_deref());
            state.close_pending_pane_save(desktop_close_unsaved_save.as_deref());
        })),
        on_close_unsaved_discard: Rc::new(RefCell::new(move || {
            let mut state = on_close_unsaved_discard.borrow_mut();
            state.close_pending_file_discard(desktop_close_unsaved_discard.as_deref());
            state.close_pending_pane_discard(desktop_close_unsaved_discard.as_deref());
        })),
        on_close_unsaved_cancel: Rc::new(RefCell::new(move || {
            let mut state = on_close_unsaved_cancel.borrow_mut();
            state.cancel_pending_file_close();
            state.cancel_pending_pane_close();
        })),
        // Cmd+Enter in a terminal pane: route the pane's current input through
        // the default harness (an agent turn), scoped to that pane's own
        // conversation + cwd + medium/project. This mirrors `on_send_message`
        // for the chat composer, but keyed to the terminal's pane so the turn
        // lands on the right conversation and project.
        // Cmd/Ctrl+Enter in a terminal pane: run the turn on the terminal's own
        // conversation. A brand-new conversation is only created once, when the
        // pane has no conversation yet, so subsequent messages keep appending to
        // the thread the user is on (no accidental new conversation each send).
        on_terminal_command: Rc::new(RefCell::new(move |pane_id: u64, text: String| {
            let mut state = on_terminal_cmd.borrow_mut();
            // The terminal pane's cwd is the working dir its shell was spawned
            // in; keep that path so the agent turn is scoped to the same folder.
            let cwd = state
                .pane_sessions
                .get(&pane_id)
                .map(|s| s.path.clone())
                .unwrap_or_default();
            // A turn that cannot run opens no conversation either: no model means
            // nothing to answer the prompt, so the composer shows the notice band
            // and the sidebar gains no row for a turn that never happened.
            if state.can_run_agent_turn(pane_id) && !state.pane_owns_conversation(pane_id) {
                state.bind_pane_new_conversation(pane_id, desktop_term_cmd.as_deref());
            }
            // Re-seat the pane's path (a fresh pane session may not have one).
            if let Some(session) = state.pane_sessions.get_mut(&pane_id) {
                if session.path.is_empty() {
                    session.path = cwd.clone();
                }
            }
            send_agent_prompt(
                &mut state,
                desktop_term_cmd.as_ref(),
                &media_term_cmd,
                &text,
                pane_id,
            );
        })),
        // The terminal pane's rich input: a submitted line is a shell command
        // for this pane's own PTY, exactly what the grid above the bar does
        // with a line the user types there.
        on_run_shell_command: Rc::new(RefCell::new(move |pane_id: u64, text: String| {
            let mut state = on_run_shell_command.borrow_mut();
            // The shell's own bar: the command is the shell's, so it stays in
            // the shell's history (no conversation to own it).
            run_terminal_command(&mut state, &text, pane_id, None);
            // The composer cleared its own editor on submit; the app-owned draft
            // is what the next frame rebuilds that editor from, so it clears
            // here too — for this pane, which the submit just activated.
            if let Some(session) = state.pane_sessions.get_mut(&pane_id) {
                session.draft.clear();
            }
            if state.active_pane_id == pane_id {
                state.composer_draft.clear();
            }
        })),
        // Launch a real TUI agent (codex/claude/...) inside a pane's PTY. The
        // pane must already have a terminal session (a terminal pane); we ensure
        // one so a freshly split pane also works, then hand the command to the
        // shell and flip the pane to native-first agent mode.
        on_launch_tui_agent: Rc::new(RefCell::new(move |pane_id: u64, command: String| {
            // The shell starts in this pane's own directory, the directory its
            // pill names: a shell started anywhere else reports where it landed,
            // and the pane follows its own shell.
            let cwd = on_launch_tui_agent
                .borrow()
                .pane_sessions
                .get(&pane_id)
                .map(|session| session.path.clone())
                .unwrap_or_default();
            let terminal = on_launch_tui_agent.borrow().terminal.clone();
            let mut reg = terminal.borrow_mut();
            reg.ensure_session(pane_id, &cwd);
            reg.launch_agent(pane_id, &command);
            reg.set_input(pane_id, String::new());
        })),
        on_select_space: Rc::new(RefCell::new(move |index: usize| {
            let mut state = on_select_space.borrow_mut();
            if index < state.spaces.len() {
                state.active_space = index;
                state.spaces[index].recompute_layout();
                state.active_pane_id = state.spaces[index].root.first_leaf_id();
                // The tab carries the environment it was opened on. Bring the
                // window's selector (and the sidebar's filter) onto it, or the
                // tab would show the previous tab's conversations.
                state.workspace_routing = None;
                adopt_space_medium(&mut state, &media_select_space, index);
            }
            state.sync_active_view();
            if let Some(desktop) = &desktop_select_space {
                state.refresh_messages(desktop);
                state.save_panes(desktop);
            }
            adopt_space_routing(&mut state, index);
        })),
        // Walk the tab strip (Ctrl+Tab / Ctrl+Shift+Tab). A single-tab
        // workspace has nowhere to go, so nothing is refreshed or persisted.
        on_switch_space: Rc::new(RefCell::new(move |delta: i32| {
            let mut state = on_switch_space.borrow_mut();
            if !state.switch_space(delta) {
                return;
            }
            let active = state.active_space;
            state.workspace_routing = None;
            adopt_space_medium(&mut state, &media_switch_space, active);
            state.sync_active_view();
            if let Some(desktop) = &desktop_switch_space {
                state.refresh_messages(desktop);
                state.save_panes(desktop);
            }
            adopt_space_routing(&mut state, active);
        })),
        // Close a space (tab) by index. The last remaining space is never
        // removed: it resets to a fresh empty chat pane, so closing a tab never
        // exits the program (the previous behavior owned the single pane and a
        // close on it tore the window down).
        on_close_space: Rc::new(RefCell::new(move |index: usize| {
            let mut state = on_close_space.borrow_mut();
            close_space_at(&mut state, index, desktop_close_space.as_deref());
            // The tab that took the closed one's place brings its own
            // environment with it.
            let active = state.active_space;
            adopt_space_medium(&mut state, &media_close_space, active);
            adopt_space_routing(&mut state, active);
        })),
        on_add_space: Rc::new(RefCell::new(move || {
            let mut state = on_add_space.borrow_mut();
            let medium_id = media_add_space.borrow().selected_medium_id().to_string();
            let routing = crate::media::medium_routing(&medium_id);
            let id = state.next_pane_id;
            state.next_pane_id += 1;
            // An unnamed space: its tab derives its label from what it holds —
            // here the working directory the pane opens in.
            state.spaces.push(
                Space::unnamed(Pane::Leaf { id, kind: PaneKind::Terminal })
                    .with_medium(medium_id.clone()),
            );
            state.active_space = state.spaces.len() - 1;
            state.active_pane_id = id;
            state.workspace_routing = Some(WorkspaceRouting::from_routing(routing));
            ensure_pane_hover(&mut state, id);
            // A new space is a plain PTY with its own cwd. No conversation is
            // created here: the thread appears only once an agent turn runs in
            // the pane (see `on_terminal_command`), so opening a workspace never
            // leaves an empty "New conversation" row behind.
            let project_id = media_add_space.borrow().default_project_for_medium(&medium_id);
            let path = default_pane_path(&project_id, desktop_add_space.as_deref());
            state.set_active_pane_path(path);
            state.refresh_space_labels();
            state.sync_active_view();
            if let Some(desktop) = &desktop_add_space {
                state.save_panes(desktop);
            }
        })),
        // Add a new space whose default environment is `medium_id`: open a fresh
        // PTY pane scoped to that medium's default project. The workspace has no
        // conversation until an agent turn runs in it.
        on_add_space_with_medium: Rc::new(RefCell::new(move |medium_id: String| {
            let mut state = on_add_space_with_medium.borrow_mut();
            // A space on Remote before anything is configured asks for the
            // configuration first; saving opens the space.
            if medium_id == "remote" && !state.remote_configured() {
                state.open_remote_settings_tab(desktop_add_space_with_medium.as_deref());
                return;
            }
            add_space_with_medium(
                &mut state,
                &media_add_space_with_medium,
                desktop_add_space_with_medium.as_deref(),
                &medium_id,
            );
        })),
        // The topbar "+" menu's "Add new medium…" item opens the add-medium dialog.
        on_open_add_medium: Rc::new(RefCell::new(move || {
            let mut state = on_open_add_medium.borrow_mut();
            state.add_medium_dialog_open = true;
            state.add_medium_draft.clear();
            state.add_medium_focused = true;
        })),
        on_add_medium_draft_change: Rc::new(RefCell::new(move |value: String| {
            on_add_medium_draft_change.borrow_mut().add_medium_draft = value;
        })),
        on_add_medium_focus_change: Rc::new(RefCell::new(move |focused: bool| {
            on_add_medium_focus_change.borrow_mut().add_medium_focused = focused;
        })),
        on_add_medium_close: Rc::new(RefCell::new(move || {
            let mut state = on_add_medium_close.borrow_mut();
            state.add_medium_dialog_open = false;
            state.add_medium_draft.clear();
            state.add_medium_focused = false;
        })),
        on_space_press: Rc::new(RefCell::new(move |index: usize, at: Vector2F| {
            let mut state = on_space_press.borrow_mut();
            state.space_press = Some(index);
            state.space_press_at = Some(at);
            state.space_hover = Some(index);
            // The press, not the release, is what the double click is counted
            // from: the count is warp-new's own rule (`SpacePressRun`), read
            // here and acted on by `on_workspace_click` when the button comes
            // back up. A press on a different tab, or one that arrives after
            // the window, starts a run of its own.
            let now = std::time::Instant::now();
            let run = state.space_press_run.clone();
            let count = run
                .borrow()
                .filter(|run| {
                    run.index == index
                        && now.duration_since(run.pressed_at).as_millis() <= MULTI_CLICK_INTERVAL_MS
                })
                .map(|run| run.count + 1)
                .unwrap_or(1);
            *run.borrow_mut() = Some(SpacePressRun {
                index,
                pressed_at: now,
                count,
            });
        })),
        on_space_hover: Rc::new(RefCell::new(move |hover: Option<usize>| {
            on_space_hover.borrow_mut().space_hover = hover;
        })),
        on_space_reorder: Rc::new(RefCell::new(move |from: usize, to: usize| {
            let mut state = on_space_reorder.borrow_mut();
            let len = state.spaces.len();
            if from >= len || to >= len {
                return;
            }
            // The tab follows the pointer from `to` on, even while it has not
            // crossed a neighbour yet: that is what tells the release this was
            // a drag and not a click. A move that changes nothing still records
            // it, so a press that travelled never turns into a select.
            state.space_drag = Some(to);
            if from == to {
                return;
            }
            state.reorder_space(from, to);
            if let Some(desktop) = &desktop_space_reorder {
                state.save_panes(desktop);
            }
        })),
        on_space_release: Rc::new(RefCell::new(move || {
            let mut state = on_space_release.borrow_mut();
            state.space_press = None;
            state.space_press_at = None;
            state.space_drag = None;
            state.space_hover = None;
        })),
        on_pane_activate: Rc::new(RefCell::new(move |id: u64| {
            let mut state = on_pane_activate.borrow_mut();
            state.focus_pane(id);
            // A click on a floating pane raises it: its place in the z-order is
            // the space's own state and is persisted with the rect.
            let space_idx = state.active_space;
            if let Some(space) = state.spaces.get_mut(space_idx) {
                if space.layout == LayoutKind::Floating {
                    space.raise_pane(id);
                }
            }
            state.sync_active_view();
            if let Some(desktop) = &desktop_pane_activate {
                state.refresh_messages(desktop);
                state.save_panes(desktop);
            }
        })),
        // Ctrl/Cmd+Arrow: move focus to the pane adjacent to the active one in
        // the active space. The focus highlight follows `active_pane_id`.
        on_pane_navigate: Rc::new(RefCell::new(move |dir: NavDir| {
            let mut state = on_pane_navigate.borrow_mut();
            if state.current_tab != AppTab::Chat {
                return;
            }
            let space_idx = state.active_space;
            let Some(space) = state.spaces.get(space_idx) else {
                return;
            };
            if let Some(next) = space.navigate(state.active_pane_id, dir) {
                state.focus_pane(next);
                state.sync_active_view();
                if let Some(desktop) = &desktop_pane_navigate {
                    state.refresh_messages(desktop);
                    state.save_panes(desktop);
                }
            }
        })),
        on_pane_drag_start: Rc::new(RefCell::new(move |id: u64| {
            on_pane_drag_start.borrow_mut().dragging_pane_id = Some(id);
        })),
        on_pane_drag_move: Rc::new(RefCell::new(move |id: u64, ratio: f32| {
            let mut state = on_pane_drag_move.borrow_mut();
            let space_idx = state.active_space;
            if let Some(space) = state.spaces.get_mut(space_idx) {
                space.set_ratio(id, ratio);
            }
        })),
        on_pane_drag_end: Rc::new(RefCell::new(move || {
            let mut state = on_pane_drag_end.borrow_mut();
            state.dragging_pane_id = None;
            if let Some(desktop) = &desktop_pane_drag {
                state.save_panes(desktop);
            }
        })),
        on_pane_lift: Rc::new(RefCell::new(move |pane_id: u64, position: Vector2F| {
            on_pane_lift.borrow_mut().begin_pane_lift(pane_id, position);
        })),
        on_pane_lift_move: Rc::new(RefCell::new(
            move |position: Vector2F, drop_index: Option<usize>, drop_target| {
                on_pane_lift_move
                    .borrow_mut()
                    .drag_pane_to(position, drop_index, drop_target);
            },
        )),
        // Drop the lifted pane on the tab strip, where it becomes a tab of its
        // own carrying the pane itself (its id and its kind, so its PTY, its
        // conversation and its file follow it), or beside the pane it was
        // released over in the active space, where it becomes a new split. A
        // release over neither cancels. The space it left collapses onto its
        // surviving pane, is consumed when it held nothing else, or — the last
        // one — resets to a fresh empty chat pane exactly as closing it would,
        // so the window is never left without a tab.
        on_pane_drop: Rc::new(RefCell::new(
            move |drop_index: Option<usize>, drop_target| {
                let mut state = on_pane_drop.borrow_mut();
                match state.release_pane_drag(drop_index, drop_target) {
                    crate::state::PaneDrop::MovedInSpace => {
                        state.refresh_space_labels();
                        state.sync_active_view();
                        if let Some(desktop) = &desktop_pane_drop {
                            state.refresh_messages(desktop);
                            state.save_panes(desktop);
                        }
                    }
                    crate::state::PaneDrop::NewTab(moved) => {
                        if let Some(fresh) = moved.reset_pane {
                            ensure_pane_hover(&mut state, fresh);
                            state.bind_pane_new_conversation(fresh, desktop_pane_drop.as_deref());
                        }
                        state.refresh_space_labels();
                        state.sync_active_view();
                        if let Some(desktop) = &desktop_pane_drop {
                            state.refresh_messages(desktop);
                            state.save_panes(desktop);
                        }
                    }
                    crate::state::PaneDrop::Cancelled => {}
                }
            },
        )),
        on_agent_delete: Rc::new(RefCell::new(move |id: String| {
            let mut state = on_agent_delete.borrow_mut();
            state.conversations.retain(|c| c.id != id);
            state.agent_cards.remove(&id);
            if state.selected_id.as_deref() == Some(id.as_str()) {
                let next = state.conversations.first().map(|c| c.id.clone());
                // If the deleted conversation was the active pane's own
                // conversation, rebind the pane to the next selected one.
                let owned_by_active = state
                    .pane_sessions
                    .get(&state.active_pane_id)
                    .map(|s| s.conversation_id == id)
                    .unwrap_or(false);
                state.selected_id = next.clone();
                if let Some(next) = next {
                    if owned_by_active {
                        state.bind_active_pane_conversation(next, desktop_agent_delete.as_deref());
                    }
                }
            }
        })),
        on_settings_back: Rc::new(RefCell::new(move || {
            on_settings_back.borrow_mut().current_tab = AppTab::Chat;
        })),
        on_settings_navigate: Rc::new(RefCell::new(move |page: SettingsPage| {
            on_settings_navigate.borrow_mut().settings_page = page;
        })),
        on_toggle_dark_mode: action_toggle_dark_mode.clone(),
        // Set a custom theme color (as `#rrggbb`) and persist it in config so
        // the live scheme matches what the color wheel picked.
        on_set_theme_primary: Rc::new(RefCell::new(move |hex: String| {
            on_set_theme_primary.borrow_mut().theme_primary = Some(hex.clone());
            if let Some(desktop) = &desktop_theme_primary {
                let mut config = desktop.config();
                config.theme.primary = Some(hex);
                if let Err(e) = desktop.save_config(&config) {
                    log::warn!("save_config (theme) failed: {e}");
                }
            }
        })),
        on_set_theme_secondary: Rc::new(RefCell::new(move |hex: String| {
            on_set_theme_secondary.borrow_mut().theme_secondary = Some(hex.clone());
            if let Some(desktop) = &desktop_theme_secondary {
                let mut config = desktop.config();
                config.theme.secondary = Some(hex);
                if let Err(e) = desktop.save_config(&config) {
                    log::warn!("save_config (theme) failed: {e}");
                }
            }
        })),
        on_set_theme_accent: Rc::new(RefCell::new(move |hex: String| {
            on_set_theme_accent.borrow_mut().theme_accent = Some(hex.clone());
            if let Some(desktop) = &desktop_theme_accent {
                let mut config = desktop.config();
                config.theme.accent = hex;
                if let Err(e) = desktop.save_config(&config) {
                    log::warn!("save_config (theme) failed: {e}");
                }
            }
        })),
        on_save_profile: Rc::new(RefCell::new(move |name: String, email: String| {
            let mut state = on_save_profile.borrow_mut();
            state.settings_profile_name = name;
            state.settings_profile_email = email;
        })),
        on_save_llm: Rc::new(RefCell::new(
            move |provider: String,
                  model: String,
                  api_key: String,
                  base_url: String,
                  temperature: String| {
                let mut state = on_save_llm.borrow_mut();
                state.settings_llm_provider = provider.clone();
                state.settings_llm_model = model;
                state.settings_llm_api_key = api_key;
                state.settings_llm_base_url = base_url;
                state.settings_llm_temperature = temperature;
                if let Some(desktop) = &desktop_save_llm {
                    let base = if state.settings_llm_base_url.trim().is_empty() {
                        None
                    } else {
                        Some(state.settings_llm_base_url.as_str())
                    };
                    let temperature = state.settings_llm_temperature.parse::<f32>().ok();
                    if let Err(e) = desktop.set_llm_setting(
                        &provider,
                        &state.settings_llm_api_key,
                        base,
                        &state.settings_llm_model,
                        temperature,
                    ) {
                        log::warn!("set_llm_setting failed: {e}");
                    }
                    // Mirror the configured model into the global
                    // `~/.goble/config.toml` so it shows in the composer's
                    // model tray / slash picker (models are config-driven).
                    let mut config = desktop.config();
                    let model_id = state.settings_llm_model.trim().to_string();
                    if !model_id.is_empty() {
                        // The adopted schema keys a model by slug; an entry that
                        // already declares this id keeps its slug.
                        let slug = config
                            .model_for(&model_id)
                            .map(|(slug, _)| slug.to_string())
                            .unwrap_or_else(|| model_id.clone());
                        let entry = config.model.entry(slug.clone()).or_default();
                        entry.model = model_id.clone();
                        if entry.name.is_none() {
                            entry.name = Some(model_id.clone());
                        }
                        if !state.settings_llm_base_url.trim().is_empty() {
                            entry.base_url = Some(state.settings_llm_base_url.clone());
                        }
                        if !state.settings_llm_api_key.trim().is_empty() {
                            entry.api_key = Some(state.settings_llm_api_key.clone());
                        }
                        entry.hidden = None;
                        config.models.default = Some(slug);
                    }
                    if let Err(e) = desktop.save_config(&config) {
                        log::warn!("save_config (llm) failed: {e}");
                    }
                }
                // The composer should reflect the newly configured model.
                state.selected_model = state.settings_llm_model.clone();
                if let Some(desktop) = &desktop_save_llm {
                    state.models = desktop.available_models(&state.settings_llm_provider);
                }
                state.llm_dialog_open = false;
                // First run: now that a key is configured, ask where the agent
                // should run before continuing the conversation.
                if !state.settings_llm_api_key.trim().is_empty() {
                    state.show_llm_key_banner = false;
                    state.show_workspace_choice = true;
                }
            },
        )),
        on_add_worker: Rc::new(RefCell::new(move |name: String, url: String| {
            let mut state = on_add_worker.borrow_mut();
            let id = format!(
                "w-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            );
            if let Some(desktop) = &desktop_add_worker {
                if let Err(e) = desktop.add_worker(WorkerId(id), name.clone(), url) {
                    log::warn!("add_worker failed: {e}");
                }
                state.refresh_settings(desktop);
            } else {
                state.settings_workers.push((id, name, url, false));
            }
        })),
        on_remove_worker: Rc::new(RefCell::new(move |id: String| {
            let mut state = on_remove_worker.borrow_mut();
            if let Some(desktop) = &desktop_remove_worker {
                desktop.remove_worker(&WorkerId(id.clone()));
                state.refresh_settings(desktop);
            } else {
                state.settings_workers.retain(|w| w.0 != id);
            }
        })),
        // A link clicked in the transcript: hand it to the guarded external
        // opener, which refuses every non-http(s) scheme.
        on_open_url: Rc::new(RefCell::new(move |url: String| {
            if let Err(e) = crate::state::open_external_url(&url) {
                log::warn!("transcript link not opened: {e}");
            }
        })),
        on_vault_unlock: Rc::new(RefCell::new(move |passphrase: String| {
            let mut state = on_vault_unlock.borrow_mut();
            if let Some(desktop) = &desktop_unlock_vault {
                match desktop.unlock_vault(passphrase) {
                    Ok(_) => {
                        state.settings_vault_unlocked = true;
                        state.refresh_settings(desktop);
                    }
                    Err(e) => log::warn!("unlock_vault failed: {e}"),
                }
            } else {
                state.settings_vault_unlocked = true;
            }
        })),
        on_create_cluster: Rc::new(RefCell::new(move |name: String, passphrase: String| {
            let mut state = on_create_cluster.borrow_mut();
            if let Some(desktop) = &desktop_create_cluster {
                match desktop.create_cluster(&name, &passphrase) {
                    Ok(_) => {
                        state.settings_cluster_name = name;
                        state.settings_cluster_configured = true;
                    }
                    Err(e) => log::warn!("create_cluster failed: {e}"),
                }
            } else {
                state.settings_cluster_name = name;
                state.settings_cluster_configured = true;
            }
        })),
        on_unlock_cluster: Rc::new(RefCell::new(move |passphrase: String| {
            let mut state = on_unlock_cluster.borrow_mut();
            if let Some(desktop) = &desktop_unlock_cluster {
                match desktop.unlock_cluster_identity(&passphrase) {
                    Ok(true) => state.refresh_settings(desktop),
                    Ok(false) => log::warn!("unlock_cluster_identity returned false"),
                    Err(e) => log::warn!("unlock_cluster_identity failed: {e}"),
                }
            } else {
                state.settings_cluster_configured = true;
            }
        })),
        on_add_authorized_key: Rc::new(RefCell::new(
            move |name: String, pem: String, fingerprint: String| {
                let mut state = on_add_authorized_key.borrow_mut();
                let id = format!("k-{}", state.settings_authorized_keys.len() + 1);
                state.settings_authorized_keys.push((id, name, fingerprint));
                let _ = pem;
            },
        )),
        on_remove_authorized_key: Rc::new(RefCell::new(move |id: String| {
            on_remove_authorized_key
                .borrow_mut()
                .settings_authorized_keys
                .retain(|k| k.0 != id);
        })),
        on_config_llm_key: Rc::new(RefCell::new(move || {
            let mut state = on_config_llm_key.borrow_mut();
            state.prime_llm_form();
            state.llm_dialog_open = true;
        })),
        on_open_config_file: Rc::clone(&action_open_config_file),
        on_file_chip_select: Rc::clone(&action_file_chip_select),
        on_file_chip_close: Rc::clone(&action_file_chip_close),
        on_close_llm_dialog: Rc::new(RefCell::new(move || {
            on_close_llm_dialog.borrow_mut().llm_dialog_open = false;
        })),
        on_choose_workspace: action_choose_workspace.clone(),
        // First-run: dismiss the model-key banner without configuring a key.
        // This is not a dead end: it marks onboarding complete so the flow
        // moves on, and the user can still configure a key later via settings.
        on_dismiss_llm_key_banner: Rc::new(RefCell::new(move || {
            let mut state = on_dismiss_llm_key_banner.borrow_mut();
            state.show_llm_key_banner = false;
            state.onboarding_done = true;
            if let Some(desktop) = &desktop_dismiss_key {
                if let Err(e) = desktop.set_onboarding_done() {
                    log::warn!("set_onboarding_done failed: {e}");
                }
            }
        })),
        // First-run: dismiss the local/remote workspace choice without deciding.
        on_dismiss_workspace_choice: Rc::new(RefCell::new(move || {
            let mut state = on_dismiss_workspace_choice.borrow_mut();
            state.show_workspace_choice = false;
            state.show_llm_key_banner = false;
            state.onboarding_done = true;
            if let Some(desktop) = &desktop_dismiss_workspace {
                if let Err(e) = desktop.set_onboarding_done() {
                    log::warn!("set_onboarding_done failed: {e}");
                }
            }
            state.current_tab = AppTab::Chat;
        })),
        // First-run: dismiss the compact getting-started tip.
        on_dismiss_onboarding_tip: Rc::new(RefCell::new(move || {
            on_dismiss_onboarding_tip.borrow_mut().show_onboarding_tip = false;
        })),
        // Cmd+K: toggle the command palette overlay.
        on_toggle_command_palette: Rc::new(RefCell::new(move || {
            let mut state = on_toggle_command_palette.borrow_mut();
            let was_open = state.command_palette_open;
            state.command_palette_open = !was_open;
            if state.command_palette_open {
                state.command_palette_query.clear();
                state.command_palette_index = 0;
            }
        })),
        on_close_command_palette: Rc::new(RefCell::new(move || {
            on_close_command_palette.borrow_mut().command_palette_open = false;
        })),
        on_command_palette_change: Rc::new(RefCell::new(move |value: String| {
            let mut state = on_command_palette_change.borrow_mut();
            state.command_palette_query = value;
            state.command_palette_index = 0;
        })),
        on_command_palette_move: Rc::new(RefCell::new(move |index: usize| {
            on_command_palette_move.borrow_mut().command_palette_index = index;
        })),
    }
}

/// Bring the window's environment selector onto the tab at `index`.
///
/// A space carries the medium it was opened on, but the sidebar filter and the
/// topbar's selector read the window's selected medium. Without this, a switch
/// onto a local tab would keep listing the previous tab's environment and the
/// local conversations would stay out of sight.
fn adopt_space_medium(state: &mut UiState, media: &Rc<RefCell<MediaState>>, index: usize) {
    let Some(medium_id) = state
        .spaces
        .get(index)
        .map(|space| space.medium.clone())
    else {
        return;
    };
    if !medium_id.trim().is_empty() {
        let _ = media.borrow_mut().select_medium(&medium_id);
    }
}

/// Open a new space whose default environment is `medium_id`: a fresh PTY
/// pane scoped to that medium's default project. The workspace has no
/// conversation until an agent turn runs in it. Shared by the topbar "+" menu
/// and by the remote-setup dialog, which opens the space after it configures
/// the environment the space asked for.
fn add_space_with_medium(
    state: &mut UiState,
    media: &Rc<RefCell<MediaState>>,
    desktop: Option<&DesktopState>,
    medium_id: &str,
) {
    let routing = crate::media::medium_routing(medium_id);
    let id = state.next_pane_id;
    state.next_pane_id += 1;
    state.spaces.push(
        Space::unnamed(Pane::Leaf {
            id,
            kind: PaneKind::Terminal,
        })
        .with_medium(medium_id.to_string()),
    );
    state.active_space = state.spaces.len() - 1;
    state.active_pane_id = id;
    state.workspace_routing = Some(WorkspaceRouting::from_routing(routing));
    // Make the chosen environment the active medium so the new space's turns
    // run there and the sidebar shows that environment's folders.
    let _ = media.borrow_mut().select_medium(medium_id);
    ensure_pane_hover(state, id);
    // Plain PTY: the thread is created lazily, on the pane's first agent turn,
    // so "+" never creates an empty conversation.
    let project_id = media.borrow().default_project_for_medium(medium_id);
    let path = default_pane_path(&project_id, desktop);
    state.set_active_pane_path(path);
    state.refresh_space_labels();
    state.sync_active_view();
    if let Some(desktop) = desktop {
        state.save_panes(desktop);
    }
}

/// Fall back to the tab's environment for the turn route when the pane did not
/// name one of its own. A conversation's persisted routing is the pane's own
/// choice and wins; a tab whose fresh pane has no conversation yet routes where
/// the tab was opened.
fn adopt_space_routing(state: &mut UiState, index: usize) {
    if state.workspace_routing.is_some() {
        return;
    }
    let Some(medium_id) = state
        .spaces
        .get(index)
        .map(|space| space.medium.clone())
    else {
        return;
    };
    if !medium_id.trim().is_empty() {
        state.workspace_routing = Some(WorkspaceRouting::from_routing(crate::media::medium_routing(
            &medium_id,
        )));
    }
}

/// One git job the review pane can start. The commit carries the message the
/// bar held when the user pressed Commit.
enum GitJob {
    Commit(String),
    Push,
    CreatePr,
    Revert(String),
}

/// Run `job` in `cwd` on its own thread and put its word in `result`. A push
/// or a pull request waits on the network, so the frame must not.
fn spawn_git_job(cwd: String, job: GitJob, result: DiffGitResult) {
    std::thread::spawn(move || {
        let outcome = run_git_job(&cwd, job);
        if let Ok(mut slot) = result.lock() {
            *slot = Some(outcome);
        }
    });
}

fn run_git_job(cwd: &str, job: GitJob) -> Result<String, String> {
    match job {
        GitJob::Commit(message) => {
            let add = git(cwd, &["add", "-A"])?;
            if !add.status.success() {
                return Err(git_failure(&add));
            }
            let commit = git(cwd, &["commit", "-m", &message])?;
            if !commit.status.success() {
                return Err(git_failure(&commit));
            }
            Ok("Committed".to_string())
        }
        GitJob::Push => {
            push_branch(cwd)?;
            Ok("Pushed".to_string())
        }
        GitJob::CreatePr => {
            // `gh` asks a terminal where to push a branch that has no upstream,
            // and a worker has no terminal. Pushing first makes the branch's
            // remote explicit, so the pull request is the only question left.
            push_branch(cwd)?;
            let output = Command::new("gh")
                .current_dir(cwd)
                .args(["pr", "create", "--fill"])
                .output()
                .map_err(|error| format!("gh is not available: {error}"))?;
            if !output.status.success() {
                return Err(git_failure(&output));
            }
            Ok(first_line(&output.stdout).unwrap_or_else(|| "Pull request created".to_string()))
        }
        GitJob::Revert(path) => {
            let tracked = git(cwd, &["ls-files", "--error-unmatch", "--", &path])?;
            let output = if tracked.status.success() {
                git(cwd, &["checkout", "HEAD", "--", &path])?
            } else {
                // An untracked file has no `HEAD` version to restore; its whole
                // content is the change, so reverting removes it.
                git(cwd, &["clean", "-f", "--", &path])?
            };
            if !output.status.success() {
                return Err(git_failure(&output));
            }
            Ok(format!("Reverted {path}"))
        }
    }
}

/// Push the checked-out branch. A branch that was never pushed has no
/// upstream; the first push creates one. Any other refusal is reported as it
/// came.
fn push_branch(cwd: &str) -> Result<(), String> {
    let push = git(cwd, &["push"])?;
    if push.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&push.stderr);
    if !stderr.contains("no upstream") && !stderr.contains("set-upstream") {
        return Err(git_failure(&push));
    }
    let set = git(cwd, &["push", "--set-upstream", "origin", "HEAD"])?;
    if !set.status.success() {
        return Err(git_failure(&set));
    }
    Ok(())
}

fn git(cwd: &str, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|error| format!("git is not available: {error}"))
}

/// The one line a failure is reported by: the last non-empty line git wrote,
/// which is the reason rather than the hint above it.
fn git_failure(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    last_line(&stderr)
        .or_else(|| last_line(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_else(|| "git failed".to_string())
}

fn first_line(bytes: &[u8]) -> Option<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn last_line(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

/// Run the global search over the active pane's working directory and store the
/// rows. The query field calls this when the query changes, never per frame:
/// the search reads every file under the root, so it runs on the worker's own
/// thread and the frame collects the answer when it lands. Clearing the query
/// abandons a walk in flight and clears the results rather than searching for
/// nothing.
fn run_global_search(state: &mut UiState) {
    let query = state.global_search_query.clone();
    if query.trim().is_empty() {
        state.global_search_worker.cancel();
        state.global_search_rows.clear();
        state.global_search_searched = false;
        state.global_search_error = None;
        return;
    }
    let root = state.active_working_directory();
    state.global_search_worker.search(&root, &query);
    // Until the walk answers, the summary says it is still searching. The rows
    // of the previous query stay on screen meanwhile, so typing does not blank
    // the band on every keystroke.
    state.global_search_searched = false;
    state.global_search_error = None;
}
