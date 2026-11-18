use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::hosts::HostEntry;
use goble_desktop_service::DesktopState;

use crate::media::MediaState;
use crate::state::{default_pane_path, UiState};
use crate::ui::{Pane, PaneKind, Space, SplitDir};

/// Split the active pane of the active space along `dir`, focusing the new pane
/// and binding it to a distinct conversation + cwd so the two panes never share
/// a transcript. `kind` is the kind of the newly created leaf (so a terminal
/// pane can be opened off a chat pane).
pub(super) fn split_active_pane(
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
            state.focus_pane(new_id);
            ensure_pane_hover(state, new_id);
            state.bind_pane_new_conversation(new_id, desktop.map(|d| d.as_ref()));
            let project_id = media.selected_project_id().to_string();
            let path = default_pane_path(&project_id, desktop.map(|d| d.as_ref()));
            state.set_active_pane_path(path);
            state.sync_active_view();
        }
    }
}

/// Open a saved SSH host in a new terminal pane and run the host's `ssh -t`
/// line in it. The pane is bound through [`TerminalRegistry::set_ssh_session`]
/// before the line is written, so the existing chip and the agent's shell route
/// read the session from the same place a typed `ssh host` line puts it.
pub(super) fn open_ssh_host_pane(
    state: &mut UiState,
    desktop: Option<&Arc<DesktopState>>,
    media: &MediaState,
    host: &HostEntry,
) {
    split_active_pane(state, SplitDir::Vertical, desktop, media, PaneKind::Terminal);
    let pane_id = state.active_pane_id;
    let cwd = state
        .pane_sessions
        .get(&pane_id)
        .map(|session| session.path.clone())
        .unwrap_or_default();
    let session = host.ssh_session();
    state.bind_ssh_pane(pane_id, Some(host.name.clone()), session);
    run_saved_host_line(state, pane_id, &cwd, host);
    if let Some(desktop) = desktop {
        state.save_panes(desktop);
    }
}

/// Reconnect `pane_id` to the saved host `name` after its session ended. The
/// shell is restarted first: an `exit` leaves a dead pty behind, and writing
/// the ssh line into that would do nothing.
pub(super) fn reconnect_ssh_host(
    state: &mut UiState,
    desktop: Option<&Arc<DesktopState>>,
    pane_id: u64,
    name: &str,
) -> bool {
    let Some(host) = state
        .ssh_host_entries
        .iter()
        .find(|host| host.name == name)
        .cloned()
    else {
        // The entry was removed from hosts.toml since the pane opened. There is
        // no command to run, so the offer is dropped rather than drawing a
        // reconnect that cannot.
        state.pane_ssh.remove(&pane_id);
        return false;
    };
    let cwd = state
        .pane_sessions
        .get(&pane_id)
        .map(|session| session.path.clone())
        .unwrap_or_default();
    let session = host.ssh_session();
    state.bind_ssh_pane(pane_id, Some(host.name.clone()), session);
    {
        let mut terminal = state.terminal.borrow_mut();
        terminal.restart_session(pane_id, &cwd);
    }
    run_saved_host_line(state, pane_id, &cwd, &host);
    if let Some(desktop) = desktop {
        state.save_panes(desktop);
    }
    true
}

/// Write the host's command line into the pane's shell. The session is already
/// bound by the caller; this only gives the shell the line to run.
fn run_saved_host_line(state: &mut UiState, pane_id: u64, cwd: &str, host: &HostEntry) {
    let mut terminal = state.terminal.borrow_mut();
    terminal.ensure_session(pane_id, cwd);
    if let Some(session) = terminal.sessions.get_mut(&pane_id) {
        let mut line = host.command_line();
        line.push('\n');
        session.write(line.as_bytes());
    }
}

/// Open `file_path` in a file-view pane of the active space, and focus it.
///
/// The view opens beside the pane that asked for it, in the tab on screen. A
/// pane that is already showing a file takes the new one as another chip
/// instead of splitting again, so browsing a directory with the file view
/// focused walks through files in one pane rather than tiling the tab with
/// them. The pane keeps no conversation and no working directory of its own:
/// the explorer's tree follows the last pane that had one.
pub(super) fn open_file_pane(
    state: &mut UiState,
    file_path: String,
    desktop: Option<&Arc<DesktopState>>,
) {
    if state.active_pane_shows_file() {
        let active = state.active_pane_id;
        state.show_file_in_pane(active, file_path);
    } else {
        let kind = PaneKind::File {
            path: file_path.clone(),
        };
        let space_idx = state.active_space;
        let Some(space) = state.spaces.get_mut(space_idx) else {
            return;
        };
        let Some(new_id) = space.split_with_kind(
            state.active_pane_id,
            SplitDir::Horizontal,
            &mut state.next_pane_id,
            kind,
        ) else {
            return;
        };
        state.pane_open_files.insert(new_id, vec![file_path]);
        state.focus_pane(new_id);
        ensure_pane_hover(state, new_id);
    }
    state.sync_active_view();
    if let Some(desktop) = desktop {
        state.save_panes(desktop);
    }
}

/// Open the working tree's review pane for `source`'s directory (or focus the
/// one this tab already has), optionally showing one changed file.
///
/// The pane carries the directory it reviews, so it survives a restart and a
/// lift into another tab. A tab keeps at most one review pane per directory:
/// pressing the composer's diff control again focuses that pane instead of
/// tiling the tab with duplicates.
pub(super) fn open_diff_pane(
    state: &mut UiState,
    source: u64,
    file: Option<String>,
    desktop: Option<&Arc<DesktopState>>,
) {
    let cwd = state
        .pane_sessions
        .get(&source)
        .map(|session| session.path.clone())
        .filter(|path| !path.trim().is_empty())
        .unwrap_or_else(|| state.composer_path.clone());
    if cwd.trim().is_empty() {
        return;
    }
    let space_idx = state.active_space;
    let Some(space) = state.spaces.get(space_idx) else {
        return;
    };
    if let Some(id) = space.diff_leaf_for(&cwd) {
        state.focus_pane(id);
        let controls = state.pane_controls_mut(id);
        *controls.diff_selected.borrow_mut() = file;
        controls.diff_scroll.borrow_mut().reset();
        if let Some(desktop) = desktop {
            state.save_panes(desktop);
        }
        return;
    }
    let anchor = if space.root.contains_leaf(source) {
        source
    } else {
        state.active_pane_id
    };
    let Some(space) = state.spaces.get_mut(space_idx) else {
        return;
    };
    let Some(new_id) = space.split_with_kind(
        anchor,
        SplitDir::Horizontal,
        &mut state.next_pane_id,
        PaneKind::Diff { cwd },
    ) else {
        return;
    };
    state.focus_pane(new_id);
    ensure_pane_hover(state, new_id);
    {
        let controls = state.pane_controls_mut(new_id);
        *controls.diff_selected.borrow_mut() = file;
    }
    state.sync_active_view();
    if let Some(desktop) = desktop {
        state.save_panes(desktop);
    }
}

/// Ensure a hover flag exists for a pane so the pane-header hover survives
/// the per-frame element rebuild.
pub(super) fn ensure_pane_hover(state: &mut UiState, pane_id: u64) {
    state
        .pane_hover
        .entry(pane_id)
        .or_insert_with(|| Rc::new(RefCell::new(false)));
}

/// The reason a busy shell gives for refusing the switch into agent mode, word
/// for word the reference's own `EnterAgentViewError::LongRunningCommand`
/// (`Cannot enter agent mode while a command is running.`). The pane draws it
/// over the shell it stayed on, so the chord never does nothing silently.
pub(super) const HARNESS_BUSY_REFUSAL: &str = "Cannot enter agent mode while a command is running.";

/// Open `pane_id`'s harness: the pane shows its agent view over its own shell.
///
/// A pane with no conversation of its own gets one bound first, so the view
/// always has a transcript to draw; entering the view also pushes the
/// conversation's card into the pane's block list, which is what the terminal
/// shows as the way back to it. The caller decides the pane's kind: the harness
/// only has a terminal to return to when the pane is a terminal leaf.
///
/// Switching to the agent view never depends on a model being runnable: the
/// view opens, the pane gets a conversation to draw if it had none, and the
/// transcript's notice band names the missing key or model. Refusing the switch
/// instead left the pane on its shell with nothing to say why the chord (or the
/// sidebar's "New conversation" row, which lands here) did nothing.
///
/// Two guards, both on this one entry point, both the reference's own:
///
/// * **Already in the agent view** — entering again pushes nothing (no second
///   conversation, no second card, no second view). `AlreadyInAgentView` is
///   warp-new's guard for the same direction, and the pane really is entered
///   twice: a card click and a `Cmd+Enter` that arrive before the next frame's
///   rebuild both land on the tree that drew the shell.
/// * **A command running in the pane's shell** — the switch is refused and says
///   so (`HARNESS_BUSY_REFUSAL`), because the agent's own commands cannot run in
///   a shell that is not at a prompt: the claim attaches to the block that is
///   waiting for a command, and a running command's block is not one. The pane
///   records the reason and draws it over the shell it stayed on. An SSH-bound
///   pane is exempt — see `TerminalRegistry::running_a_command` — so the
///   switch works on exactly the session it exists for.
pub(super) fn open_pane_harness(
    state: &mut UiState,
    pane_id: u64,
    desktop: Option<&Arc<DesktopState>>,
) {
    // A viewer pane has no shell to switch from: its conversation runs on a
    // worker, so mounting the agent view over a pty it does not have would
    // invent a terminal for it. The pane already reports its own state.
    if state.pane_is_viewer(pane_id) {
        return;
    }
    if state.pane_controls(pane_id).harness_mode {
        return;
    }
    if state.terminal.borrow().running_a_command(pane_id) {
        state.pane_controls_mut(pane_id).harness_refusal = Some(HARNESS_BUSY_REFUSAL.to_string());
        return;
    }
    state.pane_controls_mut(pane_id).harness_refusal = None;
    state.pane_controls_mut(pane_id).harness_mode = true;
    if !state.pane_owns_conversation(pane_id) {
        state.bind_pane_new_conversation(pane_id, desktop.map(|d| d.as_ref()));
    }
    // The band is the agent view's own report, so it follows whatever the pane
    // can run right now — set before the view switches so the first frame drawn
    // already carries it.
    state.show_llm_key_banner = !state.can_run_agent_turn(pane_id);
    if let Some(conversation_id) = state.pane_conversation_id(pane_id) {
        let label = state.conversation_name(&conversation_id);
        state.enter_agent_view(pane_id, &conversation_id, &label);
    }
}

/// Close the space (tab) at `index`.
///
/// The last remaining space is never removed: it resets to a fresh empty chat
/// pane, so closing a tab never exits the program (an earlier version owned the
/// single pane, and a close on it tore the window down).
pub(super) fn close_space_at(state: &mut UiState, index: usize, desktop: Option<&DesktopState>) {
    if index >= state.spaces.len() {
        return;
    }
    // Drop every pane session/runtime/hover for the space's leaves and kill any
    // terminal sessions along with them.
    let closed = state.spaces.remove(index);
    let leaves = collect_leaf_ids(&closed.root);
    // A pane in the closed tab owns nothing any more: its session, runtime,
    // hover flag, child view, shell and viewer session all go with it — the last
    // one because the worker's status path finds the panes a report is about by
    // the viewer sessions alone, so a session left behind is a dead pane id it
    // would keep reporting on.
    for id in leaves {
        state.forget_pane(id);
    }
    // If the space being closed was the last one, restore a fresh empty chat
    // space so the window is never left with zero tabs.
    if state.spaces.is_empty() {
        let id = state.next_pane_id;
        state.next_pane_id += 1;
        // Nobody named this space: its tab label is derived from what it holds,
        // so it reads the agent label until its conversation has a subject.
        state.spaces.push(Space::unnamed(Pane::Leaf { id, kind: PaneKind::Chat }));
        state.active_space = 0;
        state.active_pane_id = id;
        ensure_pane_hover(state, id);
        state.bind_pane_new_conversation(id, desktop);
        state.refresh_space_labels();
        state.sync_active_view();
        if let Some(desktop) = desktop {
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
    if let Some(desktop) = desktop {
        state.refresh_messages(desktop);
        state.save_panes(desktop);
    }
}

/// Every leaf pane id in a pane tree, used to drop a space's sessions when its
/// tab is closed.
pub(super) fn collect_leaf_ids(pane: &Pane) -> Vec<u64> {
    match pane {
        Pane::Leaf { id, .. } => vec![*id],
        Pane::Split { first, second, .. } => {
            let mut ids = collect_leaf_ids(first);
            ids.extend(collect_leaf_ids(second));
            ids
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Opening a second file while a file pane is focused adds it to that
    /// pane's chips: the space keeps its layout instead of tiling one pane per
    /// file.
    #[test]
    fn a_second_file_joins_the_file_pane_instead_of_splitting() {
        let mut state = UiState::mock();
        let space = state.active_space;
        let first = "/tmp/alpha.txt".to_string();
        open_file_pane(&mut state, first.clone(), None);
        let pane = state.active_pane_id;
        let leaves = state.spaces[space].root.leaf_count();

        let second = "/tmp/beta.txt".to_string();
        open_file_pane(&mut state, second.clone(), None);

        assert_eq!(
            state.active_pane_id, pane,
            "the file pane keeps the focus it had"
        );
        assert_eq!(
            state.spaces[space].root.leaf_count(),
            leaves,
            "a second file adds no pane"
        );
        assert_eq!(
            state.pane_open_files.get(&pane).cloned().unwrap_or_default(),
            vec![first, second],
            "the pane holds both files, the shown one last"
        );
    }
}
