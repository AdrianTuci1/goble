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
        for space in &mut self.spaces {
            space.recompute_layout();
        }
        self.active_space = saved.active_space.min(self.spaces.len() - 1);
        // Restore the per-pane session data (conversation id, draft, cwd) so a
        // pane stays the same independent conversation across a restart.
        self.pane_sessions = saved.pane_sessions;
        // A pane opened on a saved SSH host comes back bound to it: the chip
        // and the shell-tool route read the same registry entry a freshly
        // opened pane fills.
        self.pane_ssh = saved.pane_ssh;
        for (pane_id, binding) in &self.pane_ssh {
            if let Some(session) = &binding.session {
                self.terminal
                    .borrow_mut()
                    .set_ssh_session(*pane_id, Some(session.clone()));
            }
        }
        // Seed the terminal registry with the daemon sessions the panes were
        // attached to. `ensure_session` reuses an id only while the daemon still
        // reports that session alive.
        {
            let mut terminal = self.terminal.borrow_mut();
            for (pane_id, session) in &self.pane_sessions {
                terminal.adopt_session_id(*pane_id, session.daemon_session_id.clone());
            }
        }
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

    /// Copy the daemon session ids the terminal registry holds into the pane
    /// state, so the next layout save remembers which shell each pane is on.
    /// The session is created while a pane paints, which is after the action
    /// that saved the layout; this runs before the frame's snapshot and
    /// reports whether a pane's id changed.
    pub fn adopt_daemon_session_ids(&mut self) -> bool {
        let live: Vec<(u64, String)> = {
            let terminal = self.terminal.borrow();
            terminal
                .session_ids
                .iter()
                .map(|(pane_id, session_id)| (*pane_id, session_id.clone()))
                .collect()
        };
        let mut changed = false;
        for (pane_id, session_id) in live {
            if let Some(session) = self.pane_sessions.get_mut(&pane_id) {
                if session.daemon_session_id != session_id {
                    session.daemon_session_id = session_id;
                    changed = true;
                }
            }
        }
        changed
    }

    /// Persist the pane layout (spaces + active space/pane) so it survives an
    /// app restart. Called whenever a pane action mutates the layout.
    pub fn save_panes(&self, desktop: &DesktopState) {
        // A session's daemon id is assigned when the pane first paints, after
        // the layout's own save points. Overlay the registry's live ids so the
        // persisted state can reattach next run instead of starting a new shell.
        let mut pane_sessions = self.pane_sessions.clone();
        {
            let terminal = self.terminal.borrow();
            for (pane_id, session_id) in &terminal.session_ids {
                if let Some(session) = pane_sessions.get_mut(pane_id) {
                    session.daemon_session_id = session_id.clone();
                }
            }
        }
        let persisted = PersistedPaneState {
            spaces: self.spaces.clone(),
            active_space: self.active_space,
            active_pane_id: self.active_pane_id,
            pane_sessions,
            pane_ssh: self.pane_ssh.clone(),
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

    /// Open the inline rename field on the tab at `index`, seeded with the label
    /// that tab draws right now. One entry point for both paths that start a
    /// rename — a double click on the tab and the tab menu's "Rename tab" — so
    /// the two can never open the field in different states.
    ///
    /// The tab becomes the active one first: the field is drawn in the *active*
    /// tab's slot (see `build_workspace_strip`), so renaming a tab the menu was
    /// opened on while another one is on screen would otherwise open a field the
    /// user cannot see.
    pub fn begin_space_rename(&mut self, index: usize) {
        if index >= self.spaces.len() {
            return;
        }
        if self.active_space != index {
            self.active_space = index;
            self.active_pane_id = self.spaces[index].root.first_leaf_id();
            self.sync_active_view();
        }
        self.space_rename_draft = self.space_label(index);
        self.space_rename_editing = true;
        self.space_rename_focused = true;
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
            // A viewer pane has no local directory to be named after: its
            // conversation runs on a worker, so the tab reads the conversation's
            // subject, exactly as a chat pane's does.
            Some(PaneKind::Worker) => self.pane_subject(pane_id),
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
            Some(PaneKind::Worker) => self.pane_subject(pane_id),
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

    /// The settings tab: the space index and the leaf id of the **first**
    /// settings leaf in traversal order, wherever it sits in the layout.
    ///
    /// The lookup is by leaf kind, never by name or id: closing the tab and
    /// lifting the pane into another space both change an id, and the tab can
    /// be renamed like any other, so only the kind is stable. A layout that
    /// somehow holds two settings leaves answers with the first and is left
    /// exactly as it is — silently rewriting the user's layout on load would be
    /// the worse failure.
    pub fn settings_pane(&self) -> Option<(usize, u64)> {
        self.spaces
            .iter()
            .enumerate()
            .find_map(|(index, space)| space.settings_leaf().map(|id| (index, id)))
    }

    /// The page the settings tab shows. The page rides in the leaf, so the rail
    /// and the pane cannot drift apart and the page comes back with the pane.
    /// A layout with no settings leaf has no page: `Appearance`.
    pub fn settings_page(&self) -> SettingsCategory {
        match self
            .settings_pane()
            .and_then(|(space, id)| self.spaces[space].leaf_kind(id))
        {
            Some(PaneKind::Settings { page }) => *page,
            _ => SettingsCategory::Appearance,
        }
    }

    /// Point the settings tab's leaf at another page, so the page travels with
    /// the pane.
    pub fn settings_set_page(&mut self, page: SettingsCategory) {
        let Some((space, id)) = self.settings_pane() else {
            return;
        };
        self.spaces[space].set_leaf_kind(id, PaneKind::Settings { page });
    }

    /// Open the settings tab, or bring the one that already exists to the
    /// front. At most one is ever created: the lookup is by kind, so pressing
    /// the button again lands on the same tab however it is renamed.
    pub fn open_settings_tab(&mut self, desktop: Option<&DesktopState>) {
        // The strip is drawn in `AppTab::Chat` only, so a press from another
        // mode comes back to it — otherwise the tab it opened would sit off
        // screen and nothing would appear to have happened.
        self.current_tab = AppTab::Chat;
        if let Some((space, id)) = self.settings_pane() {
            if self.active_space == space && self.active_pane_id == id {
                // Already the pane the keyboard is in: nothing at all, not
                // even a save.
                return;
            }
            self.active_space = space;
            self.active_pane_id = id;
            self.focus_settings_pane();
            self.sync_active_view();
            if let Some(desktop) = desktop {
                self.save_panes(desktop);
            }
            return;
        }
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        self.spaces.push(Space::new(
            "Settings",
            Pane::Leaf {
                id,
                kind: PaneKind::Settings {
                    page: SettingsCategory::Appearance,
                },
            },
        ));
        self.active_space = self.spaces.len() - 1;
        self.active_pane_id = id;
        self.focus_settings_pane();
        // The pane has no session of its own (nothing in it runs a command), so
        // the per-pane flags its body reads are seeded the way a restored
        // tree's are.
        self.pane_hover
            .entry(id)
            .or_insert_with(|| Rc::new(RefCell::new(false)));
        self.ensure_pane_controls();
        self.refresh_space_labels();
        self.sync_active_view();
        if let Some(desktop) = desktop {
            self.save_panes(desktop);
        }
    }

    /// Put the keyboard at the settings tab's rail, on its first row, with no
    /// field holding the caret — the state it opens in.
    fn focus_settings_pane(&mut self) {
        self.settings_focus = SettingsFocus::Rail;
        self.settings_pane_focus = 0;
        self.settings_pane_field_active = false;
        self.settings_field_edit_start = None;
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

    /// Move the active space `delta` tabs along the strip (Ctrl+Tab /
    /// Ctrl+Shift+Tab), wrapping at both ends: the last tab's next tab is the
    /// first, the first tab's previous is the last. One tab has nowhere to go,
    /// so nothing is touched — the layout is not rewritten for a switch that
    /// did not happen.
    ///
    /// The keyboard lands on the target tab's first leaf, the pane
    /// `on_select_space` focuses — or on the settings rail when that leaf is the
    /// settings pane, whose body would otherwise keep the keys the switch just
    /// took. Returns whether the active space moved.
    pub fn switch_space(&mut self, delta: i32) -> bool {
        let len = self.spaces.len();
        if len <= 1 {
            return false;
        }
        let target = (self.active_space as i32 + delta).rem_euclid(len as i32) as usize;
        if target == self.active_space {
            return false;
        }
        self.active_space = target;
        self.spaces[target].recompute_layout();
        self.active_pane_id = self.spaces[target].root.first_leaf_id();
        if matches!(
            self.spaces[target].leaf_kind(self.active_pane_id),
            Some(PaneKind::Settings { .. })
        ) {
            self.focus_settings_pane();
        }
        true
    }

    /// Select a layout for the space at `index` and persist it. Returns whether
    /// a space was found; a no-op when it already has that layout still saves,
    /// because the caller's action is the explicit choice.
    pub fn set_space_layout(
        &mut self,
        index: usize,
        layout: LayoutKind,
        desktop: Option<&DesktopState>,
    ) -> bool {
        let Some(space) = self.spaces.get_mut(index) else {
            return false;
        };
        space.set_layout(layout);
        if self.active_space == index {
            self.floating_drag.borrow_mut().take();
        }
        if let Some(desktop) = desktop {
            self.save_panes(desktop);
        }
        true
    }

    /// Cycle the active space through BSP → master-stack → floating, the
    /// gesture Cmd+Shift+L and the palette entry run.
    pub fn cycle_space_layout(&mut self, desktop: Option<&DesktopState>) -> LayoutKind {
        let index = self.active_space;
        let Some(space) = self.spaces.get(index) else {
            return LayoutKind::Bsp;
        };
        let next = space.layout.cycle();
        self.set_space_layout(index, next, desktop);
        next
    }

    /// Read the saved hosts from `~/.goble/hosts.toml`. The picker calls this
    /// when it opens, so an edit to the file is seen without a restart; a
    /// missing file (or no home) yields an empty list rather than an error the
    /// picker would have nothing to say about.
    pub fn reload_saved_hosts(&mut self) {
        self.ssh_host_entries = goble_core::app_home::GobleHome::locate()
            .ok()
            .and_then(|home| goble_core::hosts::HostsFile::load(&home.hosts_path()).ok())
            .map(|file| file.valid_hosts())
            .unwrap_or_default();
    }

    /// Bind `pane_id` to `session`, remembering `host_name` when the pane was
    /// opened from a saved host. The registry is the one place the chip and the
    /// shell route read, so the binding is written there too.
    pub fn bind_ssh_pane(
        &mut self,
        pane_id: u64,
        host_name: Option<String>,
        session: goble_core::ssh_command::SshSession,
    ) {
        self.terminal
            .borrow_mut()
            .set_ssh_session(pane_id, Some(session.clone()));
        // A live session answers the offer: a pane that reconnected by typing
        // its own `ssh` line is not asked about the host it left.
        if self
            .ssh_reconnect_prompt
            .as_ref()
            .is_some_and(|(pane, _)| *pane == pane_id)
        {
            self.ssh_reconnect_prompt = None;
        }
        self.pane_ssh.insert(
            pane_id,
            SshPaneBinding {
                host_name,
                session: Some(session),
                reconnect: None,
            },
        );
    }

    /// Note that the pane's SSH session ended. A pane opened from a saved host
    /// keeps the host's name as the reconnect offer; a typed, anonymous session
    /// leaves nothing behind.
    pub fn note_ssh_ended(&mut self, pane_id: u64) -> Option<String> {
        self.terminal.borrow_mut().set_ssh_session(pane_id, None);
        let binding = self.pane_ssh.get_mut(&pane_id)?;
        binding.session = None;
        let reconnect = binding.host_name.clone();
        binding.reconnect = reconnect.clone();
        if reconnect.is_none() {
            self.pane_ssh.remove(&pane_id);
        }
        reconnect
    }

    /// Read a line the user submitted at a shell's own prompt for where it
    /// leaves that shell: an interactive `ssh` line binds the pane to the host
    /// it names, and `exit` returns it to the local shell — and offers a saved
    /// host again. A line typed in the grid and one typed at the rich input bar
    /// both land here, so the chip and the reconnect offer do not depend on
    /// which surface the key went through.
    pub fn note_shell_line(&mut self, pane_id: u64, line: &str) {
        if self.pane_is_viewer(pane_id) {
            return;
        }
        if let Some(target) = goble_core::ssh_command::parse_ssh_command(line) {
            let hosts = self.ssh_hosts_for_command();
            let session =
                target.resolve(hosts.as_ref(), &goble_core::ssh_hosts::current_user());
            self.bind_ssh_pane(pane_id, None, session);
        } else if goble_core::ssh_command::returns_to_local_shell(line) {
            if let Some(host) = self.note_ssh_ended(pane_id) {
                self.ssh_reconnect_prompt = Some((pane_id, host));
            }
        }
    }

    /// The saved hosts file as the picker and the Connections page read it.
    pub fn saved_hosts(&self) -> &[HostEntry] {
        &self.ssh_host_entries
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
