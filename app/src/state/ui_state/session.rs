use super::*;

impl UiState {
    /// The child view `pane_id` shows, if any.
    pub fn sub_agent_view(&self, pane_id: u64) -> Option<&SubAgentChildView> {
        self.sub_agent_views.get(&pane_id)
    }

    /// Move the focus to `pane_id` within the space on screen.
    ///
    /// The pane drawn over the panes space gives way to the one the focus moved
    /// to: warp-new clears its maximized pane whenever the focused pane changes
    /// (`app/src/pane_group/focus_state.rs`), so a pane that gave up the focus
    /// cannot keep the whole space. Moving the focus to the pane that is already
    /// the expanded one leaves the expansion standing.
    pub fn focus_pane(&mut self, pane_id: u64) {
        if self.active_pane_id != pane_id {
            self.maximized_pane = None;
        }
        self.active_pane_id = pane_id;
        // A pane waiting on a decision types into its card (or question), so
        // the rich input that held the keyboard in the pane the user came from
        // must not keep it here.
        if self
            .pane_runtime
            .get(&pane_id)
            .is_some_and(|rt| rt.pending_command.is_some() || rt.pending_ask.is_some())
        {
            self.composer_focused = false;
        }
    }

    /// Point the global "active pane" fields at the active pane's own session +
    /// runtime so anything still reading the singletons sees the right pane.
    pub fn sync_active_view(&mut self) {
        if let Some(session) = self.pane_sessions.get(&self.active_pane_id) {
            if !session.conversation_id.is_empty() {
                self.selected_id = Some(session.conversation_id.clone());
            }
            self.composer_draft = session.draft.clone();
            self.composer_path = session.path.clone();
        }
        let rt = self.pane_runtime.get(&self.active_pane_id);
        self.chat_messages = rt.map(|r| r.messages.clone()).unwrap_or_default();
        self.pending_ask = rt.and_then(|r| r.pending_ask.clone());
        self.queued_prompt = rt.and_then(|r| r.queued_prompt.clone());
        self.agent_busy = rt.map(|r| r.busy).unwrap_or(false);
    }

    /// Ensure every chat leaf pane has a [`PaneSession`] so it is an
    /// independent session. Panes restored from disk keep their persisted
    /// conversation; a fresh/deleted pane is bound to a new conversation (via
    /// the store when a desktop is available) or to the currently selected one
    /// so the first pane tracks the sidebar selection.
    pub fn ensure_pane_sessions(&mut self, desktop: Option<&DesktopState>) {
        ensure_all_chat_pane_sessions(self, desktop);
    }

    /// Bind `pane_id` to a brand-new distinct conversation so the pane never
    /// shares a transcript with another pane. Uses the store when available
    /// (persisting the conversation), else allocates a synthetic id (mock).
    pub fn bind_pane_new_conversation(&mut self, pane_id: u64, desktop: Option<&DesktopState>) {
        self.bind_pane_new_conversation_titled(pane_id, NEW_CONVERSATION_TITLE, desktop);
    }

    /// Bind `pane_id` to a brand-new conversation created under `title`.
    ///
    /// The sidebar's "New conversation" row carries a typed subject when it has
    /// one, so the conversation the tab opens is created under that name rather
    /// than gaining the placeholder and being renamed later.
    pub fn bind_pane_new_conversation_titled(
        &mut self,
        pane_id: u64,
        title: &str,
        desktop: Option<&DesktopState>,
    ) {
        let conversation_id = match desktop {
            Some(d) => match d.create_chat(title, None, None) {
                Ok(id) => id,
                Err(e) => {
                    log::warn!("create_chat for new pane failed: {e}");
                    format!("pane-{pane_id}")
                }
            },
            None => format!("pane-{pane_id}"),
        };
        let session = self.pane_sessions.entry(pane_id).or_default();
        session.conversation_id = conversation_id;
        self.clear_pane_transcript_selection(pane_id);
    }

    /// Register the conversation a CLI harness holds in `pane_id`, so a harness
    /// started from the pane becomes a chat in the sidebar like any other.
    ///
    /// The first launch of a harness in a pane opens a chat named for the
    /// conversation the harness recorded in the pane's directory — the same
    /// content-derived name a native conversation carries — with the harness
    /// recorded as its provider; launching the same harness in the same pane
    /// again returns to that chat instead of stacking a duplicate. The pane is
    /// bound to it, so the sidebar card, the tab's mark and the pane's own bar
    /// all name the same conversation.
    pub fn bind_pane_harness_chat(
        &mut self,
        pane_id: u64,
        harness: &'static goble_core::harness::HarnessSpec,
        desktop: Option<&DesktopState>,
    ) {
        self.bind_pane_harness_chat_deriving(pane_id, harness, desktop, |cwd| {
            crate::terminal::session_title(harness.id, cwd)
        });
    }

    /// [`Self::bind_pane_harness_chat`] with the title lookup passed in, so a
    /// test names the conversation without a session store on disk.
    fn bind_pane_harness_chat_deriving(
        &mut self,
        pane_id: u64,
        harness: &'static goble_core::harness::HarnessSpec,
        desktop: Option<&DesktopState>,
        derive_title: impl FnOnce(&str) -> Option<String>,
    ) {
        let key = (pane_id, harness.id.to_string());
        if let Some(conversation_id) = self.harness_chats.get(&key).cloned() {
            self.record_pane_conversation(pane_id, &conversation_id);
            return;
        }
        let Some(desktop) = desktop else {
            return;
        };
        let path = self
            .pane_sessions
            .get(&pane_id)
            .map(|session| session.path.clone())
            .unwrap_or_default();
        // The conversation is named for what the harness recorded in this
        // directory, the way a native conversation is named for what was said
        // in it. A harness that has recorded nothing yet keeps its own name —
        // never a name built from the directory, which is its own row on the
        // card — and a prompt or the session that lands later names it.
        let title = derive_title(&path)
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| harness.name.to_string());
        let derived = title != harness.name;
        let conversation_id = match desktop.create_chat(&title, Some(harness.id), None) {
            Ok(id) => id,
            Err(e) => {
                log::warn!("create_chat for harness {} failed: {e}", harness.id);
                return;
            }
        };
        // The directory the harness runs in is the card's own row and the key
        // its session store is read with, so it is recorded with the chat
        // rather than inferred from a pane that may later be closed.
        if !path.trim().is_empty() {
            if let Err(e) = desktop.set_chat_working_dir(&conversation_id, &path) {
                log::warn!("set_chat_working_dir for harness {} failed: {e}", harness.id);
            }
        }
        // The session the conversation will own is claimed later
        // ([`Self::harness_session_id`]), when the harness has written it and
        // the conversation's own timestamp can tell it from its neighbours.
        // Naming one here would hand a fresh card the newest session of the
        // directory, which may belong to another conversation.

        // A name the app derived is not the user's: recording it lets a prompt
        // name the conversation later, the way [`agent_subject`] replaces the
        // subject it wrote itself. Nothing is recorded for the harness's own
        // name, so the session store is still asked for a title later.
        if derived {
            self.harness_title_attempts
                .insert(conversation_id.clone(), title.clone());
        }
        self.harness_chats.insert(key, conversation_id.clone());
        self.record_pane_conversation(pane_id, &conversation_id);
    }

    /// Rename a harness conversation that is still named for its harness, from
    /// a title the conversation's own content gave it.
    ///
    /// A CLI harness writes its turns into its own store, so the naming native
    /// conversations get from their transcript never reaches one: its card
    /// keeps the harness's name until something names it. The caller brings the
    /// name — the subject of the prompt the user just sent, or the title the
    /// harness recorded for its session — and this writes it over a neutral
    /// name, or over one the app derived itself. A subject the user typed
    /// stands.
    ///
    /// Returns whether the title was written.
    pub fn retitle_unnamed_harness_conversation(
        &mut self,
        conversation_id: &str,
        harness: &goble_core::harness::HarnessSpec,
        title: Option<String>,
        desktop: &DesktopState,
    ) -> bool {
        let Some(title) = title.filter(|title| !title.trim().is_empty()) else {
            return false;
        };
        let Some(chat) = desktop
            .list_chats()
            .into_iter()
            .find(|chat| chat.id == conversation_id)
        else {
            return false;
        };
        let directory = chat.working_dir.clone().unwrap_or_default();
        // A name the app itself derived is the app's to replace, as the subject
        // `agent_subject` wrote is; the name the user typed stands.
        let derived = self.harness_title_attempts.get(conversation_id) == Some(&chat.title);
        if chat.title == title
            || !(derived || is_unnamed_harness_title(&chat.title, harness, &directory))
        {
            return false;
        }
        if let Err(e) = desktop.set_chat_title(conversation_id, &title) {
            log::warn!("set_chat_title for harness {conversation_id} failed: {e}");
            return false;
        }
        // The card the frame draws reads `conversations`, so the renamed card
        // reads the new name now rather than at the refresh this write emits.
        if let Some(entry) = self
            .conversations
            .iter_mut()
            .find(|entry| entry.id == conversation_id)
        {
            entry.name = title;
        }
        true
    }

    /// Open `conversation_id` in a tab of its own in the top bar.
    ///
    /// A conversation already on screen is not opened twice: the space holding
    /// a pane bound to it becomes the active tab and that pane takes the focus,
    /// so clicking the same sidebar row brings its thread forward instead of
    /// piling up duplicate tabs. Otherwise a new unnamed space opens with one
    /// agent pane bound to the conversation, on the environment the
    /// conversation itself runs in.
    pub fn open_conversation_tab(
        &mut self,
        conversation_id: String,
        desktop: Option<&DesktopState>,
    ) {
        if conversation_id.is_empty() {
            return;
        }
        // Already open: bring its tab and pane forward. The sidebar row is the
        // conversation's one entry point, so a second click is a look at what
        // is already there rather than another copy of it.
        if let Some((space_index, pane_id)) = self.open_conversation_pane(&conversation_id) {
            self.active_space = space_index;
            self.active_pane_id = pane_id;
            // A terminal pane already bound to the conversation comes forward
            // on the conversation's agent surface. The row is the thread's
            // entry point, and a harness launched from the pane leaves it on
            // the shell after Esc: focusing it there would put the conversation
            // in terminal mode, where the next `!` line runs in the pane's
            // terminal instead of the conversation's shell. A child view open
            // over the parent's transcript is left as well, so the row lands
            // on the conversation it names.
            if self.pane_is_terminal(pane_id) {
                self.sub_agent_views.remove(&pane_id);
                self.open_harness_agent_view(pane_id);
            }
            self.sync_active_view();
            if let Some(desktop) = desktop {
                self.refresh_messages(desktop);
            }
            return;
        }
        // The environment the conversation runs in is persisted on the
        // conversation, so the tab opens where the thread already works rather
        // than on whichever environment happens to be selected.
        let routing = self
            .conversations
            .iter()
            .find(|entry| entry.id == conversation_id)
            .and_then(|entry| routing_from_str(&entry.workspace_routing))
            .unwrap_or(WorkspaceRouting::Local);
        // Every conversation opens in the agent view, including one that ran
        // inside a CLI harness: goble draws the thread in its own agent surface
        // with the harness it carries, rather than mounting the harness's TUI
        // screen. The conversation's directory still travels with the pane, so
        // a turn runs where the thread worked.
        let conversation = self
            .conversations
            .iter()
            .find(|entry| entry.id == conversation_id)
            .cloned();
        let directory = conversation
            .as_ref()
            .map(|entry| crate::state::expand_path(&entry.directory))
            .filter(|path| !path.is_empty());
        let opened_harness = conversation
            .as_ref()
            .and_then(|entry| goble_core::harness::harness_for(&entry.harness))
            .is_some();
        let medium = crate::media::medium_for_routing(routing).to_string();
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        self.spaces.push(
            Space::unnamed(Pane::Leaf {
                id,
                kind: PaneKind::Chat,
            })
            .with_medium(medium),
        );
        self.active_space = self.spaces.len() - 1;
        self.active_pane_id = id;
        self.workspace_routing = Some(routing);
        self.pane_hover
            .entry(id)
            .or_insert_with(|| Rc::new(RefCell::new(false)));
        // The pane owns a session before it can be bound: the binding writes
        // the conversation into that entry, and the reuse lookup below reads it
        // back on the next click. The pane starts where the conversation
        // worked, so a turn runs in the right tree.
        let session = self.pane_sessions.entry(id).or_default();
        if let Some(directory) = directory {
            session.path = directory;
        }
        // Binding the new pane loads the conversation's transcript, adopts the
        // routing its worker/local shape follows, and points the active-view
        // fields at the tab that just opened.
        self.bind_active_pane_conversation(conversation_id, desktop);
        // A harness conversation opens with its own harness selected: the pane
        // is in agent mode from the first frame, and its header, tabs and
        // composer name the harness the thread belongs to.
        if opened_harness {
            self.open_harness_agent_view(id);
        }
        self.refresh_space_labels();
        if let Some(desktop) = desktop {
            self.save_panes(desktop);
        }
    }

    /// Open the agent view of the conversation a CLI harness just started in
    /// `pane_id`, so the harness is used through goble's agent surface from the
    /// moment it launches. Its own TUI keeps running in the pane's pty — Esc
    /// returns to it — but the surface the user works in is the conversation's.
    ///
    /// A pane with no conversation has nothing to open; the launch paths bind
    /// one first (`bind_pane_harness_chat`).
    pub fn open_harness_agent_view(&mut self, pane_id: u64) {
        let Some(conversation_id) = self
            .pane_sessions
            .get(&pane_id)
            .map(|session| session.conversation_id.clone())
            .filter(|conversation_id| !conversation_id.is_empty())
        else {
            return;
        };
        self.pane_controls_mut(pane_id).harness_refusal = None;
        let label = self.conversation_name(&conversation_id);
        self.enter_agent_view(pane_id, &conversation_id, &label);
        // The band is the agent view's own report: a launch still opens the
        // view, and the transcript says what a turn is missing when nothing can
        // run it.
        self.show_llm_key_banner = !self.can_run_agent_turn(pane_id);
        if self.active_pane_id == pane_id {
            self.composer_focused = true;
        }
    }

    /// The CLI harness the pane's conversation belongs to, when it belongs to
    /// one. The native harness is `None`: it is the app's own agent, and its
    /// model is still the pane's to switch.
    pub fn pane_conversation_harness(
        &self,
        pane_id: u64,
    ) -> Option<&'static goble_core::harness::HarnessSpec> {
        let conversation_id = self
            .pane_sessions
            .get(&pane_id)
            .map(|session| session.conversation_id.as_str())
            .filter(|conversation_id| !conversation_id.is_empty())?;
        let entry = self
            .conversations
            .iter()
            .find(|entry| entry.id == conversation_id)?;
        goble_core::harness::harness_for(&entry.harness).filter(|spec| spec.id != "goble")
    }

    /// The harness session `conversation_id` owns.
    ///
    /// A conversation that already recorded one keeps it, even after a newer
    /// session ran in the same directory. One that has none picks the session
    /// whose timestamp is closest to the conversation's own `updated_at`,
    /// skipping sessions another conversation already claimed — so two cards in
    /// one directory name and resume two sessions instead of sharing the
    /// newest. The choice is written to `chats.agent_id`, which is what makes it
    /// survive a restart.
    pub fn harness_session_id(
        &mut self,
        conversation_id: &str,
        harness: &goble_core::harness::HarnessSpec,
        cwd: &str,
        desktop: Option<&DesktopState>,
    ) -> Option<String> {
        let cwd = crate::state::expand_path(cwd);
        let Some(desktop) = desktop else {
            return crate::terminal::newest_session(harness.id, &cwd);
        };
        let chats = desktop.list_chats();
        let chat = chats.iter().find(|chat| chat.id == conversation_id)?;
        if let Some(session) = chat
            .agent_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            return Some(session.to_string());
        }
        let claimed: std::collections::HashSet<&str> = chats
            .iter()
            .filter(|other| other.id != conversation_id)
            .filter_map(|other| other.agent_id.as_deref())
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .collect();
        let sessions = crate::terminal::sessions_newest_first(harness.id, &cwd);
        let unclaimed: Vec<&(String, std::time::SystemTime)> = sessions
            .iter()
            .filter(|(id, _)| !claimed.contains(id.as_str()))
            .collect();
        let updated = chrono::DateTime::parse_from_rfc3339(&chat.updated_at)
            .ok()
            .map(std::time::SystemTime::from);
        let session = match updated {
            Some(updated) => unclaimed.into_iter().min_by_key(|(_, time)| {
                time.duration_since(updated)
                    .or_else(|_| updated.duration_since(*time))
                    .unwrap_or_default()
            }),
            None => unclaimed.first().copied(),
        }?;
        let session = session.0.clone();
        if let Err(e) = desktop.set_chat_agent_id(conversation_id, Some(&session)) {
            log::warn!("set_chat_agent_id for {conversation_id} failed: {e}");
        }
        Some(session)
    }

    /// The tab and pane already showing `conversation_id`, in tab order.
    fn open_conversation_pane(&self, conversation_id: &str) -> Option<(usize, u64)> {
        for (index, space) in self.spaces.iter().enumerate() {
            let mut leaves = Vec::new();
            space.root.leaves(&mut leaves);
            for (pane_id, _) in leaves {
                let bound = self
                    .pane_sessions
                    .get(&pane_id)
                    .is_some_and(|session| session.conversation_id == conversation_id);
                if bound {
                    return Some((index, pane_id));
                }
            }
        }
        None
    }

    /// Bind the active pane to `conversation_id` (used when the sidebar selects
    /// a conversation or a new chat is created), mirror it into `selected_id`,
    /// and reload the bound pane's transcript so it never keeps stale content
    /// from a previously displayed conversation.
    ///
    /// When `desktop` is available the pane's transcript is re-read from the
    /// store for the newly bound conversation. Without a store the pane's
    /// runtime is cleared (there is no per-conversation data to reload), which
    /// also stops a freshly created conversation from inheriting the previous
    /// one's messages.
    pub fn bind_active_pane_conversation(
        &mut self,
        conversation_id: String,
        desktop: Option<&DesktopState>,
    ) {
        // A child view shows a conversation reached from the pane's own
        // transcript (S6). Once the pane binds elsewhere that transcript is
        // stale and its composer-less body would hide the newly-selected
        // conversation, so the pane goes back to the shell — the whole way
        // back: leaving the agent view turns the pane's harness off too, so the
        // surface it draws is the one `pane_view` reports. The child's card
        // stays in the block list and its messages stay in the store.
        if self.sub_agent_views.remove(&self.active_pane_id).is_some() {
            self.leave_agent_view(self.active_pane_id);
        }
        let pane_id = self.active_pane_id;
        if let Some(session) = self.pane_sessions.get_mut(&pane_id) {
            session.conversation_id = conversation_id.clone();
        }
        self.clear_pane_transcript_selection(pane_id);
        self.selected_id = Some(conversation_id);
        if let Some(desktop) = desktop {
            self.refresh_messages(desktop);
        } else {
            if let Some(rt) = self.pane_runtime.get_mut(&self.active_pane_id) {
                rt.messages.clear();
                rt.pending_ask = None;
                rt.reset_command_edit();
                rt.queued_prompt = None;
            }
            self.sync_active_view();
        }
    }

    /// Fork `pane_id`'s conversation: a new conversation that starts from the
    /// messages its source has so far, bound to the pane that asked for it.
    ///
    /// This is warp-new's "fork conversation": the fork inherits the transcript
    /// as its context (the messages are copied into the new conversation, which
    /// is what the next turn reads as history), while the source conversation is
    /// left untouched. The pane then shows the fork, so the next prompt continues
    /// from there.
    ///
    /// Returns the new conversation id, or `None` when the pane has no
    /// conversation yet or there is no store to fork in.
    pub fn fork_pane_conversation(
        &mut self,
        pane_id: u64,
        desktop: Option<&DesktopState>,
    ) -> Option<String> {
        let desktop = desktop?;
        let source = self.pane_conversation_id(pane_id)?;
        let messages = desktop.list_chat_messages(&source).unwrap_or_default();
        let title = desktop
            .list_chats()
            .into_iter()
            .find(|c| c.id == source)
            .map(|c| format!("Fork of {}", c.title))
            .unwrap_or_else(|| "Forked conversation".to_string());
        let forked = match desktop.create_chat(&title, None, None) {
            Ok(id) => id,
            Err(e) => {
                log::warn!("fork: create_chat failed: {e}");
                return None;
            }
        };
        // The copied transcript is the fork's context: the next turn's history
        // is built from these rows, so the fork continues the conversation
        // instead of starting blank.
        for message in &messages {
            if let Err(e) = desktop.add_chat_message(&forked, &message.role, &message.content) {
                log::warn!("fork: copying a message failed: {e}");
            }
        }
        if let Some(session) = self.pane_sessions.get_mut(&pane_id) {
            session.conversation_id = forked.clone();
            session.draft.clear();
        }
        if pane_id == self.active_pane_id {
            self.selected_id = Some(forked.clone());
        }
        // A fork is a new conversation, so its tokens start at zero; the
        // source's totals stay with the source.
        self.conversation_usage.remove(&forked);
        self.refresh_messages(desktop);
        Some(forked)
    }

    /// Set the active pane's composer draft and the global active view.
    pub fn set_active_pane_draft(&mut self, draft: String) {
        if let Some(session) = self.pane_sessions.get_mut(&self.active_pane_id) {
            session.draft = draft.clone();
        }
        self.composer_draft = draft;
    }

    /// Set the active pane's working directory label and the global active view.
    ///
    /// A deliberate write of the pane's directory, like [`Self::set_pane_path`]:
    /// it retires what the pane's shell has reported so far.
    pub fn set_active_pane_path(&mut self, path: String) {
        self.retire_reported_cwd(self.active_pane_id);
        if let Some(session) = self.pane_sessions.get_mut(&self.active_pane_id) {
            session.path = path.clone();
        } else {
            self.pane_sessions.insert(
                self.active_pane_id,
                PaneSession {
                    conversation_id: String::new(),
                    draft: self.composer_draft.clone(),
                    path: path.clone(),
                    daemon_session_id: String::new(),
                },
            );
        }
        self.composer_path = path.clone();
        self.composer_branch = current_branch(&path);
    }

    /// Append a user/assistant message to the active pane's transcript (the
    /// mock/dev path that has no backend store). Keeps the global view in sync.
    pub fn push_active_message(&mut self, message: ChatMessage) {
        let rt = self.pane_runtime.entry(self.active_pane_id).or_default();
        rt.messages.push(message.clone());
        self.chat_messages.push(message);
    }

    /// This pane's transcript scroll state, creating nothing if it has no entry
    /// yet (a pane that was never prepared gets a fresh following state).
    pub fn pane_scroll(&self, pane_id: u64) -> Rc<RefCell<ScrollState>> {
        self.pane_chat_scroll
            .get(&pane_id)
            .cloned()
            .unwrap_or_else(|| Rc::new(RefCell::new(ScrollState::following())))
    }

    /// Whether this pane's usage disclosure is expanded, creating nothing if it
    /// has no entry yet (collapsed).
    pub fn pane_usage_open(&self, pane_id: u64) -> Rc<RefCell<bool>> {
        self.pane_usage_open
            .get(&pane_id)
            .cloned()
            .unwrap_or_else(|| Rc::new(RefCell::new(false)))
    }

    /// This pane's transcript drag selection. `ensure_pane_controls` seeds an
    /// entry for every pane, so this is the same cell across frames; a pane
    /// that has none gets a fresh, empty selection.
    pub fn pane_transcript_selection(
        &self,
        pane_id: u64,
    ) -> Rc<RefCell<goble_ui::TranscriptSelection>> {
        self.pane_transcript_selection
            .get(&pane_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Forget the selection a pane's transcript carried. Called whenever the
    /// pane starts showing different text (another conversation, another
    /// surface): the frozen rows describe what was on screen, and a highlight
    /// left over them would land on the new text.
    pub fn clear_pane_transcript_selection(&self, pane_id: u64) {
        if let Some(selection) = self.pane_transcript_selection.get(&pane_id) {
            selection.borrow_mut().cancel();
        }
    }

    /// Build the per-pane chat snapshot (transcript + draft + path) keyed by
    /// pane id, used to render each chat leaf as an independent session.
    pub fn pane_chat_snapshot(&self) -> HashMap<u64, PaneChatSnapshot> {
        let executions = self.live_executions.len();
        let mut out = HashMap::new();
        for (pane_id, session) in &self.pane_sessions {
            let rt = self.pane_runtime.get(pane_id);
            out.insert(
                *pane_id,
                PaneChatSnapshot {
                    conversation_id: session.conversation_id.clone(),
                    messages: rt.map(|r| r.messages.clone()).unwrap_or_default(),
                    composer_draft: session.draft.clone(),
                    composer_path: session.path.clone(),
                    // A viewer pane's connection state is the same map the
                    // events write; a pane with no entry is not a viewer pane
                    // and draws no connection line.
                    worker: self
                        .pane_workers
                        .get(pane_id)
                        .map(|worker| WorkerPaneSnapshot {
                            worker_id: worker.worker_id.clone(),
                            attach: worker.attach.clone(),
                            session: worker.session.clone(),
                        }),
                    composer_attachments: self.pane_attachments(*pane_id),
                    pending_ask: rt.and_then(|r| r.pending_ask.clone()),
                    pending_command: rt.and_then(|r| r.pending_command.clone()),
                    command_selection: rt
                        .and_then(|r| r.command_selection.clone())
                        .unwrap_or_else(|| Rc::new(RefCell::new(0))),
                    command_scroll: rt
                        .map(|r| r.command_scroll.clone())
                        .unwrap_or_else(|| Rc::new(RefCell::new(goble_ui::ScrollState::default()))),
                    command_draft: rt
                        .map(|r| r.command_draft.clone())
                        .unwrap_or_else(|| Rc::new(RefCell::new(String::new()))),
                    command_caret: rt
                        .map(|r| r.command_caret.clone())
                        .unwrap_or_else(|| Rc::new(RefCell::new(0))),
                    command_editor_scroll: rt
                        .map(|r| r.command_editor_scroll.clone())
                        .unwrap_or_else(|| Rc::new(RefCell::new(goble_ui::ScrollState::default()))),
                    queued_prompt: rt.and_then(|r| r.queued_prompt.clone()),
                    agent_busy: rt.map(|r| r.busy).unwrap_or(false),
                    turn_status: pane_turn_status(
                        rt,
                        executions,
                        self.running_sub_agent_count(*pane_id),
                    ),
                    sub_agents: self.sub_agent_rows(*pane_id),
                    sub_agent_panel_rows: self.pane_sub_agents(*pane_id),
                    turn_error: rt.and_then(|r| r.turn_error.clone()),
                    sub_agent_view: self.sub_agent_views.get(pane_id).map(|view| {
                        crate::ui::SubAgentViewSnapshot {
                            child_id: view.child_id.clone(),
                            // The title reads the live record, so its status,
                            // activity and elapsed time are as current as the
                            // transcript row's; the mounted copy is what is left
                            // once the record has gone from the pane's live set.
                            row: self
                                .live_sub_agent_row(*pane_id, &view.child_id)
                                .or_else(|| view.row.clone()),
                            messages: view.messages.clone(),
                            draft: view.draft.clone(),
                            caret: view.caret.clone(),
                        }
                    }),
                    scroll: self.pane_scroll(*pane_id),
                    text_selection: self.pane_transcript_selection(*pane_id),
                    usage_open: self.pane_usage_open(*pane_id),
                    usage: self
                        .conversation_usage
                        .get(&session.conversation_id)
                        .copied()
                        .unwrap_or_default(),
                },
            );
        }
        out
    }

    /// This pane's rich-input controls, creating the entry (seeded from the
    /// window globals) on first use.
    pub fn pane_controls_mut(&mut self, pane_id: u64) -> &mut PaneControls {
        let model = self.selected_model.clone();
        let branch = self.composer_branch.clone();
        self.pane_controls
            .entry(pane_id)
            .or_insert_with(|| PaneControls::new(model, self.approval_mode, branch))
    }

    /// Take any finished git job's word for the review pane. The worker runs
    /// off the UI thread; this is where its result reaches the status line and
    /// asks the working tree to be read again.
    pub fn collect_diff_git_results(&mut self) {
        for controls in self.pane_controls.values_mut() {
            let outcome = controls
                .diff_git_result
                .lock()
                .ok()
                .and_then(|mut slot| slot.take());
            let Some(outcome) = outcome else {
                continue;
            };
            let (text, ok) = match outcome {
                Ok(text) => (text, true),
                Err(text) => (text, false),
            };
            // A commit that landed consumed its message. A failed one keeps it,
            // so the user can fix the tree and press Commit again without
            // retyping.
            if ok && text == "Committed" {
                *controls.diff_commit_message.borrow_mut() = String::new();
            }
            *controls.diff_git_status.borrow_mut() = Some((text, ok));
            *controls.diff_refresh.borrow_mut() += 1;
        }
    }

    /// A copy of this pane's rich-input controls. A pane that has no entry yet
    /// reads the window globals, so the first frame shows sane values.
    pub fn pane_controls(&self, pane_id: u64) -> PaneControls {
        self.pane_controls
            .get(&pane_id)
            .cloned()
            .unwrap_or_else(|| {
                PaneControls::new(
                    self.selected_model.clone(),
                    self.approval_mode,
                    self.composer_branch.clone(),
                )
            })
    }

    /// The per-pane controls as the views draw them: the model control carries
    /// the model the pane's turns would actually run, and reads
    /// [`MODEL_NOT_CONFIGURED`] when nothing resolves. An empty control would
    /// say nothing at all exactly while the pane cannot run a turn, which is the
    /// state the notice band exists to explain.
    ///
    /// A copy: a label the view needs is not a choice the pane made.
    pub fn pane_controls_snapshot(&self) -> HashMap<u64, PaneControls> {
        let mut out = self.pane_controls.clone();
        for (pane_id, controls) in out.iter_mut() {
            if !controls.model.trim().is_empty() {
                continue;
            }
            let resolved = self.pane_agent_model(*pane_id);
            controls.model = if resolved.trim().is_empty() {
                MODEL_NOT_CONFIGURED.to_string()
            } else {
                resolved
            };
        }
        out
    }

    /// Ensure every rendered leaf pane has a controls entry, so the composer
    /// always reads a stable per-pane value rather than the window globals.
    pub fn ensure_pane_controls(&mut self) {
        let mut ids = Vec::new();
        for space in &self.spaces {
            collect_leaf_pane_ids(&space.root, &mut ids);
        }
        ids.extend(self.pane_sessions.keys().copied());
        ids.push(self.active_pane_id);
        for id in ids {
            self.pane_controls_mut(id);
            // The whole-transcript filter bar is per pane too (its open flag and
            // selection live in app state so they survive the rebuild).
            self.terminal_global_filters.entry(id).or_default();
            // Each pane's transcript owns a persistent scroll state, so the
            // user's scrollback and the follow-the-stream flag survive the
            // per-frame rebuild. A following state opens at the latest message.
            self.pane_chat_scroll
                .entry(id)
                .or_insert_with(|| Rc::new(RefCell::new(ScrollState::following())));
            // Each pane's transcript owns one drag selection too, so a highlight
            // drawn in one pane never appears in another and survives the
            // per-frame rebuild.
            self.pane_transcript_selection
                .entry(id)
                .or_insert_with(|| Rc::new(RefCell::new(goble_ui::TranscriptSelection::default())));
            // A terminal pane's own history scrolls the same way: one following
            // state per pane, opening at the newest command section.
            self.pane_terminal_scroll
                .entry(id)
                .or_insert_with(|| Rc::new(RefCell::new(ScrollState::following())));
            // A file view opens at the file's first line and keeps the user's
            // place, so its offset is a plain one, never a following one.
            self.file_scroll
                .entry(id)
                .or_insert_with(|| Rc::new(RefCell::new(ScrollState::default())));
            // The same view scrolls sideways too: its lines do not wrap, so a
            // long line runs past the pane and the wheel can follow it.
            self.file_hscroll
                .entry(id)
                .or_insert_with(|| Rc::new(RefCell::new(ScrollState::default())));
            // The usage disclosure starts collapsed and remembers its state.
            self.pane_usage_open
                .entry(id)
                .or_insert_with(|| Rc::new(RefCell::new(false)));
        }
    }

    /// Retire what `pane_id`'s shell has reported so far: the app has just set
    /// that pane's directory itself — and told the shell to move there — so a
    /// report made before the write is not where the pane is going.
    fn retire_reported_cwd(&self, pane_id: u64) {
        if let Some(session) = self.terminal.borrow_mut().sessions.get_mut(&pane_id) {
            session.retire_reported_cwds();
        }
    }

    /// Set one pane's working directory and branch pill without touching the
    /// window globals unless it is the active pane.
    ///
    /// The pane's own shell retires what it has reported so far: the app is the
    /// one deciding where the pane is, and a report made before that decision —
    /// a `cd` still in flight, or the directory a shell was started in — cannot
    /// take it back.
    pub fn set_pane_path(&mut self, pane_id: u64, path: String) {
        self.retire_reported_cwd(pane_id);
        let branch = current_branch(&path);
        match self.pane_sessions.get_mut(&pane_id) {
            Some(session) => session.path = path.clone(),
            None => {
                self.pane_sessions.insert(
                    pane_id,
                    PaneSession {
                        conversation_id: String::new(),
                        draft: String::new(),
                        path: path.clone(),
                        daemon_session_id: String::new(),
                    },
                );
            }
        }
        {
            let controls = self.pane_controls_mut(pane_id);
            controls.branch = branch.clone();
            let flag = controls.dir_menu_open.clone();
            *flag.borrow_mut() = false;
        }
        if pane_id == self.active_pane_id {
            self.composer_path = path;
            self.composer_branch = branch;
        }
    }

    /// Follow the working directory each pane's own shell reports.
    ///
    /// `cd` prints nothing, so a prompt that moved to another directory is
    /// visible only on the shell-integration channel the pane's pty carries (the
    /// hooks and OSC 7), never in the output. The rich input's directory pill
    /// draws the pane's session path, so the report is what keeps it honest.
    ///
    /// Only a move is adopted. The session decides what counts as one: a report
    /// it has already offered says nothing new, and the directory a shell was
    /// started in is where the shell was put rather than a move — so a pane
    /// opened at a chosen directory keeps it while its shell starts elsewhere.
    pub fn adopt_reported_cwds(&mut self) {
        let moves: Vec<(u64, String)> = {
            let mut terminal = self.terminal.borrow_mut();
            // Every live shell reads its pty here, before a surface is built:
            // the harness's shell tool is serviced by these pumps, and a pane
            // that draws its agent view (or sits in a background tab) still
            // owns the shell its commands run in.
            terminal.pump_sessions();
            terminal
                .sessions
                .iter_mut()
                .filter_map(|(pane_id, session)| {
                    session.take_cwd_move().map(|path| (*pane_id, path))
                })
                .collect()
        };
        for (pane_id, path) in moves {
            if self.pane_sessions.get(&pane_id).map(|s| s.path.as_str()) == Some(path.as_str()) {
                continue;
            }
            self.set_pane_path(pane_id, path);
        }
    }
}

/// Whether `title` is still the placeholder a harness conversation is created
/// with, rather than a name the conversation's content gave it.
///
/// A conversation opens named for what its harness recorded for the directory,
/// and keeps the harness's own name when it recorded nothing — or, before that,
/// the `Harness · directory` fallback. None of those names is the user's, so any
/// of them is the harness's to replace; the directory is its own row on the
/// card, which is why the old fallback names nothing worth keeping.
pub(super) fn is_unnamed_harness_title(
    title: &str,
    harness: &goble_core::harness::HarnessSpec,
    directory: &str,
) -> bool {
    let title = title.trim();
    if title.is_empty() || title == NEW_CONVERSATION_TITLE || title == harness.name {
        return true;
    }
    let legacy = |dir: &str| format!("{} · {}", harness.name, dir);
    !directory.is_empty()
        && (title == legacy(directory) || title == legacy(&crate::state::display_path(directory)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desktop(dir: &std::path::Path) -> std::sync::Arc<DesktopState> {
        DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("in-memory store"),
            goble_desktop_service::ThreadStore::new(dir).expect("thread store"),
        )
    }

    /// The conversation a CLI harness opens is named for the conversation the
    /// harness recorded in the pane's directory, and the directory never
    /// reaches the title: the card has a row of its own for it. The name the
    /// harness store gave is the app's, so a prompt replaces it — exactly as the
    /// agent replaces the subject it wrote itself.
    #[test]
    fn a_harness_conversation_takes_the_title_its_session_records() {
        let store = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = desktop(store.path());
        let harness = goble_core::harness::harness_by_id("grok").expect("grok harness");
        let mut state = UiState::mock();
        state.pane_sessions.entry(1).or_default().path = "/Users/me/Projects/goble".to_string();

        state.bind_pane_harness_chat_deriving(1, harness, Some(&desktop), |cwd| {
            (cwd == "/Users/me/Projects/goble").then(|| "Sidebar titles".to_string())
        });

        let chat = desktop
            .list_chats()
            .into_iter()
            .find(|chat| chat.provider.as_deref() == Some("grok"))
            .expect("the harness conversation");
        assert_eq!(chat.title, "Sidebar titles");
        assert_eq!(
            chat.working_dir.as_deref(),
            Some("/Users/me/Projects/goble"),
            "the card records the directory the harness works in"
        );
        assert_eq!(
            state
                .pane_sessions
                .get(&1)
                .map(|s| s.conversation_id.clone()),
            Some(chat.id.clone())
        );

        assert!(state.retitle_unnamed_harness_conversation(
            &chat.id,
            harness,
            Some("Arata un text colorat in chat".to_string()),
            &desktop
        ));
        let retitled = desktop
            .list_chats()
            .into_iter()
            .find(|entry| entry.id == chat.id)
            .expect("the harness conversation");
        assert_eq!(retitled.title, "Arata un text colorat in chat");
    }

    /// A harness that has recorded no session yet is named for itself — not for
    /// the directory, and never `Harness · directory`.
    #[test]
    fn a_harness_with_no_session_keeps_the_harness_name() {
        let store = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = desktop(store.path());
        let harness = goble_core::harness::harness_by_id("antigravity").expect("harness");
        let mut state = UiState::mock();
        state.pane_sessions.entry(1).or_default().path = "/tmp/goble".to_string();

        state.bind_pane_harness_chat_deriving(1, harness, Some(&desktop), |_| None);

        let chat = desktop.list_chats().into_iter().next().expect("a chat");
        assert_eq!(chat.title, "Antigravity");
        assert!(!chat.title.contains('·'), "{}", chat.title);
        assert!(!chat.title.contains("/tmp/goble"), "{}", chat.title);
    }

    /// A neutral name is replaced by what the conversation's content gave it; a
    /// name the user typed is never taken back. The old `Harness · directory`
    /// fallback counts as neutral, since the directory is its own row now.
    #[test]
    fn only_an_unnamed_harness_conversation_is_retitled() {
        let store = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = desktop(store.path());
        let harness = goble_core::harness::harness_by_id("grok").expect("grok harness");
        let mut state = UiState::mock();
        let title_of = |id: &str| {
            desktop
                .list_chats()
                .into_iter()
                .find(|chat| chat.id == id)
                .map(|chat| chat.title)
                .expect("the chat")
        };

        let named = desktop
            .create_chat("Planul de lansare", Some(harness.id), None)
            .expect("create chat");
        assert!(!state.retitle_unnamed_harness_conversation(
            &named,
            harness,
            Some("Ceva derivat".to_string()),
            &desktop
        ));
        assert_eq!(title_of(&named), "Planul de lansare");

        let fallback = desktop
            .create_chat(harness.name, Some(harness.id), None)
            .expect("create chat");
        desktop
            .set_chat_working_dir(&fallback, "/Users/me/Projects/goble")
            .expect("working dir");
        assert!(state.retitle_unnamed_harness_conversation(
            &fallback,
            harness,
            Some("Sidebar titles".to_string()),
            &desktop
        ));
        assert_eq!(title_of(&fallback), "Sidebar titles");

        let legacy = desktop
            .create_chat("Grok · /Users/me/Projects/goble", Some(harness.id), None)
            .expect("create chat");
        desktop
            .set_chat_working_dir(&legacy, "/Users/me/Projects/goble")
            .expect("working dir");
        assert!(state.retitle_unnamed_harness_conversation(
            &legacy,
            harness,
            Some("Goble to become an enterprise ready product".to_string()),
            &desktop
        ));
        assert_eq!(
            title_of(&legacy),
            "Goble to become an enterprise ready product"
        );

        // Nothing names the conversation, so nothing is written.
        let quiet = desktop
            .create_chat("New conversation", Some(harness.id), None)
            .expect("create chat");
        assert!(!state.retitle_unnamed_harness_conversation(&quiet, harness, None, &desktop));
        assert_eq!(title_of(&quiet), NEW_CONVERSATION_TITLE);
    }
}
