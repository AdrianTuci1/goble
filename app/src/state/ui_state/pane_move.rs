//! Lifting a pane out of its grid and dropping it into the tab strip, where it
//! becomes a tab of its own (warp-new's "drag a pane onto the tab bar").
//!
//! The pane that moves is the same `Pane::Leaf` — its id and its kind — so
//! everything the app keys by that id follows it: a terminal keeps its PTY
//! session, an agent pane its conversation, a file pane its path and its scroll
//! offset. Nothing is re-created and no pane falls back to a default.
//!
//! The drag lives in app state ([`PaneDrag`]) because the element tree is
//! rebuilt every frame, and the reshaping it performs on release is a plain
//! function over the space list ([`move_pane_to_new_space`]) so the rules can
//! be tested without a pointer.

use super::*;

use goble_ui::geometry::Vector2F;

/// How far the pointer must travel after the press before a lifted pane reads
/// as a drag: a plain click on a pane's header must not flash a ghost card.
const PANE_LIFT_THRESHOLD: f32 = 4.0;

/// A pane lifted out of its grid and carried by the pointer, toward the tab
/// strip. Owned by app state so it survives the per-frame rebuild the press
/// itself triggers.
#[derive(Clone, Debug)]
pub struct PaneDrag {
    /// The leaf being dragged.
    pub pane_id: u64,
    /// The space it is being taken out of.
    pub source_space: usize,
    /// The title the ghost card draws: the pane's own label, read when the
    /// lift began.
    pub title: String,
    /// Where the button went down, in window coordinates.
    pub origin: Vector2F,
    /// Where the pointer is now.
    pub position: Vector2F,
    /// The tab index the drop would land at — an insertion point between the
    /// drawn tabs — or `None` while the pointer is off the strip, where a
    /// release cancels the drag and changes nothing.
    pub drop_index: Option<usize>,
}

impl PaneDrag {
    /// Whether the pointer has travelled far enough for the lift to read as a
    /// drag (and so to draw the ghost).
    pub fn moved(&self) -> bool {
        (self.position.x - self.origin.x).abs() > PANE_LIFT_THRESHOLD
            || (self.position.y - self.origin.y).abs() > PANE_LIFT_THRESHOLD
    }
}

/// What dropping a lifted pane did to the space list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaneMove {
    /// The tab the pane landed on.
    pub tab: usize,
    /// The fresh chat pane the last remaining space was reset to, when the
    /// source space was reset in place instead of being consumed. The caller
    /// gives it a conversation of its own, as closing a space would.
    pub reset_pane: Option<u64>,
}

/// Move the leaf `pane_id` out of the space at `source_space` and back in as a
/// tab of its own at `index` — an insertion point between the tabs as they are
/// drawn now, so `0` is before the first tab and `spaces.len()` after the last.
///
/// What is left behind follows the rules closing a pane already uses: a split
/// collapses onto its surviving pane, a space whose only pane this was is
/// consumed by the move (it does not stay behind empty), and the last remaining
/// space is reset in place to a fresh empty chat pane (`next_id` allocates it)
/// exactly as closing it would, so the window is never left without a tab.
/// Returns `None` when there is no such pane.
pub fn move_pane_to_new_space(
    spaces: &mut Vec<Space>,
    source_space: usize,
    pane_id: u64,
    index: usize,
    next_id: &mut u64,
) -> Option<PaneMove> {
    let kind = spaces.get(source_space)?.leaf_kind(pane_id)?.clone();
    let medium = spaces[source_space].medium.clone();
    // `close` collapses the split the pane came out of and reports `None` when
    // the pane *was* the space: nothing survived it.
    let collapsed = spaces[source_space].close(pane_id).is_some();
    let pane = Pane::Leaf { id: pane_id, kind };
    let tab;
    let reset_pane;
    if collapsed {
        tab = index.min(spaces.len());
        spaces.insert(tab, Space::unnamed(pane).with_medium(medium));
        reset_pane = None;
    } else if spaces.len() > 1 {
        spaces.remove(source_space);
        // The consumed tab is gone from the list, so an insertion point past it
        // names the tab one place earlier now.
        let at = if index > source_space { index - 1 } else { index };
        tab = at.min(spaces.len());
        spaces.insert(tab, Space::unnamed(pane).with_medium(medium));
        reset_pane = None;
    } else {
        let fresh = *next_id;
        *next_id += 1;
        spaces[source_space] = Space::unnamed(Pane::Leaf { id: fresh, kind: PaneKind::Chat });
        tab = if index > source_space {
            source_space + 1
        } else {
            source_space
        };
        spaces.insert(tab, Space::unnamed(pane).with_medium(medium));
        reset_pane = Some(fresh);
    }
    Some(PaneMove { tab, reset_pane })
}

impl UiState {
    /// Lift the pane `pane_id` into a drag: its header was pressed at
    /// `position`, and until the button comes up the pointer carries it toward
    /// the tab strip. A press that names no leaf of the active space — the only
    /// tree that is mounted — lifts nothing, and neither does one that lands
    /// while a split divider is already holding the pointer.
    pub fn begin_pane_lift(&mut self, pane_id: u64, position: Vector2F) {
        if self.dragging_pane_id.is_some() {
            return;
        }
        let Some(space) = self.spaces.get(self.active_space) else {
            return;
        };
        if !space.root.contains_leaf(pane_id) {
            return;
        }
        let title = self.pane_title(pane_id);
        self.pane_drag = Some(PaneDrag {
            pane_id,
            source_space: self.active_space,
            title,
            origin: position,
            position,
            drop_index: None,
        });
    }

    /// Track the pointer while a pane is lifted, with the tab index the drop
    /// would land at (`None` while the pointer is off the tab strip).
    pub fn drag_pane_to(&mut self, position: Vector2F, drop_index: Option<usize>) {
        if let Some(drag) = self.pane_drag.as_mut() {
            drag.position = position;
            drag.drop_index = drop_index;
        }
    }

    /// End a pane lift. `Some(index)` gives the pane a tab of its own at that
    /// insertion point, `None` — a release anywhere but the strip — cancels
    /// with no change to the workspace.
    ///
    /// The moved pane is the tab that is on screen and holds the focus
    /// afterwards, whether or not the source space survived the move.
    pub fn drop_pane(&mut self, drop_index: Option<usize>) -> Option<PaneMove> {
        let drag = self.pane_drag.take()?;
        let index = drop_index?;
        let moved = move_pane_to_new_space(
            &mut self.spaces,
            drag.source_space,
            drag.pane_id,
            index,
            &mut self.next_pane_id,
        )?;
        self.active_space = moved.tab;
        self.active_pane_id = drag.pane_id;
        Some(moved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ui::SplitDir;

    /// A chat leaf with `id`.
    fn chat(id: u64) -> Pane {
        Pane::Leaf {
            id,
            kind: PaneKind::Chat,
        }
    }

    /// Two named spaces: `A` holding a horizontal split of panes 1 and 5, and
    /// `B` holding pane 2. A split spends the counter on its node id first and
    /// its new leaf id second, so the counter starts at 4 and comes back at 6.
    fn split_source() -> (Vec<Space>, u64) {
        let mut space = Space::new("A", chat(1));
        let mut next_id = 4;
        let second = space
            .split(1, SplitDir::Horizontal, &mut next_id)
            .expect("the leaf splits");
        assert_eq!(second, 5, "the split allocated a new leaf");
        (vec![space, Space::new("B", chat(2))], next_id)
    }

    #[test]
    fn a_pane_leaves_a_split_and_the_survivor_takes_the_space() {
        let (mut spaces, mut next_id) = split_source();
        let moved = move_pane_to_new_space(&mut spaces, 0, 5, 1, &mut next_id)
            .expect("pane 5 is in space A");

        assert_eq!(spaces.len(), 3, "the pane became a tab of its own");
        assert_eq!(moved.tab, 1, "it landed after its source tab");
        // The source space kept its place, collapsed onto the pane that stayed.
        assert_eq!(
            spaces[0].root.leaf_kind(1),
            Some(&PaneKind::Chat),
            "the surviving pane is the space's root now"
        );
        assert_eq!(
            spaces[0].root.max_id(),
            1,
            "the split collapsed onto the surviving pane"
        );
        assert!(
            !spaces[0].root.contains_leaf(5),
            "the moved pane is not in its old space"
        );
        assert_eq!(
            spaces[1].root.leaf_kind(5),
            Some(&PaneKind::Chat),
            "the moved pane is the new tab's root"
        );
        assert_eq!(
            spaces[2].root.leaf_kind(2),
            Some(&PaneKind::Chat),
            "the tab that followed the source kept its pane"
        );
        assert_eq!(moved.reset_pane, None, "no space had to be reset");
    }

    #[test]
    fn a_single_pane_space_is_consumed_by_the_move() {
        let mut spaces = vec![Space::new("A", chat(1)), Space::new("B", chat(2))];
        let mut next_id = 3;
        let moved = move_pane_to_new_space(&mut spaces, 0, 1, 1, &mut next_id)
            .expect("pane 1 is in space A");

        assert_eq!(spaces.len(), 2, "the emptied space is gone, the pane is a tab");
        // The insertion point is counted on the strip as it was drawn — the
        // source tab included — so point 1 is the slot the consumed tab left.
        assert_eq!(moved.tab, 0, "the moved tab takes the place of the consumed one");
        assert!(
            !spaces.iter().any(|s| s.name == "A"),
            "the consumed tab is not left behind empty"
        );
        assert_eq!(
            spaces[0].root.first_leaf_id(),
            1,
            "the pane is a tab of its own where its old tab was"
        );
        assert_eq!(
            spaces[1].root.leaf_kind(2),
            Some(&PaneKind::Chat),
            "the tab that followed kept its pane"
        );
        assert_eq!(moved.reset_pane, None);
    }

    #[test]
    fn a_pane_dropped_before_its_own_tab_lands_before_it() {
        let mut spaces = vec![Space::new("A", chat(1)), Space::new("B", chat(2))];
        let mut next_id = 3;
        let moved = move_pane_to_new_space(&mut spaces, 0, 1, 0, &mut next_id)
            .expect("pane 1 is in space A");

        assert_eq!(moved.tab, 0, "insertion point 0 is before every tab");
        assert_eq!(spaces[0].root.first_leaf_id(), 1, "the moved pane leads");
        assert_eq!(spaces[1].root.first_leaf_id(), 2, "B keeps its pane");
        assert_eq!(spaces.len(), 2);
    }

    #[test]
    fn the_last_space_resets_to_a_fresh_chat_pane() {
        let mut spaces = vec![Space::new("A", chat(1))];
        let mut next_id = 2;
        let moved = move_pane_to_new_space(&mut spaces, 0, 1, 1, &mut next_id)
            .expect("pane 1 is in space A");

        assert_eq!(spaces.len(), 2, "the window keeps a tab and gains the pane's");
        assert_eq!(moved.tab, 1, "the pane's tab lands past the reset one");
        let fresh = moved.reset_pane.expect("the last space was reset");
        assert_eq!(fresh, 2, "the reset pane is a fresh id, never the moved one");
        assert_eq!(
            spaces[0].root.leaf_kind(fresh),
            Some(&PaneKind::Chat),
            "the source space is a fresh empty chat pane"
        );
        assert_eq!(
            spaces[0].root.leaf_kind(1),
            None,
            "and no longer holds the pane that left"
        );
        assert_eq!(spaces[1].root.leaf_kind(1), Some(&PaneKind::Chat));
    }

    #[test]
    fn the_last_space_resets_in_place_when_the_drop_names_its_own_slot() {
        let mut spaces = vec![Space::new("A", chat(1))];
        let mut next_id = 2;
        let moved = move_pane_to_new_space(&mut spaces, 0, 1, 0, &mut next_id)
            .expect("pane 1 is in space A");

        assert_eq!(moved.tab, 0, "insertion point 0 is before the source tab");
        assert_eq!(spaces[0].root.first_leaf_id(), 1, "the moved pane leads");
        assert_eq!(spaces[1].root.first_leaf_id(), 2, "the reset space follows it");
        assert_eq!(moved.reset_pane, Some(2));
    }

    #[test]
    fn an_unknown_pane_moves_nothing() {
        let (mut spaces, mut next_id) = split_source();
        assert_eq!(move_pane_to_new_space(&mut spaces, 0, 99, 1, &mut next_id), None);
        assert_eq!(move_pane_to_new_space(&mut spaces, 9, 1, 1, &mut next_id), None);
        assert_eq!(spaces.len(), 2, "neither call touched the tab list");
        assert!(spaces[0].root.contains_leaf(1) && spaces[0].root.contains_leaf(5));
    }

    /// The drag state: a lift names the pressed pane and the space it is in,
    /// and a release off the strip clears it without touching the workspace.
    #[test]
    fn a_release_off_the_strip_cancels_the_drag() {
        let (spaces, next_id) = split_source();
        let mut state = UiState::mock();
        state.spaces = spaces.clone();
        state.active_space = 0;
        state.active_pane_id = 1;
        state.next_pane_id = next_id;

        state.begin_pane_lift(5, Vector2F::new(200.0, 100.0));
        let drag = state.pane_drag.as_ref().expect("the pane is lifted");
        assert_eq!(drag.pane_id, 5);
        assert_eq!(drag.source_space, 0);
        assert!(!drag.moved(), "a press that has not travelled is not a drag yet");

        state.drag_pane_to(Vector2F::new(300.0, 10.0), None);
        assert!(
            state.pane_drag.as_ref().unwrap().moved(),
            "the pointer travelled, so the lift is a drag"
        );
        assert_eq!(state.drop_pane(None), None, "a release off the strip cancels");
        assert!(state.pane_drag.is_none(), "the drag state is cleared");
        assert_eq!(state.spaces.len(), 2, "no tab was added");
        assert_eq!(
            state.spaces[0].root.max_id(),
            5,
            "the split is untouched"
        );
        assert_eq!(state.active_space, 0, "and nothing was refocused");
    }

    #[test]
    fn dropping_the_pane_on_the_strip_focuses_it_in_its_new_tab() {
        let (spaces, next_id) = split_source();
        let mut state = UiState::mock();
        state.spaces = spaces.clone();
        state.active_space = 0;
        state.active_pane_id = 1;
        state.next_pane_id = next_id;
        // The moved pane's own session: it must still be the pane's afterwards.
        state.pane_sessions.insert(
            5,
            PaneSession {
                conversation_id: "conv-moved".to_string(),
                draft: "draft".to_string(),
                path: "/tmp/moved".to_string(),
            },
        );

        state.begin_pane_lift(5, Vector2F::new(200.0, 100.0));
        state.drag_pane_to(Vector2F::new(300.0, 10.0), Some(1));
        let moved = state.drop_pane(Some(1)).expect("the pane moved");

        assert_eq!(moved.tab, 1);
        assert_eq!(state.active_space, 1, "the moved pane's tab is on screen");
        assert_eq!(state.active_pane_id, 5, "and the moved pane holds the focus");
        assert_eq!(
            state.spaces[1].root.leaf_kind(5),
            Some(&PaneKind::Chat),
            "the tab holds the pane itself, by id"
        );
        let session = state.pane_sessions.get(&5).expect("its session travelled");
        assert_eq!(session.conversation_id, "conv-moved");
        assert_eq!(session.path, "/tmp/moved");
        assert_eq!(
            state.spaces[0].root.max_id(),
            1,
            "the space the pane left collapsed onto its survivor"
        );
        assert_eq!(spaces.len(), 2, "the fixture itself is never mutated");
    }

    #[test]
    fn a_terminal_pane_keeps_its_kind_when_it_becomes_a_tab() {
        // One tab, holding one terminal pane and nothing else: the move has to
        // carry that very pane out — never a fresh chat standing in for it — and
        // reset the tab it left, exactly as closing it would.
        let mut state = UiState::mock();
        state.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 5,
            kind: PaneKind::Terminal,
        })];
        state.active_space = 0;
        state.active_pane_id = 5;
        state.next_pane_id = 6;
        // The terminal's own session binding, keyed by pane id: it has to be
        // the same entry afterwards, which is what keeps its PTY alive.
        state.pane_sessions.insert(
            5,
            PaneSession {
                conversation_id: String::new(),
                draft: String::new(),
                path: "/tmp/pty".to_string(),
            },
        );

        state.begin_pane_lift(5, Vector2F::new(200.0, 100.0));
        state.drag_pane_to(Vector2F::new(300.0, 10.0), Some(1));
        let moved = state.drop_pane(Some(1)).expect("the pane moved");

        assert_eq!(state.spaces.len(), 2, "the window keeps a tab and gains one");
        assert_eq!(
            state.spaces[moved.tab].root.leaf_kind(5),
            Some(&PaneKind::Terminal),
            "the terminal pane is the same terminal pane, not a fresh chat"
        );
        assert_eq!(
            state.spaces[moved.tab].root.first_leaf_id(),
            5,
            "and it is the whole of its new tab"
        );
        assert_eq!(
            state
                .pane_sessions
                .get(&5)
                .map(|session| session.path.as_str()),
            Some("/tmp/pty"),
            "its session is the pane's own, still keyed by the same id"
        );
        assert_eq!(
            moved.reset_pane,
            Some(6),
            "the last space was reset, not emptied"
        );
        assert_eq!(
            state.spaces[moved.tab - 1].root.leaf_kind(6),
            Some(&PaneKind::Chat),
            "the reset tab is a fresh empty chat pane"
        );
    }
}
