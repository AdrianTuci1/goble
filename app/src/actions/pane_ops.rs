use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;

use crate::media::MediaState;
use crate::state::{default_pane_path, UiState};
use crate::ui::{Pane, PaneKind, SplitDir};

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
        state.active_pane_id = new_id;
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
pub(super) fn open_pane_harness(
    state: &mut UiState,
    pane_id: u64,
    desktop: Option<&Arc<DesktopState>>,
) {
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
