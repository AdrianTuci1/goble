//! The chat pane's transcript: the identity header, the scrollable message list
//! and the rich composer pinned below it.
//!
//! One module per surface of the pane: the builder chain that wires the view up
//! ([`builder`]), the transcript's per-frame rebuild and its key handling
//! ([`transcript`]), and the element protocol the view answers ([`element`]).
//! What the surfaces share — the view's own state and the inline screen frame it
//! renders — lives here.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::elements::chat_content::{ChatAction, ChatMessage, SubAgentRow, ToolDisplayMode};
use crate::elements::{
    AskUserUi, CommandProposalUi, Element, Point, PopupMenuItem, ScrollState, TerminalFilter,
    TurnStatus,
};
use crate::geometry::Vector2F;
use crate::vim::{Clipboard, VimState};
use goble_core::harness::CommandDecision;
use goble_core::llm::TokenUsage;

mod builder;
mod element;
mod transcript;

#[cfg(test)]
mod tests;

/// A live remote-desktop frame rendered inline at the end of the transcript.
#[derive(Clone)]
struct InlineScreen {
    source: String,
    frame_seq: u64,
    width: u32,
    height: u32,
    data: Arc<[u8]>,
}

pub struct ChatView {
    header: Option<Box<dyn Element>>,
    /// An inline notice rendered at the top of the transcript (inside the chat
    /// area rather than as a floating dialog), e.g. the "no API key" error.
    notice: Option<Box<dyn Element>>,
    messages: Vec<ChatMessage>,
    quick_actions: Vec<(String, Rc<RefCell<dyn FnMut() + 'static>>)>,
    on_send: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_cmd_enter: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_action: Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    /// App-owned per-block filter state, keyed by a terminal block's content
    /// key; shared down to each `ChatMessageBubble` so the filter tray's open
    /// flag + selection persist across the per-frame rebuild.
    terminal_filters: Rc<RefCell<HashMap<String, TerminalFilter>>>,
    /// App-owned collapsed/expanded state for the reasoning rows, keyed by the
    /// reasoning fragment's key, shared down to each `ChatMessageBubble`.
    reasoning_expanded: Rc<RefCell<HashMap<String, bool>>>,
    /// App-owned three-state fold for the tool calls, keyed by
    /// [`tool_fold_key`], shared down to each `ChatMessageBubble`.
    tool_fold: Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    /// The live sub-agent records the tool-call rows read their status from,
    /// keyed by the id of the tool call that spawned each child; held by the
    /// app from S4's `chat:subagent_*` events and handed down every frame.
    sub_agents: HashMap<String, SubAgentRow>,
    /// The whole-transcript filter, applied to every terminal block (filtered
    /// via a bar above the transcript). App-owned so it survives the rebuild.
    global_terminal_filter: Option<TerminalFilter>,
    on_copy_terminal: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    empty_title: Option<String>,
    empty_subtitle: Option<String>,
    composer_value: Rc<RefCell<String>>,
    /// The composer editor's insertion point. Shared with the host because the
    /// view is rebuilt every frame, like the draft itself.
    composer_caret: Rc<RefCell<usize>>,
    /// Modal (vim) editing for the composer editor, when the user turned it on:
    /// the host's mode state and the system clipboard its registers reach.
    composer_vim: Option<Rc<RefCell<VimState>>>,
    composer_clipboard: Option<Rc<RefCell<dyn Clipboard>>>,
    /// What the conversation has spent, from the provider's own token counts.
    /// Drawn at the end of the transcript; accounting only, never a price.
    usage: TokenUsage,
    /// Whether the usage disclosure is expanded. App-owned so it survives the
    /// per-frame rebuild.
    usage_open: Rc<RefCell<bool>>,
    /// Fork the conversation into a new one. `None` on a surface that cannot
    /// (a mock/dev transcript with no store behind it).
    on_fork: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    composer_focused: bool,
    composer_model_label: Option<String>,
    composer_path: Option<String>,
    composer_stop_visible: bool,
    /// The live turn-status footer: what the pane is doing, or what is still
    /// running once its turn stops. [`TurnStatus::Idle`] gives the row zero
    /// height, so it costs nothing when nothing is in flight.
    turn_status: TurnStatus,
    composer_harness_label: Option<String>,
    composer_branch_label: Option<String>,
    on_composer_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_composer_focus_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    on_attach: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_voice: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_model: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_stop: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    composer_model_items: Vec<PopupMenuItem>,
    composer_model_menu_open: Rc<RefCell<bool>>,
    on_select_model_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    composer_harness_items: Vec<PopupMenuItem>,
    composer_harness_menu_open: Rc<RefCell<bool>>,
    on_select_harness_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    composer_dir_items: Vec<PopupMenuItem>,
    composer_dir_menu_open: Rc<RefCell<bool>>,
    on_select_dir_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    composer_branch_items: Vec<PopupMenuItem>,
    composer_branch_menu_open: Rc<RefCell<bool>>,
    on_select_branch_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    on_composer_slash: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    pending_ask: Option<AskUserUi>,
    on_answer_ask: Option<Rc<RefCell<dyn FnMut(String, Option<(String, String)>) + 'static>>>,
    on_skip_ask: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    /// The command the harness suspended on for approval (A6/R1). While set, the
    /// composer draws the proposal card and its editor holds the selected
    /// candidate as the in-place edit surface.
    composer_proposal: Option<CommandProposalUi>,
    /// The selected candidate index. App-owned so the choice survives the
    /// per-frame composer rebuild.
    composer_proposal_selection: Rc<RefCell<usize>>,
    on_command_decision: Option<Rc<RefCell<dyn FnMut(String, CommandDecision) + 'static>>>,
    auto_approve: bool,
    on_toggle_auto_approve: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    /// A prompt the user sent while the agent was running, queued (not
    /// interrupted) and shown inline as a pending block (warp-new model).
    queued_prompt: Option<String>,
    on_send_queued: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_dismiss_queued: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    /// A live remote desktop stream shown inline at the end of the transcript
    /// when the harness hands off control (the "harness takes over" moment).
    inline_screen: Option<InlineScreen>,
    on_close_inline_screen: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    /// A detected BYOH handoff URI rendered as a "open remote desktop" action.
    screen_link: Option<String>,
    on_open_screen_link: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    /// The transcript's scroll offset. App-owned in practice (the view is
    /// rebuilt every frame); defaults to following the stream so a standalone
    /// view still clips and tails its content.
    scroll: Rc<RefCell<ScrollState>>,
    /// Whether this chat pane is the workspace's active pane. Only the active
    /// pane's transcript takes the fold key (`e`), so a sibling pane sharing the
    /// tree cannot swallow it. A standalone view is active.
    pane_active: bool,
    /// A read-only transcript (the sub-agent child view) drops the composer and
    /// its divider: the input line belongs to the conversation the pane sends
    /// to, so while the pane shows another conversation it would edit nothing.
    show_composer: bool,
    /// The transcript-level Escape handler, fired while the pane is active and
    /// nothing inside consumed the key first. The child view uses it to return
    /// to the parent's transcript; a plain transcript wires nothing, so Escape
    /// keeps reaching whatever is focused (the composer's proposal, say).
    on_escape: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatView {
    pub fn new() -> Self {
        Self {
            header: None,
            notice: None,
            messages: Vec::new(),
            quick_actions: Vec::new(),
            on_send: None,
            on_cmd_enter: None,
            on_action: None,
            terminal_filters: Rc::new(RefCell::new(HashMap::new())),
            reasoning_expanded: Rc::new(RefCell::new(HashMap::new())),
            tool_fold: Rc::new(RefCell::new(HashMap::new())),
            sub_agents: HashMap::new(),
            global_terminal_filter: None,
            on_copy_terminal: None,
            empty_title: None,
            empty_subtitle: None,
            composer_value: Rc::new(RefCell::new(String::new())),
            composer_caret: Rc::new(RefCell::new(0)),
            composer_vim: None,
            composer_clipboard: None,
            usage: TokenUsage::default(),
            usage_open: Rc::new(RefCell::new(false)),
            on_fork: None,
            composer_focused: false,
            composer_model_label: None,
            composer_path: None,
            composer_stop_visible: false,
            turn_status: TurnStatus::Idle,
            on_composer_change: None,
            on_composer_focus_change: None,
            on_attach: None,
            on_voice: None,
            on_select_model: None,
            on_stop: None,
            composer_model_items: Vec::new(),
            composer_model_menu_open: Rc::new(RefCell::new(false)),
            on_select_model_item: None,
            composer_harness_label: None,
            composer_branch_label: None,
            composer_harness_items: Vec::new(),
            composer_harness_menu_open: Rc::new(RefCell::new(false)),
            on_select_harness_item: None,
            composer_dir_items: Vec::new(),
            composer_dir_menu_open: Rc::new(RefCell::new(false)),
            on_select_dir_item: None,
            composer_branch_items: Vec::new(),
            composer_branch_menu_open: Rc::new(RefCell::new(false)),
            on_select_branch_item: None,
            on_composer_slash: None,
            pending_ask: None,
            on_answer_ask: None,
            on_skip_ask: None,
            composer_proposal: None,
            composer_proposal_selection: Rc::new(RefCell::new(0)),
            on_command_decision: None,
            auto_approve: false,
            on_toggle_auto_approve: None,
            queued_prompt: None,
            on_send_queued: None,
            on_dismiss_queued: None,
            inline_screen: None,
            on_close_inline_screen: None,
            screen_link: None,
            on_open_screen_link: None,
            scroll: Rc::new(RefCell::new(ScrollState::following())),
            pane_active: true,
            show_composer: true,
            on_escape: None,
            root: None,
            size: None,
            origin: None,
        }
    }
}

impl Default for ChatView {
    fn default() -> Self {
        Self::new()
    }
}
