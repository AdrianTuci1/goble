//! Callback wiring: turns app state + backend into [`UiActions`] closures.
//!
//! The view tree built by [`crate::ui`] only knows about these callbacks; the
//! actual behavior (mutating [`UiState`], persisting through [`DesktopState`])
//! lives here in the executable.

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
use crate::ui::{
    AppTab, CronEntry, NavDir, Pane, PaneKind, Space, SplitDir, UiActions, WorkspaceRouting,
};

pub fn make_actions(
    state: Rc<RefCell<UiState>>,
    desktop: Option<Arc<DesktopState>>,
    media: Rc<RefCell<MediaState>>,
    window_control: WindowControl,
) -> UiActions {
    let on_search_change = Rc::clone(&state);
    let on_search_focus_change = Rc::clone(&state);
    let on_create_change = Rc::clone(&state);
    let on_create_focus_change = Rc::clone(&state);
    let on_create_submit = Rc::clone(&state);
    let on_select_conversation = Rc::clone(&state);
    let on_select_tab = Rc::clone(&state);
    let on_composer_change = Rc::clone(&state);
    let on_composer_slash = Rc::clone(&state);
    let on_composer_focus_change = Rc::clone(&state);
    let on_send_message = Rc::clone(&state);
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
    let on_answer_ask = Rc::clone(&state);
    let on_skip_ask = Rc::clone(&state);
    let on_toggle_auto_approve = Rc::clone(&state);
    let on_send_queued = Rc::clone(&state);
    let on_dismiss_queued = Rc::clone(&state);
    let on_settings = Rc::clone(&state);
    let on_projects = Rc::clone(&state);
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
    let on_pane_activate = Rc::clone(&state);
    let on_pane_navigate = Rc::clone(&state);
    let on_pane_drag_start = Rc::clone(&state);
    let on_pane_drag_move = Rc::clone(&state);
    let on_pane_drag_end = Rc::clone(&state);
    let on_agent_delete = Rc::clone(&state);
    let on_settings_back = Rc::clone(&state);
    let on_settings_navigate = Rc::clone(&state);
    let on_toggle_dark_mode = Rc::clone(&state);
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
    let desktop_stop = desktop.clone();
    let desktop_answer = desktop.clone();
    let desktop_skip = desktop.clone();
    let desktop_auto = desktop.clone();
    let desktop_send_queued = desktop.clone();
    let desktop_cron_create = desktop.clone();
    let desktop_cron_delete = desktop.clone();
    let desktop_save_llm = desktop.clone();
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
    let desktop_add_space = desktop.clone();
    let desktop_add_space_with_medium = desktop.clone();
    let desktop_space_reorder = desktop.clone();
    let desktop_pane_activate = desktop.clone();
    let desktop_pane_navigate = desktop.clone();
    let desktop_pane_drag = desktop.clone();
    let desktop_agent_delete = desktop.clone();
    let media_send = Rc::clone(&media);
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
            let title = if state.new_conversation_draft.trim().is_empty() {
                // The sidebar's "New agent" row has no text field, so a blank
                // draft means the user clicked it directly: create a default.
                "New agent".to_string()
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
            let configured = !state.settings_llm_api_key.trim().is_empty();
            // The active pane owns the composer, so route the turn to its own
            // conversation (an independent session per pane).
            let pane_id = state.active_pane_id;
            let pane_busy = state
                .pane_runtime
                .get(&pane_id)
                .map(|r| r.busy)
                .unwrap_or(false);
            // While a turn is running, don't start a second one (that would
            // interrupt the agent). Queue the prompt and render it as a pending
            // block; it is sent when the current turn finishes or via "Send now".
            if pane_busy {
                let rt = state.pane_runtime.entry(pane_id).or_default();
                rt.queued_prompt = Some(text.clone());
                state.queued_prompt = Some(text);
                state.set_active_pane_draft(String::new());
                return;
            }
            let mut ran_turn = false;
            // The model used is the composer's selected model, falling back to
            // the configured setting when nothing is selected yet.
            let model = if state.selected_model.trim().is_empty() {
                state.settings_llm_model.clone()
            } else {
                state.selected_model.clone()
            };
            let chat_id = state.pane_conversation_id(pane_id);
            if let (Some(desktop), Some(chat_id)) = (&desktop_send, chat_id) {
                if configured {
                    // Run the real harness turn on a background task, routed to
                    // the conversation's chosen runtime. The harness inserts the
                    // user message and the assistant/tool output into the store
                    // and emits `chat:updated`, so the UI refreshes as the turn
                    // progresses (including any tool-call output).
                    let (medium_id, project_id, session_id) = {
                        let media_b = media_send.borrow();
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
                    if let Err(e) = crate::runtime::run_turn(
                        desktop,
                        &chat_id,
                        &text,
                        &state.settings_llm_provider,
                        &model,
                        state.workspace_routing,
                        &medium_id,
                        &project_id,
                        &session_id,
                        &state.composer_path,
                        Some(state.selected_harness.as_str()),
                    ) {
                        log::warn!("run_chat_turn failed: {e}");
                        let _ = desktop.add_chat_message(
                            &chat_id,
                            "assistant",
                            &format!("(nu am putut porni modelul: {e})"),
                        );
                    } else {
                        // A turn is now in flight on a background task; keep the
                        // Stop button visible until `chat:turn_finished` (or Stop)
                        // clears it.
                        if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                            rt.busy = true;
                        }
                        state.agent_busy = true;
                        ran_turn = true;
                    }
                } else {
                    // No key configured yet: keep the user's message but do not
                    // fabricate a canned response (nothing is actually running).
                    // Surface the model-key banner overlay so the user configures
                    // a provider before the agent replies.
                    let _ = desktop.add_chat_message(&chat_id, "user", &text);
                    state.show_llm_key_banner = true;
                }
                state.refresh_messages(desktop);
            } else {
                // No backend conversation bound (or no store): keep the message
                // in the active pane's in-memory transcript.
                state.push_active_message(ChatMessage::from_markdown(
                    ChatRole::User,
                    text.clone(),
                ));
                if !configured {
                    state.show_llm_key_banner = true;
                }
                state.sync_active_view();
            }
            state.set_active_pane_draft(String::new());
            state.agent_busy = ran_turn;
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
        on_model_select: Rc::new(RefCell::new(move |model: String| {
            let mut state = on_model_select.borrow_mut();
            state.selected_model = model;
            *state.model_menu_open.borrow_mut() = false;
        })),
        // The composer's left pill selects the work environment (medium), not a
        // registered harness: an unknown medium id is ignored by `select_medium`.
        on_select_harness: Rc::new(RefCell::new(move |medium_id: String| {
            let mut media = on_select_medium.borrow_mut();
            let _ = media.select_medium(&medium_id);
            drop(media);
            let state = on_select_harness_state.borrow_mut();
            *state.harness_menu_open.borrow_mut() = false;
        })),
        on_select_dir: Rc::new(RefCell::new(move |session_id: String| {
            let loc = on_select_dir.borrow().session_location(&session_id);
            let mut media = on_select_dir.borrow_mut();
            if let Some((medium_id, project_id, sid)) = loc {
                if media.select_session(&medium_id, &project_id, &sid) {
                    let path = media.selected_session_path();
                    on_select_dir_state.borrow_mut().set_active_pane_path(path);
                }
            }
        })),
        on_select_branch: Rc::new(RefCell::new(move |branch: String| {
            on_select_branch.borrow_mut().composer_branch = branch;
        })),
        on_copy: Rc::new(RefCell::new(move || {
            log::info!("copy transcript pressed");
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
            }
            state.agent_busy = false;
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
        on_toggle_auto_approve: Rc::new(RefCell::new(move |enabled: bool| {
            let mut state = on_toggle_auto_approve.borrow_mut();
            state.auto_approve = enabled;
            if let Some(desktop) = &desktop_auto {
                if let Err(e) = desktop.set_auto_approve(enabled) {
                    log::warn!("set_auto_approve failed: {e}");
                }
            }
        })),
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
                ) {
                    log::warn!("run_chat_turn (queued) failed: {e}");
                } else {
                    if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                        rt.busy = true;
                    }
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
        on_settings: Rc::new(RefCell::new(move || {
            on_settings.borrow_mut().current_tab = AppTab::Settings;
        })),
        on_projects: Rc::new(RefCell::new(move || {
            on_projects.borrow_mut().current_tab = AppTab::Projects;
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
        on_terminal_command: Rc::new(RefCell::new(move |pane_id: u64, text: String| {
            let mut state = on_terminal_cmd.borrow_mut();
            let configured = !state.settings_llm_api_key.trim().is_empty();
            let model = if state.selected_model.trim().is_empty() {
                state.settings_llm_model.clone()
            } else {
                state.selected_model.clone()
            };
            let chat_id = state.pane_conversation_id(pane_id);
            let cwd = state
                .pane_sessions
                .get(&pane_id)
                .map(|s| s.path.clone())
                .unwrap_or_default();
            if let (Some(desktop), Some(chat_id)) = (&desktop_term_cmd, chat_id) {
                if configured {
                    let (medium_id, project_id, session_id) = {
                        let media_b = media_term_cmd.borrow();
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
                    if let Err(e) = crate::runtime::run_turn(
                        desktop,
                        &chat_id,
                        &text,
                        &state.settings_llm_provider,
                        &model,
                        state.workspace_routing,
                        &medium_id,
                        &project_id,
                        &session_id,
                        &cwd,
                        Some(state.selected_harness.as_str()),
                    ) {
                        log::warn!("run_chat_turn (terminal) failed: {e}");
                        let _ = desktop.add_chat_message(
                            &chat_id,
                            "assistant",
                            &format!("(nu am putut porni modelul: {e})"),
                        );
                    } else {
                        if let Some(rt) = state.pane_runtime.get_mut(&pane_id) {
                            rt.busy = true;
                        }
                        state.agent_busy = true;
                    }
                } else {
                    let _ = desktop.add_chat_message(&chat_id, "user", &text);
                    state.show_llm_key_banner = true;
                }
                state.refresh_messages(desktop);
            } else {
                state.push_active_message(ChatMessage::from_markdown(
                    ChatRole::User,
                    text.clone(),
                ));
                if !configured {
                    state.show_llm_key_banner = true;
                }
                state.sync_active_view();
            }
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
                    Pane::Leaf { id, kind: PaneKind::Chat },
                )
                .with_medium(medium_id.clone()),
            );
            state.active_space = state.spaces.len() - 1;
            state.active_pane_id = id;
            state.workspace_routing = Some(WorkspaceRouting::from_routing(routing));
            ensure_pane_hover(&mut state, id);
            // A new space is an independent conversation + cwd.
            state.bind_pane_new_conversation(id, desktop_add_space.as_deref());
            // Persist the new thread's workspace routing so it shows up in the
            // matching sidebar folder after a refresh.
            if let (Some(desktop), Some(conv)) =
                (&desktop_add_space, state.pane_conversation_id(id))
            {
                if let Err(e) = desktop.set_chat_workspace_routing(&conv, Some(routing)) {
                    log::warn!("set_chat_workspace_routing failed: {e}");
                }
            }
            let project_id = media_add_space.borrow().default_project_for_medium(&medium_id);
            let path = default_pane_path(&project_id, desktop_add_space.as_deref());
            state.set_active_pane_path(path);
            state.sync_active_view();
            if let Some(desktop) = &desktop_add_space {
                state.save_panes(desktop);
            }
        })),
        // Add a new space whose default environment is `medium_id`: open a fresh
        // chat pane, scope its cwd to that medium's default project and set the
        // new thread's workspace routing to the medium (so it fills the matching
        // sidebar folder). Used by the topbar "+" environment menu.
        on_add_space_with_medium: Rc::new(RefCell::new(move |medium_id: String| {
            let mut state = on_add_space_with_medium.borrow_mut();
            let routing = crate::media::medium_routing(&medium_id);
            let id = state.next_pane_id;
            state.next_pane_id += 1;
            let count = state.spaces.len() + 1;
            state.spaces.push(
                Space::new(
                    format!("Space {count}"),
                    Pane::Leaf { id, kind: PaneKind::Chat },
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
            state.bind_pane_new_conversation(id, desktop_add_space_with_medium.as_deref());
            if let (Some(desktop), Some(conv)) = (
                &desktop_add_space_with_medium,
                state.pane_conversation_id(id),
            ) {
                if let Err(e) = desktop.set_chat_workspace_routing(&conv, Some(routing)) {
                    log::warn!("set_chat_workspace_routing failed: {e}");
                }
            }
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

/// Split the active pane of the active space along `dir`, focusing the new pane
/// and binding it to a distinct conversation + cwd so the two panes never share
/// a transcript. `kind` is the kind of the newly created leaf (so a terminal
/// pane can be opened off a chat pane).
fn split_active_pane(
    state: &mut UiState,
    dir: SplitDir,
    desktop: Option<&Arc<DesktopState>>,
    media: &MediaState,
    kind: PaneKind,
) {
    let space_idx = state.active_space;
    if let Some(space) = state.spaces.get_mut(space_idx) {
        if let Some(new_id) =
            space.split_with_kind(state.active_pane_id, dir, &mut state.next_pane_id, kind)
        {
            state.active_pane_id = new_id;
            ensure_pane_hover(state, new_id);
            state.bind_pane_new_conversation(new_id, desktop.map(|d| d.as_ref()));
            let project_id = media.selected_project_id().to_string();
            let path = default_pane_path(&project_id, desktop.map(|d| d.as_ref()));
            state.set_active_pane_path(path);
            state.sync_active_view();
        }
    }
}

/// Ensure a hover flag exists for a pane so the pane-header hover survives
/// the per-frame element rebuild.
fn ensure_pane_hover(state: &mut UiState, pane_id: u64) {
    state
        .pane_hover
        .entry(pane_id)
        .or_insert_with(|| Rc::new(RefCell::new(false)));
}

#[cfg(test)]
mod pane_independence_tests {
    use super::*;
    use goble_core::store::Store;
    use goble_desktop_service::ThreadStore;

    use crate::media::MediaState;
    use crate::state::UiState;

    fn desktop_state() -> (Arc<DesktopState>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = DesktopState::new(
            Store::open_in_memory().expect("store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        );
        (state, dir)
    }

    fn build(
        desktop: &Arc<DesktopState>,
    ) -> (Rc<RefCell<UiState>>, UiActions, Rc<RefCell<MediaState>>) {
        let state = Rc::new(RefCell::new(UiState::from_desktop(desktop)));
        let media = Rc::new(RefCell::new(MediaState::mock()));
        let actions = make_actions(
            Rc::clone(&state),
            Some(Arc::clone(desktop)),
            Rc::clone(&media),
            WindowControl::default(),
        );
        (state, actions, media)
    }

    #[test]
    fn split_binds_new_pane_to_a_distinct_conversation() {
        let (desktop, _dir) = desktop_state();
        let chat_id = desktop.create_chat("Initial", None, None).expect("create chat");
        let (state, actions, _media) = build(&desktop);

        (actions.on_split_right.borrow_mut())();

        let s = state.borrow();
        assert_eq!(s.active_pane_id, 3, "split creates + focuses a new leaf");
        let conv1 = s
            .pane_sessions
            .get(&1)
            .map(|x| x.conversation_id.clone())
            .unwrap_or_default();
        let conv3 = s
            .pane_sessions
            .get(&3)
            .map(|x| x.conversation_id.clone())
            .unwrap_or_default();
        assert!(!conv3.is_empty(), "split pane binds a new conversation");
        assert_ne!(conv1, conv3, "two panes never share a conversation");
        assert!(
            !s.pane_sessions.get(&3).unwrap().path.is_empty(),
            "new pane initializes a cwd"
        );
        assert_eq!(
            s.selected_id.as_deref(),
            Some(conv3.as_str()),
            "selected_id mirrors the active pane's conversation"
        );
        drop(s);
        let _ = chat_id;
    }

    #[test]
    fn send_routes_to_active_pane_conversation() {
        let (desktop, _dir) = desktop_state();
        let chat_id = desktop.create_chat("Initial", None, None).expect("create chat");
        let (state, actions, _media) = build(&desktop);

        (actions.on_split_right.borrow_mut())();
        let conv3 = { state.borrow().pane_sessions.get(&3).unwrap().conversation_id.clone() };
        assert_ne!(conv3, chat_id);

        // No key configured: the user message is kept, routed to pane 3's own
        // conversation, not pane 1's.
        (actions.on_composer_change.borrow_mut())("hi".to_string());
        (actions.on_send_message.borrow_mut())("hi".to_string());

        let messages3 = desktop.list_chat_messages(&conv3).expect("list pane 3");
        assert_eq!(messages3.len(), 1);
        assert_eq!(messages3[0].role, "user");
        let messages1 = desktop.list_chat_messages(&chat_id).expect("list pane 1");
        assert_eq!(messages1.len(), 0, "pane 1's conversation stays untouched");
    }

    #[test]
    fn selecting_environment_session_updates_active_pane_path() {
        let (desktop, _dir) = desktop_state();
        let (state, _actions, media) = build(&desktop);

        let media_actions = crate::media::make_media_actions(
            Rc::clone(&media),
            Rc::clone(&state),
            Some(Arc::clone(&desktop)),
        );
        // Picking the remote session leaf scopes the turn to medium+project+session
        // and points the active pane's cwd at that session's path.
        (media_actions.on_select_session.borrow_mut())(
            "remote-xrdp".to_string(),
            "remote".to_string(),
            "s-remote-xrdp".to_string(),
        );

        assert_eq!(state.borrow().composer_path, "/workspace/remote");
        assert_eq!(
            state.borrow().pane_sessions.get(&1).unwrap().path,
            "/workspace/remote",
            "active pane's cwd is stored per-pane"
        );
        let media_b = media.borrow();
        assert_eq!(media_b.selected_medium_id(), "remote-xrdp");
        assert_eq!(media_b.selected_project_id(), "remote");
        assert_eq!(media_b.selected_session_id(), "s-remote-xrdp");
    }

    #[test]
    fn on_select_harness_selects_medium() {
        // The composer's left pill selects the work environment (medium), not a
        // registered harness: picking a medium resets the project to its default
        // and closes the menu; an unknown id leaves the selection unchanged.
        let (desktop, _dir) = desktop_state();
        let (_state, actions, media) = build(&desktop);
        assert_eq!(media.borrow().selected_medium_id(), "local");

        (actions.on_select_harness.borrow_mut())("remote-xrdp".to_string());
        assert_eq!(media.borrow().selected_medium_id(), "remote-xrdp");
        assert_eq!(media.borrow().selected_project_id(), "remote");
        assert_eq!(media.borrow().selected_session_id(), "");

        (actions.on_select_harness.borrow_mut())("nope".to_string());
        assert_eq!(
            media.borrow().selected_medium_id(),
            "remote-xrdp",
            "an unknown medium id leaves the selection unchanged"
        );
    }

    #[test]
    fn add_space_with_medium_sets_medium_and_workspace_routing() {
        let (desktop, _dir) = desktop_state();
        let (state, actions, media) = build(&desktop);

        (actions.on_add_space_with_medium.borrow_mut())("remote-xrdp".to_string());

        let s = state.borrow();
        let last = s.spaces.last().expect("a new space was added");
        assert_eq!(last.medium, "remote-xrdp", "space carries its default medium");
        assert_eq!(
            s.workspace_routing,
            Some(WorkspaceRouting::Remote),
            "new thread routes remote"
        );
        assert_eq!(s.active_space, s.spaces.len() - 1, "new space becomes active");
        drop(s);
        assert_eq!(
            media.borrow().selected_medium_id(),
            "remote-xrdp",
            "the chosen environment becomes the active medium"
        );
    }

    #[test]
    fn on_add_medium_adds_persists_and_selects_custom_medium() {
        let (desktop, _dir) = desktop_state();
        let (state, _actions, media) = build(&desktop);
        let media_actions = crate::media::make_media_actions(
            Rc::clone(&media),
            Rc::clone(&state),
            Some(Arc::clone(&desktop)),
        );

        (media_actions.on_add_medium.borrow_mut())("Staging VPS".to_string());

        assert!(
            media.borrow().mediums.iter().any(|m| m.label == "Staging VPS"),
            "the custom medium is added to the tree"
        );
        assert_eq!(media.borrow().selected_medium_id(), "staging-vps");
        assert!(
            desktop.get_ui_mediums().is_some(),
            "the custom medium is persisted so it reappears"
        );
        assert!(
            !state.borrow().add_medium_dialog_open,
            "the add-medium dialog closes after adding"
        );
    }

    #[test]
    fn new_terminal_splits_and_binds_own_conversation_and_cwd() {
        let (desktop, _dir) = desktop_state();
        let chat_id = desktop.create_chat("Initial", None, None).expect("create chat");
        let (state, actions, _media) = build(&desktop);

        (actions.on_new_terminal.borrow_mut())();

        let s = state.borrow();
        let term_pane = s.active_pane_id;
        assert_eq!(s.spaces[0].root.first_leaf_id(), 1, "chat leaf stays first");
        let term_conv = s.pane_sessions.get(&term_pane).unwrap().conversation_id.clone();
        assert_ne!(term_conv, chat_id, "terminal pane binds a distinct conversation");
        assert!(
            !s.pane_sessions.get(&term_pane).unwrap().path.is_empty(),
            "terminal pane initializes a cwd to spawn its shell"
        );
        drop(s);
    }

    #[test]
    fn terminal_command_routes_to_panes_own_conversation() {
        let (desktop, _dir) = desktop_state();
        let chat_id = desktop.create_chat("Initial", None, None).expect("create chat");
        let (state, actions, _media) = build(&desktop);

        (actions.on_new_terminal.borrow_mut())();
        let (term_pane, term_conv) = {
            let s = state.borrow();
            let pane = s.active_pane_id;
            (
                pane,
                s.pane_sessions.get(&pane).unwrap().conversation_id.clone(),
            )
        };
        assert_ne!(term_conv, chat_id);

        // No API key configured: Cmd+Enter in the terminal keeps a user message
        // on the terminal pane's own conversation (mirrors how chat sends route
        // to the active pane) and does not touch the chat conversation.
        (actions.on_terminal_command.borrow_mut())(term_pane, "run me as an agent".to_string());

        let term_msgs = desktop.list_chat_messages(&term_conv).expect("list terminal conv");
        assert_eq!(term_msgs.len(), 1);
        assert_eq!(term_msgs[0].role, "user");
        let chat_msgs = desktop.list_chat_messages(&chat_id).expect("list chat conv");
        assert_eq!(chat_msgs.len(), 0, "chat conversation stays untouched");
    }

    #[test]
    fn toggle_fullscreen_flips_state_and_requests_window() {
        let (desktop, _dir) = desktop_state();
        let state = Rc::new(RefCell::new(UiState::from_desktop(&desktop)));

        // Install a handler that records the requested fullscreen value.
        let last = Rc::new(RefCell::new(None));
        let last_clone = last.clone();
        let window_control = WindowControl::default();
        window_control.install(move |fullscreen| *last_clone.borrow_mut() = Some(fullscreen));

        let actions = make_actions(
            Rc::clone(&state),
            Some(Arc::clone(&desktop)),
            Rc::new(RefCell::new(MediaState::mock())),
            window_control,
        );

        assert!(!state.borrow().fullscreen, "window starts windowed");
        (actions.on_toggle_fullscreen.borrow_mut())();
        assert!(state.borrow().fullscreen, "toggle enters fullscreen");
        assert_eq!(last.borrow_mut().take(), Some(true), "window requested fullscreen");

        (actions.on_toggle_fullscreen.borrow_mut())();
        assert!(!state.borrow().fullscreen, "toggle leaves fullscreen");
        assert_eq!(last.borrow_mut().take(), Some(false), "window requested windowed");
    }

    #[test]
    fn clear_transcript_clears_active_pane_messages() {
        let (desktop, _dir) = desktop_state();
        let chat_id = desktop.create_chat("Initial", None, None).expect("create chat");
        let state = Rc::new(RefCell::new(UiState::from_desktop(&desktop)));
        {
            let mut s = state.borrow_mut();
            s.bind_active_pane_conversation(chat_id.clone(), Some(desktop.as_ref()));
            desktop
                .add_chat_message(&chat_id, "user", "hello")
                .expect("add message");
            s.refresh_messages(&desktop);
            assert!(!s.chat_messages.is_empty(), "transcript is populated");
        }
        let actions = make_actions(
            Rc::clone(&state),
            Some(Arc::clone(&desktop)),
            Rc::new(RefCell::new(MediaState::mock())),
            WindowControl::default(),
        );
        (actions.on_clear_transcript.borrow_mut())();
        assert!(
            state.borrow().chat_messages.is_empty(),
            "clear empties the active pane's transcript"
        );
    }
}
