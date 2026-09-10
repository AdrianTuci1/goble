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
use goble_core::harness::ToolCallStatus;
use goble_desktop_service::DesktopState;
use goble_ui::{
    AgentCardUi, AskUserUi, ChatFragment, ChatMessage, ChatRole, ConversationEntry,
    ConversationStatus, ScrollState, SettingsPage, TerminalData, TerminalFilter, TerminalLine,
    TerminalStatus, ToolCall,
};

use goble_terminal::blocks::{BlockId, BlockView};

use crate::emulator::VisibleBlock;
use crate::terminal::TerminalRegistry;
use crate::ui::{
    AppTab, CostEntry, CronEntry, ExecutionEntry, HarnessEntry, LlmFormField, Pane,
    PaneChatSnapshot, PaneKind, SettingsCategory, Space, TaskEntry, TimelineEntry, WorkflowEntry,
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
        | ChatFragmentKind::Code(s) => Some(s.as_str()),
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

/// Short human label for a workflow [`Trigger`], e.g. `cron 0 12 * * *`.
fn trigger_label(trigger: &Trigger) -> String {
    match trigger {
        Trigger::Manual => "manual".to_string(),
        Trigger::Cron { expression } => format!("cron {expression}"),
        Trigger::Http { path } => format!("http {path}"),
        Trigger::Heartbeat { interval_seconds } => format!("heartbeat {interval_seconds}s"),
    }
}

/// Build a terminal-style block from a stored tool-result message. The harness
/// writes tool output as `"<call_id>\n<output>"`, so the first line becomes the
/// block title and the remaining lines render as mono output lines.
///
/// The block carries no status: a tool call's real status lives on the
/// assistant row's persisted `tool_calls` column (Q5) and is drawn on the
/// tool-call row, so nothing here infers success or failure from the text.
fn tool_terminal_data(content: &str) -> TerminalData {
    let mut parts = content.splitn(2, '\n');
    let title = parts.next().unwrap_or("tool").trim();
    let body = parts.next().unwrap_or("").trim();

    let mut lines = Vec::new();
    for line in body.lines() {
        let text = line.trim_end().to_string();
        if text.is_empty() {
            lines.push(TerminalLine::info(" "));
        } else {
            lines.push(TerminalLine::output(text));
        }
    }
    if lines.is_empty() {
        lines.push(TerminalLine::info("(no output)"));
    }

    TerminalData::new(
        if title.is_empty() { "tool" } else { title }.to_string(),
        lines,
    )
}

/// One store row plus the message it parsed into. Remembered per row id so a
/// refresh can tell an unchanged row from an edited one.
#[derive(Clone, Debug)]
struct CachedMessage {
    role: String,
    content: String,
    tool_calls: Option<String>,
    message: ChatMessage,
}

impl CachedMessage {
    /// Whether `row` still carries the fields this entry was parsed from. An
    /// edit to any of them (a streaming delta appends to `content`) invalidates
    /// the entry so the message is re-parsed.
    fn matches(&self, row: &goble_desktop_service::ChatMessage) -> bool {
        self.role == row.role && self.content == row.content && self.tool_calls == row.tool_calls
    }
}

/// Parse-once-per-change cache for one pane's transcript.
///
/// Each store row is parsed into a [`ChatMessage`] once and remembered under its
/// row id together with the row fields it was parsed from. A refresh re-parses
/// only the rows whose content (or role / tool calls) changed and reuses every
/// other message, so a frame that carries no new delta does no parsing at all.
/// Entries for rows that disappeared are dropped.
#[derive(Clone, Debug, Default)]
pub struct MessageParseCache {
    entries: HashMap<String, CachedMessage>,
    parses: u64,
}

impl MessageParseCache {
    /// Build the transcript for `rows` (in store order), re-parsing only the
    /// rows whose content changed since the previous call.
    pub fn resolve(&mut self, rows: &[goble_desktop_service::ChatMessage]) -> Vec<ChatMessage> {
        let mut messages = Vec::with_capacity(rows.len());
        let mut next: HashMap<String, CachedMessage> = HashMap::with_capacity(rows.len());
        for row in rows {
            let message = match self.entries.get(&row.id) {
                Some(cached) if cached.matches(row) => cached.message.clone(),
                _ => {
                    self.parses += 1;
                    parse_chat_row(row)
                }
            };
            next.insert(
                row.id.clone(),
                CachedMessage {
                    role: row.role.clone(),
                    content: row.content.clone(),
                    tool_calls: row.tool_calls.clone(),
                    message: message.clone(),
                },
            );
            messages.push(message);
        }
        self.entries = next;
        messages
    }

    /// How many rows this cache has parsed. Test observability for the
    /// parse-once-per-change contract; not read by the app.
    pub fn parse_count(&self) -> u64 {
        self.parses
    }
}

/// Convert a live `chat:tool` event into the renderable [`ToolCall`] the
/// transcript overlay uses.
fn tool_call_from_event(call: &goble_desktop_service::ToolCallEvent) -> ToolCall {
    ToolCall {
        id: call.id.clone(),
        name: call.name.clone(),
        arguments: serde_json::to_string(&call.arguments).unwrap_or_default(),
        status: call.status,
        result: call.result.clone(),
    }
}

/// Overlay a pane's in-flight calls on the transcript built from persisted
/// rows: a call already present is updated to its live record, and a call not
/// yet persisted is attached to the trailing assistant message (or a fresh one)
/// so it renders while it runs. Only running calls are held, so a persisted
/// terminal state is never overwritten.
fn overlay_in_flight(messages: &mut Vec<ChatMessage>, in_flight: &HashMap<String, ToolCall>) {
    if in_flight.is_empty() {
        return;
    }
    for call in in_flight.values() {
        if let Some(existing) = messages
            .iter_mut()
            .flat_map(|m| m.tool_calls.iter_mut())
            .find(|c| c.id == call.id)
        {
            *existing = call.clone();
            continue;
        }
        match messages
            .iter_mut()
            .rev()
            .find(|m| m.role == ChatRole::Assistant)
        {
            Some(message) => message.tool_calls.push(call.clone()),
            None => {
                let mut message = ChatMessage::new(ChatRole::Assistant, Vec::new());
                message.tool_calls.push(call.clone());
                messages.push(message);
            }
        }
    }
}

/// Overlay the pane's accumulated reasoning steps on the transcript: they are
/// pulled out of and re-inserted into the trailing assistant message so each
/// step renders as its own row ahead of that message's prose. Re-running is
/// idempotent, so a refresh or a live delta never duplicates a row.
fn overlay_reasoning(messages: &mut Vec<ChatMessage>, reasoning: &[ReasoningRow]) {
    if reasoning.is_empty() {
        return;
    }
    let index = match messages.iter().rposition(|m| m.role == ChatRole::Assistant) {
        Some(index) => index,
        None => {
            messages.push(ChatMessage::new(ChatRole::Assistant, Vec::new()));
            messages.len() - 1
        }
    };
    let message = &mut messages[index];
    message.fragments.retain(|f| {
        !matches!(
            f.kind,
            goble_ui::elements::chat_content::ChatFragmentKind::Reasoning { .. }
        )
    });
    let mut fragments: Vec<ChatFragment> = reasoning
        .iter()
        .map(|row| {
            ChatFragment::reasoning(
                reasoning_row_key(&row.chat_id, row.step),
                row.mode.clone(),
                row.text.clone(),
                row.done,
            )
        })
        .collect();
    fragments.extend(std::mem::take(&mut message.fragments));
    message.fragments = fragments;
}

/// The app-owned expand-state key for one reasoning step. It is stable while
/// the step's text streams, so a row the user opened stays open.
pub(crate) fn reasoning_row_key(chat_id: &str, step: usize) -> String {
    format!("{chat_id}:{step}")
}

/// Parse one stored row into a [`ChatMessage`]: a tool result becomes a terminal
/// block, everything else is Markdown, and tool-call metadata is attached.
fn parse_chat_row(row: &goble_desktop_service::ChatMessage) -> ChatMessage {
    let role = match row.role.as_str() {
        "user" => ChatRole::User,
        "tool" => ChatRole::Tool,
        _ => ChatRole::Assistant,
    };
    // Tool results are stored as "<call_id>\n<output>". Present them as a
    // distinct terminal block instead of assistant prose so the user can tell
    // execution output apart.
    let mut message = if role == ChatRole::Tool {
        ChatMessage::new(
            role,
            vec![ChatFragment::terminal(tool_terminal_data(&row.content))],
        )
    } else {
        ChatMessage::from_markdown(role, row.content.clone())
    };
    if let Some(tc) = row.tool_calls.as_deref() {
        message = message.with_tool_calls(ToolCall::from_llm_json(tc));
    }
    message
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
    /// Parsed-message cache, so a refresh re-parses only the rows whose content
    /// changed instead of every message on every event.
    pub parse_cache: MessageParseCache,
    pub pending_ask: Option<AskUserUi>,
    pub queued_prompt: Option<String>,
    pub busy: bool,
    /// The screen source the harness handed off to (shown inline in chat).
    pub inline_screen_source: Option<String>,
    /// A detected BYOH handoff URI from the assistant output.
    pub screen_link: Option<String>,
    /// Live tool calls still running for this pane, keyed by call id. Fed by the
    /// `chat:tool` event and overlaid on the transcript immediately, so a
    /// running call is visible before the turn ends. A finish/error clears the
    /// entry and the re-read from the store carries the terminal state.
    pub in_flight_tools: HashMap<String, ToolCall>,
    /// The model's reasoning (thinking) steps for this pane, fed by the live
    /// `chat:reasoning` events. Overlaid on the transcript as their own rows;
    /// replaced when the next reasoning turn starts (step 0).
    pub reasoning: Vec<ReasoningRow>,
}

/// One reasoning (thinking) step carried by the live `chat:reasoning` events,
/// accumulated as the step's deltas stream in.
#[derive(Clone, Debug, PartialEq)]
pub struct ReasoningRow {
    /// The store conversation the step belongs to (used to key its expand state
    /// and to scope the row to its pane).
    pub chat_id: String,
    pub step: usize,
    pub mode: String,
    pub text: String,
    pub done: bool,
}

impl PaneRuntime {
    /// Fold one live reasoning transition into the pane's accumulated steps.
    ///
    /// A `started` at step 0 begins a new reasoning turn, so the previous
    /// turn's rows are dropped; a `delta` appends to the currently open step; a
    /// `done` finalises the step with the authoritative full text.
    fn apply_reasoning(&mut self, event: &goble_desktop_service::ReasoningEvent) {
        use goble_desktop_service::ReasoningPhase;
        match event.phase {
            ReasoningPhase::Started => {
                let step = event.step.unwrap_or(0);
                if step == 0 {
                    self.reasoning.clear();
                }
                if let Some(row) = self.reasoning.iter_mut().find(|r| r.step == step) {
                    row.mode = event.mode.clone();
                    row.done = false;
                } else {
                    self.reasoning.push(ReasoningRow {
                        chat_id: event.chat_id.clone(),
                        step,
                        mode: event.mode.clone(),
                        text: String::new(),
                        done: false,
                    });
                }
            }
            ReasoningPhase::Delta => match self.reasoning.last_mut() {
                Some(row) => row.text.push_str(&event.delta),
                None => self.reasoning.push(ReasoningRow {
                    chat_id: event.chat_id.clone(),
                    step: 0,
                    mode: String::new(),
                    text: event.delta.clone(),
                    done: false,
                }),
            },
            ReasoningPhase::Done => {
                let step = event.step.unwrap_or(0);
                if let Some(row) = self.reasoning.iter_mut().find(|r| r.step == step) {
                    row.mode = event.mode.clone();
                    if let Some(content) = &event.content {
                        row.text = content.clone();
                    }
                    row.done = true;
                }
            }
        }
    }
}

/// The rich-input controls of one pane.
///
/// The workspace is only a container: everything the composer shows for one
/// pty/agent session (auto-approve, model, git branch, harness mode and the
/// dropdown open flags) lives here, keyed by pane id, so two pty/agent windows
/// in the same workspace keep independent controls instead of sharing the
/// window-global ones.
#[derive(Clone, Debug)]
pub struct PaneControls {
    /// Whether this pane auto-approves `ask_user` questions.
    pub auto_approve: bool,
    /// This pane's model (shown as the composer's model label).
    pub model: String,
    /// This pane's git branch pill value.
    pub branch: String,
    /// Whether the harness is active for this pane: the rich input routes
    /// turns to the agent instead of the plain shell. Toggled at the rich
    /// input with Cmd+Enter; Esc returns the pane to the plain pty.
    pub harness_mode: bool,
    /// Which view this pane's terminal surface shows: the shell's own history
    /// (the terminal filter) or one conversation's agent view. Cmd+Enter and a
    /// click on a conversation's card enter that view; Esc returns to the
    /// terminal.
    pub view: BlockView,
    pub model_menu_open: Rc<RefCell<bool>>,
    pub profile_menu_open: Rc<RefCell<bool>>,
    pub harness_menu_open: Rc<RefCell<bool>>,
    pub dir_menu_open: Rc<RefCell<bool>>,
    pub branch_menu_open: Rc<RefCell<bool>>,
}

impl PaneControls {
    pub fn new(model: String, auto_approve: bool, branch: String) -> Self {
        Self {
            auto_approve,
            model,
            branch,
            harness_mode: false,
            view: BlockView::Terminal,
            model_menu_open: Rc::new(RefCell::new(false)),
            profile_menu_open: Rc::new(RefCell::new(false)),
            harness_menu_open: Rc::new(RefCell::new(false)),
            dir_menu_open: Rc::new(RefCell::new(false)),
            branch_menu_open: Rc::new(RefCell::new(false)),
        }
    }
}

impl Default for PaneControls {
    fn default() -> Self {
        Self::new(String::new(), false, String::new())
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
    /// App-owned open flags for each pane's agent-header 3-dots menu, keyed by
    /// pane id. Per-pane (rather than a single shared flag) so opening the tray
    /// in one split pane does not open it in the other panes sharing the view.
    pub agent_header_menus: HashMap<u64, Rc<RefCell<bool>>>,
    /// Per-terminal-block filter state (open flag + selected filter), keyed by
    /// the block's content key. Shared with the UI so the filter tray's open
    /// state + selection survive the per-frame element rebuild.
    pub terminal_filters: Rc<RefCell<HashMap<String, TerminalFilter>>>,
    /// Per-reasoning-row collapsed/expanded state, keyed by the row's
    /// `<conversation>:<step>` key. Shared with the UI so a row the user
    /// expanded stays expanded across the per-frame element rebuild.
    pub reasoning_expanded: Rc<RefCell<HashMap<String, bool>>>,
    /// Whole-transcript terminal filter (open flag + selected filter) per pane,
    /// so the filter bar of one pty/agent pane does not open in its sibling.
    pub terminal_global_filters: HashMap<u64, TerminalFilter>,
    pub crons_open: bool,
    pub crons: Vec<CronEntry>,
    /// Harness workflows (real daemon workflow store).
    pub workflows: Vec<WorkflowEntry>,
    /// Executions from the daemon execution ledger (real data).
    pub executions: Vec<ExecutionEntry>,
    /// Durable tasks from the persistence layer (real data).
    pub tasks: Vec<TaskEntry>,
    /// Chronological records derived from execution/session/task data.
    pub timeline: Vec<TimelineEntry>,
    /// Cost rows derived from real execution/usage records.
    pub costs: Vec<CostEntry>,
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
    /// Whether the Settings overlay panel is open (replaces the old Settings tab).
    pub settings_overlay_open: bool,
    /// Active settings category in the overlay.
    pub settings_category: SettingsCategory,
    /// Mouse: invert the wheel/scroll direction.
    pub settings_invert_scroll: bool,
    /// Mouse: scroll speed multiplier (1..=100, default 50).
    pub settings_scroll_speed: i32,
    /// Editor: font size as a whole-app zoom factor (mirrors `ui_zoom`).
    pub settings_font_size: f32,
    /// Custom theme color overrides as `#rrggbb` hex, set via the color wheel
    /// pickers in Settings→Appearance. `None` uses the built-in theme color.
    pub theme_primary: Option<String>,
    pub theme_secondary: Option<String>,
    pub theme_accent: Option<String>,
    /// Active color-wheel drag (which picker + region), owned here so the
    /// per-frame element rebuild keeps the drag alive.
    pub theme_color_drag: Rc<RefCell<Option<crate::ui::color_picker::ColorPickerDrag>>>,
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
    /// Whether the left conversation sidebar is shown. Toggled by the topbar's
    /// sidebar button; the main area fills the window when it is hidden.
    pub sidebar_visible: bool,
    /// Whether the sidebar's conversation list shows every conversation. While
    /// false only a few cards are drawn, followed by a "View all" button.
    pub conversations_expanded: bool,
    /// Scroll offset of the sidebar's conversation list (owned here so it
    /// survives the per-frame element rebuild).
    pub sidebar_scroll: Rc<RefCell<ScrollState>>,
    /// Scroll offset of the settings overlay's content pane.
    pub settings_scroll: Rc<RefCell<ScrollState>>,
    /// Whether the topbar workspace frame is in inline-rename mode (entered by
    /// double-clicking the active space's name).
    pub space_rename_editing: bool,
    /// The in-progress space name while renaming.
    pub space_rename_draft: String,
    /// Whether the rename field holds focus.
    pub space_rename_focused: bool,
    /// Per-card interaction state (hover / delete menu), owned here so it
    /// survives the per-frame element rebuild. Keyed by conversation id.
    pub agent_cards: HashMap<String, Rc<RefCell<AgentCardUi>>>,
    /// Hover flag for the sidebar's "New conversation" row, owned here so the row
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
    /// Per-pane rich-input controls (auto-approve, model, branch, harness mode,
    /// dropdown open flags), keyed by pane id so two pty/agent panes in the
    /// same workspace never share them.
    pub pane_controls: HashMap<u64, PaneControls>,
    /// Per-pane transcript scroll offset, keyed by pane id. Owned here (not on
    /// the element, which is rebuilt every frame) so a pane's scrollback
    /// position and its follow-the-stream state survive the rebuild.
    pub pane_chat_scroll: HashMap<u64, Rc<RefCell<ScrollState>>>,
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
    /// App-owned open flag for the terminal pane's "Run agent" menu.
    pub terminal_run_agent_menu_open: Rc<RefCell<bool>>,
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
            agent_header_menus: HashMap::new(),
            terminal_filters: Rc::new(RefCell::new(HashMap::new())),
            reasoning_expanded: Rc::new(RefCell::new(HashMap::new())),
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
            theme_primary: None,
            theme_secondary: None,
            theme_accent: None,
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
            pane_controls: HashMap::new(),
            pane_chat_scroll: HashMap::new(),
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

    /// The pane's own shell for an agent turn, when the pane has a terminal.
    ///
    /// A terminal pane's session is the harness's shell-tool route (P4): the
    /// agent's commands run in the user's own shell, visibly, instead of the
    /// sandbox. A pane that has never started a shell has no session and `None`
    /// keeps the sandboxed runner.
    pub fn pane_session(
        &self,
        pane_id: u64,
        conversation_id: &str,
    ) -> Option<std::sync::Arc<dyn goble_core::harness::PaneSession>> {
        self.terminal.borrow().pane_session(pane_id, conversation_id)
    }

    /// The blocks a pane's terminal view draws, oldest first: the shell's own
    /// history. A command the agent ran is a real block in this list too, since
    /// it ran in the same shell.
    pub fn pane_terminal_view(&self, pane_id: u64) -> Vec<VisibleBlock> {
        self.terminal
            .borrow()
            .visible_blocks(pane_id, &BlockView::Terminal)
    }

    /// The blocks a conversation's agent view draws in `pane_id`, oldest first:
    /// the commands the agent ran for that conversation. A command the user
    /// typed has an owner that names no conversation, so it is not one of them.
    pub fn pane_agent_view(&self, pane_id: u64, conversation_id: &str) -> Vec<VisibleBlock> {
        self.terminal.borrow().visible_blocks(
            pane_id,
            &BlockView::Agent {
                conversation_id: conversation_id.to_string(),
            },
        )
    }

    /// The view this pane's terminal surface shows. A pane with no controls
    /// entry yet shows the shell's own history.
    pub fn pane_view(&self, pane_id: u64) -> BlockView {
        self.pane_controls(pane_id).view
    }

    /// Enter `conversation_id`'s agent view in `pane_id`: push the card that
    /// stands for the conversation into the pane's block list (so the terminal
    /// keeps a way back to it) and point the pane's filter at the conversation.
    ///
    /// Returns the card's block id, or `None` when the pane has no live
    /// session. Entering the same conversation again reuses its card, so a
    /// round trip through the terminal leaves one card, not a stack.
    pub fn enter_agent_view(
        &mut self,
        pane_id: u64,
        conversation_id: &str,
        label: &str,
    ) -> Option<BlockId> {
        let block =
            self.terminal
                .borrow_mut()
                .push_agent_view_block(pane_id, conversation_id, label);
        self.pane_controls_mut(pane_id).view = BlockView::Agent {
            conversation_id: conversation_id.to_string(),
        };
        block
    }

    /// Leave the agent view: the pane goes back to the shell's own history.
    /// The card the conversation left behind stays in the list.
    pub fn leave_agent_view(&mut self, pane_id: u64) {
        self.pane_controls_mut(pane_id).view = BlockView::Terminal;
    }

    /// The display name of a conversation, falling back to its id when the
    /// sidebar does not know it (a card naming a conversation from elsewhere).
    pub fn conversation_name(&self, conversation_id: &str) -> String {
        self.conversations
            .iter()
            .find(|c| c.id == conversation_id)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| conversation_id.to_string())
    }

    /// Whether `pane_id` owns a conversation of its own (rather than lazily
    /// following the sidebar selection, as the initial pane does). A pane with
    /// no conversation of its own — a freshly opened PTY workspace — gets one
    /// created on its first agent turn.
    pub fn pane_owns_conversation(&self, pane_id: u64) -> bool {
        self.pane_sessions
            .get(&pane_id)
            .map(|s| !s.conversation_id.is_empty())
            .unwrap_or(false)
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
                // Only the rows whose content changed are re-parsed; the rest
                // are reused from the cache, so a frame with no new delta (or a
                // handful of deltas on one streaming row) costs no parsing for
                // the unchanged tail of the transcript.
                rt.messages = rt.parse_cache.resolve(&msgs);
            }
            Err(e) => {
                log::warn!("list_chat_messages({conv}): {e}");
            }
        }
        // The live reasoning steps are overlaid first, so they sit ahead of the
        // assistant message's prose; a running tool call is then overlaid after
        // the store read, visible even if its persisted row has not landed.
        overlay_reasoning(&mut rt.messages, &rt.reasoning);
        overlay_in_flight(&mut rt.messages, &rt.in_flight_tools);
        // The Local/Remote runtime decision is tracked per-conversation.
        if pane_id == self.active_pane_id {
            self.workspace_routing = desktop
                .get_chat_workspace_routing(conv)
                .ok()
                .flatten()
                .and_then(|s| routing_from_str(&s));
        }
    }

    /// Apply one live `chat:tool` event: a running call is held in the owning
    /// pane's in-flight map and overlaid on the transcript immediately, without
    /// re-reading the store; a finished/errored call clears its overlay entry,
    /// leaving the persisted terminal state to the next refresh.
    pub fn apply_tool_event(&mut self, call: &goble_desktop_service::ToolCallEvent) {
        let pane_id = self
            .pane_id_for_conversation(&call.chat_id)
            .unwrap_or(self.active_pane_id);
        {
            let rt = self.pane_runtime.entry(pane_id).or_default();
            if call.status == ToolCallStatus::Running {
                rt.in_flight_tools
                    .insert(call.id.clone(), tool_call_from_event(call));
            } else {
                rt.in_flight_tools.remove(&call.id);
            }
            overlay_in_flight(&mut rt.messages, &rt.in_flight_tools);
        }
        if pane_id == self.active_pane_id {
            self.sync_active_view();
        }
    }

    /// Apply one live `chat:reasoning` event: fold it into the owning pane's
    /// reasoning rows and overlay them on the transcript immediately, so the
    /// model's thinking appears as it streams instead of only after the store
    /// re-read that `chat:updated` triggers.
    pub fn apply_reasoning_event(&mut self, event: &goble_desktop_service::ReasoningEvent) {
        let pane_id = self
            .pane_id_for_conversation(&event.chat_id)
            .unwrap_or(self.active_pane_id);
        {
            let rt = self.pane_runtime.entry(pane_id).or_default();
            rt.apply_reasoning(event);
            overlay_reasoning(&mut rt.messages, &rt.reasoning);
        }
        if pane_id == self.active_pane_id {
            self.sync_active_view();
        }
    }

    /// How many live tool calls the pane is currently overlaying (test
    /// observability; not read by the app).
    pub fn pane_in_flight_tool_count(&self, pane_id: u64) -> usize {
        self.pane_runtime
            .get(&pane_id)
            .map(|rt| rt.in_flight_tools.len())
            .unwrap_or(0)
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

    /// This pane's transcript scroll state, creating nothing if it has no entry
    /// yet (a pane that was never prepared gets a fresh following state).
    pub fn pane_scroll(&self, pane_id: u64) -> Rc<RefCell<ScrollState>> {
        self.pane_chat_scroll
            .get(&pane_id)
            .cloned()
            .unwrap_or_else(|| Rc::new(RefCell::new(ScrollState::following())))
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
                    scroll: self.pane_scroll(*pane_id),
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
        // Theme is persisted in `~/.goble/config.toml`.
        let theme = desktop.config().theme;
        self.settings_dark_mode = theme.dark;
        self.theme_primary = theme.primary;
        self.theme_secondary = theme.secondary;
        self.theme_accent = Some(theme.accent);
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
            agent_header_menus: HashMap::new(),
            terminal_filters: Rc::new(RefCell::new(HashMap::new())),
            reasoning_expanded: Rc::new(RefCell::new(HashMap::new())),
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
            theme_primary: None,
            theme_secondary: None,
            theme_accent: None,
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
                    queued_prompt: None,
                    busy: false,
                    inline_screen_source: None,
                    screen_link: None,
                    in_flight_tools: HashMap::new(),
                    reasoning: Vec::new(),
                },
            )]),
            pane_controls: HashMap::new(),
            pane_chat_scroll: HashMap::new(),
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
        // Seed the per-pane rich-input controls for the restored tree as well.
        self.ensure_pane_controls();
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

    /// Rename the active space, persisting the pane layout. A blank name is
    /// ignored so the frame can never be left unnamed.
    pub fn rename_active_space(&mut self, name: String, desktop: Option<&DesktopState>) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if let Some(space) = self.spaces.get_mut(self.active_space) {
            space.name = name.to_string();
        }
        if let Some(desktop) = desktop {
            self.save_panes(desktop);
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
    use crate::emulator::Emulator;
    use crate::terminal::TerminalSession;

    #[test]
    fn mock_pane_is_bound_to_its_own_conversation() {
        let state = UiState::mock();
        let session = state.pane_sessions.get(&1).expect("pane 1 has a session");
        assert_eq!(session.conversation_id, "c1");
        assert_eq!(state.pane_conversation_id(1).as_deref(), Some("c1"));
    }

    /// The two views start as the shell's own history: the preamble block is in
    /// the terminal view, and no conversation has a block until the agent runs
    /// one in this pane.
    #[test]
    fn the_pane_views_start_as_the_shells_own_history() {
        let state = UiState::mock();
        state
            .terminal
            .borrow_mut()
            .sessions
            .insert(1, TerminalSession::with_emulator(Emulator::new(80, 24)));

        let terminal = state.pane_terminal_view(1);
        assert_eq!(terminal.len(), 1, "the preamble block is shell history");
        assert!(terminal[0].command.is_empty());
        assert!(
            state.pane_agent_view(1, "c1").is_empty(),
            "no conversation has run a command in this pane"
        );
        assert!(
            state.pane_terminal_view(9).is_empty(),
            "a pane with no session has no view"
        );
    }

    #[test]
    fn live_tool_event_fills_and_clears_the_pane_in_flight_map() {
        let mut state = UiState::mock();
        let running = goble_desktop_service::ToolCallEvent {
            chat_id: "c1".into(),
            id: "t1".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({"path": "src/lib.rs"}),
            status: ToolCallStatus::Running,
            result: None,
        };
        state.apply_tool_event(&running);
        assert_eq!(
            state.pane_in_flight_tool_count(1),
            1,
            "a started call gains an in-flight entry for its pane"
        );
        let overlaid = state
            .pane_runtime
            .get(&1)
            .and_then(|rt| {
                rt.messages
                    .iter()
                    .flat_map(|m| m.tool_calls.iter())
                    .find(|c| c.id == "t1")
            });
        assert!(
            overlaid.is_some_and(|c| c.status == ToolCallStatus::Running),
            "the running call is overlaid on the transcript without a store re-read"
        );

        // A second call is tracked independently.
        state.apply_tool_event(&goble_desktop_service::ToolCallEvent {
            id: "t2".into(),
            name: "run_command".into(),
            ..running.clone()
        });
        assert_eq!(state.pane_in_flight_tool_count(1), 2);

        let finished = goble_desktop_service::ToolCallEvent {
            status: ToolCallStatus::Finished,
            result: Some("ok".into()),
            ..running.clone()
        };
        state.apply_tool_event(&finished);
        assert_eq!(
            state.pane_in_flight_tool_count(1),
            1,
            "a finished call clears its in-flight entry"
        );

        state.apply_tool_event(&goble_desktop_service::ToolCallEvent {
            status: ToolCallStatus::Error,
            result: Some("boom".into()),
            ..goble_desktop_service::ToolCallEvent {
                id: "t2".into(),
                ..running
            }
        });
        assert_eq!(
            state.pane_in_flight_tool_count(1),
            0,
            "an errored call clears its in-flight entry too"
        );
    }

    #[test]
    fn reasoning_events_reach_the_transcript_and_render_recessed_rows() {
        use goble_desktop_service::{ReasoningEvent, ReasoningPhase};
        use goble_ui::elements::AppContext;
        use goble_ui::render::RenderCommand;
        use goble_ui::test_util::render_element;
        use goble_ui::theme::ColorToken;
        use goble_ui::{ChatView, Element};

        let event = |phase, step, mode: &str, delta: &str, content: Option<&str>| ReasoningEvent {
            chat_id: "c1".into(),
            step,
            mode: mode.to_string(),
            delta: delta.to_string(),
            content: content.map(str::to_string),
            decision: None,
            phase,
        };

        let mut state = UiState::mock();
        state.apply_reasoning_event(&event(
            ReasoningPhase::Started,
            Some(0),
            "contemplating",
            "",
            None,
        ));
        state.apply_reasoning_event(&event(
            ReasoningPhase::Delta,
            None,
            "",
            "weighing options",
            None,
        ));
        state.apply_reasoning_event(&event(
            ReasoningPhase::Done,
            Some(0),
            "contemplating",
            "",
            Some("weighing options"),
        ));

        let rows = &state.pane_runtime.get(&1).unwrap().reasoning;
        assert_eq!(
            rows.len(),
            1,
            "the deltas accumulate into one reasoning step"
        );
        assert_eq!(rows[0].text, "weighing options");
        assert!(rows[0].done, "the done transition finalises the step");

        let messages = state.pane_runtime.get(&1).unwrap().messages.clone();
        assert!(
            messages
                .iter()
                .flat_map(|m| m.fragments.iter())
                .any(|f| matches!(
                    &f.kind,
                    goble_ui::ChatFragmentKind::Reasoning { text, .. } if text == "weighing options"
                )),
            "the reasoning step is overlaid on the transcript"
        );

        let app = AppContext::default();
        let render = |state: &UiState| -> Vec<RenderCommand> {
            let messages = state.pane_runtime.get(&1).unwrap().messages.clone();
            let mut view: Box<dyn goble_ui::Element> = ChatView::new()
                .with_messages(messages)
                .with_reasoning_expanded(state.reasoning_expanded.clone())
                .finish();
            render_element(&mut view, goble_ui::vec2f(600.0, 400.0), &app)
        };
        let has_text = |commands: &[RenderCommand], needle: &str| {
            commands
                .iter()
                .any(|c| matches!(c, RenderCommand::DrawText { text, .. } if text.contains(needle)))
        };

        let collapsed = render(&state);
        assert!(
            has_text(&collapsed, "Thinking"),
            "the reasoning header renders"
        );
        assert!(
            !has_text(&collapsed, "weighing options"),
            "the thinking body is collapsed by default"
        );
        let header_color = collapsed.iter().find_map(|c| match c {
            RenderCommand::DrawText { text, color, .. } if text.contains("Thinking") => {
                Some(*color)
            }
            _ => None,
        });
        assert_eq!(
            header_color,
            Some(app.theme.color(ColorToken::Muted)),
            "the reasoning row is recessed (muted)"
        );

        // Expanding the app-owned row shows the thinking body.
        state
            .reasoning_expanded
            .borrow_mut()
            .insert(reasoning_row_key("c1", 0), true);
        let expanded = render(&state);
        assert!(
            has_text(&expanded, "weighing options"),
            "an expanded reasoning row shows its body"
        );
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

    #[test]
    fn pane_controls_are_isolated_per_pane() {
        let mut state = UiState::mock();
        state.active_pane_id = 1;
        state.selected_model = "global-model".into();
        {
            let c1 = state.pane_controls_mut(1);
            c1.model = "m1".into();
            c1.auto_approve = true;
            c1.harness_mode = true;
            let flag = c1.model_menu_open.clone();
            *flag.borrow_mut() = true;
        }
        {
            let c2 = state.pane_controls_mut(2);
            c2.model = "m2".into();
        }
        // Two pty/agent panes in one workspace never share the composer state.
        assert_eq!(state.pane_controls(1).model, "m1");
        assert_eq!(state.pane_controls(2).model, "m2");
        assert!(state.pane_controls(1).auto_approve);
        assert!(!state.pane_controls(2).auto_approve);
        assert!(state.pane_controls(1).harness_mode);
        assert!(!state.pane_controls(2).harness_mode);
        let menu_1 = state.pane_controls(1).model_menu_open;
        let menu_2 = state.pane_controls(2).model_menu_open;
        assert!(*menu_1.borrow(), "pane 1's model menu is open");
        assert!(!*menu_2.borrow(), "pane 2's model menu stays closed");
        // A pane with no entry yet reads the window globals.
        assert_eq!(state.pane_controls(99).model, "global-model");
        // Every rendered pane gets an entry when the snapshot is prepared.
        state.spaces = vec![Space::new(
            "A",
            Pane::Split {
                id: 10,
                dir: crate::ui::SplitDir::Vertical,
                ratio: 0.5,
                first: Box::new(Pane::Leaf { id: 1, kind: PaneKind::Terminal }),
                second: Box::new(Pane::Leaf { id: 2, kind: PaneKind::Terminal }),
            },
        )];
        state.ensure_pane_controls();
        assert!(state.pane_controls.contains_key(&1));
        assert!(state.pane_controls.contains_key(&2));
    }

    #[test]
    fn refresh_reparses_only_the_changed_message() {
        let dir = tempfile::tempdir().expect("create temp thread store dir");
        let desktop = DesktopState::new(
            goble_core::store::Store::open_in_memory().expect("open in-memory store"),
            goble_desktop_service::ThreadStore::new(dir.path()).expect("open thread store"),
        );
        let chat_id = desktop
            .create_chat("Stream", None, None)
            .expect("create chat");
        let store = desktop.store_clone();
        let now = "2026-09-10T00:00:00Z";
        store
            .insert_chat_message("m1", &chat_id, "user", "hello", None, now)
            .expect("insert user row");
        store
            .insert_chat_message("m2", &chat_id, "assistant", "first ", None, now)
            .expect("insert assistant row");

        let mut state = UiState::mock();
        state.pane_sessions.get_mut(&1).unwrap().conversation_id = chat_id.clone();
        state.selected_id = Some(chat_id.clone());
        let parses = |s: &UiState| s.pane_runtime.get(&1).unwrap().parse_cache.parse_count();

        state.refresh_messages(&desktop);
        assert_eq!(parses(&state), 2, "the first load parses every row once");
        assert_eq!(state.pane_runtime.get(&1).unwrap().messages.len(), 2);

        // A frame with no new delta must re-parse nothing.
        state.refresh_messages(&desktop);
        assert_eq!(parses(&state), 2, "an unchanged refresh parses nothing");

        // One delta lands on the last row; only that row is re-parsed.
        store
            .append_chat_message_content("m2", "delta")
            .expect("append delta");
        state.refresh_messages(&desktop);
        assert_eq!(parses(&state), 3, "only the changed row is re-parsed");

        let msgs = &state.pane_runtime.get(&1).unwrap().messages;
        assert_eq!(msgs.len(), 2, "both rows are still in the transcript");
        assert_eq!(
            msgs[0].fragments,
            ChatMessage::from_markdown(ChatRole::User, "hello").fragments
        );
        assert_eq!(
            msgs[1].fragments,
            ChatMessage::from_markdown(ChatRole::Assistant, "first delta").fragments,
            "the delta is reflected in the re-parsed message"
        );
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
