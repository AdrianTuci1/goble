use super::title::agent_subject;
use super::*;

impl UiState {
    /// Take the search the worker has finished, when it answered the newest
    /// query. Called once per frame, so a result appears as soon as the walk
    /// behind it lands — whichever pane's directory the user has moved to.
    pub fn take_global_search_result(&mut self) {
        let Some(outcome) = self.global_search_worker.take() else {
            return;
        };
        self.global_search_rows = outcome.rows;
        self.global_search_error = outcome.error;
        self.global_search_searched = true;
    }

    /// Start from real backend data. Falls back to an empty state when the
    /// store has no conversations yet; the sidebar shows an empty list until
    /// the user creates the first chat.
    pub fn from_desktop(desktop: &DesktopState) -> Self {
        let mut state = Self {
            current_tab: AppTab::Chat,
            selected_id: None,
            conversations: Vec::new(),
            search_query: String::new(),
            search_focused: false,
            new_conversation_draft: String::new(),
            create_focused: false,
            chat_messages: Vec::new(),
            pending_ask: None,
            queued_prompt: None,
            composer_draft: String::new(),
            composer_focused: false,
            models: Vec::new(),
            selected_model: String::new(),
            harnesses: vec![HarnessEntry::internal("internal", "Goble")],
            selected_harness: "internal".to_string(),
            model_menu_open: Rc::new(RefCell::new(false)),
            harness_menu_open: Rc::new(RefCell::new(false)),
            dir_menu_open: Rc::new(RefCell::new(false)),
            dir_menu_scroll: PanelScroll::default(),
            branch_menu_open: Rc::new(RefCell::new(false)),
            composer_branch: current_branch(&current_dir_display()),
            agent_name: "Goble Agent".to_string(),
            agent_busy: false,
            auto_approve: false,
            right_sidebar_open: false,
            fullscreen: false,
            sub_agent_views: HashMap::new(),
            maximized_pane: None,
            terminal_filters: Rc::new(RefCell::new(HashMap::new())),
            reasoning_expanded: Rc::new(RefCell::new(HashMap::new())),
            tool_fold: Rc::new(RefCell::new(HashMap::new())),
            terminal_global_filters: HashMap::new(),
            crons_open: false,
            task_workflow_open: false,
            task_workflow_selected: Rc::new(RefCell::new(0)),
            task_workflow_phase: Rc::new(RefCell::new(0)),
            shortcuts_help_open: false,
            shortcuts_help_filter: String::new(),
            shortcuts_help_index: 0,
            shortcuts_help_scroll: Rc::new(RefCell::new(ScrollState::default())),
            crons: Vec::new(),
            workflows: Vec::new(),
            executions: Vec::new(),
            tasks: Vec::new(),
            timeline: Vec::new(),
            costs: Vec::new(),
            settings_page: SettingsPage::Profile,
            settings_profile_name: String::new(),
            settings_profile_email: String::new(),
            settings_dark_mode: false,
            settings_llm_provider: "openai".to_string(),
            settings_llm_model: "gpt-4o".to_string(),
            settings_llm_api_key: String::new(),
            settings_llm_base_url: String::new(),
            settings_llm_temperature: "0.7".to_string(),
            settings_workers: Vec::new(),
            settings_cluster_name: String::new(),
            settings_cluster_configured: false,
            settings_authorized_keys: Vec::new(),
            settings_vault_unlocked: false,
            settings_focus: SettingsFocus::Rail,
            settings_pane_focus: 0,
            settings_pane_field_active: false,
            settings_field_edit_start: None,
            settings_environment_groups: Vec::new(),
            settings_environment_open_group: None,
            settings_environment_group_draft: String::new(),
            settings_environment_secret_name: String::new(),
            settings_environment_secret_value: String::new(),
            settings_environment_editing: None,
            settings_ssh_hosts: None,
            settings_ssh_read: false,
            settings_ssh_home: None,
            settings_ssh_selected: None,
            settings_invert_scroll: false,
            settings_scroll_speed: 50,
            settings_font_size: 1.0,
            vim_mode: false,
            conversation_usage: HashMap::new(),
            theme_primary: None,
            theme_secondary: None,
            theme_accent: None,
            theme_color_target: crate::ui::color_picker::ColorTarget::Primary,
            theme_color_drag: Rc::new(RefCell::new(None)),
            show_llm_key_banner: false,
            show_workspace_choice: false,
            workspace_routing: None,
            llm_dialog_open: false,
            llm_dialog_provider: Rc::new(RefCell::new(String::new())),
            llm_dialog_model: Rc::new(RefCell::new(String::new())),
            llm_dialog_api_key: Rc::new(RefCell::new(String::new())),
            llm_dialog_base_url: Rc::new(RefCell::new(String::new())),
            llm_dialog_temperature: Rc::new(RefCell::new(String::new())),
            llm_dialog_focus: Rc::new(RefCell::new(None)),
            sidebar_width: SIDEBAR_WIDTH,
            sidebar_dragging: false,
            sidebar_drag_origin_x: 0.0,
            sidebar_drag_start_width: SIDEBAR_WIDTH,
            sidebar_visible: true,
            conversations_expanded: false,
            sidebar_view: SidebarView::default(),
            starred_conversations: HashSet::new(),
            collapsed_sections: HashSet::new(),
            explorer_expanded: HashSet::new(),
            global_search_query: String::new(),
            global_search_rows: Vec::new(),
            global_search_searched: false,
            global_search_error: None,
            global_search_focused: false,
            global_search_worker: Rc::new(SearchWorker::new()),
            sidebar_scroll: Rc::new(RefCell::new(ScrollState::default())),
            explorer_hover: Rc::new(RefCell::new(None)),
            explorer_scroll: Rc::new(RefCell::new(ScrollState::default())),
            global_search_scroll: Rc::new(RefCell::new(ScrollState::default())),
            settings_scroll: Rc::new(RefCell::new(ScrollState::default())),
            space_rename_editing: false,
            space_press_run: Rc::new(RefCell::new(None)),
            space_rename_draft: String::new(),
            space_rename_focused: false,
            space_menu: Rc::new(RefCell::new(None)),
            agent_cards: HashMap::new(),
            new_agent_hover: Rc::new(RefCell::new(false)),
            spaces: default_spaces(),
            active_space: 0,
            space_hover: None,
            space_press: None,
            space_press_at: None,
            space_drag: None,
            pane_drag: None,
            pane_bounds: Rc::new(RefCell::new(HashMap::new())),
            next_pane_id: 2,
            active_pane_id: 1,
            dragging_pane_id: None,
            composer_path: current_dir_display(),
            pane_hover: HashMap::from([(1u64, Rc::new(RefCell::new(false)))]),
            pane_sessions: HashMap::from([(1u64, PaneSession {
                conversation_id: String::new(),
                draft: String::new(),
                path: current_dir_display(),
                daemon_session_id: String::new(),
            })]),
            pane_runtime: HashMap::new(),
            pane_workers: HashMap::new(),
            pane_ssh: HashMap::new(),
            ssh_host_entries: Vec::new(),
            ssh_host_picker_open: false,
            ssh_host_picker_index: 0,
            ssh_reconnect_prompt: None,
            floating_drag: Rc::new(RefCell::new(None)),
            live_executions: HashMap::new(),
            pane_controls: HashMap::new(),
            pane_chat_scroll: HashMap::new(),
            pane_usage_open: HashMap::new(),
            pane_attachments: HashMap::new(),
            pane_terminal_scroll: HashMap::new(),
            file_scroll: HashMap::new(),
            terminal: Rc::new(RefCell::new(TerminalRegistry::default())),
            command_palette_open: false,
            command_palette_query: String::new(),
            command_palette_index: 0,
            inbox: Vec::new(),
            inbox_open: false,
            inbox_index: 0,
            inbox_error: None,
            inbox_scroll: Rc::new(RefCell::new(ScrollState::default())),
            next_inbox_id: 1,
            show_onboarding_tip: false,
            add_space_menu_open: Rc::new(RefCell::new(false)),
            env_selector_open: Rc::new(RefCell::new(false)),
            env_selector_hover: Rc::new(RefCell::new(None)),
            add_medium_dialog_open: false,
            add_medium_draft: String::new(),
            add_medium_focused: false,
            terminal_run_agent_menu_open: Rc::new(RefCell::new(false)),
            onboarding_done: false,
        };
        state.refresh_from_desktop(desktop);
        state.refresh_harnesses();
        state.prime_llm_form();
        state.restore_panes(desktop);
        state.ensure_pane_sessions(Some(desktop));
        state.refresh_messages(desktop);
        state.sync_active_view();
        // A returning run that already finished (or skipped) first run skips
        // the onboarding overlays and getting-started tip.
        state.onboarding_done = desktop.onboarding_done();
        if state.onboarding_done {
            state.show_llm_key_banner = false;
            state.show_workspace_choice = false;
            state.show_onboarding_tip = false;
        }
        state
    }

    /// Reload conversations, messages, crons and agent name from the backend.
    pub fn refresh_from_desktop(&mut self, desktop: &DesktopState) {
        self.refresh_conversations(desktop);
        self.refresh_crons(desktop);
        self.refresh_agent_name(desktop);
        self.refresh_settings(desktop);
        self.auto_approve = desktop.get_auto_approve();
        self.refresh_observability(desktop);
    }

    /// Reload the harness observability pages (workflows, executions, tasks,
    /// timeline, costs) from the embedded daemon / store.
    pub fn refresh_observability(&mut self, desktop: &DesktopState) {
        self.refresh_workflows(desktop);
        self.refresh_executions(desktop);
        self.refresh_tasks(desktop);
        self.refresh_timeline(desktop);
        self.refresh_costs(desktop);
    }

    /// Workflows registered with the daemon (all of them, not just cron ones).
    pub fn refresh_workflows(&mut self, desktop: &DesktopState) {
        self.workflows = desktop
            .list_workflows()
            .into_iter()
            .map(|wf| WorkflowEntry {
                id: wf.id,
                name: wf.name,
                description: wf.description,
                steps: wf
                    .steps
                    .into_iter()
                    .map(|step| WorkflowStepEntry {
                        name: step.name,
                        agent_id: step.agent_id.0,
                    })
                    .collect(),
                trigger: trigger_label(&wf.trigger),
                enabled: wf.enabled,
                created_at: time_ago(&wf.created_at),
            })
            .collect();
    }

    /// Executions from the daemon execution ledger.
    pub fn refresh_executions(&mut self, desktop: &DesktopState) {
        self.executions = desktop
            .list_executions()
            .into_iter()
            .map(|ex| ExecutionEntry {
                id: ex.id,
                agent_id: ex.agent_id.unwrap_or_default(),
                worker_id: ex.worker_id,
                status: ex.status,
                started_at: time_ago(&ex.started_at),
                finished_at: ex.finished_at.as_deref().map(time_ago),
                step_count: ex.trace.steps.len(),
            })
            .collect();
    }

    /// Durable tasks from the persistence layer.
    pub fn refresh_tasks(&mut self, desktop: &DesktopState) {
        self.tasks = desktop
            .list_tasks()
            .into_iter()
            .map(|task| TaskEntry {
                id: task.task_id,
                session_id: task.session_id.0,
                trigger: task.trigger,
                status: task.status,
                created_at: time_ago(&task.created_at),
            })
            .collect();
    }

    /// Merge execution + session + task records into one chronological event
    /// stream (most recent first). Sessions fall back to `default`/`local` when
    /// their project/medium identity is empty. Rows are sorted by the raw RFC3339
    /// timestamp (which orders correctly for UTC), then rendered with a relative
    /// "time ago" label.
    pub fn refresh_timeline(&mut self, desktop: &DesktopState) {
        // (raw_at, display_label, kind, status) so sorting stays chronological.
        let mut entries: Vec<(String, TimelineEntry)> = Vec::new();
        // Read raw timestamps from the desktop rather than the relativized
        // execution entries so the sort order is truly chronological.
        for ex in desktop.list_executions() {
            entries.push((
                ex.started_at.clone(),
                TimelineEntry {
                    at: time_ago(&ex.started_at),
                    kind: "execution".to_string(),
                    label: format!("Execution {} (agent {})", ex.id, ex.agent_id.unwrap_or_default()),
                    status: Some(ex.status),
                },
            ));
        }
        for task in &self.tasks {
            entries.push((
                task.created_at.clone(),
                TimelineEntry {
                    at: time_ago(&task.created_at),
                    kind: "task".to_string(),
                    label: format!("Task {} ({})", task.id, task.trigger),
                    status: Some(task.status.clone()),
                },
            ));
        }
        for session in desktop.list_sessions() {
            let project = if session.project_id.0.is_empty() {
                "default".to_string()
            } else {
                session.project_id.0.clone()
            };
            let medium = if session.medium_id.0.is_empty() {
                "local".to_string()
            } else {
                session.medium_id.0.clone()
            };
            entries.push((
                session.created_at.clone(),
                TimelineEntry {
                    at: time_ago(&session.created_at),
                    kind: "session".to_string(),
                    label: format!("Session {} ({project}/{medium})", session.session_id.0),
                    status: None,
                },
            ));
        }
        entries.sort_by(|a, b| {
            b.0.cmp(&a.0).then_with(|| a.1.label.cmp(&b.1.label))
        });
        self.timeline = entries.into_iter().map(|(_, entry)| entry).collect();
    }

    /// Cost rows derived from real execution/usage records. There is no cost
    /// backend yet; when an execution trace carries a cost-like metric we sum
    /// it, otherwise we surface an honest derived aggregate rather than
    /// fabricated billing numbers.
    pub fn refresh_costs(&mut self, desktop: &DesktopState) {
        let mut total_cost: f64 = 0.0;
        let mut cost_metrics = 0usize;
        for ex in desktop.list_executions() {
            for metric in &ex.trace.metrics {
                let name = metric.name.to_lowercase();
                if name.contains("cost") || name.contains("usd") || name.contains("price") {
                    total_cost += metric.value;
                    cost_metrics += 1;
                }
            }
        }
        self.costs.clear();
        if cost_metrics > 0 {
            self.costs.push(CostEntry {
                id: "derived-cost".to_string(),
                label: "Estimated spend".to_string(),
                amount: format!("${:.4}", total_cost),
                note: format!(
                    "Summed from {cost_metrics} cost metric(s) across executions; no cost backend wired."
                ),
            });
        }
        let executions = self.executions.len();
        let tasks = self.tasks.len();
        if cost_metrics == 0 {
            self.costs.push(CostEntry {
                id: "derived-usage".to_string(),
                label: "Recorded usage".to_string(),
                amount: format!("{} executions · {} tasks", executions, tasks),
                note: "No cost metric is recorded by the daemon yet; showing real usage counts instead of fabricated billing.".to_string(),
            });
        }
    }

    /// Rebuild the harness list the composer can route to. In this build the
    /// only harness is the native internal one; external harnesses are launched
    /// from a terminal rather than registered here.
    pub fn refresh_harnesses(&mut self) {
        self.harnesses = vec![HarnessEntry::internal("internal", "Goble")];
        self.selected_harness = "internal".to_string();
    }

    /// The status a conversation's sidebar card carries, read off the live pane
    /// runtime that owns it.
    ///
    /// A running turn outranks the conversation's history: while it runs, or
    /// while it waits on an approval or a question, the card is
    /// [`ConversationStatus::Waiting`]; a turn that failed leaves
    /// [`ConversationStatus::Error`]; a pane stopped at a prompt is
    /// [`ConversationStatus::Stopped`]; a finished turn is
    /// [`ConversationStatus::Success`]. A pane that has not reported a turn
    /// reads its conversation's history instead, which is what keeps a
    /// restored conversation green rather than gray until its next turn. A
    /// conversation no pane accounts for is read the same way: an agent reply
    /// means the last turn finished ([`ConversationStatus::Success`]), and no
    /// reply means there is nothing to report ([`ConversationStatus::Default`]).
    pub fn conversation_status(
        &self,
        conversation_id: &str,
        has_agent_reply: bool,
    ) -> ConversationStatus {
        let runtime = self
            .pane_id_for_conversation(conversation_id)
            .and_then(|pane_id| self.pane_runtime.get(&pane_id));
        if let Some(rt) = runtime {
            let waiting_on_user = rt.pending_command.is_some() || rt.pending_ask.is_some();
            if rt.busy || waiting_on_user || rt.agent_state == PaneAgentState::NeedsInput {
                return ConversationStatus::Waiting;
            }
            if rt.turn_error.is_some() || rt.agent_state == PaneAgentState::Errored {
                return ConversationStatus::Error;
            }
            match rt.agent_state {
                PaneAgentState::Idle => return ConversationStatus::Stopped,
                PaneAgentState::Done => return ConversationStatus::Success,
                // No turn observed in this run: the conversation's own history
                // is the only honest report (a restored pane lands here).
                PaneAgentState::Unknown => {}
                PaneAgentState::Working | PaneAgentState::NeedsInput | PaneAgentState::Errored => {
                    // Already answered above; kept exhaustive on purpose.
                }
            }
        }
        if has_agent_reply {
            ConversationStatus::Success
        } else {
            ConversationStatus::Default
        }
    }

    /// Re-derive every conversation's card status from the pane runtime.
    ///
    /// A turn settling — or a pane stopping — changes a card's colour without
    /// writing anything to the store, so the statuses are re-read each frame
    /// rather than only when a conversation changes.
    pub fn refresh_conversation_statuses(&mut self) {
        for index in 0..self.conversations.len() {
            let entry = &self.conversations[index];
            let status = self.conversation_status(&entry.id, entry.has_agent_reply);
            self.conversations[index].status = status;
        }
    }

    /// Rebuild the sidebar's conversation list from the service.
    ///
    /// A conversation the agent has answered in is also named here: the subject
    /// is derived from what was said in it ([`agent_subject`]) and written back
    /// through [`DesktopState::set_chat_title`], so the row drawn below reads it
    /// at once and the next launch reads it from the store. The write happens
    /// only when the subject actually changes — a conversation the agent has
    /// already named, or one the user named, is left alone — so the
    /// `chats:updated` the write emits cannot feed this call back into itself.
    pub fn refresh_conversations(&mut self, desktop: &DesktopState) {
        let chats = desktop.list_chats();
        self.conversations = chats
            .iter()
            .map(|c| {
                // One read of the conversation's messages answers what its card
                // previews, whether the agent has said anything in it, and what
                // the conversation is about.
                let messages = desktop.list_chat_messages(&c.id).unwrap_or_default();
                let last = messages
                    .last()
                    .map(|m| m.content.clone())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "New conversation".to_string());
                // An assistant delta is written the moment the model starts
                // answering and a tool result the moment one runs, so this
                // turns true as soon as the conversation stops being a place
                // the user typed into and becomes one the agent answered. The
                // roles are the store's own strings; `system` is neither.
                let has_agent_reply = messages
                    .iter()
                    .any(|m| m.role == "assistant" || m.role == "tool");
                let title = match agent_subject(
                    &c.title,
                    &messages
                        .iter()
                        .map(|m| (m.role.as_str(), m.content.as_str()))
                        .collect::<Vec<_>>(),
                ) {
                    Some(subject) => {
                        if let Err(e) = desktop.set_chat_title(&c.id, &subject) {
                            log::warn!("set_chat_title for {} failed: {e}", c.id);
                        }
                        subject
                    }
                    None => c.title.clone(),
                };
                ConversationEntry::new(c.id.clone(), title, last, time_ago(&c.updated_at))
                    .with_workspace_routing(
                        c.workspace_routing
                            .clone()
                            .unwrap_or_else(|| "local".to_string()),
                    )
                    // The directory the conversation works in, drawn on its
                    // card under the subject.
                    .with_directory(c.working_dir.clone().unwrap_or_default())
                    .with_has_agent_reply(has_agent_reply)
                    // What the card's status square reports, from the pane
                    // that is running the conversation right now.
                    .with_status(self.conversation_status(&c.id, has_agent_reply))
            })
            .collect();
        // Keep one shared card-state entry per conversation id so hover / the
        // delete menu survive across frames; drop entries for removed chats.
        let ids: std::collections::HashSet<String> =
            self.conversations.iter().map(|c| c.id.clone()).collect();
        self.agent_cards.retain(|id, _| ids.contains(id));
        for c in &self.conversations {
            self.agent_cards
                .entry(c.id.clone())
                .or_insert_with(|| Rc::new(RefCell::new(AgentCardUi::default())));
        }
        if let Some(selected) = &self.selected_id {
            if !chats.iter().any(|c| &c.id == selected) {
                self.selected_id = chats.first().map(|c| c.id.clone());
            }
        } else {
            self.selected_id = chats.first().map(|c| c.id.clone());
        }
        self.refresh_messages(desktop);
    }

    /// Reload every chat pane's transcript from its own conversation, then
    /// re-derive the global "active pane" view. Pane identity is per-pane
    /// ([`UiState::pane_sessions`]), so two panes never share a transcript.
    pub fn refresh_messages(&mut self, desktop: &DesktopState) {
        let pane_ids: Vec<u64> = self.pane_sessions.keys().copied().collect();
        for pane_id in pane_ids {
            let conv = self.pane_conversation_id(pane_id);
            if let Some(conv) = conv {
                if !conv.is_empty() {
                    self.refresh_pane(pane_id, &conv, desktop);
                }
            }
        }
        // A child view open in a pane is a live transcript too: the child keeps
        // writing rows to its own conversation while the user reads it, and its
        // record keeps changing status, activity and elapsed time.
        let open_children: Vec<u64> = self.sub_agent_views.keys().copied().collect();
        for pane_id in open_children {
            let child_id = match self.sub_agent_views.get(&pane_id) {
                Some(view) => view.child_id.clone(),
                None => continue,
            };
            let row = self.live_sub_agent_row(pane_id, &child_id);
            if let Some(view) = self.sub_agent_views.get_mut(&pane_id) {
                view.refresh(row, desktop);
            }
        }
        self.sync_active_view();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The card's status is the owning pane's own state: a running turn (and a
    /// turn held on an approval or a question) is `Waiting`, the failure it
    /// settles with is `Error`, a successful finish is `Success` and a stop is
    /// `Stopped`. A conversation no pane owns falls back to its messages.
    #[test]
    fn a_conversations_status_reads_the_pane_that_owns_it() {
        let mut state = UiState::mock();

        // The pane has observed no turn in this run — a restored conversation —
        // so its own answered history is what the card reports.
        assert_eq!(
            state.conversation_status("c1", true),
            ConversationStatus::Success
        );

        // A turn the app issued is running.
        state.begin_turn(1);
        assert_eq!(
            state.conversation_status("c1", true),
            ConversationStatus::Waiting
        );

        // A turn that finished is done.
        state.begin_turn(1);
        state.finish_turn(1, TurnOutcome::Success);
        assert_eq!(
            state.conversation_status("c1", true),
            ConversationStatus::Success
        );

        // The failed turn it settles into is the red square, even though the
        // conversation holds an agent reply.
        state.begin_turn(1);
        state.apply_turn_error("c1", "model overloaded");
        assert_eq!(
            state.conversation_status("c1", true),
            ConversationStatus::Error
        );

        // A stopped turn is quiet again.
        state.begin_turn(1);
        state.stop_turn(1);
        assert_eq!(
            state.conversation_status("c1", true),
            ConversationStatus::Stopped
        );

        // A question the agent is waiting on is `Waiting` even with no turn
        // running, because the card's yellow is the user's turn to act.
        let mut waiting = UiState::mock();
        waiting.apply_ask_user("c1", "Deploy?", Vec::new());
        assert_eq!(
            waiting.conversation_status("c1", true),
            ConversationStatus::Waiting
        );

        // A conversation no pane holds is read from its own messages.
        assert_eq!(
            state.conversation_status("c2", true),
            ConversationStatus::Success
        );
        assert_eq!(
            state.conversation_status("c2", false),
            ConversationStatus::Default
        );
    }

    /// The statuses the sidebar draws are re-read from the pane runtime, so a
    /// turn settling colours its card without a store write.
    #[test]
    fn refreshing_the_statuses_colours_the_cards_the_panes_report() {
        let mut state = UiState::mock();
        state.begin_turn(1);
        state.refresh_conversation_statuses();
        let card = |state: &UiState, id: &str| {
            state
                .conversations
                .iter()
                .find(|entry| entry.id == id)
                .map(|entry| entry.status)
        };
        assert_eq!(card(&state, "c1"), Some(ConversationStatus::Waiting));
        assert_eq!(card(&state, "c2"), Some(ConversationStatus::Success));

        state.finish_turn(1, TurnOutcome::Success);
        state.refresh_conversation_statuses();
        assert_eq!(card(&state, "c1"), Some(ConversationStatus::Success));
    }
}
