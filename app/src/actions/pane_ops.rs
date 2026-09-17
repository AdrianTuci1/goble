use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

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

/// Open `file_path` in a file-view pane of the active space, and focus it.
///
/// The view opens beside the pane that asked for it, in the tab on screen. A
/// pane that is already showing a file takes the new one over instead of
/// splitting again, so browsing a directory with the file view focused walks
/// through files in one pane rather than tiling the tab with them. The pane
/// keeps no conversation and no working directory of its own: the explorer's
/// tree follows the last pane that had one.
pub(super) fn open_file_pane(
    state: &mut UiState,
    file_path: String,
    desktop: Option<&Arc<DesktopState>>,
) {
    let kind = PaneKind::File { path: file_path };
    if state.active_pane_shows_file() {
        let (active_space, pane_id) = (state.active_space, state.active_pane_id);
        if let Some(space) = state.spaces.get_mut(active_space) {
            space.set_leaf_kind(pane_id, kind);
        }
    } else {
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
        state.focus_pane(new_id);
        ensure_pane_hover(state, new_id);
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

/// Let the panes in `ids` go of the desktop each was showing, closing every
/// desktop whose last viewer is one of them.
///
/// The rule is [`UiState::release_inline_screen`]'s and stays there: a desktop
/// closes only when no pane anywhere still names it, so a source another pane or
/// another tab is watching survives. This is the one place the app acts on that
/// answer, so a dismissed card, a closed pane and a closed tab all release a
/// desktop under the same terms.
pub(super) fn release_inline_screens(
    state: &mut UiState,
    ids: impl IntoIterator<Item = u64>,
    desktop: Option<&DesktopState>,
) {
    for id in ids {
        if let Some(source) = state.release_inline_screen(id) {
            if let Some(desktop) = desktop {
                desktop.close_remote_screen(&source);
            }
        }
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
    // terminal sessions along with them. The panes' desktops are released first,
    // before their runtimes go: a tab closing is a viewer leaving like any other,
    // so a desktop this tab was the last one showing closes with it while one
    // another tab still shows stays open.
    let closed = state.spaces.remove(index);
    let leaves = collect_leaf_ids(&closed.root);
    release_inline_screens(state, leaves.iter().copied(), desktop);
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
