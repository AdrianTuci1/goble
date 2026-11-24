/// 360: the first real launch starts with the sidebar closed; later launches
/// respect the persisted preference. Test fixtures default open so existing
/// layout assertions keep their historical shape.
fn initial_sidebar_visible(desktop: &DesktopState) -> bool {
    #[cfg(test)]
    {
        let _ = desktop;
        true
    }
    #[cfg(not(test))]
    {
        desktop.get_ui_sidebar_visible().unwrap_or(false)
    }
}

use super::session::is_unnamed_harness_title;
use super::title::deterministic_subject;
use super::*;

impl UiState {
    /// Keep the side panel's tab list and selection usable. The list is never
    /// empty — closing the last tab brings Overview back — and an active index
    /// left out of range (a test fixture, a stale layout) falls back to the
    /// first tab instead of drawing nothing.
    pub fn ensure_side_tabs(&mut self) {
        if self.active_side_tab >= self.side_tabs.len() {
            self.active_side_tab = 0;
        }
    }

    /// The tab the side panel is showing. Overview is the fallback, so the
    /// panel has a surface even before the first [`Self::ensure_side_tabs`].
    pub fn active_side_tab_kind(&self) -> SideTabKind {
        self.side_tabs
            .get(self.active_side_tab)
            .copied()
            .unwrap_or(SideTabKind::Overview)
    }

    /// Y: open `path` in the side panel's Files tab. No pane is split or
    /// retargeted: the tab reads the file under `SIDE_FILE_PANE_ID`.
    pub fn open_file_side(&mut self, path: String) {
        self.last_opened_file = Some(path.clone());
        self.side_files_show_explorer = false;
        self.side_file = Some(path);
        self.add_side_tab(super::SideTabKind::Files);
        self.right_sidebar_open = true;
    }

    /// Open the Files tab without naming a file (the "Open File" action keeps
    /// whatever the tab was showing, or its honest empty state).
    pub fn open_files_tab(&mut self) {
        // The Files control opens the panel's own tree, not a pane beside the
        // conversation: any main-pane Explorer left from the old flow is
        // closed first, so the tree lives in exactly one place.
        let mut leaves = Vec::new();
        if let Some(space) = self.spaces.get(self.active_space) {
            space.root.leaves(&mut leaves);
        }
        let explorers: Vec<u64> = leaves
            .into_iter()
            .filter(|(_, kind)| matches!(kind, crate::state::PaneKind::Explorer))
            .map(|(id, _)| id)
            .collect();
        for id in explorers {
            self.close_pane(id, None);
        }
        self.side_files_show_explorer = true;
        self.add_side_tab(super::SideTabKind::Files);
        self.right_sidebar_open = true;
    }

    /// The Files tab always opens onto the file the user last touched or the
    /// file the active pane shows, so it is never an empty placeholder when a
    /// file exists to show.
    fn hydrate_side_file(&mut self) {
        if self.side_file.is_some() {
            return;
        }
        if let Some(path) = self.last_opened_file.clone() {
            self.side_file = Some(path);
            return;
        }
        let active = self.active_pane_id;
        if let Some(crate::state::PaneKind::File { path }) = self
            .spaces
            .get(self.active_space)
            .and_then(|space| space.leaf_kind(active))
        {
            self.side_file = Some(path.clone());
        }
    }

    /// The side panel was opened: keep the tabs and land on the one that was
    /// active. The Overview surface is the body's home when there is no tab,
    /// but it is not a chip and opening the panel must not add one.
    pub fn open_side_panel(&mut self) {
        if self.side_tabs.is_empty() {
            self.active_side_tab = 0;
            return;
        }
        self.active_side_tab = self.active_side_tab.min(self.side_tabs.len() - 1);
    }

    /// Make the tab at `index` the panel's surface. An index outside the strip
    /// is ignored rather than selecting a tab that is not there.
    pub fn select_side_tab(&mut self, index: usize) {
        if index < self.side_tabs.len() {
            if self.side_tabs[index] == super::SideTabKind::Files {
                self.hydrate_side_file();
            }
            self.active_side_tab = index;
        }
    }

    /// Add a tab kind, or show it when it is already in the strip: one tab per
    /// kind, so the "+" menu never stacks duplicates of the same pane view.
    pub fn add_side_tab(&mut self, kind: SideTabKind) {
        if kind == SideTabKind::Files {
            self.hydrate_side_file();
        }
        *self.side_tab_add_menu_open.borrow_mut() = false;
        if kind != SideTabKind::NewTab {
            if let Some(index) = self
                .side_tabs
                .iter()
                .position(|tab| *tab == SideTabKind::NewTab)
            {
                if index == self.active_side_tab {
                    self.side_tabs[index] = kind;
                    return;
                }
            }
        }
        match self.side_tabs.iter().position(|tab| *tab == kind) {
            Some(index) => self.active_side_tab = index,
            None => {
                self.side_tabs.push(kind);
                self.active_side_tab = self.side_tabs.len() - 1;
            }
        }
    }

    /// 297: move a tab, keeping the active tab selected.
    pub fn reorder_side_tab(&mut self, from: usize, to: usize) {
        if from >= self.side_tabs.len() || to >= self.side_tabs.len() || from == to {
            return;
        }
        let active = self.side_tabs.get(self.active_side_tab).copied();
        let tab = self.side_tabs.remove(from);
        self.side_tabs.insert(to, tab);
        if let Some(active) = active {
            self.active_side_tab = self
                .side_tabs
                .iter()
                .position(|tab| *tab == active)
                .unwrap_or(0);
        }
    }

    /// Close the tab at `index`. Closing the active tab selects its neighbour —
    /// the next one when there is one, the previous one otherwise — and closing
    /// the last tab brings Overview back, so the panel always has a surface.
    pub fn close_side_tab(&mut self, index: usize) {
        if index >= self.side_tabs.len() {
            return;
        }
        self.side_tabs.remove(index);
        if self.side_tabs.is_empty() {
            self.active_side_tab = 0;
            return;
        }
        if index < self.active_side_tab {
            self.active_side_tab -= 1;
        } else if index == self.active_side_tab {
            self.active_side_tab = index.min(self.side_tabs.len() - 1);
        }
    }

    /// Move the selection by `step` tabs, wrapping at both ends. A horizontal
    /// swipe on the panel sends one step per completed gesture, so a swipe past
    /// the last tab returns to the first.
    pub fn cycle_side_tab(&mut self, step: i32) {
        self.ensure_side_tabs();
        let len = self.side_tabs.len();
        if len <= 1 || step == 0 {
            return;
        }
        self.active_side_tab = (self.active_side_tab as i32 + step).rem_euclid(len as i32) as usize;
    }

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
            sidebar_search_caret: Rc::new(RefCell::new(0)),
            new_conversation_draft: String::new(),
            create_focused: false,
            chat_messages: Vec::new(),
            pending_ask: None,
            queued_prompt: None,
            composer_draft: String::new(),
            composer_focused: false,
            side_composer_focused: false,
            models: Vec::new(),
            model_groups: Vec::new(),
            composer_catalog: goble_harness_types::HarnessCatalog::empty(),
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
            work_dock_open: Rc::new(RefCell::new(HashMap::new())),
            work_dock_show_all: Rc::new(RefCell::new(HashMap::new())),
            maximized_pane: None,
            terminal_filters: Rc::new(RefCell::new(HashMap::new())),
            reasoning_expanded: Rc::new(RefCell::new(HashMap::new())),
            tool_fold: Rc::new(RefCell::new(HashMap::new())),
            tool_hover: Rc::new(RefCell::new(None)),
            terminal_global_filters: HashMap::new(),
            crons_open: false,
            workflow_views: HashMap::new(),
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
            settings_profile_name: String::new(),
            settings_dark_mode: false,
            settings_llm_provider: "openai".to_string(),
            settings_llm_model: "gpt-4o".to_string(),
            settings_llm_api_key: String::new(),
            settings_llm_base_url: String::new(),
            settings_llm_temperature: "0.7".to_string(),
            settings_workers: Vec::new(),
            settings: crate::ui::settings::SettingsUiState::default(),
            update: None,
            update_view: crate::update::UpdateView::default(),
            vim_mode: false,
            auto_save_on_close: false,
            pending_close_pane: None,
            pending_close_file: None,
            conversation_usage: HashMap::new(),
            conversation_last_usage: HashMap::new(),
            conversation_usage_calls: HashMap::new(),
            theme_primary: None,
            theme_secondary: None,
            theme_accent: None,
            theme_bg: None,
            theme_surface: None,
            theme_preset: None,
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
            remote_deploy: RemoteDeploy::default(),
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
            side_split_ratio: 0.5,
            side_split_dragging: false,
            content_lane_prefs: std::collections::HashMap::new(),
            content_lane_drag_pane: None,
            content_lane_dragging: false,
            content_lane_drag_edge: None,
            content_lane_drag_origin_x: 0.0,
            content_lane_drag_start_width: 0.0,
            content_lane_drag_column: 0.0,
            content_lane_drag_start_pref: None,
            content_lane_drag_moved: false,
            sidebar_visible: initial_sidebar_visible(desktop),
            responsive: Responsive::default(),
            responsive_narrow: false,
            sidebar_narrow_expanded: false,
            responsive_width: 0.0,
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
            global_search_caret: Rc::new(RefCell::new(0)),
            global_search_worker: Rc::new(SearchWorker::new()),
            sidebar_scroll: Rc::new(RefCell::new(ScrollState::default())),
            explorer_hover: Rc::new(RefCell::new(None)),
            sidebar_hover_row: Rc::new(RefCell::new(None)),
            sidebar_focus_row: Rc::new(RefCell::new(None)),
            sidebar_menu_open: Rc::new(RefCell::new(false)),
            sidebar_flat_list: Rc::new(RefCell::new(false)),
            sidebar_recent_first: Rc::new(RefCell::new(true)),
            sidebar_archived_filter: Rc::new(RefCell::new(crate::ui::ArchivedFilter::Default)),
            sidebar_rename: None,
            sidebar_workspaces: Vec::new(),
            sidebar_workspace_rename: None,
            sidebar_group_ui: Rc::new(RefCell::new(HashMap::new())),
            archive_confirm: None,
            toast: None,
            toast_until: None,
            next_toast_seq: 0,
            sidebar_drag: Rc::new(RefCell::new(Default::default())),
            sidebar_session_order: Rc::new(RefCell::new(std::collections::HashMap::new())),
            explorer_scroll: Rc::new(RefCell::new(ScrollState::default())),
            global_search_scroll: Rc::new(RefCell::new(ScrollState::default())),
            spotlight: crate::ui::spotlight::SpotlightState::default(),
            space_rename_editing: false,
            command_held: false,
            space_press_run: Rc::new(RefCell::new(None)),
            space_rename_draft: String::new(),
            space_rename_focused: false,
            space_menu: Rc::new(RefCell::new(None)),
            file_menu: Rc::new(RefCell::new(None)),
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
            ssh_target_open: false,
            ssh_target_draft: String::new(),
            ssh_target_focused: false,
            ssh_config_path: String::new(),
            ssh_config_hosts: Vec::new(),
            ssh_target_error: None,
            sidebar_add_menu_open: Rc::new(RefCell::new(false)),
            conversations_add_menu_open: Rc::new(RefCell::new(false)),
            automations_open: false,
            automations_highlight: 0,
            project_menu_open: Rc::new(RefCell::new(false)),
            changed_files_open: Rc::new(RefCell::new(false)),
            settings_overlay_open: false,
            settings_overlay_hamburger: Rc::new(RefCell::new(false)),
            settings_overlay_wide: true,
            chrome_overview_open: Rc::new(RefCell::new(false)),
            chrome_more_open: Rc::new(RefCell::new(false)),
            side_panel_maximized: false,
            side_tabs: Vec::new(),
            side_review: None,
            side_file: None,
            last_opened_file: None,
            side_files_show_explorer: false,
            active_side_tab: 0,
            side_rail_open: true,
            side_tab_add_menu_open: Rc::new(RefCell::new(false)),
            side_swipe_accum: Rc::new(RefCell::new(0.0)),
            send_immediately: false,
            ssh_reconnect_prompt: None,
            floating_drag: Rc::new(RefCell::new(None)),
            live_executions: HashMap::new(),
            pane_controls: HashMap::new(),
            diff_reviews: Rc::new(RefCell::new(HashMap::new())),
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
            open_menu: None,
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
        self.refresh_workspaces(desktop);
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

    /// Workflow runs from the persisted ledger, newest first: the live ones —
    /// recorded when they start — and the settled ones.
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
                // A settled run's duration is its whole span; a live one is
                // measured to now, so its clock keeps moving.
                elapsed: match run.finished_at.as_deref() {
                    Some(finished_at) => elapsed_between(&run.started_at, finished_at),
                    None => elapsed_since(&run.started_at),
                },
                // A live run's clock keeps moving from the moment the ledger
                // was read; a settled run's span is final.
                elapsed_measured_at: run.finished_at.is_none().then(std::time::Instant::now),
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
            // DSH separates "running" (the spinner ring) from "stopped on the
            // user" (the amber dot): a pending approval/question is the latter,
            // and it outranks the turn's own busy flag.
            let waiting_on_user = rt.pending_command.is_some() || rt.pending_ask.is_some();
            if waiting_on_user || rt.agent_state == PaneAgentState::NeedsInput {
                return ConversationStatus::Waiting;
            }
            if rt.busy || rt.agent_state == PaneAgentState::Working {
                return ConversationStatus::Running;
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
    pub(crate) fn conversation_is_live(&self, conversation_id: &str) -> bool {
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
                if let Err(e) =
                    desktop.set_chat_working_dir(&chat.id, &crate::state::display_path(&directory))
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

    /// Name a conversation from the first prompt the user sends into it.
    ///
    /// A conversation is created under [`NEW_CONVERSATION_TITLE`], and the
    /// first thing the user says in it is what it is about. Naming it from that
    /// prompt as the turn starts is what gives a conversation a subject while it
    /// is still new: the sidebar row, the tab label and the pane's own bar read
    /// a real name instead of the placeholder for the whole first turn (and no
    /// reply ever names a conversation).
    ///
    /// The name written here is the deterministic subject
    /// ([`deterministic_subject`]): the topic keywords of the prompt, not the
    /// prompt itself. It is the fallback that stands when no provider key exists,
    /// and the name the conversation carries while the model-backed title
    /// request ([`spawn_title_generation`]) is in flight; that request replaces
    /// it with a generated subject when it returns.
    ///
    /// Only the placeholder is replaced. A subject the user typed (in the create
    /// row, or through the sidebar's rename row) is theirs and stands, and a
    /// conversation already named keeps that name — the first prompt is the one
    /// that names a conversation.
    ///
    /// The write reaches the store, the in-memory list the sidebar is built
    /// from, and the frontends through the `chats:updated` it emits, so the card,
    /// the tab and the pane bar update without a restart. The local row and tab
    /// label are also updated at once, so this frame already names the
    /// conversation.
    ///
    /// Returns whether this was the conversation's first prompt (whether it was
    /// still under the placeholder), which is what tells the caller to ask the
    /// model for a subject. A prompt with nothing to name in it (`thanks!`)
    /// writes nothing but still reports the first prompt.
    pub fn name_conversation_from_prompt(
        &mut self,
        conversation_id: &str,
        prompt: &str,
        desktop: &DesktopState,
    ) -> bool {
        if conversation_id.is_empty() {
            return false;
        }
        let Some(chat) = desktop
            .list_chats()
            .into_iter()
            .find(|chat| chat.id == conversation_id)
        else {
            return false;
        };
        // Only the placeholder is the agent's to replace: anything else was
        // typed by the user or written by an earlier naming pass.
        if chat.title != NEW_CONVERSATION_TITLE {
            return false;
        }
        let subject = deterministic_subject(prompt);
        if let Some(subject) = subject
            .as_deref()
            .filter(|subject| *subject != NEW_CONVERSATION_TITLE)
        {
            if let Err(e) = desktop.set_chat_title(conversation_id, subject) {
                log::warn!("set_chat_title for {conversation_id} failed: {e}");
                return true;
            }
            // The card the frame draws reads `conversations`, so the row reads
            // the new name now rather than at the refresh the write emits; the
            // tab and the pane bar follow the card.
            if let Some(entry) = self
                .conversations
                .iter_mut()
                .find(|entry| entry.id == conversation_id)
            {
                entry.name = subject.to_string();
            }
            self.refresh_space_labels();
        }
        true
    }

    /// Rebuild the sidebar's conversation list from the service.
    ///
    /// A conversation is named from its first prompt, not from its replies: the
    /// subject is written at turn start by
    /// [`Self::name_conversation_from_prompt`] (and refined by the model-backed
    /// title request). This pass only reads the durable titles back, so a
    /// conversation keeps the subject it was given however many turns follow.
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
        // The session menu's two durable flags live beside the title in the
        // store: the archived filter reads one set, the Starred block the
        // other, and both are loaded once here rather than per row.
        let archived: std::collections::HashSet<String> = desktop
            .archived_chat_ids()
            .unwrap_or_default()
            .into_iter()
            .collect();
        let starred: std::collections::HashSet<String> = desktop
            .starred_chat_ids()
            .unwrap_or_default()
            .into_iter()
            .collect();
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
                let title = retitled
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
                        (last, has_agent_reply)
                    }
                };
                ConversationEntry::new(c.id.clone(), title, last, time_ago(&c.updated_at))
                    // The Workspaces group the row is listed under: the last
                    // segment of the directory it works in, so conversations
                    // sit under the same "workspace" the DSH sidebar shows.
                    .with_folder(conversation_group(c.working_dir.as_deref()))
                    // The raw stamp the row formats itself (the compact
                    // relative time); the preformatted `timestamp` above stays
                    // for the dashboard and the older card.
                    .with_updated_at(c.updated_at.clone())
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
                    // The row's archived styling/behaviour and the persisted
                    // pin, both read from the store this pass.
                    .with_archived(archived.contains(&c.id))
                    .with_starred(starred.contains(&c.id))
            })
            .collect();
        // The Starred block is the persisted pin: keep the UI set in step with
        // the store so a restart restores it and a menu toggle (which writes
        // through the service) is reflected on the next refresh.
        self.starred_conversations = starred;
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

        // A turn the app issued is running: the sidebar draws the running ring.
        state.begin_turn(1);
        assert_eq!(
            state.conversation_status("c1", true),
            ConversationStatus::Running
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
        assert_eq!(card(&state, "c1"), Some(ConversationStatus::Running));
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

    /// The whole path a new conversation's subject takes: it opens under the
    /// placeholder, its first prompt names it at once with a deterministic
    /// subject (a topic, not the message), the write is durable and the sidebar
    /// entry, the tab label and the pane's own bar all read the subject. The
    /// model-backed title request then replaces the fallback with a generated
    /// subject; later turns leave that subject alone, and a name the user typed
    /// is never taken back.
    #[test]
    fn a_new_conversation_is_named_from_its_first_prompt() {
        use goble_core::llm::{CompletionResponse, MockProvider};

        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("in-memory store"),
            goble_desktop_service::ThreadStore::new(dir.path()).expect("thread store"),
        );

        // A tab the sidebar's "New conversation" row opens: one pane on a
        // conversation created under the placeholder.
        let mut state = UiState::mock();
        let pane_id = state.active_pane_id;
        state.bind_pane_new_conversation_titled(pane_id, NEW_CONVERSATION_TITLE, Some(&desktop));
        let chat = state
            .pane_conversation_id(pane_id)
            .expect("the tab's conversation");
        state.refresh_space_labels();
        assert_eq!(
            state.spaces[state.active_space].name, NEW_AGENT_TAB_LABEL,
            "a conversation nobody has named opens on the placeholder tab label"
        );
        assert_eq!(state.pane_subject(pane_id), NEW_AGENT_TAB_LABEL);

        // The first prompt names it at once — there is no assistant reply in the
        // store yet — with the deterministic subject, which is the topic and not
        // the message.
        let prompt = "Arata un text colorat in chat";
        assert!(
            state.name_conversation_from_prompt(&chat, prompt, &desktop),
            "the first prompt is the one that names the conversation"
        );
        let fallback = "Text colorat in chat";
        assert_eq!(
            desktop.list_chats()[0].title,
            fallback,
            "the deterministic subject is written to the store"
        );
        assert_ne!(
            desktop.list_chats()[0].title,
            prompt,
            "the title is the topic, not the user's message"
        );

        // The `chats:updated` the write emits is what rebuilds the list: the
        // entry, the tab label and the pane bar all read the subject.
        state.refresh_conversations(&desktop);
        state.refresh_space_labels();
        assert_eq!(state.conversations[0].name, fallback);
        assert_eq!(
            state.spaces[state.active_space].name, fallback,
            "the topbar tab reads the conversation subject"
        );
        assert_eq!(
            state.pane_subject(pane_id),
            fallback,
            "the pane header reads the conversation subject"
        );

        // The model-backed request replaces the fallback with the generated
        // subject, and every surface reads the refined title after the refresh
        // the write's `chats:updated` triggers.
        let provider = MockProvider::new(
            "title-test",
            CompletionResponse::new("Colorful chat text", Vec::new()),
        );
        let runtime = tokio::runtime::Runtime::new().expect("build runtime");
        let generated = runtime
            .block_on(super::title::generate_and_store_title(
                &desktop,
                &provider,
                "title-test",
                "title-model",
                &chat,
                prompt,
            ))
            .expect("the generated subject");
        assert_eq!(generated, "Colorful chat text");
        assert_eq!(
            desktop.list_chats()[0].title,
            "Colorful chat text",
            "the generated subject is durable"
        );
        state.refresh_conversations(&desktop);
        state.refresh_space_labels();
        assert_eq!(state.conversations[0].name, "Colorful chat text");
        assert_eq!(state.spaces[state.active_space].name, "Colorful chat text");
        assert_eq!(state.pane_subject(pane_id), "Colorful chat text");

        // Later turns do not retitle the conversation: replies name nothing.
        assert!(
            !state.name_conversation_from_prompt(&chat, "Now make it green", &desktop),
            "a conversation a prompt already named keeps its name"
        );
        desktop
            .add_chat_message(&chat, "user", prompt)
            .expect("the prompt the turn ran");
        desktop
            .add_chat_message(&chat, "assistant", "Textul colorat apare acum in chat.")
            .expect("the agent's reply");
        state.refresh_conversations(&desktop);
        assert_eq!(
            state.conversations[0].name, "Colorful chat text",
            "the settled turn leaves the subject alone"
        );

        // A subject the user typed is never taken back.
        desktop
            .set_chat_title(&chat, "Planul de lansare")
            .expect("the user's rename");
        assert!(
            !state.name_conversation_from_prompt(&chat, "Something else entirely", &desktop),
            "a prompt does not rename a conversation the user named"
        );
        assert_eq!(
            runtime.block_on(super::title::generate_and_store_title(
                &desktop,
                &provider,
                "title-test",
                "title-model",
                &chat,
                prompt,
            )),
            None,
            "a user's name is not the app's to replace"
        );
        state.refresh_conversations(&desktop);
        assert_eq!(
            desktop.list_chats()[0].title,
            "Planul de lansare",
            "the conversation's own words do not take a user's name back"
        );

        // A conversation created under a subject the user typed is left alone
        // from the start — the create row's draft is the user's name.
        let named = desktop
            .create_chat("Ship the topbar", None, None)
            .expect("a conversation the user named");
        assert!(!state.name_conversation_from_prompt(&named, "fix the sidebar", &desktop));
        assert_eq!(
            desktop
                .list_chats()
                .into_iter()
                .find(|chat| chat.id == named)
                .map(|chat| chat.title),
            Some("Ship the topbar".to_string())
        );
    }

    /// A prompt with nothing to name in it leaves the conversation on the
    /// placeholder — a neutral placeholder, not the message — and reports itself
    /// as the first prompt, so the model may still name it. A prompt that does
    /// name a topic replaces the placeholder.
    #[test]
    fn a_pleasantry_does_not_name_a_new_conversation() {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("in-memory store"),
            goble_desktop_service::ThreadStore::new(dir.path()).expect("thread store"),
        );
        let chat = desktop
            .create_chat(NEW_CONVERSATION_TITLE, None, None)
            .expect("a conversation nobody has named");
        let mut state = UiState::mock();
        assert!(
            state.name_conversation_from_prompt(&chat, "Thanks!", &desktop),
            "the pleasantry is still the conversation's first prompt"
        );
        assert!(state.name_conversation_from_prompt(&chat, "   ", &desktop));
        assert_eq!(desktop.list_chats()[0].title, NEW_CONVERSATION_TITLE);

        assert!(state.name_conversation_from_prompt(&chat, "fix the sidebar", &desktop));
        assert_eq!(desktop.list_chats()[0].title, "Sidebar");
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

/// The "workspace" group a conversation is listed under: the last segment of
/// the directory it works in. A conversation that has not run anywhere, one
/// sitting in the home root (which names no project) and one on the filesystem
/// root all fall back to `General`, the folder every conversation used to sit
/// in.
pub fn conversation_group(directory: Option<&str>) -> String {
    let Some(directory) = directory.map(str::trim).filter(|d| !d.is_empty()) else {
        // Conversations that have not run in a directory are listed under a
        // plain "Conversations" heading: no folder icon and no workspace menu.
        return "Conversations".to_string();
    };
    let path = std::path::Path::new(directory);
    let in_home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .is_some_and(|home| path == home);
    if in_home || path.parent().is_none() {
        return "General".to_string();
    }
    match path.file_name().and_then(|name| name.to_str()) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => "General".to_string(),
    }
}
