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
        // A conversation the store no longer holds — deleted in another process,
        // or cleaned out of the database — is not restored: the pane goes back
        // to following the sidebar selection instead of owning a thread that is
        // gone (a turn sent to it would write rows under a chat that is not
        // there, leaving them unreachable from the sidebar).
        let live: HashSet<String> = desktop
            .list_chats()
            .into_iter()
            .map(|chat| chat.id)
            .collect();
        for session in self.pane_sessions.values_mut() {
            if !session.conversation_id.is_empty() && !live.contains(&session.conversation_id) {
                session.conversation_id.clear();
            }
        }
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
    ///
    /// This is the user's own name: the space is marked named, so the label
    /// derived from what the tab holds never takes it back, however the pane's
    /// directory or conversation changes later.
    pub fn rename_active_space(&mut self, name: String, desktop: Option<&DesktopState>) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if let Some(space) = self.spaces.get_mut(self.active_space) {
            space.name = name.to_string();
            space.named = true;
        }
        if let Some(desktop) = desktop {
            self.save_panes(desktop);
        }
    }

    /// The label the tab at `index` draws: the name the user typed for it, or —
    /// for a tab nobody has named — what the tab holds, read off its **focused**
    /// pane.
    ///
    /// * A terminal pane names its working directory the way the `~` symbol
    ///   reads it (`~`, `~/Projects/goble`), the same shortening the composer's
    ///   working-directory pill uses. A tab opened on another path therefore
    ///   reads that path.
    /// * An agent pane names its conversation's subject (the chat's title), or
    ///   [`NEW_AGENT_TAB_LABEL`] while that conversation has no subject yet.
    /// * A file view names the file's path.
    ///
    /// A tab holding several panes — terminal and agent mixed — reads as its
    /// focused pane, so the tab names whatever the user is working in; a tab
    /// that is not on screen (whose own focus is not tracked) reads as its
    /// first pane.
    pub fn space_label(&self, index: usize) -> String {
        let Some(space) = self.spaces.get(index) else {
            return String::new();
        };
        if space.named {
            return space.name.clone();
        }
        let pane_id = self.space_focused_pane(index);
        match space.leaf_kind(pane_id) {
            Some(PaneKind::Chat) => self.pane_subject(pane_id),
            Some(PaneKind::File { path }) => display_path(path),
            // A terminal pane — and any leaf kind that later appears — names
            // where it runs.
            _ => display_path(&self.pane_working_path(pane_id)),
        }
    }

    /// The title the ghost card of a lifted pane draws (see
    /// [`PaneDrag`](crate::state::PaneDrag)): the same naming rule a tab's
    /// label uses, but read off the pane itself — a split's tab names its
    /// *focused* pane, and the pane being dragged is often not that one.
    pub fn pane_title(&self, pane_id: u64) -> String {
        let kind = self
            .spaces
            .iter()
            .find_map(|space| space.leaf_kind(pane_id).cloned());
        match kind {
            Some(PaneKind::Chat) => self.pane_subject(pane_id),
            Some(PaneKind::File { path }) => display_path(&path),
            _ => display_path(&self.pane_working_path(pane_id)),
        }
    }

    /// Keep every unnamed tab's label honest: it is re-derived from what the tab
    /// holds now, so a terminal tab that moves to another directory renames
    /// itself and an agent tab takes its conversation's subject the moment it
    /// has one. A tab the user renamed is left exactly as they typed it.
    pub fn refresh_space_labels(&mut self) {
        for index in 0..self.spaces.len() {
            if self.spaces[index].named {
                continue;
            }
            let label = self.space_label(index);
            self.spaces[index].name = label;
        }
    }

    /// The pane a tab's label follows: the focused leaf of the active tab, and
    /// the first leaf of a tab that is not on screen (a background tab's own
    /// focus is not tracked).
    fn space_focused_pane(&self, index: usize) -> u64 {
        let space = &self.spaces[index];
        if index == self.active_space && space.root.contains_leaf(self.active_pane_id) {
            self.active_pane_id
        } else {
            space.root.first_leaf_id()
        }
    }

    /// The directory a pane runs in, falling back to the window's own when the
    /// pane never got a session (a layout restored or built by hand).
    fn pane_working_path(&self, pane_id: u64) -> String {
        self.pane_sessions
            .get(&pane_id)
            .map(|session| session.path.clone())
            .filter(|path| !path.trim().is_empty())
            .unwrap_or_else(|| self.composer_path.clone())
    }

    /// The subject a pane's conversation carries: the title the conversation was
    /// created with, or [`NEW_AGENT_TAB_LABEL`] while it has none — a
    /// conversation nobody named is titled [`NEW_CONVERSATION_TITLE`], which is a
    /// placeholder rather than a subject.
    fn pane_subject(&self, pane_id: u64) -> String {
        let subject = self
            .pane_conversation_id(pane_id)
            .and_then(|id| self.conversations.iter().find(|c| c.id == id))
            .map(|c| c.name.trim().to_string())
            .unwrap_or_default();
        if subject.is_empty() || subject == NEW_CONVERSATION_TITLE {
            NEW_AGENT_TAB_LABEL.to_string()
        } else {
            subject
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
