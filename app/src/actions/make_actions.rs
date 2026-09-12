use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::agent::Trigger;
use goble_core::worker::WorkerId;
use goble_core::workflow::WorkflowId;
use goble_desktop_service::DesktopState;
use goble_ui::platform::WindowControl;
use goble_ui::{ChatMessage, ChatRole, ConversationEntry, SettingsPage};

use crate::media::MediaState;
use crate::state::{default_pane_path, routing_to_str, UiState};
use crate::terminal::{classify_input, InputClass};
use crate::ui::{
    AppTab, CronEntry, NavDir, Pane, PaneKind, SettingsCategory, Space, SplitDir, UiActions,
    WorkspaceRouting,
};
use crate::ui::color_picker::ColorTarget;

use super::pane_ops::{collect_leaf_ids, ensure_pane_hover, split_active_pane};
use super::prompt::{run_terminal_command, send_agent_prompt};

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
    let on_toggle_conversations_expanded = Rc::clone(&state);
    let on_workspace_click = Rc::clone(&state);
    let on_space_rename_change = Rc::clone(&state);
    let on_space_rename_focus = Rc::clone(&state);
    let on_space_rename_commit = Rc::clone(&state);
    let on_space_rename_cancel = Rc::clone(&state);
    let desktop_space_rename_focus = desktop.clone();
    let desktop_space_rename_commit = desktop.clone();
    // Timestamp of the last workspace-frame click, for double-click detection
    // (the frame has no dropdown; a second click within the window starts the
    // inline rename).
    let workspace_click_at = Rc::new(RefCell::new(None::<std::time::Instant>));
    let on_select_conversation = Rc::clone(&state);
    let on_select_tab = Rc::clone(&state);
    let on_composer_change = Rc::clone(&state);
    let on_composer_slash = Rc::clone(&state);
    let on_composer_focus_change = Rc::clone(&state);
    let on_send_message = Rc::clone(&state);
    let on_run_shell_command = Rc::clone(&state);
    let on_cmd_enter = Rc::clone(&state);
    let on_attach = Rc::clone(&state);
    let on_voice = Rc::clone(&state);
    let on_model_select = Rc::clone(&state);
    let on_select_harness_state = Rc::clone(&state);
    let on_select_dir = Rc::clone(&media);
    let on_select_medium = Rc::clone(&media);
    let on_create_medium = Rc::clone(&media);
    let on_select_dir_state = Rc::clone(&state);
    let on_select_branch = Rc::clone(&state);
    let on_stop = Rc::clone(&state);
    let on_open_sub_agent = Rc::clone(&state);
    let on_close_sub_agent = Rc::clone(&state);
    let desktop_open_sub_agent = desktop.clone();
    let on_answer_ask = Rc::clone(&state);
    let on_skip_ask = Rc::clone(&state);
    let on_command_decision = Rc::clone(&state);
    let on_toggle_auto_approve = Rc::clone(&state);
    let on_toggle_vim_mode = Rc::clone(&state);
    let on_fork_conversation = Rc::clone(&state);
    let on_set_harness_mode = Rc::clone(&state);
    let on_open_agent_view = Rc::clone(&state);
    let on_send_queued = Rc::clone(&state);
    let on_dismiss_queued = Rc::clone(&state);
    let on_settings = Rc::clone(&state);
    let on_settings_close = Rc::clone(&state);
    let on_settings_category = Rc::clone(&state);
    let on_settings_category_step = Rc::clone(&state);
    let on_set_theme_target = Rc::clone(&state);
    let on_toggle_invert_scroll = Rc::clone(&state);
    let on_set_scroll_speed = Rc::clone(&state);
    let on_set_font_size = Rc::clone(&state);
    let on_reload_model_config = Rc::clone(&state);
    let on_projects = Rc::clone(&state);
    let on_workflows = Rc::clone(&state);
    let on_executions = Rc::clone(&state);
    let on_timeline = Rc::clone(&state);
    let on_costs = Rc::clone(&state);
    let on_mcps = Rc::clone(&state);
    let on_open_crons = Rc::clone(&state);
    let on_close_crons = Rc::clone(&state);
    let on_toggle_right_sidebar = Rc::clone(&state);
    let on_toggle_fullscreen = Rc::clone(&state);
    let on_clear_transcript = Rc::clone(&state);
    let on_cron_create = Rc::clone(&state);
    let on_cron_delete = Rc::clone(&state);
    let on_cron_trigger = Rc::clone(&state);
    let on_sidebar_drag_start = Rc::clone(&state);
    let on_sidebar_drag_move = Rc::clone(&state);
    let on_sidebar_drag_end = Rc::clone(&state);
    let on_split_right = Rc::clone(&state);
    let on_split_down = Rc::clone(&state);
    let on_new_terminal = Rc::clone(&state);
    let on_close_pane = Rc::clone(&state);
    let on_terminal_cmd = Rc::clone(&state);
    let on_launch_tui_agent = Rc::clone(&state);
    let on_select_space = Rc::clone(&state);
    let on_add_space = Rc::clone(&state);
    let on_add_space_with_medium = Rc::clone(&state);
    let on_open_add_medium = Rc::clone(&state);
    let on_add_medium_draft_change = Rc::clone(&state);
    let on_add_medium_focus_change = Rc::clone(&state);
    let on_add_medium_close = Rc::clone(&state);
    let on_space_press = Rc::clone(&state);
    let on_space_hover = Rc::clone(&state);
    let on_space_reorder = Rc::clone(&state);
    let on_space_release = Rc::clone(&state);
    let on_close_space = Rc::clone(&state);
    let on_pane_activate = Rc::clone(&state);
    let on_pane_navigate = Rc::clone(&state);
    let on_pane_drag_start = Rc::clone(&state);
    let on_pane_drag_move = Rc::clone(&state);
    let on_pane_drag_end = Rc::clone(&state);
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
    let on_close_inline_screen = Rc::clone(&state);
    let on_open_screen_link = Rc::clone(&state);
    let on_vault_unlock = Rc::clone(&state);
    let on_create_cluster = Rc::clone(&state);
    let on_unlock_cluster = Rc::clone(&state);
    let on_add_authorized_key = Rc::clone(&state);
    let on_remove_authorized_key = Rc::clone(&state);
    let on_config_llm_key = Rc::clone(&state);
    let on_choose_workspace = Rc::clone(&state);
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
    let desktop_stop = desktop.clone();
    let desktop_answer = desktop.clone();
    let desktop_skip = desktop.clone();
    let desktop_command = desktop.clone();
    let desktop_auto = desktop.clone();
    let desktop_vim = desktop.clone();
    let desktop_fork = desktop.clone();
    let desktop_set_harness = desktop.clone();
    let desktop_send_queued = desktop.clone();
    let desktop_cron_create = desktop.clone();
    let desktop_cron_delete = desktop.clone();
    let desktop_save_llm = desktop.clone();
    let desktop_dark_mode = desktop.clone();
    let desktop_theme_primary = desktop.clone();
    let desktop_theme_secondary = desktop.clone();
    let desktop_theme_accent = desktop.clone();
    let desktop_reload_model = desktop.clone();
    let ui_zoom_font = ui_zoom.clone();
    let desktop_add_worker = desktop.clone();
    let desktop_remove_worker = desktop.clone();
    let desktop_unlock_vault = desktop.clone();
    let desktop_create_cluster = desktop.clone();
    let desktop_unlock_cluster = desktop.clone();
    let desktop_choose_workspace = desktop.clone();
    let desktop_dismiss_key = desktop.clone();
    let desktop_dismiss_workspace = desktop.clone();
    let desktop_split_right = desktop.clone();
    let desktop_split_down = desktop.clone();
    let desktop_new_terminal = desktop.clone();
    let desktop_close_pane = desktop.clone();
    let desktop_term_cmd = desktop.clone();
    let desktop_select_space = desktop.clone();
    let desktop_close_space = desktop.clone();
    let desktop_add_space = desktop.clone();
    let desktop_add_space_with_medium = desktop.clone();
    let desktop_space_reorder = desktop.clone();
    let desktop_workspace_click = desktop.clone();
    let desktop_pane_activate = desktop.clone();
    let desktop_pane_navigate = desktop.clone();
    let desktop_pane_drag = desktop.clone();
    let desktop_agent_delete = desktop.clone();
    let media_send = Rc::clone(&media);
    let media_cmd_enter = Rc::clone(&media);
    let media_send_queued = Rc::clone(&media);
    let media_split_right = Rc::clone(&media);
    let media_split_down = Rc::clone(&media);
    let media_new_terminal = Rc::clone(&media);
    let media_term_cmd = Rc::clone(&media);
    let media_add_space = Rc::clone(&media);
    let media_add_space_with_medium = Rc::clone(&media);

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
        on_create_submit: Rc::new(RefCell::new(move || {
            let mut state = on_create_submit.borrow_mut();
            // If the active pane already sits on a fresh, message-less
            // conversation, reuse it instead of creating another: repeated
            // clicks on "New conversation" must not pile up empty duplicates.
            if let Some(conv) = state.pane_conversation_id(state.active_pane_id) {
                if !conv.is_empty() {
                    let fresh = match &desktop_create {
                        Some(desktop) => desktop
                            .list_chat_messages(&conv)
                            .map(|m| m.is_empty())
                            .unwrap_or(false),
                        None => state
                            .pane_runtime
                            .get(&state.active_pane_id)
                            .map(|rt| rt.messages.is_empty())
                            .unwrap_or(true),
                    };
                    if fresh {
                        state.new_conversation_draft.clear();
                        state.selected_id = Some(conv);
                        return;
                    }
                }
            }
            let title = if state.new_conversation_draft.trim().is_empty() {
                // The sidebar's "New conversation" row has no text field, so a
                // blank draft means the user clicked it directly: create a default.
                "New conversation".to_string()
            } else {
                state.new_conversation_draft.trim().to_string()
            };
            // A new conversation belongs to the currently selected environment,
            // so it shows up in the matching sidebar folder.
            let routing = crate::media::medium_routing(
                on_create_medium.borrow().selected_medium_id(),
            );
            if let Some(desktop) = &desktop_create {
                match desktop.create_chat(&title, None, None) {
                    Ok(id) => {
                        if let Err(e) =
                            desktop.set_chat_workspace_routing(&id, Some(routing))
                        {
                            log::warn!("set_chat_workspace_routing failed: {e}");
                        }
                        state.new_conversation_draft.clear();
                        // Bind the newly created conversation to the active pane
                        // and reload its (empty) transcript into the pane.
                        state.bind_active_pane_conversation(id, desktop_create.as_deref());
                        state.refresh_from_desktop(desktop);
                    }
                    Err(e) => log::warn!("create_chat failed: {e}"),
                }
            } else {
                let id = format!("c-{}", state.conversations.len() + 1);
                state.conversations.insert(
                    0,
                    ConversationEntry::new(id.clone(), title, "New conversation", "now")
                        .with_workspace_routing(routing),
                );
                state.bind_active_pane_conversation(id, None);
                state.new_conversation_draft.clear();
            }
        })),
        on_select_conversation: Rc::new(RefCell::new(move |id: String| {
            let mut state = on_select_conversation.borrow_mut();
            // Selecting a conversation binds the active pane to it (so the pane
            // shows that conversation), keeps the sidebar highlight in sync and
            // reloads that conversation's messages into the pane.
            state.bind_active_pane_conversation(id, desktop_select.as_deref());
        })),
        on_select_tab: Rc::new(RefCell::new(move |tab: AppTab| {
            on_select_tab.borrow_mut().current_tab = tab;
        })),
        on_composer_change: Rc::new(RefCell::new(move |value: String| {
            on_composer_change.borrow_mut().set_active_pane_draft(value);
        })),
        on_composer_slash: Rc::new(RefCell::new(move || {
            let mut state = on_composer_slash.borrow_mut();
            state.command_palette_open = true;
            state.command_palette_query.clear();
            state.command_palette_index = 0;
        })),
        on_composer_focus_change: Rc::new(RefCell::new(move |focused: bool| {
            on_composer_focus_change.borrow_mut().composer_focused = focused;
        })),
        on_send_message: Rc::new(RefCell::new(move |text: String| {
            let mut state = on_send_message.borrow_mut();
            // The active pane owns the composer, so route the turn to its own
            // conversation (an independent session per pane).
            let pane_id = state.active_pane_id;
            // The chat composer is a prompt line in agent mode (an active agent
            // conversation): an input is an agent prompt unless it starts with
            // `!`, which forces a terminal command (warp-new input model).
            match classify_input(&text, true) {
                Some(InputClass::TerminalCommand(cmd)) => {
                    run_terminal_command(&mut state, &cmd, pane_id);
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
        // own conversation. A brand-new conversation is only created once, when
        // the pane has no conversation yet, so subsequent messages keep appending
        // to the thread the user is on (no accidental new conversation each send).
        on_cmd_enter: Rc::new(RefCell::new(move |text: String| {
            // Cmd+Enter with nothing typed is the gesture that opens the pane's
            // agent view; the switch itself belongs to the caller, so there is
            // no prompt to send here.
            if text.trim().is_empty() {
                return;
            }
            let mut state = on_cmd_enter.borrow_mut();
            let pane_id = state.active_pane_id;
            if state.pane_conversation_id(pane_id).is_none() {
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
        on_attach: Rc::new(RefCell::new(move || {
            on_attach.borrow_mut().push_active_message(ChatMessage::from_markdown(
                ChatRole::Assistant,
                "(attach — file picker coming soon)",
            ));
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
        // The composer's left pill selects the work environment (medium), not a
        // registered harness: an unknown medium id is ignored by `select_medium`.
        on_select_harness: Rc::new(RefCell::new(move |pane_id: u64, medium_id: String| {
            let mut media = on_select_medium.borrow_mut();
            let _ = media.select_medium(&medium_id);
            drop(media);
            let mut state = on_select_harness_state.borrow_mut();
            {
                let controls = state.pane_controls_mut(pane_id);
                let flag = controls.harness_menu_open.clone();
                *flag.borrow_mut() = false;
            }
            *state.harness_menu_open.borrow_mut() = false;
        })),
        on_select_dir: Rc::new(RefCell::new(move |pane_id: u64, session_id: String| {
            let loc = on_select_dir.borrow().session_location(&session_id);
            let mut media = on_select_dir.borrow_mut();
            if let Some((medium_id, project_id, sid)) = loc {
                if media.select_session(&medium_id, &project_id, &sid) {
                    let path = media.selected_session_path();
                    drop(media);
                    on_select_dir_state
                        .borrow_mut()
                        .set_pane_path(pane_id, path);
                }
            }
        })),
        on_select_branch: Rc::new(RefCell::new(move |pane_id: u64, branch: String| {
            let mut state = on_select_branch.borrow_mut();
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
        // Copy a terminal block's text to the system clipboard. Uses the macOS
        // `pbcopy` helper (no extra dependency); other platforms log for now.
        on_copy_terminal: Rc::new(RefCell::new(move |text: String| {
            #[cfg(target_os = "macos")]
            {
                use std::io::Write;
                if let Ok(mut child) = std::process::Command::new("pbcopy")
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                {
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(text.as_bytes());
                    }
                    let _ = child.wait();
                } else {
                    log::warn!("could not launch pbcopy to copy terminal block");
                }
            }
            #[cfg(not(target_os = "macos"))]
            log::info!("copy terminal block: {text}");
        })),
        on_restart: Rc::new(RefCell::new(move || {
            log::info!("restart agent pressed");
        })),
        // Agent-header 3-dots menu: rename the current agent/conversation. The
        // store rename is not wired yet; log so the menu item is not a dead end.
        on_rename_agent: Rc::new(RefCell::new(move || {
            log::info!("rename agent pressed (coming soon)");
        })),
        // Agent-header 3-dots menu: clear the active pane's in-memory transcript
        // (messages + suspended ask + queued prompt). The backend store is left
        // intact so the conversation can be recovered by restarting the session.
        on_clear_transcript: Rc::new(RefCell::new(move || {
            let mut state = on_clear_transcript.borrow_mut();
            let pane_id = state.active_pane_id;
            if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                rt.messages.clear();
                rt.pending_ask = None;
                rt.pending_command = None;
                rt.command_selection = None;
                rt.queued_prompt = None;
            }
            state.pending_ask = None;
            state.queued_prompt = None;
            state.sync_active_view();
        })),
        on_stop: Rc::new(RefCell::new(move || {
            let mut state = on_stop.borrow_mut();
            if let (Some(desktop), Some(chat_id)) = (&desktop_stop, state.selected_id.clone()) {
                let _ = desktop.cancel_chat_turn(&chat_id);
            }
            let pane_id = state.active_pane_id;
            if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                rt.busy = false;
                rt.turn_started_at = None;
                rt.pending_command = None;
                rt.command_selection = None;
            }
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
                if let (Some(desktop), Some(chat_id)) = (&desktop_answer, state.selected_id.clone())
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
            if let (Some(desktop), Some(chat_id)) = (&desktop_skip, state.selected_id.clone()) {
                if let Err(e) = desktop.resume_chat_turn(
                    &chat_id,
                    "(skipped)",
                    None,
                    &state.settings_llm_provider,
                    &model,
                ) {
                    log::warn!("resume_chat_turn (skip) failed: {e}");
                } else {
                    if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                        rt.busy = true;
                        rt.pending_ask = None;
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
                            state.clear_command_proposal(pane_id, &proposal_id);
                            if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                                rt.busy = true;
                            }
                            state.agent_busy = true;
                        }
                        Err(e) => log::warn!("resume_command_chat_turn failed: {e}"),
                    }
                    state.refresh_messages(desktop);
                }
            },
        )),
        on_toggle_auto_approve: Rc::new(RefCell::new(move |pane_id: u64, enabled: bool| {
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
        })),
        // Modal (vim) editing for the rich input, off by default. The choice is
        // persisted; turning it off drops every pane's half-typed command and
        // returns its editor to insert mode, so no pane is left in a mode whose
        // keys the editor no longer reads.
        on_toggle_vim_mode: Rc::new(RefCell::new(move |enabled: bool| {
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
        })),
        clipboard: crate::clipboard::system_clipboard(),
        // The transcript footer's Fork: a new conversation inheriting this
        // pane's transcript, bound to the pane that asked for it.
        on_fork_conversation: Rc::new(RefCell::new(move |pane_id: u64| {
            let mut state = on_fork_conversation.borrow_mut();
            if state.fork_pane_conversation(pane_id, desktop_fork.as_deref()).is_some() {
                if pane_id == state.active_pane_id {
                    state.sync_active_view();
                }
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
            state.pane_controls_mut(pane_id).harness_mode = on;
            if on {
                if !state.pane_owns_conversation(pane_id) {
                    state.bind_pane_new_conversation(pane_id, desktop_set_harness.as_deref());
                }
                if let Some(conversation_id) = state.pane_conversation_id(pane_id) {
                    let label = state.conversation_name(&conversation_id);
                    state.enter_agent_view(pane_id, &conversation_id, &label);
                }
            } else {
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
            let pane_id = state.active_pane_id;
            let Some(prompt) = state
                .pane_runtime
                .get_mut(&pane_id)
                .and_then(|rt| rt.queued_prompt.take())
            else {
                return;
            };
            state.queued_prompt = None;
            let model = if state.selected_model.trim().is_empty() {
                state.settings_llm_model.clone()
            } else {
                state.selected_model.clone()
            };
            if let (Some(desktop), Some(chat_id)) =
                (&desktop_send_queued, state.pane_conversation_id(pane_id))
            {
                // "Send now": interrupt the in-flight turn and submit immediately,
                // routed to the conversation's chosen runtime.
                let _ = desktop.cancel_chat_turn(&chat_id);
                let (medium_id, project_id, session_id) = {
                    let media_b = media_send_queued.borrow();
                    let session_id = if media_b.selected_session_id().is_empty() {
                        chat_id.clone()
                    } else {
                        media_b.selected_session_id().to_string()
                    };
                    (
                        media_b.selected_medium_id().to_string(),
                        media_b.selected_project_id().to_string(),
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
            log::info!("inbox pressed (coming soon)");
        })),
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
        on_workspace_click: Rc::new(RefCell::new(move |index: usize| {
            let mut state = on_workspace_click.borrow_mut();
            if index >= state.spaces.len() {
                return;
            }
            let now = std::time::Instant::now();
            let double = workspace_click_at
                .borrow()
                .map(|t| now.duration_since(t).as_millis() < 400)
                .unwrap_or(false);
            *workspace_click_at.borrow_mut() = if double { None } else { Some(now) };
            if double {
                // Second click on a chip: rename that workspace inline.
                state.active_space = index;
                state.active_pane_id = state.spaces[index].root.first_leaf_id();
                state.space_rename_draft = state.spaces[index].name.clone();
                state.space_rename_editing = true;
                state.space_rename_focused = true;
                return;
            }
            // A single click selects the clicked workspace (Warp-style chip row).
            if state.active_space != index {
                state.active_space = index;
                state.active_pane_id = state.spaces[index].root.first_leaf_id();
                state.sync_active_view();
                if let Some(desktop) = &desktop_workspace_click {
                    state.refresh_messages(desktop);
                    state.save_panes(desktop);
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
        on_settings: Rc::new(RefCell::new(move || {
            // Settings becomes a floating overlay (the old Settings tab is gone).
            on_settings.borrow_mut().settings_overlay_open = true;
        })),
        on_settings_close: Rc::new(RefCell::new(move || {
            on_settings_close.borrow_mut().settings_overlay_open = false;
        })),
        on_settings_category: Rc::new(RefCell::new(move |category: SettingsCategory| {
            on_settings_category.borrow_mut().settings_category = category;
        })),
        on_settings_category_step: Rc::new(RefCell::new(move |delta: i32| {
            let mut state = on_settings_category_step.borrow_mut();
            let all = SettingsCategory::ALL;
            let count = all.len() as i32;
            if count == 0 {
                return;
            }
            let pos = all.iter().position(|c| *c == state.settings_category).unwrap_or(0) as i32;
            let next = (pos + delta).rem_euclid(count) as usize;
            state.settings_category = all[next];
        })),
        on_set_theme_target: Rc::new(RefCell::new(move |target: ColorTarget| {
            on_set_theme_target.borrow_mut().theme_color_target = target;
        })),
        on_toggle_invert_scroll: Rc::new(RefCell::new(move |enabled: bool| {
            on_toggle_invert_scroll.borrow_mut().settings_invert_scroll = enabled;
        })),
        on_set_scroll_speed: Rc::new(RefCell::new(move |speed: i32| {
            on_set_scroll_speed.borrow_mut().settings_scroll_speed = speed.clamp(1, 100);
        })),
        on_set_font_size: Rc::new(RefCell::new(move |size: f32| {
            let mut state = on_set_font_size.borrow_mut();
            state.settings_font_size = size.clamp(0.5, 2.0);
            // Drive the real window zoom from the same shared cell the keyboard
            // shortcut and menubar use, so the settings control reflects 1:1.
            *ui_zoom_font.borrow_mut() = state.settings_font_size;
        })),
        on_reload_model_config: Rc::new(RefCell::new(move || {
            let mut state = on_reload_model_config.borrow_mut();
            if let Some(desktop) = &desktop_reload_model {
                if let Ok(home) = goble_core::app_home::GobleHome::locate() {
                    desktop.reload_config(&home.config_path());
                }
                state.models = desktop.available_models(&state.settings_llm_provider);
            }
        })),
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
        on_toggle_right_sidebar: Rc::new(RefCell::new(move || {
            let mut state = on_toggle_right_sidebar.borrow_mut();
            state.right_sidebar_open = !state.right_sidebar_open;
        })),
        // Agent-header 3-dots menu: toggle the agent/window borderless fullscreen.
        // The fullscreen flag is app-owned so the menu's checked state survives
        // the per-frame rebuild; the platform window follows via `window_control`.
        on_toggle_fullscreen: Rc::new(RefCell::new(move || {
            let mut state = on_toggle_fullscreen.borrow_mut();
            state.fullscreen = !state.fullscreen;
            window_control.set_fullscreen(state.fullscreen);
        })),
        on_cron_create: Rc::new(RefCell::new(move || {
            let mut state = on_cron_create.borrow_mut();
            if let Some(desktop) = &desktop_cron_create {
                match desktop.create_workflow(
                    "New scheduled task",
                    "Created from the crons drawer",
                    vec![],
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
            state.push_active_message(ChatMessage::from_markdown(
                ChatRole::Assistant,
                format!("Am pornit cron-ul „{name}”."),
            ));
            let pane_id = state.active_pane_id;
            if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                rt.busy = true;
            }
            state.agent_busy = true;
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
            split_active_pane(
                &mut state,
                SplitDir::Horizontal,
                desktop_split_right.as_ref(),
                &media,
                PaneKind::Chat,
            );
            drop(media);
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
                PaneKind::Chat,
            );
            drop(media);
            if let Some(desktop) = &desktop_split_down {
                state.save_panes(desktop);
            }
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
            let space_idx = state.active_space;
            let active_id = state.active_pane_id;
            let next = if let Some(space) = state.spaces.get_mut(space_idx) {
                space.close(active_id)
            } else {
                None
            };
            // Drop the closed pane's session/runtime/hover so it is reclaimed.
            state.pane_sessions.remove(&active_id);
            state.pane_runtime.remove(&active_id);
            state.pane_hover.remove(&active_id);
            // A child view open in the closed pane goes with it: the next pane to
            // take this id must not inherit a stranger's transcript.
            state.sub_agent_views.remove(&active_id);
            // Drop any terminal session (killing its shell) + input mirror.
            state.terminal.borrow_mut().drop_pane(active_id);
            if let Some(next) = next {
                state.active_pane_id = next;
            }
            state.sync_active_view();
            if let Some(desktop) = &desktop_close_pane {
                state.refresh_messages(desktop);
                state.save_panes(desktop);
            }
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
            if !state.pane_owns_conversation(pane_id) {
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
            run_terminal_command(&mut state, &text, pane_id);
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
            let terminal = on_launch_tui_agent.borrow().terminal.clone();
            let mut reg = terminal.borrow_mut();
            reg.ensure_session(pane_id, "");
            reg.launch_agent(pane_id, &command);
            reg.set_input(pane_id, String::new());
        })),
        on_select_space: Rc::new(RefCell::new(move |index: usize| {
            let mut state = on_select_space.borrow_mut();
            if index < state.spaces.len() {
                state.active_space = index;
                state.active_pane_id = state.spaces[index].root.first_leaf_id();
            }
            state.sync_active_view();
            if let Some(desktop) = &desktop_select_space {
                state.refresh_messages(desktop);
                state.save_panes(desktop);
            }
        })),
        // Close a space (tab) by index. The last remaining space is never
        // removed: it resets to a fresh empty chat pane, so closing a tab never
        // exits the program (the previous behavior owned the single pane and a
        // close on it tore the window down).
        on_close_space: Rc::new(RefCell::new(move |index: usize| {
            let mut state = on_close_space.borrow_mut();
            if index >= state.spaces.len() {
                return;
            }
            // Drop every pane session/runtime/hover for the space's leaves and
            // kill any terminal sessions along with them.
            let closed = state.spaces.remove(index);
            for id in collect_leaf_ids(&closed.root) {
                state.pane_sessions.remove(&id);
                state.pane_runtime.remove(&id);
                state.pane_hover.remove(&id);
                state.sub_agent_views.remove(&id);
                state.terminal.borrow_mut().drop_pane(id);
            }
            // If the space being closed was the last one, restore a fresh empty
            // chat space so the window is never left with zero tabs.
            if state.spaces.is_empty() {
                let id = state.next_pane_id;
                state.next_pane_id += 1;
                state.spaces.push(Space::new(
                    "Space 1",
                    Pane::Leaf { id, kind: PaneKind::Chat },
                ));
                state.active_space = 0;
                state.active_pane_id = id;
                ensure_pane_hover(&mut state, id);
                state.bind_pane_new_conversation(id, desktop_close_space.as_deref());
                state.sync_active_view();
                if let Some(desktop) = &desktop_close_space {
                    state.refresh_messages(desktop);
                    state.save_panes(desktop);
                }
                return;
            }
            // Re-index the active space after the removal.
            if index < state.active_space {
                state.active_space -= 1;
            } else if index == state.active_space {
                state.active_space = state.active_space.min(state.spaces.len() - 1);
            }
            // Focus the new active space's first leaf.
            state.active_pane_id = state.spaces[state.active_space].root.first_leaf_id();
            state.sync_active_view();
            if let Some(desktop) = &desktop_close_space {
                state.refresh_messages(desktop);
                state.save_panes(desktop);
            }
        })),
        on_add_space: Rc::new(RefCell::new(move || {
            let mut state = on_add_space.borrow_mut();
            let medium_id = media_add_space.borrow().selected_medium_id().to_string();
            let routing = crate::media::medium_routing(&medium_id);
            let id = state.next_pane_id;
            state.next_pane_id += 1;
            let count = state.spaces.len() + 1;
            state.spaces.push(
                Space::new(
                    format!("Space {count}"),
                    Pane::Leaf { id, kind: PaneKind::Terminal },
                )
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
            let routing = crate::media::medium_routing(&medium_id);
            let id = state.next_pane_id;
            state.next_pane_id += 1;
            let count = state.spaces.len() + 1;
            state.spaces.push(
                Space::new(
                    format!("Space {count}"),
                    Pane::Leaf { id, kind: PaneKind::Terminal },
                )
                .with_medium(medium_id.clone()),
            );
            state.active_space = state.spaces.len() - 1;
            state.active_pane_id = id;
            state.workspace_routing = Some(WorkspaceRouting::from_routing(routing));
            // Make the chosen environment the active medium so the new space's
            // turns run there and the sidebar shows that environment's folders.
            let _ = media_add_space_with_medium.borrow_mut().select_medium(&medium_id);
            ensure_pane_hover(&mut state, id);
            // Plain PTY: the thread is created lazily, on the pane's first agent
            // turn, so "+" never creates an empty conversation.
            let project_id =
                media_add_space_with_medium.borrow().default_project_for_medium(&medium_id);
            let path = default_pane_path(&project_id, desktop_add_space_with_medium.as_deref());
            state.set_active_pane_path(path);
            state.sync_active_view();
            if let Some(desktop) = &desktop_add_space_with_medium {
                state.save_panes(desktop);
            }
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
        on_space_press: Rc::new(RefCell::new(move |index: usize| {
            let mut state = on_space_press.borrow_mut();
            state.space_press = Some(index);
            state.space_hover = Some(index);
        })),
        on_space_hover: Rc::new(RefCell::new(move |hover: Option<usize>| {
            on_space_hover.borrow_mut().space_hover = hover;
        })),
        on_space_reorder: Rc::new(RefCell::new(move |from: usize, to: usize| {
            let mut state = on_space_reorder.borrow_mut();
            let len = state.spaces.len();
            if from == to || from >= len || to >= len {
                return;
            }
            state.reorder_space(from, to);
            // Track the dragged tab at its new index so the drag continues to
            // follow the pointer across the per-frame rebuild.
            state.space_drag = Some(to);
            if let Some(desktop) = &desktop_space_reorder {
                state.save_panes(desktop);
            }
        })),
        on_space_release: Rc::new(RefCell::new(move || {
            let mut state = on_space_release.borrow_mut();
            state.space_press = None;
            state.space_drag = None;
            state.space_hover = None;
        })),
        on_pane_activate: Rc::new(RefCell::new(move |id: u64| {
            let mut state = on_pane_activate.borrow_mut();
            state.active_pane_id = id;
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
                state.active_pane_id = next;
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
        on_toggle_dark_mode: Rc::new(RefCell::new(move |enabled: bool| {
            on_toggle_dark_mode.borrow_mut().settings_dark_mode = enabled;
            if let Some(desktop) = &desktop_dark_mode {
                let mut config = desktop.config();
                config.theme.dark = enabled;
                if let Err(e) = desktop.save_config(&config) {
                    log::warn!("save_config (theme) failed: {e}");
                }
            }
        })),
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
                    config.llm.default_provider = state.settings_llm_provider.clone();
                    config.llm.default_model = state.settings_llm_model.clone();
                    let exists = config.llm.models.iter().any(|m| {
                        m.id == state.settings_llm_model && m.provider == state.settings_llm_provider
                    });
                    if !exists && !state.settings_llm_model.is_empty() {
                        config.llm.models.push(goble_core::config::ModelConfig {
                            id: state.settings_llm_model.clone(),
                            provider: state.settings_llm_provider.clone(),
                            label: state.settings_llm_model.clone(),
                            api_key_secret_id: state.settings_llm_api_key.clone(),
                            base_url: if state.settings_llm_base_url.trim().is_empty() {
                                None
                            } else {
                                Some(state.settings_llm_base_url.clone())
                            },
                            enabled: true,
                        });
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
        on_close_inline_screen: Rc::new(RefCell::new(move || {
            let mut state = on_close_inline_screen.borrow_mut();
            let pane_id = state.active_pane_id;
            if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                rt.inline_screen_source = None;
            }
        })),
        on_open_screen_link: Rc::new(RefCell::new(move |uri: String| {
            let mut state = on_open_screen_link.borrow_mut();
            // The root view owns the screen sheet; it pops this and performs
            // the open (selecting the source derived from the URI).
            state.pending_screen_link_open = Some(uri);
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
        on_close_llm_dialog: Rc::new(RefCell::new(move || {
            on_close_llm_dialog.borrow_mut().llm_dialog_open = false;
        })),
        on_choose_workspace: Rc::new(RefCell::new(move |routing: WorkspaceRouting| {
            let mut state = on_choose_workspace.borrow_mut();
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
        })),
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
