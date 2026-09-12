use super::*;

impl UiState {
    /// Restore the persisted pane layout (spaces + active space/pane) that a
    /// previous run saved, if any. Leaves the in-memory defaults untouched when
    /// nothing was persisted or the blob cannot be parsed, so a fresh install
    /// starts clean. The id counter is recomputed from the restored tree so new
    /// panes never collide with restored ones.
    pub fn restore_panes(&mut self, desktop: &DesktopState) {
        let Some(json) = desktop.get_ui_panes() else {
            return;
        };
        let Ok(saved) = serde_json::from_str::<PersistedPaneState>(&json) else {
            log::warn!("failed to parse persisted pane state");
            return;
        };
        if saved.spaces.is_empty() {
            return;
        }
        self.spaces = saved.spaces;
        self.active_space = saved.active_space.min(self.spaces.len() - 1);
        // Restore the per-pane session data (conversation id, draft, cwd) so a
        // pane stays the same independent conversation across a restart.
        self.pane_sessions = saved.pane_sessions;
        let max_id = self
            .spaces
            .iter()
            .map(|space| space.root.max_id())
            .max()
            .unwrap_or(0);
        self.next_pane_id = max_id + 1;
        let root = &self.spaces[self.active_space].root;
        if root.contains_leaf(saved.active_pane_id) {
            self.active_pane_id = saved.active_pane_id;
        } else {
            self.active_pane_id = root.first_leaf_id();
        }
        // Rebuild the per-pane hover flags so pane-header hover survives the
        // per-frame element rebuild for the restored tree.
        let mut ids = Vec::new();
        for space in &self.spaces {
            collect_pane_ids(&space.root, &mut ids);
        }
        for id in ids {
            self.pane_hover
                .entry(id)
                .or_insert_with(|| Rc::new(RefCell::new(false)));
        }
        // Seed the per-pane rich-input controls for the restored tree as well.
        self.ensure_pane_controls();
    }

    /// Persist the pane layout (spaces + active space/pane) so it survives an
    /// app restart. Called whenever a pane action mutates the layout.
    pub fn save_panes(&self, desktop: &DesktopState) {
        let persisted = PersistedPaneState {
            spaces: self.spaces.clone(),
            active_space: self.active_space,
            active_pane_id: self.active_pane_id,
            pane_sessions: self.pane_sessions.clone(),
        };
        if let Ok(json) = serde_json::to_string(&persisted) {
            if let Err(e) = desktop.set_ui_panes(&json) {
                log::warn!("failed to persist pane state: {e}");
            }
        }
    }

    /// Rename the active space, persisting the pane layout. A blank name is
    /// ignored so the frame can never be left unnamed.
    pub fn rename_active_space(&mut self, name: String, desktop: Option<&DesktopState>) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if let Some(space) = self.spaces.get_mut(self.active_space) {
            space.name = name.to_string();
        }
        if let Some(desktop) = desktop {
            self.save_panes(desktop);
        }
    }

    /// Move the space at `from` to index `to`, keeping the active space the
    /// same pane tree wherever it lands. The active pane id is unchanged (it
    /// points into the active space, whose identity does not change).
    pub fn reorder_space(&mut self, from: usize, to: usize) {
        let len = self.spaces.len();
        if from == to || from >= len || to >= len {
            return;
        }
        let space = self.spaces.remove(from);
        self.spaces.insert(to, space);
        if self.active_space == from {
            self.active_space = to;
        } else if self.active_space < from && self.active_space >= to {
            self.active_space += 1;
        } else if self.active_space > from && self.active_space <= to {
            self.active_space -= 1;
        }
    }
}
