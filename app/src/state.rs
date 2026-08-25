//! App-owned UI state.
//!
//! The element tree is rebuilt from this state on every frame, so state lives
//! here (in the executable) instead of inside the hot-reloaded dylib. That
//! keeps text input focus/value across rebuilds and survives library swaps.
//!
//! This module owns the data only — the callbacks that mutate it live in
//! [`crate::actions`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use chrono::{DateTime, Utc};
use goble_core::agent::Trigger;
use goble_desktop_service::DesktopState;
use goble_ui::{
    AgentCardUi, ChatFragment, ChatMessage, ChatRole, ConversationEntry, ConversationStatus,
    SettingsPage, TerminalData, TerminalLine, TerminalStatus,
};

use crate::hot_ui::{AppTab, CronEntry};
use goble_ui_hot::SIDEBAR_WIDTH;

/// Format an RFC3339 timestamp as a short relative "time ago" label (e.g.
/// "40 min ago"). Falls back to the raw string when it cannot be parsed.
fn time_ago(updated_at: &str) -> String {
    match DateTime::parse_from_rfc3339(updated_at) {
        Ok(ts) => {
            let now = Utc::now();
            let dur = now.signed_duration_since(ts.with_timezone(&Utc));
            let secs = dur.num_seconds().max(0);
            if secs < 60 {
                "just now".to_string()
            } else if secs < 3600 {
                format!("{} min ago", secs / 60)
            } else if secs < 86400 {
                format!("{}h ago", secs / 3600)
            } else if secs < 172800 {
                "Yesterday".to_string()
            } else {
                format!("{} days ago", secs / 86400)
            }
        }
        Err(_) => updated_at.to_string(),
    }
}

#[derive(Clone)]
pub struct UiState {
    pub current_tab: AppTab,
    pub conversations: Vec<ConversationEntry>,
    pub selected_id: Option<String>,
    pub search_query: String,
    pub search_focused: bool,
    pub new_conversation_draft: String,
    pub create_focused: bool,
    pub thread_messages: Vec<ChatMessage>,
    pub chat_messages: Vec<ChatMessage>,
    pub composer_draft: String,
    pub composer_focused: bool,
    /// Model choices shown in the composer's model dropdown.
    pub models: Vec<String>,
    /// Currently selected model (shown as the composer's model label).
    pub selected_model: String,
    /// App-owned open flags for the composer model / account menus.
    pub model_menu_open: Rc<RefCell<bool>>,
    pub profile_menu_open: Rc<RefCell<bool>>,
    pub agent_name: String,
    pub agent_busy: bool,
    pub right_sidebar_open: bool,
    pub crons_open: bool,
    pub crons: Vec<CronEntry>,
    pub settings_page: SettingsPage,
    pub settings_profile_name: String,
    pub settings_profile_email: String,
    pub settings_dark_mode: bool,
    pub settings_llm_provider: String,
    pub settings_llm_model: String,
    pub settings_llm_api_key: String,
    pub settings_llm_base_url: String,
    pub settings_llm_temperature: String,
    pub settings_workers: Vec<(String, String, String, bool)>,
    pub settings_cluster_name: String,
    pub settings_cluster_configured: bool,
    pub settings_authorized_keys: Vec<(String, String, String)>,
    pub settings_vault_unlocked: bool,
    pub sidebar_width: f32,
    pub sidebar_dragging: bool,
    pub sidebar_drag_origin_x: f32,
    pub sidebar_drag_start_width: f32,
    /// Per-card interaction state (hover / delete menu), owned here so it
    /// survives the per-frame element rebuild. Keyed by conversation id.
    pub agent_cards: HashMap<String, Rc<RefCell<AgentCardUi>>>,
    /// Hover flag for the sidebar's "New agent" row, owned here so the row
    /// highlight survives the per-frame element rebuild.
    pub new_agent_hover: Rc<RefCell<bool>>,
}

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
            thread_messages: Vec::new(),
            chat_messages: Vec::new(),
            composer_draft: String::new(),
            composer_focused: false,
            models: vec![
                "goble-agent".to_string(),
                "goble-agent (fast)".to_string(),
                "goble-agent (reasoning)".to_string(),
            ],
            selected_model: "goble-agent".to_string(),
            model_menu_open: Rc::new(RefCell::new(false)),
            profile_menu_open: Rc::new(RefCell::new(false)),
            agent_name: "Goble Agent".to_string(),
            agent_busy: false,
            right_sidebar_open: false,
            crons_open: false,
            crons: Vec::new(),
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
            sidebar_width: SIDEBAR_WIDTH,
            sidebar_dragging: false,
            sidebar_drag_origin_x: 0.0,
            sidebar_drag_start_width: SIDEBAR_WIDTH,
            agent_cards: HashMap::new(),
            new_agent_hover: Rc::new(RefCell::new(false)),
        };
        state.refresh_from_desktop(desktop);
        state
    }

    /// Reload conversations, messages, crons and agent name from the backend.
    pub fn refresh_from_desktop(&mut self, desktop: &DesktopState) {
        self.refresh_conversations(desktop);
        self.refresh_crons(desktop);
        self.refresh_agent_name(desktop);
        self.refresh_settings(desktop);
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

    pub fn refresh_messages(&mut self, desktop: &DesktopState) {
        let Some(chat_id) = self.selected_id.clone() else {
            self.chat_messages.clear();
            return;
        };
        match desktop.list_chat_messages(&chat_id) {
            Ok(msgs) => {
                self.chat_messages = msgs
                    .into_iter()
                    .filter_map(|m| {
                        let role = match m.role.as_str() {
                            "user" => ChatRole::User,
                            _ => ChatRole::Assistant,
                        };
                        Some(ChatMessage::from_markdown(role, m.content))
                    })
                    .collect();
            }
            Err(e) => {
                log::warn!("list_chat_messages({chat_id}): {e}");
            }
        }
    }

    pub fn refresh_crons(&mut self, desktop: &DesktopState) {
        self.crons = desktop
            .list_workflows()
            .into_iter()
            .map(|wf| {
                let schedule = match wf.trigger {
                    Trigger::Cron { expression } => expression,
                    _ => "manual".to_string(),
                };
                CronEntry::new(wf.id, wf.name, schedule, "unknown").with_enabled(wf.enabled)
            })
            .collect();
    }

    pub fn refresh_agent_name(&mut self, desktop: &DesktopState) {
        if let Some(name) = desktop.list_agents().first().map(|a| a.name.clone()) {
            self.agent_name = name;
        }
    }

    /// Reload settings data (workers, cluster, vault, LLM) from the backend.
    pub fn refresh_settings(&mut self, desktop: &DesktopState) {
        self.settings_workers = desktop
            .list_workers()
            .into_iter()
            .map(|w| (w.id.clone(), w.name.clone(), w.url.clone(), w.paired))
            .collect();
        if let Some(identity) = desktop.get_cluster_identity() {
            self.settings_cluster_name = identity.cluster_name.clone();
            self.settings_cluster_configured = true;
        } else {
            self.settings_cluster_configured = false;
        }
        self.settings_vault_unlocked = desktop.is_vault_unlocked();
        if let Some(s) = desktop.get_llm_setting("openai") {
            self.settings_llm_provider = "openai".to_string();
            self.settings_llm_model = s.model;
            self.settings_llm_api_key = s.api_key;
            self.settings_llm_base_url = s.base_url.unwrap_or_default();
            if let Some(t) = s.temperature {
                self.settings_llm_temperature = t.to_string();
            }
        }
    }

    /// Mock data used when the backend store cannot be opened (dev fallback).
    pub fn mock() -> Self {
        let conversations = vec![
            ConversationEntry::new("c1", "Ada", "Let's ship hot reload today", "10:42")
                .with_status(ConversationStatus::Success),
            ConversationEntry::new("c2", "Coder", "PR #12 is merged", "09:30")
                .with_status(ConversationStatus::Success),
            ConversationEntry::new("c3", "Ops", "Worker deployment done", "Yesterday")
                .with_status(ConversationStatus::Default),
            ConversationEntry::new("c4", "Research", "Drafting the plan", "2 days ago")
                .with_status(ConversationStatus::Error),
        ];

        let thread_messages = vec![ChatMessage::from_markdown(
            ChatRole::Assistant,
            "Bine ai venit! Selectează o conversație din sidebar.",
        )];
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
                "UI-ul rulează cu hot reload — modificările din `goble-ui-hot` apar instant.",
            ),
        ];

        let crons = vec![
            CronEntry::new("cr1", "Daily digest", "0 9 * * *", "Today 09:00"),
            CronEntry::new("cr2", "Nightly vault backup", "0 2 * * *", "Yesterday 02:00")
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
            thread_messages,
            chat_messages,
            composer_draft: String::new(),
            composer_focused: false,
            models: vec![
                "goble-agent".to_string(),
                "goble-agent (fast)".to_string(),
                "goble-agent (reasoning)".to_string(),
            ],
            selected_model: "goble-agent".to_string(),
            model_menu_open: Rc::new(RefCell::new(false)),
            profile_menu_open: Rc::new(RefCell::new(false)),
            agent_name: "Goble Agent".to_string(),
            agent_busy: false,
            right_sidebar_open: false,
            crons_open: false,
            crons,
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
            sidebar_width: SIDEBAR_WIDTH,
            sidebar_dragging: false,
            sidebar_drag_origin_x: 0.0,
            sidebar_drag_start_width: SIDEBAR_WIDTH,
            agent_cards: HashMap::new(),
            new_agent_hover: Rc::new(RefCell::new(false)),
        }
    }
}
