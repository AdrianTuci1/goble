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
    AgentCardUi, AskUserUi, ChatFragment, ChatMessage, ChatRole, ConversationEntry,
    ConversationStatus, SettingsPage, TerminalData, TerminalLine, TerminalStatus, ToolCall,
};

use crate::hot_ui::{AppTab, CronEntry, LlmFormField, WorkspaceRouting};
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

/// Build a terminal-style block from a stored tool-result message. The harness
/// writes tool output as `"<call_id>\n<output>"`, so the first line becomes the
/// block title and the remaining lines render as mono output (or error) lines.
fn tool_terminal_data(content: &str) -> TerminalData {
    let mut parts = content.splitn(2, '\n');
    let title = parts.next().unwrap_or("tool").trim();
    let body = parts.next().unwrap_or("").trim();
    let has_error = body.contains("ERROR:");

    let mut lines = Vec::new();
    for line in body.lines() {
        let text = line.trim_end().to_string();
        if has_error || text.contains("ERROR:") {
            lines.push(TerminalLine::error(text));
        } else if text.is_empty() {
            lines.push(TerminalLine::info(" "));
        } else {
            lines.push(TerminalLine::output(text));
        }
    }
    if lines.is_empty() {
        lines.push(TerminalLine::info("(no output)"));
    }

    let status = if has_error {
        TerminalStatus::Error
    } else {
        TerminalStatus::Success
    };
    TerminalData::new(if title.is_empty() { "tool" } else { title }.to_string(), lines)
        .with_status(status)
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
    /// A suspended agent question, rendered inline at the end of the transcript.
    /// Set from the `chat:ask_user` event and re-read from the store so it
    /// survives a refresh or app restart.
    pub pending_ask: Option<AskUserUi>,
    /// A prompt sent while the agent was busy, queued so it does not interrupt
    /// the running turn. Shown as a pending block with "Send now" / dismiss.
    pub queued_prompt: Option<String>,
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
    /// Whether the agent auto-approves `ask_user` questions (skip the ask).
    pub auto_approve: bool,
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
    /// First-run: whether the "configure a model key" banner is shown in chat.
    pub show_llm_key_banner: bool,
    /// First-run: whether the "local or remote workspace?" choice is shown.
    pub show_workspace_choice: bool,
    /// First-run routing decision once the user picks Local or Remote.
    pub workspace_routing: Option<WorkspaceRouting>,
    /// First-run: whether the model-provider dialog is open over the chat.
    pub llm_dialog_open: bool,
    /// Editable model-provider form values + focus, held here so text/focus
    /// survive the per-frame element rebuild while the dialog is open.
    pub llm_dialog_provider: Rc<RefCell<String>>,
    pub llm_dialog_model: Rc<RefCell<String>>,
    pub llm_dialog_api_key: Rc<RefCell<String>>,
    pub llm_dialog_base_url: Rc<RefCell<String>>,
    pub llm_dialog_temperature: Rc<RefCell<String>>,
    pub llm_dialog_focus: Rc<RefCell<Option<LlmFormField>>>,
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
            pending_ask: None,
            queued_prompt: None,
            composer_draft: String::new(),
            composer_focused: false,
            models: Vec::new(),
            selected_model: String::new(),
            model_menu_open: Rc::new(RefCell::new(false)),
            profile_menu_open: Rc::new(RefCell::new(false)),
            agent_name: "Goble Agent".to_string(),
            agent_busy: false,
            auto_approve: false,
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
            agent_cards: HashMap::new(),
            new_agent_hover: Rc::new(RefCell::new(false)),
        };
        state.refresh_from_desktop(desktop);
        state.prime_llm_form();
        state
    }

    /// Reload conversations, messages, crons and agent name from the backend.
    pub fn refresh_from_desktop(&mut self, desktop: &DesktopState) {
        self.refresh_conversations(desktop);
        self.refresh_crons(desktop);
        self.refresh_agent_name(desktop);
        self.refresh_settings(desktop);
        self.auto_approve = desktop.get_auto_approve();
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
            self.pending_ask = None;
            return;
        };
        // A suspended ask persists in the store, so the inline card survives a
        // refresh; answering clears it (status becomes `answered`).
        self.pending_ask = desktop
            .get_pending_ask(&chat_id)
            .ok()
            .flatten()
            .and_then(|v| {
                let question = v.get("question")?.as_str()?.to_string();
                let quick: Vec<String> = v
                    .get("quick_replies")
                    .and_then(|q| q.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                Some(AskUserUi::new(question, quick))
            });
        match desktop.list_chat_messages(&chat_id) {
            Ok(msgs) => {
                self.chat_messages = msgs
                    .into_iter()
                    .filter_map(|m| {
                        let role = match m.role.as_str() {
                            "user" => ChatRole::User,
                            "tool" => ChatRole::Tool,
                            _ => ChatRole::Assistant,
                        };
                        // Tool results are stored as "<call_id>\n<output>". Present
                        // them as a distinct terminal block instead of assistant
                        // prose so the user can tell execution output apart.
                        let mut message = if role == ChatRole::Tool {
                            ChatMessage::new(
                                role,
                                vec![ChatFragment::terminal(tool_terminal_data(&m.content))],
                            )
                        } else {
                            ChatMessage::from_markdown(role, m.content)
                        };
                        if let Some(tc) = m.tool_calls.as_deref() {
                            message = message.with_tool_calls(ToolCall::from_llm_json(tc));
                        }
                        Some(message)
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
        self.refresh_llm_models(desktop);
    }

    /// Populate the composer's model dropdown from the configured provider and
    /// default the selected model. The selection is only set on first load, so
    /// a user's per-session model choice survives subsequent refreshes.
    pub fn refresh_llm_models(&mut self, desktop: &DesktopState) {
        self.models = desktop.available_models(&self.settings_llm_provider);
        if self.selected_model.trim().is_empty() {
            self.selected_model = desktop.default_model(&self.settings_llm_provider);
        }
    }

    /// Copy the current LLM settings into the dialog's editable fields, so the
    /// model-provider dialog opens pre-filled with what's configured (empty on
    /// first run) and with no field focused.
    pub fn prime_llm_form(&self) {
        *self.llm_dialog_provider.borrow_mut() = self.settings_llm_provider.clone();
        *self.llm_dialog_model.borrow_mut() = self.settings_llm_model.clone();
        *self.llm_dialog_api_key.borrow_mut() = self.settings_llm_api_key.clone();
        *self.llm_dialog_base_url.borrow_mut() = self.settings_llm_base_url.clone();
        *self.llm_dialog_temperature.borrow_mut() = self.settings_llm_temperature.clone();
        *self.llm_dialog_focus.borrow_mut() = None;
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
            model_menu_open: Rc::new(RefCell::new(false)),
            profile_menu_open: Rc::new(RefCell::new(false)),
            agent_name: "Goble Agent".to_string(),
            agent_busy: false,
            auto_approve: false,
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
            agent_cards: HashMap::new(),
            new_agent_hover: Rc::new(RefCell::new(false)),
        }
    }
}
