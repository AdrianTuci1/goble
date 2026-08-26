//! Callback wiring: turns app state + backend into [`UiActions`] closures.
//!
//! The view tree built by `goble-ui-hot` only knows about these callbacks; the
//! actual behavior (mutating [`UiState`], persisting through [`DesktopState`])
//! lives here in the executable.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::agent::Trigger;
use goble_core::worker::WorkerId;
use goble_core::workflow::WorkflowId;
use goble_desktop_service::DesktopState;
use goble_ui::{ChatMessage, ChatRole, ConversationEntry, SettingsPage};

use crate::hot_ui::{AppTab, CronEntry, UiActions, WorkspaceRouting};
use crate::state::{routing_to_str, UiState};

pub fn make_actions(
    state: Rc<RefCell<UiState>>,
    desktop: Option<Arc<DesktopState>>,
) -> UiActions {
    let on_search_change = Rc::clone(&state);
    let on_search_focus_change = Rc::clone(&state);
    let on_create_change = Rc::clone(&state);
    let on_create_focus_change = Rc::clone(&state);
    let on_create_submit = Rc::clone(&state);
    let on_select_conversation = Rc::clone(&state);
    let on_select_tab = Rc::clone(&state);
    let on_composer_change = Rc::clone(&state);
    let on_composer_focus_change = Rc::clone(&state);
    let on_send_message = Rc::clone(&state);
    let on_attach = Rc::clone(&state);
    let on_voice = Rc::clone(&state);
    let on_model_select = Rc::clone(&state);
    let on_stop = Rc::clone(&state);
    let on_answer_ask = Rc::clone(&state);
    let on_skip_ask = Rc::clone(&state);
    let on_toggle_auto_approve = Rc::clone(&state);
    let on_send_queued = Rc::clone(&state);
    let on_dismiss_queued = Rc::clone(&state);
    let on_threads = Rc::clone(&state);
    let on_settings = Rc::clone(&state);
    let on_open_crons = Rc::clone(&state);
    let on_close_crons = Rc::clone(&state);
    let on_toggle_right_sidebar = Rc::clone(&state);
    let on_cron_create = Rc::clone(&state);
    let on_cron_delete = Rc::clone(&state);
    let on_cron_trigger = Rc::clone(&state);
    let on_sidebar_drag_start = Rc::clone(&state);
    let on_sidebar_drag_move = Rc::clone(&state);
    let on_sidebar_drag_end = Rc::clone(&state);
    let on_agent_delete = Rc::clone(&state);
    let on_settings_back = Rc::clone(&state);
    let on_settings_navigate = Rc::clone(&state);
    let on_toggle_dark_mode = Rc::clone(&state);
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
    let on_choose_workspace = Rc::clone(&state);
    let on_close_llm_dialog = Rc::clone(&state);

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
            if let Some(desktop) = &desktop_create {
                match desktop.create_chat(&title, None, None) {
                    Ok(id) => {
                        state.new_conversation_draft.clear();
                        state.selected_id = Some(id);
                        state.refresh_from_desktop(desktop);
                    }
                    Err(e) => log::warn!("create_chat failed: {e}"),
                }
            } else {
                let id = format!("c-{}", state.conversations.len() + 1);
                state.conversations.insert(
                    0,
                    ConversationEntry::new(id.clone(), title, "New conversation", "now"),
                );
                state.selected_id = Some(id);
                state.new_conversation_draft.clear();
            }
        })),
        on_select_conversation: Rc::new(RefCell::new(move |id: String| {
            let mut state = on_select_conversation.borrow_mut();
            state.selected_id = Some(id.clone());
            if let Some(desktop) = &desktop_select {
                state.refresh_messages(desktop);
            }
        })),
        on_select_tab: Rc::new(RefCell::new(move |tab: AppTab| {
            on_select_tab.borrow_mut().current_tab = tab;
        })),
        on_composer_change: Rc::new(RefCell::new(move |value: String| {
            on_composer_change.borrow_mut().composer_draft = value;
        })),
        on_composer_focus_change: Rc::new(RefCell::new(move |focused: bool| {
            on_composer_focus_change.borrow_mut().composer_focused = focused;
        })),
        on_send_message: Rc::new(RefCell::new(move |text: String| {
            let mut state = on_send_message.borrow_mut();
            let configured = !state.settings_llm_api_key.trim().is_empty();
            // While a turn is running, don't start a second one (that would
            // interrupt the agent). Queue the prompt and render it as a pending
            // block; it is sent when the current turn finishes or via "Send now".
            if state.agent_busy {
                state.queued_prompt = Some(text);
                state.composer_draft.clear();
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
            if let (Some(desktop), Some(chat_id)) = (&desktop_send, state.selected_id.clone()) {
                // Route the turn to the conversation's chosen runtime. Chat turns
                // run on the local harness (the only wired target today); a
                // `Remote` routing is persisted but remote chat execution is not
                // implemented yet, so it degrades to local and the gap is logged.
                if state.workspace_routing == Some(WorkspaceRouting::Remote) {
                    log::warn!(
                        "conversation {chat_id} is routed remote, but remote chat execution is not wired yet; running locally"
                    );
                }
                if configured {
                    // Run the real harness turn on a background task. The harness
                    // inserts the user message and the assistant/tool output into
                    // the store and emits `chat:updated`, so the UI refreshes as
                    // the turn progresses (including any tool-call output).
                    if let Err(e) = desktop.run_chat_turn(
                        &chat_id,
                        &text,
                        &state.settings_llm_provider,
                        &model,
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
                state
                    .chat_messages
                    .push(ChatMessage::from_markdown(ChatRole::User, text.clone()));
                if !configured {
                    state.show_llm_key_banner = true;
                }
            }
            state.composer_draft.clear();
            state.agent_busy = ran_turn;
        })),
        on_attach: Rc::new(RefCell::new(move || {
            on_attach
                .borrow_mut()
                .chat_messages
                .push(ChatMessage::from_markdown(
                    ChatRole::Assistant,
                    "(attach — file picker coming soon)",
                ));
        })),
        on_voice: Rc::new(RefCell::new(move || {
            on_voice
                .borrow_mut()
                .chat_messages
                .push(ChatMessage::from_markdown(
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
        on_copy: Rc::new(RefCell::new(move || {
            log::info!("copy transcript pressed");
        })),
        on_restart: Rc::new(RefCell::new(move || {
            log::info!("restart agent pressed");
        })),
        on_stop: Rc::new(RefCell::new(move || {
            let mut state = on_stop.borrow_mut();
            if let (Some(desktop), Some(chat_id)) = (&desktop_stop, state.selected_id.clone()) {
                let _ = desktop.cancel_chat_turn(&chat_id);
            }
            state.agent_busy = false;
        })),
        on_answer_ask: Rc::new(RefCell::new(move |response: String, credential: Option<(String, String)>| {
            let mut state = on_answer_ask.borrow_mut();
            let model = if state.selected_model.trim().is_empty() {
                state.settings_llm_model.clone()
            } else {
                state.selected_model.clone()
            };
            if let (Some(desktop), Some(chat_id)) = (&desktop_answer, state.selected_id.clone()) {
                if let Err(e) = desktop.resume_chat_turn(
                    &chat_id,
                    &response,
                    credential,
                    &state.settings_llm_provider,
                    &model,
                ) {
                    log::warn!("resume_chat_turn failed: {e}");
                } else {
                    state.agent_busy = true;
                    state.pending_ask = None;
                }
                state.refresh_messages(desktop);
            } else {
                state.pending_ask = None;
            }
        })),
        on_skip_ask: Rc::new(RefCell::new(move || {
            let mut state = on_skip_ask.borrow_mut();
            let model = if state.selected_model.trim().is_empty() {
                state.settings_llm_model.clone()
            } else {
                state.selected_model.clone()
            };
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
                    state.agent_busy = true;
                    state.pending_ask = None;
                }
                state.refresh_messages(desktop);
            } else {
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
            let Some(prompt) = state.queued_prompt.take() else {
                return;
            };
            let model = if state.selected_model.trim().is_empty() {
                state.settings_llm_model.clone()
            } else {
                state.selected_model.clone()
            };
            if let (Some(desktop), Some(chat_id)) = (&desktop_send_queued, state.selected_id.clone())
            {
                // "Send now": interrupt the in-flight turn and submit immediately.
                let _ = desktop.cancel_chat_turn(&chat_id);
                if let Err(e) = desktop.run_chat_turn(
                    &chat_id,
                    &prompt,
                    &state.settings_llm_provider,
                    &model,
                ) {
                    log::warn!("run_chat_turn (queued) failed: {e}");
                } else {
                    state.agent_busy = true;
                }
                state.refresh_messages(desktop);
            }
        })),
        on_dismiss_queued: Rc::new(RefCell::new(move || {
            let mut state = on_dismiss_queued.borrow_mut();
            state.queued_prompt = None;
        })),
        on_menu: Rc::new(RefCell::new(move || {
            log::info!("menu pressed");
        })),
        on_threads: Rc::new(RefCell::new(move || {
            on_threads.borrow_mut().current_tab = AppTab::Threads;
        })),
        on_inbox: Rc::new(RefCell::new(move || {
            log::info!("inbox pressed (coming soon)");
        })),
        on_settings: Rc::new(RefCell::new(move || {
            on_settings.borrow_mut().current_tab = AppTab::Settings;
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
            state.chat_messages.push(ChatMessage::from_markdown(
                ChatRole::Assistant,
                format!("Am pornit cron-ul „{name}”."),
            ));
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
            state.sidebar_width =
                (state.sidebar_drag_start_width + delta).clamp(200.0, 480.0);
        })),
        on_sidebar_drag_end: Rc::new(RefCell::new(move || {
            on_sidebar_drag_end.borrow_mut().sidebar_dragging = false;
        })),
        on_agent_delete: Rc::new(RefCell::new(move |id: String| {
            let mut state = on_agent_delete.borrow_mut();
            state.conversations.retain(|c| c.id != id);
            state.agent_cards.remove(&id);
            if state.selected_id.as_deref() == Some(id.as_str()) {
                state.selected_id = state.conversations.first().map(|c| c.id.clone());
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
        on_save_llm: Rc::new(RefCell::new(move |provider: String, model: String, api_key: String, base_url: String, temperature: String| {
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
        })),
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
        on_add_authorized_key: Rc::new(RefCell::new(move |name: String, pem: String, fingerprint: String| {
            let mut state = on_add_authorized_key.borrow_mut();
            let id = format!("k-{}", state.settings_authorized_keys.len() + 1);
            state.settings_authorized_keys.push((id, name, fingerprint));
            let _ = pem;
        })),
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
                    if let Err(e) = desktop.set_chat_workspace_routing(&chat_id, Some(routing_to_str(routing))) {
                        log::warn!("set_chat_workspace_routing failed: {e}");
                    }
                }
            }
            // Returning to chat lets the conversation continue in the chosen
            // workspace.
            state.current_tab = AppTab::Chat;
        })),
    }
}
