use super::*;

impl UiState {
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
            pending_screen_link_open: None,
            model_menu_open: Rc::new(RefCell::new(false)),
            harness_menu_open: Rc::new(RefCell::new(false)),
            dir_menu_open: Rc::new(RefCell::new(false)),
            branch_menu_open: Rc::new(RefCell::new(false)),
            composer_branch: current_branch(&current_dir_display()),
            agent_name: "Goble Agent".to_string(),
            agent_busy: false,
            auto_approve: false,
            right_sidebar_open: false,
            fullscreen: false,
            sub_agent_views: HashMap::new(),
            agent_header_menus: HashMap::new(),
            terminal_filters: Rc::new(RefCell::new(HashMap::new())),
            reasoning_expanded: Rc::new(RefCell::new(HashMap::new())),
            tool_fold: Rc::new(RefCell::new(HashMap::new())),
            terminal_global_filters: HashMap::new(),
            crons_open: false,
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
            settings_overlay_open: false,
            settings_category: SettingsCategory::Appearance,
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
            sidebar_scroll: Rc::new(RefCell::new(ScrollState::default())),
            settings_scroll: Rc::new(RefCell::new(ScrollState::default())),
            space_rename_editing: false,
            space_rename_draft: String::new(),
            space_rename_focused: false,
            agent_cards: HashMap::new(),
            new_agent_hover: Rc::new(RefCell::new(false)),
            spaces: default_spaces(),
            active_space: 0,
            space_hover: None,
            space_press: None,
            space_drag: None,
            next_pane_id: 2,
            active_pane_id: 1,
            dragging_pane_id: None,
            composer_path: current_dir_display(),
            pane_hover: HashMap::from([(1u64, Rc::new(RefCell::new(false)))]),
            pane_sessions: HashMap::from([(1u64, PaneSession {
                conversation_id: String::new(),
                draft: String::new(),
                path: current_dir_display(),
            })]),
            pane_runtime: HashMap::new(),
            live_executions: HashMap::new(),
            pane_controls: HashMap::new(),
            pane_chat_scroll: HashMap::new(),
            pane_usage_open: HashMap::new(),
            pane_terminal_scroll: HashMap::new(),
            terminal: Rc::new(RefCell::new(TerminalRegistry::default())),
            command_palette_open: false,
            command_palette_query: String::new(),
            command_palette_index: 0,
            show_onboarding_tip: false,
            add_space_menu_open: Rc::new(RefCell::new(false)),
            env_selector_open: Rc::new(RefCell::new(false)),
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

    pub fn refresh_conversations(&mut self, desktop: &DesktopState) {
        let chats = desktop.list_chats();
        self.conversations = chats
            .iter()
            .map(|c| {
                let last = desktop
                    .list_chat_messages(&c.id)
                    .ok()
                    .and_then(|msgs| {
                        msgs.last()
                            .map(|m| m.content.clone())
                            .filter(|s| !s.trim().is_empty())
                    })
                    .unwrap_or_else(|| "New conversation".to_string());
                ConversationEntry::new(c.id.clone(), c.title.clone(), last, time_ago(&c.updated_at))
                    .with_workspace_routing(
                        c.workspace_routing
                            .clone()
                            .unwrap_or_else(|| "local".to_string()),
                    )
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
