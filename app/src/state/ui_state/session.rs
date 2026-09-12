use super::*;

impl UiState {
    /// The child view `pane_id` shows, if any.
    pub fn sub_agent_view(&self, pane_id: u64) -> Option<&SubAgentChildView> {
        self.sub_agent_views.get(&pane_id)
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
        let conversation_id = match desktop {
            Some(d) => match d.create_chat("New conversation", None, None) {
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
        // conversation, so the pane goes back to the shell. The child's card
        // stays in the block list and its messages stay in the store.
        if self.sub_agent_views.remove(&self.active_pane_id).is_some() {
            self.pane_controls_mut(self.active_pane_id).view = BlockView::Terminal;
        }
        if let Some(session) = self.pane_sessions.get_mut(&self.active_pane_id) {
            session.conversation_id = conversation_id.clone();
        }
        self.selected_id = Some(conversation_id);
        if let Some(desktop) = desktop {
            self.refresh_messages(desktop);
        } else {
            if let Some(rt) = self.pane_runtime.get_mut(&self.active_pane_id) {
                rt.messages.clear();
                rt.pending_ask = None;
                rt.pending_command = None;
                rt.command_selection = None;
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
    pub fn set_active_pane_path(&mut self, path: String) {
        if let Some(session) = self.pane_sessions.get_mut(&self.active_pane_id) {
            session.path = path.clone();
        } else {
            self.pane_sessions.insert(
                self.active_pane_id,
                PaneSession {
                    conversation_id: String::new(),
                    draft: self.composer_draft.clone(),
                    path: path.clone(),
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
                    pending_ask: rt.and_then(|r| r.pending_ask.clone()),
                    pending_command: rt.and_then(|r| r.pending_command.clone()),
                    command_selection: rt
                        .and_then(|r| r.command_selection.clone())
                        .unwrap_or_else(|| Rc::new(RefCell::new(0))),
                    queued_prompt: rt.and_then(|r| r.queued_prompt.clone()),
                    agent_busy: rt.map(|r| r.busy).unwrap_or(false),
                    turn_status: pane_turn_status(rt, executions),
                    inline_screen: None,
                    screen_link: message_screen_link(&rt.map(|r| r.messages.clone()).unwrap_or_default()),
                    sub_agents: self.sub_agent_rows(*pane_id),
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
                        }
                    }),
                    scroll: self.pane_scroll(*pane_id),
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
        let auto_approve = self.auto_approve;
        let branch = self.composer_branch.clone();
        self.pane_controls
            .entry(pane_id)
            .or_insert_with(|| PaneControls::new(model, auto_approve, branch))
    }

    /// A copy of this pane's rich-input controls. A pane that has no entry yet
    /// reads the window globals, so the first frame shows sane values.
    pub fn pane_controls(&self, pane_id: u64) -> PaneControls {
        self.pane_controls.get(&pane_id).cloned().unwrap_or_else(|| {
            PaneControls::new(
                self.selected_model.clone(),
                self.auto_approve,
                self.composer_branch.clone(),
            )
        })
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
            self.terminal_global_filters
                .entry(id)
                .or_insert_with(TerminalFilter::default);
            // Each pane's transcript owns a persistent scroll state, so the
            // user's scrollback and the follow-the-stream flag survive the
            // per-frame rebuild. A following state opens at the latest message.
            self.pane_chat_scroll
                .entry(id)
                .or_insert_with(|| Rc::new(RefCell::new(ScrollState::following())));
            // A terminal pane's own history scrolls the same way: one following
            // state per pane, opening at the newest command section.
            self.pane_terminal_scroll
                .entry(id)
                .or_insert_with(|| Rc::new(RefCell::new(ScrollState::following())));
            // The usage disclosure starts collapsed and remembers its state.
            self.pane_usage_open
                .entry(id)
                .or_insert_with(|| Rc::new(RefCell::new(false)));
        }
    }

    /// Set one pane's working directory and branch pill without touching the
    /// window globals unless it is the active pane.
    pub fn set_pane_path(&mut self, pane_id: u64, path: String) {
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

    /// Get (creating if needed) the app-owned open flag for `pane_id`'s
    /// agent-header 3-dots menu. Keyed per pane so opening the tray in one
    /// split pane does not open it in the others sharing the view.
    pub fn agent_menu_open(&mut self, pane_id: u64) -> Rc<RefCell<bool>> {
        self.agent_header_menus
            .entry(pane_id)
            .or_insert_with(|| Rc::new(RefCell::new(false)))
            .clone()
    }

    /// Ensure a per-pane agent-header menu flag exists for every rendered leaf
    /// pane (walking the space tree), plus any session key and the active pane,
    /// so the UI can always read a stable app-owned flag and the tray's
    /// open/closed state persists across the per-frame rebuild. Called before
    /// the snapshot is built.
    pub fn ensure_agent_menu_flags(&mut self) {
        let mut ids = Vec::new();
        for space in &self.spaces {
            collect_leaf_pane_ids(&space.root, &mut ids);
        }
        ids.extend(self.pane_sessions.keys().copied());
        ids.push(self.active_pane_id);
        for id in ids {
            self.agent_menu_open(id);
        }
    }
}
