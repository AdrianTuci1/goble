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

/// Ensure a hover flag exists for a pane so the pane-header hover survives
/// the per-frame element rebuild.
pub(super) fn ensure_pane_hover(state: &mut UiState, pane_id: u64) {
    state
        .pane_hover
        .entry(pane_id)
        .or_insert_with(|| Rc::new(RefCell::new(false)));
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
