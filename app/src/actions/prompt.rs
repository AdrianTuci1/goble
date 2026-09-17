use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;

use crate::media::MediaState;
use crate::state::UiState;

/// Run one agent turn on `pane_id`'s own conversation (see `on_send_message`).
///
/// Shared by the chat composer's Enter path, the Cmd/Ctrl+Enter new-conversation
/// path and the terminal pane's Cmd+Enter path, so all three send the prompt
/// through the same [`crate::runtime::run_turn`] → daemon pipeline. Returns
/// whether a turn actually started (so the caller can flip the Stop button).
///
/// With no runnable model (no API key, or a key with no provider/model) there is
/// nothing to send the prompt to: no turn starts, no conversation is created for
/// it and nothing is written into one — the pane shows only the notice band
/// naming what is missing, and the composer keeps the text the user typed.
pub(super) fn send_agent_prompt(
    state: &mut UiState,
    desktop: Option<&Arc<DesktopState>>,
    media: &Rc<RefCell<MediaState>>,
    text: &str,
    pane_id: u64,
) -> bool {
    if !state.can_run_agent_turn(pane_id) {
        state.show_llm_key_banner = true;
        state.agent_busy = false;
        return false;
    }
    // The turn uses this pane's own model; a pane that has not chosen one falls
    // back to the window-global selection, then to the configured default.
    let model = state.pane_agent_model(pane_id);
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
    // global composer path for a pane without a session entry). A viewer pane
    // has no local cwd: the worker resolves the workspace directory for a turn
    // that runs on it, so this machine's path is never handed over as one.
    let path = if state.pane_is_viewer(pane_id) {
        String::new()
    } else {
        state
            .pane_sessions
            .get(&pane_id)
            .map(|s| s.path.clone())
            .unwrap_or_else(|| state.composer_path.clone())
    };
    let (medium_id, project_id, session_id) = {
        let media_b = media.borrow();
        let session_id = if media_b.selected_session_id().is_empty() {
            chat_id.clone().unwrap_or_default()
        } else {
            media_b.selected_session_id().to_string()
        };
        // A conversation that runs on a worker carries the environment chosen in
        // its own composer (A3): the persisted medium, or the one its routing
        // implies while nothing has been chosen. The project follows that
        // environment, so the turn is not scoped to another environment's
        // project. Every other pane keeps the window's selected environment.
        match state.pane_environment(pane_id) {
            Some(environment) => (
                environment.clone(),
                media_b.default_project_for_medium(&environment),
                session_id,
            ),
            None => (
                media_b.selected_medium_id().to_string(),
                media_b.selected_project_id().to_string(),
                session_id,
            ),
        }
    };
    // The pane's own shell when it has a terminal: the agent's commands then run
    // in it, visibly, instead of the sandbox. A pane with no terminal passes
    // `None` and keeps the sandboxed runner.
    let mut ran_turn = false;
    if let (Some(desktop), Some(chat_id)) = (desktop, chat_id) {
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
            // A viewer pane reports the connection it could not make instead of
            // running the turn locally: the pane's session never becomes a
            // shell, and the failure is the state it draws.
            if state.pane_is_viewer(pane_id) {
                state.worker_connection_lost(pane_id, &e.to_string());
            }
            let _ = desktop.add_chat_message(
                &chat_id,
                "assistant",
                &format!("(nu am putut porni modelul: {e})"),
            );
        } else {
            state.begin_turn(pane_id);
            ran_turn = true;
        }
        state.refresh_messages(desktop);
    } else {
        state.sync_active_view();
    }
    if pane_id == state.active_pane_id {
        state.set_active_pane_draft(String::new());
    }
    // The attachments described the draft that was just sent, so they go with
    // it: the next draft starts with no chips.
    state.pane_attachments.remove(&pane_id);
    state.agent_busy = ran_turn;
    ran_turn
}

/// Run `text` as a shell command in `pane_id`'s real PTY session.
///
/// Every pane (chat or terminal) is given a live [`TerminalSession`] on demand
/// so a terminal command actually executes in a shell; the output lands in the
/// pane's terminal buffer (rendered live for a terminal pane). The shell echoes
/// the typed line, so no separate echo is needed.
///
/// The line is also read for where it leaves the pane's shell: an interactive
/// `ssh` command binds the pane's session to the host it names — its alias
/// resolved through the machine's `~/.ssh/config` — and `exit` returns it to a
/// local shell, so the pane can name the host it is on
/// ([`goble_core::ssh_command`]).
///
/// `conversation` names the conversation this command was typed inside, when it
/// was one: the block the shell runs it in becomes that conversation's own
/// block, so the conversation's agent view draws it and the shell's history
/// leaves it out — a command typed inside a conversation is the conversation's
/// (warp-new's split between a block created in the terminal and one created in
/// an agent view). A command typed at the shell's own bar passes `None`.
pub(super) fn run_terminal_command(
    state: &mut UiState,
    text: &str,
    pane_id: u64,
    conversation: Option<&str>,
) {
    // A viewer pane has no local shell, whatever the line: its commands belong
    // to the worker, and spawning a pty here would be exactly the local
    // fallback the pane exists to refuse. Nothing is run, here or anywhere.
    if state.pane_is_viewer(pane_id) {
        return;
    }
    let cwd = state
        .pane_sessions
        .get(&pane_id)
        .map(|s| s.path.clone())
        .unwrap_or_default();
    // Where this line leaves the pane's shell: an interactive `ssh` command
    // binds the pane to the host it names, `exit` returns it to the local
    // shell. The line is read here, where the user's own command is submitted —
    // the shell's bar and the pane's agent view (`!command`) both arrive here —
    // and any other line leaves the binding where it was: whether it ran locally
    // or on the remote cannot be told from the line, so it is not guessed.
    // `~/.ssh` is only read for a line that opens a session.
    let ssh_session = match goble_core::ssh_command::parse_ssh_command(text) {
        Some(target) => {
            let hosts = state.ssh_hosts_for_command();
            Some(target.resolve(hosts.as_ref(), &goble_core::ssh_hosts::current_user()))
        }
        None => None,
    };
    let leaves_the_shell = goble_core::ssh_command::returns_to_local_shell(text);
    let mut reg = state.terminal.borrow_mut();
    if let Some(session) = ssh_session {
        reg.set_ssh_session(pane_id, Some(session));
    } else if leaves_the_shell {
        reg.set_ssh_session(pane_id, None);
    }
    reg.ensure_session(pane_id, &cwd);
    if let Some(session) = reg.sessions.get_mut(&pane_id) {
        if let Some(conversation) = conversation {
            session.run_block_for_conversation(conversation);
        }
        let mut line = text.trim_end().to_string();
        line.push('\n');
        session.write(line.as_bytes());
    }
}
