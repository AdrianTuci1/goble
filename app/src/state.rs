//! App-owned UI state.
//!
//! The element tree is rebuilt from this state on every frame, so state lives
//! here in the executable alongside the UI builder. That keeps text input
//! focus/value across rebuilds.
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

use crate::terminal::TerminalRegistry;
use crate::ui::{
    AppTab, CronEntry, HarnessEntry, LlmFormField, Pane, PaneChatSnapshot, PaneKind, Space,
    WorkspaceRouting, SIDEBAR_WIDTH,
};

/// The string form of a workspace routing choice as persisted on a chat.
pub(crate) fn routing_to_str(routing: WorkspaceRouting) -> &'static str {
    match routing {
        WorkspaceRouting::Local => "local",
        WorkspaceRouting::Remote => "remote",
    }
}

/// Parse a persisted workspace-routing string back into a [`WorkspaceRouting`].
pub(crate) fn routing_from_str(value: &str) -> Option<WorkspaceRouting> {
    match value {
        "local" => Some(WorkspaceRouting::Local),
        "remote" => Some(WorkspaceRouting::Remote),
        _ => None,
    }
}

/// Detect a BYOH desktop-handoff URI in `text` (`rdp://…` or a
/// `goble://desktop?…` link). Returns the trimmed URI substring when found, so
/// a harness that surfaces its handoff as a hyperlink can be launched by the
/// user even without a structured handoff event.
pub(crate) fn detect_screen_link(text: &str) -> Option<String> {
    let start = text.find("rdp://").or_else(|| text.find("goble://desktop"))?;
    let rest = &text[start..];
    let end = rest
        .find(char::is_whitespace)
        .map(|i| start + i)
        .unwrap_or(text.len());
    if end <= start {
        return None;
    }
    let uri = &text[start..end];
    // A trailing punctuation mark is not part of the URI.
    let uri = uri.trim_end_matches(|c: char| c == ')' || c == ']' || c == '}' || c == '.' || c == ',' || c == ';');
    if uri.is_empty() {
        None
    } else {
        Some(uri.to_string())
    }
}

/// Derive a screen-registry source id from a detected handoff URI, best-effort.
/// Remote sources are typically identified by their `user@host:port`; when that
/// cannot be parsed the host (or the whole URI) is used.
pub(crate) fn screen_source_from_link(link: &str) -> Option<String> {
    let mut candidate: Option<String> = None;
    // goble://desktop?user=&host=&port=
    if let Some(q) = link.split_once('?').map(|(_, q)| q) {
        let mut user = String::new();
        let mut host = String::new();
        let mut port = String::new();
        for pair in q.split('&') {
            let mut kv = pair.splitn(2, '=');
            let (k, v) = (kv.next().unwrap_or(""), kv.next().unwrap_or(""));
            match k {
                "user" => user = v.to_string(),
                "host" => host = v.to_string(),
                "port" => port = v.to_string(),
                _ => {}
            }
        }
        if !host.is_empty() {
            candidate = Some(if user.is_empty() {
                if port.is_empty() { host.clone() } else { format!("{host}:{port}") }
            } else if port.is_empty() {
                format!("{user}@{host}")
            } else {
                format!("{user}@{host}:{port}")
            });
        }
    }
    // rdp://[user@]host[:port] — strip scheme.
    if candidate.is_none() {
        if let Some(rest) = link.strip_prefix("rdp://") {
            let rest = rest.split_whitespace().next().unwrap_or(rest);
            let rest = rest.trim_end_matches('/');
            let host = rest.rsplit('@').next().unwrap_or(rest);
            candidate = Some(host.to_string());
        }
    }
    candidate
}

/// Extract the text of a chat fragment, for link scanning. Non-text fragments
/// (lists, actions, terminal blocks) contribute nothing.
fn fragment_text(f: &goble_ui::elements::chat_content::ChatFragment) -> Option<&str> {
    use goble_ui::elements::chat_content::ChatFragmentKind;
    match &f.kind {
        ChatFragmentKind::Text(s)
        | ChatFragmentKind::Bold(s)
        | ChatFragmentKind::Italic(s)
        | ChatFragmentKind::BoldItalic(s)
        | ChatFragmentKind::Code(s)
        | ChatFragmentKind::BlockQuote(s) => Some(s.as_str()),
        ChatFragmentKind::CodeBlock { code, .. } => Some(code.as_str()),
        ChatFragmentKind::Heading { text, .. } => Some(text.as_str()),
        ChatFragmentKind::Link { url, .. } => Some(url.as_str()),
        _ => None,
    }
}

/// Scan messages (most-recent assistant content first) for a BYOH desktop
/// handoff URI, so a harness that only prints a hyperlink still surfaces a
/// clickable handoff affordance.
fn message_screen_link(messages: &[ChatMessage]) -> Option<String> {
    for msg in messages.iter().rev() {
        if msg.role != ChatRole::Assistant {
            continue;
        }
        let mut plain = String::new();
        for f in &msg.fragments {
            if let Some(t) = fragment_text(f) {
                plain.push_str(t);
                plain.push(' ');
            }
        }
        if let Some(link) = detect_screen_link(&plain) {
            return Some(link);
        }
    }
    None
}

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
    TerminalData::new(
        if title.is_empty() { "tool" } else { title }.to_string(),
        lines,
    )
    .with_status(status)
}

/// Per-pane conversation identity + composer draft + working directory.
///
/// Each chat leaf pane owns a distinct conversation in the store plus its own
/// composer draft and cwd, so two panes never share a transcript. This is the
/// subset persisted across an app restart (`draft` + `path` + `conversation_id`
/// survive; the transcript is re-read from the store keyed by conversation id).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PaneSession {
    /// The store conversation id this pane is bound to. Empty means the pane
    /// lazily uses the currently selected conversation (`selected_id`).
    pub conversation_id: String,
    /// This pane's composer draft, independent of other panes.
    pub draft: String,
    /// This pane's working directory (shown as the composer's path label).
    pub path: String,
}

/// Runtime per-pane state not persisted across restarts: the transcript, the
/// suspended ask, the queued prompt and the busy flag. Re-read from the store
/// (keyed by [`PaneSession::conversation_id`]) on every refresh.
#[derive(Clone, Debug, Default)]
pub struct PaneRuntime {
    pub messages: Vec<ChatMessage>,
    pub pending_ask: Option<AskUserUi>,
    pub queued_prompt: Option<String>,
    pub busy: bool,
    /// The screen source the harness handed off to (shown inline in chat).
    pub inline_screen_source: Option<String>,
    /// A detected BYOH handoff URI from the assistant output.
    pub screen_link: Option<String>,
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
    /// Harnesses the composer can route a turn to (native only in this build).
    pub harnesses: Vec<HarnessEntry>,
    /// The harness the active pane routes turns to (default `internal`).
    pub selected_harness: String,
    /// A detected BYOH handoff URI the user clicked to open; the root view
    /// performs the open (screen sheet) then clears this.
    pub pending_screen_link_open: Option<String>,
    /// App-owned open flags for the composer model / account menus.
    pub model_menu_open: Rc<RefCell<bool>>,
    pub profile_menu_open: Rc<RefCell<bool>>,
    /// App-owned open flags for the composer harness / dir / branch menus.
    pub harness_menu_open: Rc<RefCell<bool>>,
    pub dir_menu_open: Rc<RefCell<bool>>,
    pub branch_menu_open: Rc<RefCell<bool>>,
    /// The git branch of the active pane's working directory (best-effort,
    /// read from `.git/HEAD`). Shown as the composer's branch pill when set.
    pub composer_branch: String,
    pub agent_name: String,
    pub agent_busy: bool,
    /// Whether the agent auto-approves `ask_user` questions (skip the ask).
    pub auto_approve: bool,
    pub right_sidebar_open: bool,
    /// Whether the agent/window is currently fullscreen (borderless). Lives in
    /// app state so it survives the per-frame element rebuild.
    pub fullscreen: bool,
    /// App-owned open flag for the agent header's 3-dots menu, so its open/closed
    /// state survives the per-frame element rebuild.
    pub agent_header_menu_open: Rc<RefCell<bool>>,
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
    /// Multiple "spaces" (warp-new style): each is a pane tree shown as a tab
    /// in the top bar. `active_space` selects the one currently rendered.
    pub spaces: Vec<Space>,
    pub active_space: usize,
    /// Hovered space tab index (app state so the highlight survives the
    /// per-frame element rebuild).
    pub space_hover: Option<usize>,
    /// Space tab currently pressed (MouseDown), used to detect a drag start.
    pub space_press: Option<usize>,
    /// Space tab currently being dragged (reordering in progress).
    pub space_drag: Option<usize>,
    /// Monotonic id counter so new panes never collision with existing ones.
    pub next_pane_id: u64,
    /// The leaf pane currently focused (target for splits/closes, highlighted).
    pub active_pane_id: u64,
    /// The split node currently being dragged, if any.
    pub dragging_pane_id: Option<u64>,
    /// The current working path shown in the composer.
    pub composer_path: String,
    /// Per-pane hover flags (keyed by pane id) so pane-header hover survives
    /// the per-frame element rebuild.
    pub pane_hover: HashMap<u64, Rc<RefCell<bool>>>,
    /// Per-pane conversation identity + composer draft + cwd. Keyed by pane id;
    /// every chat leaf pane has an entry so it is an independent session.
    pub pane_sessions: HashMap<u64, PaneSession>,
    /// Runtime per-pane transcript/ask/queued/busy state (not persisted).
    pub pane_runtime: HashMap<u64, PaneRuntime>,
    /// Live per-pane terminal sessions (PTY child + output buffer) and the local
    /// input mirrors used to route `Cmd+Enter` to the agent. Not persisted; a
    /// terminal pane is re-spawned in its cwd on next render.
    pub terminal: Rc<RefCell<TerminalRegistry>>,
    /// Whether the Cmd+K command palette overlay is open.
    pub command_palette_open: bool,
    /// The palette's filter text, used to narrow the command list.
    pub command_palette_query: String,
    /// The palette's selected row index.
    pub command_palette_index: usize,
    /// First run: a compact "getting started" hint shown once the workspace
    /// choice is made.
    pub show_onboarding_tip: bool,
    /// App-owned open flag for the topbar "+" menu (choose an environment to
    /// open a new space, or add a new medium), so it survives the per-frame
    /// element rebuild.
    pub add_space_menu_open: Rc<RefCell<bool>>,
    /// App-owned open flag for the topbar compact environment selector, so it
    /// survives the per-frame element rebuild.
    pub env_selector_open: Rc<RefCell<bool>>,
    /// Whether the "add a new medium" dialog is open over the app.
    pub add_medium_dialog_open: bool,
    /// The new medium's name as typed in the add-medium dialog.
    pub add_medium_draft: String,
    /// Whether the add-medium dialog's text field is focused.
    pub add_medium_focused: bool,
    /// Whether the user has completed (or dismissed) the first-run flow. Kept
    /// in the backend store so a returning run skips the onboarding overlays
    /// and the getting-started tip.
    pub onboarding_done: bool,
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
            profile_menu_open: Rc::new(RefCell::new(false)),
            harness_menu_open: Rc::new(RefCell::new(false)),
            dir_menu_open: Rc::new(RefCell::new(false)),
            branch_menu_open: Rc::new(RefCell::new(false)),
            composer_branch: current_branch(&current_dir_display()),
            agent_name: "Goble Agent".to_string(),
            agent_busy: false,
            auto_approve: false,
            right_sidebar_open: false,
            fullscreen: false,
            agent_header_menu_open: Rc::new(RefCell::new(false)),
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
        self.sync_active_view();
    }

    /// Find the pane (if any) bound to a store conversation id. Used to route
    /// backend events (e.g. `chat:turn_finished`) to the pane that owns the turn.
    pub fn pane_id_for_conversation(&self, conversation_id: &str) -> Option<u64> {
        self.pane_sessions.iter().find_map(|(id, s)| {
            if s.conversation_id == conversation_id {
                Some(*id)
            } else {
                None
            }
        })
    }

    /// The store conversation id a pane is bound to. A pane with no explicit
    /// conversation lazily uses the currently selected conversation, which is
    /// how the initial single pane tracks the sidebar selection.
    pub fn pane_conversation_id(&self, pane_id: u64) -> Option<String> {
        self.pane_sessions
            .get(&pane_id)
            .map(|s| s.conversation_id.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                if pane_id == self.active_pane_id {
                    self.selected_id.clone()
                } else {
                    None
                }
            })
    }

    /// Refresh one pane's runtime state (transcript + suspended ask) from the
    /// store conversation `conv`.
    fn refresh_pane(&mut self, pane_id: u64, conv: &str, desktop: &DesktopState) {
        let rt = self.pane_runtime.entry(pane_id).or_default();
        // A suspended ask persists in the store, so the inline card survives a
        // refresh; answering clears it (status becomes `answered`).
        rt.pending_ask = desktop
            .get_pending_ask(conv)
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
        match desktop.list_chat_messages(conv) {
            Ok(msgs) => {
                rt.messages = msgs
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
                log::warn!("list_chat_messages({conv}): {e}");
            }
        }
        // The Local/Remote runtime decision is tracked per-conversation.
        if pane_id == self.active_pane_id {
            self.workspace_routing = desktop
                .get_chat_workspace_routing(conv)
                .ok()
                .flatten()
                .and_then(|s| routing_from_str(&s));
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
                rt.queued_prompt = None;
            }
            self.sync_active_view();
        }
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

    /// Build the per-pane chat snapshot (transcript + draft + path) keyed by
    /// pane id, used to render each chat leaf as an independent session.
    pub fn pane_chat_snapshot(&self) -> HashMap<u64, PaneChatSnapshot> {
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
                    queued_prompt: rt.and_then(|r| r.queued_prompt.clone()),
                    agent_busy: rt.map(|r| r.busy).unwrap_or(false),
                    inline_screen: None,
                    screen_link: message_screen_link(&rt.map(|r| r.messages.clone()).unwrap_or_default()),
                },
            );
        }
        out
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
            profile_menu_open: Rc::new(RefCell::new(false)),
            harness_menu_open: Rc::new(RefCell::new(false)),
            dir_menu_open: Rc::new(RefCell::new(false)),
            branch_menu_open: Rc::new(RefCell::new(false)),
            composer_branch: current_branch(&current_dir_display()),
            agent_name: "Goble Agent".to_string(),
            agent_busy: false,
            auto_approve: false,
            right_sidebar_open: false,
            fullscreen: false,
            agent_header_menu_open: Rc::new(RefCell::new(false)),
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
                    pending_ask: None,
                    queued_prompt: None,
                    busy: false,
                    inline_screen_source: None,
                    screen_link: None,
                },
            )]),
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
            onboarding_done: false,
        }
    }

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
        self.active_space = saved.active_space.min(self.spaces.len() - 1);
        // Restore the per-pane session data (conversation id, draft, cwd) so a
        // pane stays the same independent conversation across a restart.
        self.pane_sessions = saved.pane_sessions;
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
    }

    /// Persist the pane layout (spaces + active space/pane) so it survives an
    /// app restart. Called whenever a pane action mutates the layout.
    pub fn save_panes(&self, desktop: &DesktopState) {
        let persisted = PersistedPaneState {
            spaces: self.spaces.clone(),
            active_space: self.active_space,
            active_pane_id: self.active_pane_id,
            pane_sessions: self.pane_sessions.clone(),
        };
        if let Ok(json) = serde_json::to_string(&persisted) {
            if let Err(e) = desktop.set_ui_panes(&json) {
                log::warn!("failed to persist pane state: {e}");
            }
        }
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

/// The subset of [`UiState`] pane layout persisted across app restarts.
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedPaneState {
    spaces: Vec<Space>,
    active_space: usize,
    active_pane_id: u64,
    /// Per-pane conversation/draft/cwd. `#[serde(default)]` so pane layouts
    /// saved before per-pane sessions existed still parse.
    #[serde(default)]
    pane_sessions: HashMap<u64, PaneSession>,
}

/// Collect every pane id (leaves and splits) in a pane tree.
fn collect_pane_ids(pane: &Pane, out: &mut Vec<u64>) {
    match pane {
        Pane::Leaf { id, .. } => out.push(*id),
        Pane::Split {
            id, first, second, ..
        } => {
            out.push(*id);
            collect_pane_ids(first, out);
            collect_pane_ids(second, out);
        }
    }
}

/// Collect the ids of every leaf pane (chat or terminal) in a pane tree, so
/// each leaf gets the per-session entry it needs.
fn collect_leaf_pane_ids(pane: &Pane, out: &mut Vec<u64>) {
    match pane {
        Pane::Leaf { id, .. } => out.push(*id),
        Pane::Split { first, second, .. } => {
            collect_leaf_pane_ids(first, out);
            collect_leaf_pane_ids(second, out);
        }
    }
}

/// Ensure every leaf pane has a [`PaneSession`] entry so it is an independent
/// session — a chat pane needs a transcript/cwd, and a terminal pane needs a
/// cwd to spawn its shell. A pane that is not yet bound keeps an empty
/// conversation id and lazily follows the selected conversation (this is how
/// the initial single pane tracks the sidebar); panes created by splitting /
/// adding a space are bound to a distinct conversation in
/// [`UiState::bind_pane_new_conversation`].
fn ensure_all_chat_pane_sessions(state: &mut UiState, _desktop: Option<&DesktopState>) {
    let mut leaf_ids = Vec::new();
    for space in &state.spaces {
        collect_leaf_pane_ids(&space.root, &mut leaf_ids);
    }
    let default_path = current_dir_display();
    for id in leaf_ids {
        state.pane_sessions.entry(id).or_insert_with(|| PaneSession {
            conversation_id: String::new(),
            draft: String::new(),
            path: default_path.clone(),
        });
    }
}

/// Resolve a pane's initial working directory from a project's directory when
/// known, else fall back to the process cwd (a sane default).
pub fn default_pane_path(project_id: &str, desktop: Option<&DesktopState>) -> String {
    if let Some(desktop) = desktop {
        for p in desktop.list_projects() {
            if p.project_id.0 == project_id {
                return p.directory;
            }
        }
    }
    current_dir_display()
}

/// One initial space with a single chat leaf.
fn default_spaces() -> Vec<Space> {
    vec![Space::new("Space 1", Pane::Leaf { id: 1, kind: PaneKind::Chat })]
}

/// The current working directory, used as the composer's path label.
fn current_dir_display() -> String {
    std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "/".to_string())
}

/// Best-effort git branch of `dir`, read from `.git/HEAD`. Handles a normal
/// `.git` directory and a `.git` *file* (worktree/submodule) whose first line
/// is `gitdir: <path>`. Returns empty when the directory is not a git repo.
fn current_branch(dir: &str) -> String {
    if let Ok(content) = std::fs::read_to_string(std::path::Path::new(dir).join(".git/HEAD")) {
        return parse_branch_head(&content);
    }
    // `.git` may be a file pointing at the real gitdir (worktrees, submodules).
    if let Ok(gitfile) = std::fs::read_to_string(std::path::Path::new(dir).join(".git")) {
        if let Some(gitdir) = gitfile
            .lines()
            .find_map(|l| l.trim().strip_prefix("gitdir:"))
        {
            let head = std::path::Path::new(gitdir.trim()).join("HEAD");
            if let Ok(content) = std::fs::read_to_string(&head) {
                return parse_branch_head(&content);
            }
        }
    }
    String::new()
}

fn parse_branch_head(content: &str) -> String {
    let line = content.lines().next().unwrap_or("").trim();
    if let Some(name) = line.strip_prefix("ref: refs/heads/") {
        name.to_string()
    } else if line.len() >= 40 {
        "detached".to_string()
    } else {
        String::new()
    }
}

#[cfg(test)]
mod pane_session_tests {
    use super::*;

    #[test]
    fn mock_pane_is_bound_to_its_own_conversation() {
        let state = UiState::mock();
        let session = state.pane_sessions.get(&1).expect("pane 1 has a session");
        assert_eq!(session.conversation_id, "c1");
        assert_eq!(state.pane_conversation_id(1).as_deref(), Some("c1"));
    }

    #[test]
    fn unbound_active_pane_falls_back_to_selected_conversation() {
        let mut state = UiState::mock();
        state.pane_sessions.get_mut(&1).unwrap().conversation_id.clear();
        state.selected_id = Some("sel".to_string());
        assert_eq!(state.pane_conversation_id(1).as_deref(), Some("sel"));
    }

    #[test]
    fn non_active_unbound_pane_has_no_conversation() {
        let mut state = UiState::mock();
        state.pane_sessions.get_mut(&1).unwrap().conversation_id.clear();
        // Pane 2 is not active and has no conversation -> None (not the fallback).
        assert_eq!(state.pane_conversation_id(2), None);
    }

    #[test]
    fn bind_pane_new_conversation_assigns_distinct_mock_id() {
        let mut state = UiState::mock();
        let pane1 = state.pane_sessions.get(&1).unwrap().conversation_id.clone();
        state.bind_pane_new_conversation(2, None);
        let pane2 = state.pane_sessions.get(&2).unwrap().conversation_id.clone();
        assert_eq!(pane2, "pane-2");
        assert_ne!(pane1, pane2, "two panes never share a conversation");
    }

    #[test]
    fn sync_active_view_reflects_active_pane() {
        let mut state = UiState::mock();
        state.pane_sessions.insert(
            1,
            PaneSession {
                conversation_id: "a".into(),
                draft: "draft1".into(),
                path: "/a".into(),
            },
        );
        state.pane_sessions.insert(
            2,
            PaneSession {
                conversation_id: "b".into(),
                draft: "draft2".into(),
                path: "/b".into(),
            },
        );
        state.pane_runtime.insert(
            1,
            PaneRuntime {
                messages: vec![ChatMessage::from_markdown(ChatRole::User, "m1")],
                pending_ask: None,
                queued_prompt: None,
                busy: false,
                ..PaneRuntime::default()
            },
        );
        state.pane_runtime.insert(
            2,
            PaneRuntime {
                messages: vec![ChatMessage::from_markdown(ChatRole::User, "m2")],
                pending_ask: None,
                queued_prompt: None,
                busy: true,
                ..PaneRuntime::default()
            },
        );

        state.active_pane_id = 1;
        state.sync_active_view();
        assert_eq!(state.selected_id.as_deref(), Some("a"));
        assert_eq!(state.composer_draft, "draft1");
        assert_eq!(state.composer_path, "/a");
        assert_eq!(state.chat_messages.len(), 1);
        assert!(!state.agent_busy);

        state.active_pane_id = 2;
        state.sync_active_view();
        assert_eq!(state.selected_id.as_deref(), Some("b"));
        assert_eq!(state.composer_draft, "draft2");
        assert_eq!(state.composer_path, "/b");
        assert!(state.agent_busy);
    }

    #[test]
    fn ensure_pane_sessions_adds_missing_chat_sessions() {
        let mut state = UiState::mock();
        state.spaces.push(Space::new(
            "S2",
            Pane::Leaf { id: 5, kind: PaneKind::Chat },
        ));
        state.pane_sessions.remove(&5);
        state.ensure_pane_sessions(None);
        assert!(
            state.pane_sessions.contains_key(&5),
            "a chat pane gets a session even if it was missing"
        );
    }

    #[test]
    fn set_active_pane_path_updates_pane_and_global() {
        let mut state = UiState::mock();
        state.active_pane_id = 1;
        state.set_active_pane_path("/workspace/proj".to_string());
        assert_eq!(state.pane_sessions.get(&1).unwrap().path, "/workspace/proj");
        assert_eq!(state.composer_path, "/workspace/proj");
    }

    #[test]
    fn pane_chat_snapshot_has_distinct_per_pane_data() {
        let mut state = UiState::mock();
        state.pane_sessions.insert(
            1,
            PaneSession {
                conversation_id: "a".into(),
                draft: "d1".into(),
                path: "/a".into(),
            },
        );
        state.pane_sessions.insert(
            2,
            PaneSession {
                conversation_id: "b".into(),
                draft: "d2".into(),
                path: "/b".into(),
            },
        );
        state.pane_runtime.insert(
            1,
            PaneRuntime {
                messages: vec![ChatMessage::from_markdown(ChatRole::Assistant, "x")],
                pending_ask: None,
                queued_prompt: None,
                busy: false,
                ..PaneRuntime::default()
            },
        );
        state.pane_runtime.insert(
            2,
            PaneRuntime {
                messages: Vec::new(),
                pending_ask: None,
                queued_prompt: None,
                busy: false,
                ..PaneRuntime::default()
            },
        );
        let snap = state.pane_chat_snapshot();
        assert_eq!(snap.get(&1).map(|s| s.conversation_id.as_str()), Some("a"));
        assert_eq!(snap.get(&1).unwrap().messages.len(), 1);
        assert_eq!(snap.get(&1).unwrap().composer_path, "/a");
        assert_eq!(snap.get(&2).unwrap().composer_path, "/b");
        // Distinct draft/path per pane => independent sessions.
        assert_eq!(snap.get(&1).unwrap().composer_draft, "d1");
        assert_eq!(snap.get(&2).unwrap().composer_draft, "d2");
    }
}

#[cfg(test)]
mod space_reorder_tests {
    use super::*;

    /// A UiState with three named single-pane spaces and `active` selected.
    fn three_spaces(active: usize) -> UiState {
        let mut state = UiState::mock();
        state.spaces = vec![
            Space::new("A", Pane::Leaf { id: 1, kind: PaneKind::Chat }),
            Space::new("B", Pane::Leaf { id: 2, kind: PaneKind::Chat }),
            Space::new("C", Pane::Leaf { id: 3, kind: PaneKind::Chat }),
        ];
        state.active_space = active;
        state
    }

    fn names(state: &UiState) -> Vec<&str> {
        state.spaces.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn reorder_moves_space_to_target_position() {
        let mut state = three_spaces(0);
        state.reorder_space(0, 2);
        assert_eq!(names(&state), vec!["B", "C", "A"]);
        assert_eq!(state.active_space, 2, "active space A follows to index 2");
    }

    #[test]
    fn reorder_keeps_active_space_when_a_earlier_space_moves_past_it() {
        let mut state = three_spaces(1);
        state.reorder_space(0, 2);
        assert_eq!(names(&state), vec!["B", "C", "A"]);
        // Space B (active) slid down to index 0.
        assert_eq!(state.active_space, 0);
    }

    #[test]
    fn reorder_keeps_active_space_when_a_later_space_moves_before_it() {
        let mut state = three_spaces(1);
        state.reorder_space(2, 0);
        assert_eq!(names(&state), vec!["C", "A", "B"]);
        // Space B (active) slid up to index 2.
        assert_eq!(state.active_space, 2);
    }

    #[test]
    fn reorder_to_same_index_is_a_no_op() {
        let mut state = three_spaces(1);
        state.reorder_space(1, 1);
        assert_eq!(names(&state), vec!["A", "B", "C"]);
        assert_eq!(state.active_space, 1);
    }

    #[test]
    fn reorder_out_of_range_is_a_no_op() {
        let mut state = three_spaces(1);
        state.reorder_space(0, 9);
        assert_eq!(names(&state), vec!["A", "B", "C"]);
        state.reorder_space(9, 0);
        assert_eq!(names(&state), vec!["A", "B", "C"]);
    }
}

#[cfg(test)]
mod screen_link_tests {
    use super::*;
    use goble_ui::elements::chat_content::ChatFragment;

    #[test]
    fn detect_rdp_uri_with_trailing_text() {
        assert_eq!(
            detect_screen_link("open rdp://192.168.1.5:3389 now").as_deref(),
            Some("rdp://192.168.1.5:3389")
        );
    }

    #[test]
    fn detect_rdp_uri_trims_trailing_punctuation() {
        assert_eq!(
            detect_screen_link("Take over: rdp://host:3389.").as_deref(),
            Some("rdp://host:3389")
        );
    }

    #[test]
    fn detect_goble_desktop_uri() {
        assert_eq!(
            detect_screen_link("go to goble://desktop?user=me&host=h&port=5900 end")
                .as_deref(),
            Some("goble://desktop?user=me&host=h&port=5900")
        );
    }

    #[test]
    fn detect_screen_link_ignores_plain_text() {
        assert_eq!(detect_screen_link("no link here"), None);
        assert_eq!(detect_screen_link(""), None);
    }

    #[test]
    fn detect_screen_link_survives_long_prefix() {
        // A link after a long prose prefix is still detected (the URI length
        // must not be compared against its byte offset).
        let prefix: String = "a".repeat(40);
        let text = format!("{prefix} rdp://host:3389");
        assert_eq!(detect_screen_link(&text).as_deref(), Some("rdp://host:3389"));
    }

    #[test]
    fn screen_source_from_goble_query() {
        assert_eq!(
            screen_source_from_link("goble://desktop?user=me&host=myhost&port=5900").as_deref(),
            Some("me@myhost:5900")
        );
        assert_eq!(
            screen_source_from_link("goble://desktop?host=myhost&port=5900").as_deref(),
            Some("myhost:5900")
        );
        assert_eq!(
            screen_source_from_link("goble://desktop?host=myhost").as_deref(),
            Some("myhost")
        );
    }

    #[test]
    fn screen_source_from_rdp() {
        assert_eq!(
            screen_source_from_link("rdp://user@host:3389").as_deref(),
            Some("host:3389")
        );
        assert_eq!(screen_source_from_link("rdp://host").as_deref(), Some("host"));
    }

    #[test]
    fn screen_source_from_unknown_scheme_is_none() {
        assert_eq!(screen_source_from_link("https://example.com"), None);
    }

    #[test]
    fn message_screen_link_scans_assistant_output_only() {
        let assistant = ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Handing off: rdp://host:3389")],
        );
        assert_eq!(
            message_screen_link(&[assistant]).as_deref(),
            Some("rdp://host:3389")
        );
    }

    #[test]
    fn message_screen_link_ignores_user_messages() {
        let user = ChatMessage::new(
            ChatRole::User,
            vec![ChatFragment::text("use rdp://host:3389 please")],
        );
        assert_eq!(message_screen_link(&[user]), None);
    }

    #[test]
    fn message_screen_link_prefers_most_recent_assistant() {
        let older = ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("see rdp://oldhost:3389")],
        );
        let newer = ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("now rdp://newhost:3389")],
        );
        assert_eq!(
            message_screen_link(&[older, newer]).as_deref(),
            Some("rdp://newhost:3389")
        );
    }

    #[test]
    fn pane_chat_snapshot_surfaces_screen_link_from_message() {
        let mut state = UiState::mock();
        state.pane_sessions.insert(
            1,
            PaneSession {
                conversation_id: "a".into(),
                draft: "d1".into(),
                path: "/a".into(),
            },
        );
        state.pane_runtime.insert(
            1,
            PaneRuntime {
                messages: vec![ChatMessage::new(
                    ChatRole::Assistant,
                    vec![ChatFragment::text("desktop ready: goble://desktop?user=me&host=h&port=5900")],
                )],
                pending_ask: None,
                queued_prompt: None,
                busy: false,
                ..PaneRuntime::default()
            },
        );
        let snap = state.pane_chat_snapshot();
        assert_eq!(
            snap.get(&1).and_then(|s| s.screen_link.as_deref()),
            Some("goble://desktop?user=me&host=h&port=5900")
        );
        assert!(snap.get(&1).unwrap().inline_screen.is_none());
    }
}
