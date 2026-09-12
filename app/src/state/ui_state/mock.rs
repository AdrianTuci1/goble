use super::*;

impl UiState {
    /// Test fixture: a populated mock workspace. Used only by tests; the app
    /// always builds from the real store via [`Self::from_desktop`].
    pub fn mock() -> Self {
        let conversations = vec![
            ConversationEntry::new("c1", "Ada", "Let's ship hot reload today", "10:42")
                .with_status(ConversationStatus::Success)
                .with_folder("Frontend"),
            ConversationEntry::new("c2", "Coder", "PR #12 is merged", "09:30")
                .with_status(ConversationStatus::Success)
                .with_folder("Frontend"),
            ConversationEntry::new("c3", "Ops", "Worker deployment done", "Yesterday")
                .with_status(ConversationStatus::Default)
                .with_folder("Infra"),
            ConversationEntry::new("c4", "Research", "Drafting the plan", "2 days ago")
                .with_status(ConversationStatus::Error)
                .with_folder("Research"),
        ];

        let chat_messages = vec![
            ChatMessage::from_markdown(ChatRole::User, "Salut! Cum legăm goble-ui de app?"),
            ChatMessage::new(
                ChatRole::Assistant,
                vec![
                    ChatFragment::text("Am pornit aplicația:"),
                    ChatFragment::terminal(
                        TerminalData::new(
                            "cargo run",
                            vec![
                                TerminalLine::command("cargo run"),
                                TerminalLine::output("Compiling goble-ui v0.1.0"),
                                TerminalLine::output("Finished `dev` profile in 1.2s"),
                                TerminalLine::success("Running `target/debug/goble`"),
                            ],
                        )
                        .with_status(TerminalStatus::Success),
                    ),
                ],
            ),
            ChatMessage::from_markdown(
                ChatRole::Assistant,
                "UI-ul e construit în `app` — modificările apar la recompilare.",
            ),
        ];

        // Snapshot of the mock transcript for pane 1's runtime, kept separate
        // from the global `chat_messages` field that the struct literal moves.
        let pane_mock_messages = chat_messages.clone();

        let crons = vec![
            CronEntry::new("cr1", "Daily digest", "0 9 * * *", "Today 09:00"),
            CronEntry::new(
                "cr2",
                "Nightly vault backup",
                "0 2 * * *",
                "Yesterday 02:00",
            )
            .with_enabled(false),
            CronEntry::new("cr3", "Weekly report", "0 18 * * 5", "Last Friday 18:00"),
        ];

        Self {
            current_tab: AppTab::Chat,
            selected_id: conversations.first().map(|c| c.id.clone()),
            conversations,
            search_query: String::new(),
            search_focused: false,
            new_conversation_draft: String::new(),
            create_focused: false,
            chat_messages,
            pending_ask: None,
            queued_prompt: None,
            composer_draft: String::new(),
            composer_focused: false,
            models: vec![
                "goble-agent".to_string(),
                "goble-agent (fast)".to_string(),
                "goble-agent (reasoning)".to_string(),
            ],
            selected_model: "goble-agent".to_string(),
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
            crons,
            workflows: Vec::new(),
            executions: Vec::new(),
            tasks: Vec::new(),
            timeline: Vec::new(),
            costs: Vec::new(),
            settings_page: SettingsPage::Profile,
            settings_profile_name: "Ada".to_string(),
            settings_profile_email: "ada@example.com".to_string(),
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
                conversation_id: "c1".to_string(),
                draft: String::new(),
                path: current_dir_display(),
            })]),
            pane_runtime: HashMap::from([(
                1u64,
                PaneRuntime {
                    messages: pane_mock_messages,
                    parse_cache: MessageParseCache::default(),
                    pending_ask: None,
                    pending_command: None,
                    command_selection: None,
                    queued_prompt: None,
                    busy: false,
                    turn_started_at: None,
                    inline_screen_source: None,
                    screen_link: None,
                    in_flight_tools: HashMap::new(),
                    sub_agents: HashMap::new(),
                    reasoning: Vec::new(),
                },
            )]),
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
        }
    }
}
