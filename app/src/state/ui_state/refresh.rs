use super::session::is_unnamed_harness_title;
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
            approval_mode: ApprovalMode::Manual,
            onboarding_open: false,
            onboarding_step: 0,
            onboarding_usage: String::new(),
            onboarding_name: String::new(),
            onboarding_share: false,
            share_telemetry: false,
            settings_default_model: String::new(),
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
            workflow_runs: Vec::new(),
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
            update: None,
            update_view: crate::update::UpdateView::default(),
            settings_invert_scroll: false,
            settings_scroll_speed: 50,
            settings_font_size: 1.0,
            vim_mode: false,
            auto_save_on_close: false,
            pending_close_pane: None,
            pending_close_file: None,
            conversation_usage: HashMap::new(),
            theme_primary: None,
            theme_secondary: None,
            theme_accent: None,
            theme_color_target: crate::ui::color_picker::ColorTarget::Primary,
            theme_color_drag: Rc::new(RefCell::new(None)),
            show_llm_key_banner: false,
            show_workspace_choice: false,
            workspace_routing: None,
            remote_agents: Vec::new(),
            remote_mode: goble_core::config::RemoteMode::default(),
            remote_gateway_url: String::new(),
            remote_gateway_token: String::new(),
            remote_config_path: String::new(),
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
            settings_model_menu_open: Rc::new(RefCell::new(false)),
            settings_model_anchor: Rc::new(RefCell::new(None)),
            settings_model_painted: Rc::new(RefCell::new(false)),
            space_rename_editing: false,
            command_held: false,
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
            pane_sessions: HashMap::from([(
                1u64,
                PaneSession {
                    conversation_id: String::new(),
                    draft: String::new(),
                    path: current_dir_display(),
                    daemon_session_id: String::new(),
                },
            )]),
            pane_runtime: HashMap::new(),
            pane_workers: HashMap::new(),
            pane_ssh: HashMap::new(),
            harness_chats: HashMap::new(),
            harness_title_attempts: HashMap::new(),
            harness_session_titles: std::collections::HashSet::new(),
            ssh_host_entries: Vec::new(),
            ssh_host_picker_open: false,
            ssh_host_picker_index: 0,
            ssh_reconnect_prompt: None,
            floating_drag: Rc::new(RefCell::new(None)),
            live_executions: HashMap::new(),
            pane_controls: HashMap::new(),
            pane_chat_scroll: HashMap::new(),
            pane_transcript_selection: HashMap::new(),
            pane_usage_open: HashMap::new(),
            pane_attachments: HashMap::new(),
            pane_terminal_scroll: HashMap::new(),
            file_scroll: HashMap::new(),
            file_hscroll: HashMap::new(),
            pane_open_files: HashMap::new(),
            file_buffers: HashMap::new(),
            terminal: Rc::new(RefCell::new(TerminalRegistry::for_app())),
            command_palette_open: false,
            command_palette_query: String::new(),
            command_palette_index: 0,
            inbox: Vec::new(),
            inbox_open: false,
            inbox_index: 0,
            inbox_error: None,
            inbox_scroll: Rc::new(RefCell::new(ScrollState::default())),
            inbox_anchor: Rc::new(RefCell::new(None)),
            announcement: None,
            announcement_until: None,
            next_inbox_id: 1,
            agent_dashboard: AgentDashboardState::default(),
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
        // The answers the onboarding flow and the Profile page share. Whether
        // the overlay opens is the app entry point's call (it knows this is a
        // real launch, not a test mounting the tree): see
        // `UiState::open_onboarding_if_first_run`.
        state.onboarding_done = desktop.onboarding_done();
        state.settings_profile_name = desktop.get_friendly_name();
        state.onboarding_name = state.settings_profile_name.clone();
        state.onboarding_usage = desktop.get_usage();
        state.share_telemetry = goble_telemetry::global()
            .map(|telemetry| telemetry.consent().upload)
            .unwrap_or(false);
        state.onboarding_share = state.share_telemetry;
        state
    }

    /// Reload conversations, messages, crons and agent name from the backend.
    pub fn refresh_from_desktop(&mut self, desktop: &DesktopState) {
        self.refresh_conversations(desktop);
        self.refresh_crons(desktop);
        self.refresh_agent_name(desktop);
        self.refresh_settings(desktop);
        self.auto_approve = desktop.get_auto_approve();
        // The app-wide approval policy: a stored mode wins; before one was
        // stored, mirror the legacy auto-approve switch.
        let mode = desktop
            .get_approval_mode()
            .map(|value| ApprovalMode::from_label(&value))
            .unwrap_or(if self.auto_approve {
                ApprovalMode::AlwaysApprove
            } else {
                ApprovalMode::Manual
            });
        self.apply_approval_mode(mode);
        self.refresh_observability(desktop);
    }

    /// Reload the harness observability pages (workflows, executions, tasks,
    /// timeline, costs) from the embedded daemon / store.
    pub fn refresh_observability(&mut self, desktop: &DesktopState) {
        self.refresh_workflows(desktop);
        self.refresh_workflow_runs(desktop);
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

    /// Settled workflow runs from the persisted run ledger, newest first.
    pub fn refresh_workflow_runs(&mut self, desktop: &DesktopState) {
        self.workflow_runs = desktop
            .list_workflow_runs()
            .into_iter()
            .map(|run| WorkflowRunEntry {
                id: run.id,
                workflow_id: run.workflow_id,
                status: run.status,
                error: run.error,
                steps: run
                    .steps
                    .into_iter()
                    .map(|step| WorkflowRunStepEntry {
                        name: step.name,
                        status: step.status,
                        detail: step.detail,
                    })
                    .collect(),
                started_at: time_ago(&run.started_at),
                finished_at: run.finished_at.as_deref().map(time_ago),
                output: run.output,
                log: run.log,
            })
            .collect();
    }

    /// The newest run recorded for `workflow_id`.
    pub fn latest_run(&self, workflow_id: &str) -> Option<&WorkflowRunEntry> {
        self.workflow_runs
            .iter()
            .find(|run| run.workflow_id == workflow_id)
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
                    label: format!(
                        "Execution {} (agent {})",
                        ex.id,
                        ex.agent_id.unwrap_or_default()
                    ),
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
        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.label.cmp(&b.1.label)));
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
    /// Whether a CLI harness is running in the pane that holds
    /// `conversation_id`. A harness keeps its turns in its own terminal, so this
    /// is the sign that the conversation is live even when the store holds no
    /// assistant row for it.
    fn conversation_runs_a_harness(&self, conversation_id: &str) -> bool {
        self.pane_id_for_conversation(conversation_id)
            .is_some_and(|pane_id| self.terminal.borrow().pane_harness(pane_id).is_some())
    }

    /// The harness a conversation's sidebar card draws its mark from: the CLI
    /// agent running in the pane that holds it, else the model provider the
    /// store recorded, else Goble's own native harness.
    fn conversation_harness(&self, conversation_id: &str, provider: Option<&str>) -> String {
        if let Some(harness) = self
            .pane_id_for_conversation(conversation_id)
            .and_then(|pane_id| self.terminal.borrow().pane_harness(pane_id))
        {
            return harness.id.to_string();
        }
        provider.unwrap_or("goble").to_string()
    }

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

    /// Whether a turn is still writing into `conversation_id` — running, or
    /// suspended on an approval or a question.
    ///
    /// While that is true the conversation's transcript is growing token by
    /// token, so its sidebar card keeps the preview it already shows instead
    /// of re-reading the half-written reply on every delta.
    fn conversation_is_live(&self, conversation_id: &str) -> bool {
        let Some(rt) = self
            .pane_id_for_conversation(conversation_id)
            .and_then(|pane_id| self.pane_runtime.get(&pane_id))
        else {
            return false;
        };
        rt.busy
            || rt.pending_command.is_some()
            || rt.pending_ask.is_some()
            || matches!(
                rt.agent_state,
                PaneAgentState::Working | PaneAgentState::NeedsInput
            )
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

    /// The directory a harness conversation ran in.
    ///
    /// A harness conversation is opened without a working directory of its own,
    /// so the pane bound to it names where the harness ran — the directory the
    /// card labels itself with. Without it the lookup below would have nothing
    /// to ask a session store about.
    fn harness_directory(
        &self,
        chat: &goble_desktop_service::Chat,
        harness: &goble_core::harness::HarnessSpec,
        peers: &[goble_desktop_service::Chat],
    ) -> String {
        let direct = self.direct_harness_directory(chat, harness);
        if !direct.is_empty() {
            return direct;
        }
        // A card with neither a directory of its own nor a pane still works in
        // the workspace its sibling conversations of the same harness use, when
        // they all agree on one. Two projects leave it unnamed rather than
        // guessing which session store its thread lives in.
        let mut siblings: Vec<String> = peers
            .iter()
            .filter(|peer| peer.id != chat.id && peer.provider.as_deref() == Some(harness.id))
            .map(|peer| self.direct_harness_directory(peer, harness))
            .filter(|directory| !directory.is_empty())
            // Two cards may spell one directory as `/Users/me/...` and as
            // `~/...`; they name the same workspace and must not look like two.
            .map(|directory| crate::state::display_path(&directory))
            .collect();
        siblings.sort_unstable();
        siblings.dedup();
        match siblings.as_slice() {
            [directory] => directory.clone(),
            _ => String::new(),
        }
    }

    /// The directory `chat` names itself, without asking a sibling: what it
    /// recorded, the pane that holds it, or the old `Harness · directory` title.
    fn direct_harness_directory(
        &self,
        chat: &goble_desktop_service::Chat,
        harness: &goble_core::harness::HarnessSpec,
    ) -> String {
        if let Some(directory) = chat
            .working_dir
            .as_deref()
            .map(str::trim)
            .filter(|directory| !directory.is_empty())
        {
            return directory.to_string();
        }
        if let Some(directory) = self
            .pane_id_for_conversation(&chat.id)
            .and_then(|pane| self.pane_sessions.get(&pane))
            .map(|session| session.path.trim())
            .filter(|path| !path.is_empty())
        {
            return directory.to_string();
        }
        // A card created before the directory was recorded still carries it in
        // the old `Harness · directory` title. Recovering it here is what lets
        // those conversations find their session store and be renamed.
        let prefix = format!("{} · ", harness.name);
        chat.title
            .strip_prefix(&prefix)
            .map(str::trim)
            .filter(|directory| directory.starts_with('/') || directory.starts_with('~'))
            .map(crate::state::expand_path)
            .unwrap_or_default()
    }

    /// Name the harness conversations that are still named for their harness,
    /// from the title the harness recorded for the session each one owns.
    ///
    /// A CLI harness writes its turns into its own store, so the naming native
    /// conversations get from their transcript never reaches one. What the
    /// harness does keep is a title — a summary, the first prompt — for the
    /// session it ran in the conversation's directory, and that is the
    /// conversation's name, read the same way a native conversation's is. The
    /// session is the one the conversation claimed for itself
    /// ([`Self::claim_harness_sessions`]), so two conversations of one directory
    /// keep two names.
    ///
    /// The lookup reads the filesystem, so each conversation is looked at once
    /// per run: a store that names nothing is not walked again when the next
    /// event rebuilds the list ([`UiState::harness_title_attempts`]), and a
    /// conversation named from its own session is not read again either
    /// ([`UiState::harness_session_titles`]). Returns the titles written, by
    /// conversation id, because the rows built from this refresh were read from
    /// the store before the write.
    fn retitle_harness_conversations(
        &mut self,
        desktop: &DesktopState,
        chats: &[goble_desktop_service::Chat],
    ) -> HashMap<String, String> {
        // Which session each conversation owns is settled first: the titles
        // below are read from those sessions, and a directory holding several
        // conversations would otherwise name them all from the newest one.
        self.claim_harness_sessions(desktop, chats);
        let chats = desktop.list_chats();
        let mut retitled = HashMap::new();
        for chat in &chats {
            // Only a conversation the store records a CLI harness for: a model
            // provider's own chats are named from their transcript, and an
            // empty provider is the native harness.
            let Some(harness) = chat
                .provider
                .as_deref()
                .and_then(goble_core::harness::harness_by_id)
                .filter(|spec| spec.tui)
            else {
                continue;
            };
            let session = chat
                .agent_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty());
            // The title the card carries now, when the app wrote it: a name the
            // user typed is theirs and is left alone.
            let ours = self.harness_title_attempts.get(&chat.id) == Some(&chat.title);
            let directory = self.harness_directory(chat, harness, &chats);
            if session.is_some() && self.harness_session_titles.contains(&chat.id) {
                continue;
            }
            if session.is_none() && ours {
                continue;
            }
            if !ours && !is_unnamed_harness_title(&chat.title, harness, &directory) {
                continue;
            }
            // The directory recovered from an old title is the key the harness
            // store is read with and the row the card draws, so it is recorded
            // with the conversation rather than re-derived every refresh.
            if !directory.is_empty()
                && chat
                    .working_dir
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .is_empty()
            {
                if let Err(e) = desktop
                    .set_chat_working_dir(&chat.id, &crate::state::display_path(&directory))
                {
                    log::warn!("set_chat_working_dir for {} failed: {e}", chat.id);
                }
            }
            let directory = crate::state::expand_path(&directory);
            let title = match session {
                Some(session) => {
                    crate::terminal::session_title_for(harness.id, &directory, session)
                }
                // A store with no list to choose from still names its newest
                // session; a store whose sessions all belong to other
                // conversations names nothing here, so two cards never carry
                // the same title.
                None if crate::terminal::sessions_newest_first(harness.id, &directory)
                    .is_empty() =>
                {
                    crate::terminal::session_title(harness.id, &directory)
                }
                None => None,
            };
            if session.is_some() && title.is_some() {
                self.harness_session_titles.insert(chat.id.clone());
            }
            self.harness_title_attempts
                .insert(chat.id.clone(), title.clone().unwrap_or_default());
            let Some(title) = title else {
                continue;
            };
            if self.retitle_unnamed_harness_conversation(
                &chat.id,
                harness,
                Some(title.clone()),
                desktop,
            ) {
                retitled.insert(chat.id.clone(), title);
            }
        }
        retitled
    }

    /// Give every harness conversation of a directory the session its own
    /// timestamp points at.
    ///
    /// Pairing starts from the closest timestamp and works outwards, so two
    /// conversations in one directory take two sessions and a conversation with
    /// no session of its own takes none — never a neighbour's. A session another
    /// conversation already recorded is out of the running.
    fn claim_harness_sessions(
        &mut self,
        desktop: &DesktopState,
        chats: &[goble_desktop_service::Chat],
    ) {
        let taken: std::collections::HashSet<String> = chats
            .iter()
            .filter_map(|chat| chat.agent_id.as_deref())
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .collect();
        let mut groups: HashMap<(&'static str, String), Vec<&goble_desktop_service::Chat>> =
            HashMap::new();
        for chat in chats {
            if chat
                .agent_id
                .as_deref()
                .is_some_and(|id| !id.trim().is_empty())
            {
                continue;
            }
            let Some(harness) = chat
                .provider
                .as_deref()
                .and_then(goble_core::harness::harness_by_id)
                .filter(|spec| spec.tui)
            else {
                continue;
            };
            let directory = self.harness_directory(chat, harness, chats);
            if directory.trim().is_empty() {
                continue;
            }
            groups
                .entry((harness.id, directory))
                .or_default()
                .push(chat);
        }
        for ((harness, directory), group) in groups {
            let directory = crate::state::expand_path(&directory);
            let sessions: Vec<(String, std::time::SystemTime)> =
                crate::terminal::sessions_newest_first(harness, &directory)
                    .into_iter()
                    .filter(|(id, _)| !taken.contains(id))
                    .collect();
            if sessions.is_empty() {
                continue;
            }
            for (conversation, session) in sessions_for_chats(&group, &sessions) {
                if let Err(e) = desktop.set_chat_agent_id(&conversation, Some(&session)) {
                    log::warn!("set_chat_agent_id for {conversation} failed: {e}");
                }
            }
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
    /// A conversation that ran in a CLI harness has no transcript here, so it is
    /// named from the title the harness recorded for its session
    /// ([`Self::retitle_harness_conversations`]) instead.
    ///
    /// A conversation whose turn is still running keeps the preview and reply
    /// flag its card already carries. Every streamed delta rewrites its
    /// transcript, and re-reading that would rewrite the card on every token —
    /// a list that flickers while the harness works. The settled refresh after
    /// the turn ends picks the finished text up once.
    pub fn refresh_conversations(&mut self, desktop: &DesktopState) {
        let chats = desktop.list_chats();
        // What each card reads now, by conversation id, so a live turn has
        // something settled to keep showing.
        let settled: std::collections::HashMap<String, (String, bool)> = self
            .conversations
            .iter()
            .map(|entry| {
                (
                    entry.id.clone(),
                    (entry.last_response.clone(), entry.has_agent_reply),
                )
            })
            .collect();
        // A harness conversation is named by the harness's own session store
        // rather than by its messages (there are none in the store), so those
        // titles are read before the list is built: the rows below carry the
        // names this pass wrote.
        let retitled = self.retitle_harness_conversations(desktop, &chats);
        self.conversations = chats
            .iter()
            .map(|c| {
                let live = self.conversation_is_live(&c.id);
                let previous = settled.get(&c.id);
                let mut title = retitled
                    .get(&c.id)
                    .cloned()
                    .unwrap_or_else(|| c.title.clone());
                let (last, has_agent_reply) = match previous {
                    Some((last, has_agent_reply)) if live => (last.clone(), *has_agent_reply),
                    _ => {
                        // One read of the conversation's messages answers what
                        // its card previews, whether the agent has said
                        // anything in it, and what it is about.
                        let messages = desktop.list_chat_messages(&c.id).unwrap_or_default();
                        let last = messages
                            .last()
                            .map(|m| m.content.clone())
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or_else(|| "New conversation".to_string());
                        // An assistant delta is written the moment the model
                        // starts answering and a tool result the moment one
                        // runs, so this turns true as soon as the conversation
                        // stops being a place the user typed into and becomes
                        // one the agent answered. The roles are the store's own
                        // strings; `system` is neither.
                        let has_agent_reply = messages
                            .iter()
                            .any(|m| m.role == "assistant" || m.role == "tool");
                        // A live turn's reply is half-written, so it cannot
                        // name the conversation yet: the settled refresh after
                        // the turn is what derives the subject. That also keeps
                        // a title write (and the `chats:updated` it emits) out
                        // of the per-token path.
                        if !live {
                            if let Some(subject) = agent_subject(
                                &c.title,
                                &messages
                                    .iter()
                                    .map(|m| (m.role.as_str(), m.content.as_str()))
                                    .collect::<Vec<_>>(),
                            ) {
                                if let Err(e) = desktop.set_chat_title(&c.id, &subject) {
                                    log::warn!("set_chat_title for {} failed: {e}", c.id);
                                }
                                title = subject;
                            }
                        }
                        (last, has_agent_reply)
                    }
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
                    // A CLI harness writes its turns into its own terminal, so
                    // its conversation has no assistant row in the store — and
                    // a card with no reply is not listed. The pane running the
                    // harness is the proof the conversation is live, so it is
                    // what keeps that card in the sidebar.
                    .with_has_agent_reply(
                        has_agent_reply
                            || self.conversation_runs_a_harness(&c.id)
                            // A conversation opened for a CLI harness records
                            // that harness as its provider; it stays listed
                            // after the CLI exits, because its thread lives in
                            // the terminal's scrollback, not in the store.
                            || c.provider.as_deref().is_some_and(|provider| {
                                goble_core::harness::harness_by_id(provider).is_some()
                            }),
                    )
                    // What the badge on the card's harness mark reports, from
                    // the pane that is running the conversation right now.
                    .with_status(self.conversation_status(&c.id, has_agent_reply))
                    // Which harness's mark the card leads with. The pane that
                    // holds the conversation knows the CLI agent running in it;
                    // a chat with no live pane falls back to its model provider
                    // (the only harness identity the store records), and a chat
                    // with neither runs the native harness.
                    .with_harness(self.conversation_harness(&c.id, c.provider.as_deref()))
            })
            .collect();
        // Keep one shared card-state entry per conversation id so hover / the
        // delete menu survive across frames; drop entries for removed chats.
        let ids: std::collections::HashSet<String> =
            self.conversations.iter().map(|c| c.id.clone()).collect();
        self.agent_cards.retain(|id, _| ids.contains(id));
        self.harness_title_attempts.retain(|id, _| ids.contains(id));
        self.harness_session_titles.retain(|id| ids.contains(id));
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

/// Pair conversations with the sessions of their directory, by the distance
/// between the conversation's `updated_at` and the session's own time.
///
/// Pairs are taken nearest first, and neither side is paired twice, so the
/// conversation whose timestamp sits on a session gets it, and a conversation
/// that sits on nothing is left without one.
fn sessions_for_chats(
    chats: &[&goble_desktop_service::Chat],
    sessions: &[(String, std::time::SystemTime)],
) -> Vec<(String, String)> {
    let mut pairs: Vec<(std::time::Duration, &str, &str)> = Vec::new();
    for chat in chats {
        let Ok(updated) = chrono::DateTime::parse_from_rfc3339(&chat.updated_at) else {
            continue;
        };
        let updated = std::time::SystemTime::from(updated);
        for (session, when) in sessions {
            let distance = when
                .duration_since(updated)
                .or_else(|_| updated.duration_since(*when))
                .unwrap_or_default();
            pairs.push((distance, chat.id.as_str(), session.as_str()));
        }
    }
    pairs.sort_by_key(|(distance, _, _)| *distance);
    let mut paired_chats: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut paired_sessions: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut paired = Vec::new();
    for (_, conversation, session) in pairs {
        if paired_chats.contains(conversation) || paired_sessions.contains(session) {
            continue;
        }
        paired_chats.insert(conversation);
        paired_sessions.insert(session);
        paired.push((conversation.to_string(), session.to_string()));
    }
    paired
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

    /// A live turn's card keeps the preview it already shows. The harness
    /// appends the reply delta by delta, and following that would rewrite the
    /// card on every token; the refresh after the turn ends reads the finished
    /// text once.
    #[test]
    fn a_live_turn_keeps_the_card_preview_until_it_settles() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("in-memory store"),
            goble_desktop_service::ThreadStore::new(dir.path()).expect("thread store"),
        );
        let chat = desktop.create_chat("Ada", None, None).expect("create chat");
        desktop
            .add_chat_message(&chat, "assistant", "the settled reply")
            .expect("settled reply");

        let mut state = UiState::mock();
        state
            .pane_sessions
            .get_mut(&1)
            .expect("pane 1")
            .conversation_id = chat.clone();
        state.conversations =
            vec![
                ConversationEntry::new(chat.clone(), "Ada", "the settled reply", "10:42")
                    .with_has_agent_reply(true),
            ];
        state.begin_turn(1);

        // The harness writes the reply as its deltas arrive.
        desktop
            .add_chat_message(&chat, "assistant", "the reply, half")
            .expect("streamed reply");
        state.refresh_conversations(&desktop);
        assert_eq!(
            state.conversations[0].last_response, "the settled reply",
            "a live turn does not rewrite the card's preview"
        );

        state.finish_turn(1, TurnOutcome::Success);
        state.refresh_conversations(&desktop);
        assert_eq!(
            state.conversations[0].last_response, "the reply, half",
            "the settled refresh reads the finished transcript"
        );
    }

    /// A harness conversation the user named is not looked up in the harness's
    /// session store at all: the store is the filesystem, and a name the user
    /// typed is not the harness's to replace.
    #[test]
    fn a_harness_conversation_named_by_the_user_is_not_looked_up() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("in-memory store"),
            goble_desktop_service::ThreadStore::new(dir.path()).expect("thread store"),
        );
        let chat = desktop
            .create_chat("Planul de lansare", Some("grok"), None)
            .expect("harness chat");
        desktop
            .set_chat_working_dir(&chat, "/Users/me/Projects/goble")
            .expect("working dir");

        let mut state = UiState::mock();
        state.refresh_conversations(&desktop);

        let card = state
            .conversations
            .iter()
            .find(|entry| entry.id == chat)
            .expect("the harness card");
        assert_eq!(card.name, "Planul de lansare");
        assert!(
            state.harness_title_attempts.is_empty(),
            "a named conversation is not read from the harness store"
        );
    }

    /// A harness conversation is opened without a directory of its own, so the
    /// pane that ran the harness names where its session store is read.
    #[test]
    fn a_harness_conversation_is_looked_up_where_its_pane_runs() {
        let mut state = UiState::mock();
        let harness = goble_core::harness::harness_by_id("grok").expect("grok harness");
        {
            let pane = state.pane_sessions.get_mut(&1).expect("pane 1");
            pane.conversation_id = "harness-chat".to_string();
            pane.path = "/Users/me/Projects/goble".to_string();
        }
        let chat = goble_desktop_service::Chat {
            id: "harness-chat".to_string(),
            title: "Grok".to_string(),
            provider: Some("grok".to_string()),
            model: None,
            agent_id: None,
            worker_id: None,
            workspace_routing: None,
            working_dir: None,
            updated_at: String::new(),
        };
        assert_eq!(
            state.harness_directory(&chat, harness, &[]),
            "/Users/me/Projects/goble",
            "the pane's directory names the harness conversation"
        );

        let stored = goble_desktop_service::Chat {
            working_dir: Some("/Users/me/Projects/other".to_string()),
            ..chat.clone()
        };
        assert_eq!(
            state.harness_directory(&stored, harness, &[]),
            "/Users/me/Projects/other",
            "a conversation that recorded its own directory keeps it"
        );

        let unbound = goble_desktop_service::Chat {
            id: "no-pane".to_string(),
            ..chat.clone()
        };
        assert_eq!(
            state.harness_directory(&unbound, harness, &[]),
            "",
            "a conversation no pane holds names no directory"
        );

        // A harness card that never recorded a directory takes the one its
        // siblings use, so its session store is still reachable.
        let sibling = goble_desktop_service::Chat {
            id: "sibling".to_string(),
            working_dir: Some("/Users/me/Projects/goble".to_string()),
            ..chat.clone()
        };
        assert_eq!(
            state.harness_directory(&unbound, harness, &[chat.clone(), sibling]),
            "/Users/me/Projects/goble",
            "a lone sibling directory names the unbound harness conversation"
        );

        let home = crate::state::home_directory().expect("a home directory");
        let legacy = goble_desktop_service::Chat {
            id: "legacy".to_string(),
            title: format!(
                "Grok · {}/Projects/goble",
                crate::state::display_path(&home)
            ),
            ..chat.clone()
        };
        assert_eq!(
            state.harness_directory(&legacy, harness, &[]),
            format!("{}/Projects/goble", home.trim_end_matches('/')),
            "the old title still names the directory its session store is read with"
        );
    }
}
