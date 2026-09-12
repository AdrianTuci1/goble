use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;
use goble_ui::{ChatMessage, ChatRole};

use crate::media::MediaState;
use crate::state::UiState;

/// Honest assistant reply shown when the user submits an agent prompt with no
/// LLM configured (no API key). Tells them how to proceed rather than silently
/// dropping the turn.
const NO_MODEL_REPLY: &str = "No model is configured: `settings_llm_model` and `settings_llm_api_key` \
are both empty, so I can't run this as an agent turn. Configure a provider and model \
in Settings, or prefix this line with `!` to run it as a terminal command instead.";

/// Honest assistant reply shown when an API key is present but no provider/model
/// is set, so the turn cannot run. Surfaces the gap instead of failing silently.
const MODEL_MISSING_REPLY: &str = "An API key is configured, but no provider/model is set \
(`settings_llm_model` is empty), so I can't run this as an agent turn. Pick a provider \
and model in Settings, or prefix this line with `!` to run it as a terminal command instead.";

/// Run one agent turn on `pane_id`'s own conversation (see `on_send_message`).
///
/// Shared by the chat composer's Enter path, the Cmd/Ctrl+Enter new-conversation
/// path and the terminal pane's Cmd+Enter path, so all three send the prompt
/// through the same [`crate::runtime::run_turn`] → daemon pipeline. Returns
/// whether a turn actually started (so the caller can flip the Stop button).
///
/// When no runnable model is configured (no API key, or a key with an empty
/// provider/model) the user's message is still kept in the transcript and an
/// honest assistant reply explains the gap — instead of only surfacing the
/// first-run key banner or silently failing the turn.
pub(super) fn send_agent_prompt(
    state: &mut UiState,
    desktop: Option<&Arc<DesktopState>>,
    media: &Rc<RefCell<MediaState>>,
    text: &str,
    pane_id: u64,
) -> bool {
    let configured = !state.settings_llm_api_key.trim().is_empty();
    // The turn uses this pane's own model; a pane that has not chosen one falls
    // back to the window-global selection, then to the configured default.
    let pane_model = state
        .pane_controls
        .get(&pane_id)
        .map(|c| c.model.clone())
        .filter(|m| !m.trim().is_empty());
    let model = pane_model.unwrap_or_else(|| {
        if state.selected_model.trim().is_empty() {
            state.settings_llm_model.clone()
        } else {
            state.selected_model.clone()
        }
    });
    // Auto-approve is a per-pane control: point the shared setting at this
    // pane's value so an auto-approving pane does not leak into a sibling.
    if let (Some(desktop), Some(pane_auto)) = (
        desktop,
        state.pane_controls.get(&pane_id).map(|c| c.auto_approve),
    ) {
        if desktop.get_auto_approve() != pane_auto {
            let _ = desktop.set_auto_approve(pane_auto);
        }
    }
    let chat_id = state.pane_conversation_id(pane_id);
    // The turn's working directory is the pane's own cwd (falls back to the
    // global composer path for a pane without a session entry).
    let path = state
        .pane_sessions
        .get(&pane_id)
        .map(|s| s.path.clone())
        .unwrap_or_else(|| state.composer_path.clone());
    // Resolve the fallback reply up front so the store-backed and mock paths
    // behave identically: either there is a runnable model, or we answer
    // honestly rather than pretending a turn ran.
    let fallback_reply = if !configured {
        Some(NO_MODEL_REPLY)
    } else if state.settings_llm_provider.trim().is_empty() || model.trim().is_empty() {
        Some(MODEL_MISSING_REPLY)
    } else {
        None
    };
    let mut ran_turn = false;
    if let (Some(desktop), Some(chat_id)) = (desktop, chat_id) {
        if let Some(reply) = fallback_reply {
            // No runnable model: keep the user's message and reply honestly
            // instead of only showing the key banner (or failing silently).
            let _ = desktop.add_chat_message(&chat_id, "user", text);
            let _ = desktop.add_chat_message(&chat_id, "assistant", reply);
            if !configured {
                state.show_llm_key_banner = true;
            }
        } else {
            let (medium_id, project_id, session_id) = {
                let media_b = media.borrow();
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
            // The pane's own shell when it has a terminal: the agent's commands
            // then run in it, visibly, instead of the sandbox. A pane with no
            // terminal passes `None` and keeps the sandboxed runner.
            let pane_session = state.pane_session(pane_id, &chat_id);
            if let Err(e) = crate::runtime::run_turn(
                desktop,
                &chat_id,
                text,
                &state.settings_llm_provider,
                &model,
                state.workspace_routing,
                &medium_id,
                &project_id,
                &session_id,
                &path,
                Some(state.selected_harness.as_str()),
                pane_session,
            ) {
                log::warn!("run_chat_turn failed: {e}");
                let _ = desktop.add_chat_message(
                    &chat_id,
                    "assistant",
                    &format!("(nu am putut porni modelul: {e})"),
                );
            } else {
                state.begin_turn(pane_id);
                state.agent_busy = true;
                ran_turn = true;
            }
        }
        state.refresh_messages(desktop);
    } else {
        state.push_active_message(ChatMessage::from_markdown(ChatRole::User, text.to_string()));
        if let Some(reply) = fallback_reply {
            state.push_active_message(ChatMessage::from_markdown(ChatRole::Assistant, reply));
            if !configured {
                state.show_llm_key_banner = true;
            }
        }
        state.sync_active_view();
    }
    if pane_id == state.active_pane_id {
        state.set_active_pane_draft(String::new());
    }
    state.agent_busy = ran_turn;
    ran_turn
}

/// Run `text` as a shell command in `pane_id`'s real PTY session.
///
/// Every pane (chat or terminal) is given a live [`TerminalSession`] on demand
/// so a terminal command actually executes in a shell; the output lands in the
/// pane's terminal buffer (rendered live for a terminal pane). The shell echoes
/// the typed line, so no separate echo is needed.
pub(super) fn run_terminal_command(state: &mut UiState, text: &str, pane_id: u64) {
    let cwd = state
        .pane_sessions
        .get(&pane_id)
        .map(|s| s.path.clone())
        .unwrap_or_default();
    let mut reg = state.terminal.borrow_mut();
    reg.ensure_session(pane_id, &cwd);
    if let Some(session) = reg.sessions.get_mut(&pane_id) {
        let mut line = text.trim_end().to_string();
        line.push('\n');
        session.write(line.as_bytes());
    }
}
