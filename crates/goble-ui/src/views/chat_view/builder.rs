use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::elements::chat_content::{ChatAction, ChatMessage, SubAgentRow, ToolDisplayMode};
use crate::elements::{
    AskUserUi, CommandProposalUi, Element, PopupMenuItem, ScrollState, TerminalFilter, TurnStatus,
};
use crate::vim::{Clipboard, VimState};
use goble_core::harness::CommandDecision;
use goble_core::llm::TokenUsage;

use super::{ChatView, InlineScreen};

impl ChatView {
    /// Attach the app-owned transcript scroll state, so the offset (and the
    /// user's scrollback position) survives the per-frame rebuild.
    pub fn with_scroll_state(mut self, scroll: Rc<RefCell<ScrollState>>) -> Self {
        self.scroll = scroll;
        self
    }

    /// Mark whether this chat pane is the workspace's active pane, so only the
    /// active pane's transcript takes the fold key.
    pub fn with_pane_active(mut self, active: bool) -> Self {
        self.pane_active = active;
        self
    }

    /// Drop the composer and its divider: a transcript that only shows a
    /// conversation (the sub-agent child view) must not offer an input line
    /// that writes to a different one.
    pub fn without_composer(mut self) -> Self {
        self.show_composer = false;
        self
    }

    /// Wire the transcript-level Escape key: fired when this pane is active and
    /// nothing inside it consumed the key first (the child view's way back to
    /// the parent's transcript).
    pub fn with_on_escape<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_escape = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_header(mut self, header: Box<dyn Element>) -> Self {
        self.header = Some(header);
        self
    }

    /// Render `notice` at the top of the transcript, inside the chat area.
    pub fn with_notice(mut self, notice: Option<Box<dyn Element>>) -> Self {
        self.notice = notice;
        self
    }

    pub fn with_messages(mut self, messages: Vec<ChatMessage>) -> Self {
        self.messages = messages;
        self
    }

    pub fn with_quick_action<F: FnMut() + 'static>(
        mut self,
        label: impl Into<String>,
        callback: F,
    ) -> Self {
        self.quick_actions
            .push((label.into(), Rc::new(RefCell::new(callback))));
        self
    }

    pub fn with_on_send<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_send = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Cmd/Ctrl+Enter in the composer starts a NEW agent conversation (warp-new).
    pub fn with_composer_on_cmd_enter<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_cmd_enter = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_action<F: FnMut(ChatAction) + 'static>(mut self, callback: F) -> Self {
        self.on_action = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the app-owned per-block terminal filter map (shared with app state so
    /// the tray open flag + selection persist across the per-frame rebuild).
    pub fn with_terminal_filters(
        mut self,
        terminal_filters: Rc<RefCell<HashMap<String, TerminalFilter>>>,
    ) -> Self {
        self.terminal_filters = terminal_filters;
        self
    }

    /// Set the app-owned collapsed/expanded map for the transcript's reasoning
    /// rows, so a row the user expanded stays expanded across rebuilds.
    pub fn with_reasoning_expanded(
        mut self,
        reasoning_expanded: Rc<RefCell<HashMap<String, bool>>>,
    ) -> Self {
        self.reasoning_expanded = reasoning_expanded;
        self
    }

    /// Set the app-owned three-state fold map for the transcript's tool calls,
    /// keyed by [`tool_fold_key`], so a fold survives the rebuild.
    pub fn with_tool_fold(
        mut self,
        tool_fold: Rc<RefCell<HashMap<String, ToolDisplayMode>>>,
    ) -> Self {
        self.tool_fold = tool_fold;
        self
    }

    /// Set the live sub-agent records for this pane's transcript, keyed by the
    /// id of the tool call that spawned each child. The map is rebuilt from the
    /// app's S4 events every frame, so a running child's row re-draws with it.
    pub fn with_sub_agents(mut self, sub_agents: HashMap<String, SubAgentRow>) -> Self {
        self.sub_agents = sub_agents;
        self
    }

    /// Set the whole-transcript filter (app-owned), applied to every terminal
    /// block in this conversation.
    pub fn with_global_terminal_filter(mut self, filter: Option<TerminalFilter>) -> Self {
        self.global_terminal_filter = filter;
        self
    }

    /// Set the handler for a terminal block's copy button (receives the block
    /// text); `None` hides no-op copy (the button is hidden if not provided).
    pub fn with_on_copy_terminal(
        mut self,
        on_copy: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    ) -> Self {
        self.on_copy_terminal = on_copy;
        self
    }

    pub fn with_empty_state(
        mut self,
        title: impl Into<String>,
        subtitle: impl Into<String>,
    ) -> Self {
        self.empty_title = Some(title.into());
        self.empty_subtitle = Some(subtitle.into());
        self
    }

    pub fn composer_value(&self) -> String {
        self.composer_value.borrow().clone()
    }

    /// Sets the composer draft from external state (the snapshot). The draft is
    /// re-applied on every layout, so typing is driven by the app-owned value.
    pub fn with_composer_value(self, value: impl Into<String>) -> Self {
        *self.composer_value.borrow_mut() = value.into();
        self
    }

    pub fn with_composer_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_composer_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_composer_focused(mut self, focused: bool) -> Self {
        self.composer_focused = focused;
        self
    }

    /// Share the composer's insertion point with the host, so the beam survives
    /// the per-frame rebuild.
    pub fn with_composer_caret(mut self, caret: Rc<RefCell<usize>>) -> Self {
        self.composer_caret = caret;
        self
    }

    /// The tokens this conversation has spent, from the provider's own counts.
    /// Drawn as the transcript's footer disclosure; accounting only.
    pub fn with_usage(mut self, usage: TokenUsage) -> Self {
        self.usage = usage;
        self
    }

    /// Share the usage disclosure's expanded state with the host, so it
    /// survives the per-frame rebuild.
    pub fn with_usage_open(mut self, open: Rc<RefCell<bool>>) -> Self {
        self.usage_open = open;
        self
    }

    /// Fork this conversation into a new one (the transcript footer's Fork).
    pub fn with_on_fork<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_fork = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Turn on modal (vim) editing for the composer editor: the host's mode
    /// state and the system clipboard its `"+`/`"*` registers reach.
    pub fn with_composer_vim(
        mut self,
        vim: Rc<RefCell<VimState>>,
        clipboard: Rc<RefCell<dyn Clipboard>>,
    ) -> Self {
        self.composer_vim = Some(vim);
        self.composer_clipboard = Some(clipboard);
        self
    }

    pub fn with_composer_model_label(mut self, label: impl Into<String>) -> Self {
        self.composer_model_label = Some(label.into());
        self
    }

    pub fn with_composer_path(mut self, path: impl Into<String>) -> Self {
        self.composer_path = Some(path.into());
        self
    }

    pub fn with_composer_harness_label(mut self, label: impl Into<String>) -> Self {
        self.composer_harness_label = Some(label.into());
        self
    }

    pub fn with_composer_branch_label(mut self, label: impl Into<String>) -> Self {
        self.composer_branch_label = Some(label.into());
        self
    }

    pub fn with_composer_stop_visible(mut self, visible: bool) -> Self {
        self.composer_stop_visible = visible;
        self
    }

    /// Set the live turn-status footer's state (the app reads it from C1's live
    /// accessor). [`TurnStatus::Idle`] draws nothing and takes zero height.
    pub fn with_turn_status(mut self, status: TurnStatus) -> Self {
        self.turn_status = status;
        self
    }

    pub fn with_composer_on_focus_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_composer_focus_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_composer_on_attach<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_attach = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_composer_on_voice<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_voice = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_composer_on_select_model<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_model = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_composer_on_stop<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_stop = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the composer's model dropdown (items, app-owned open flag, select callback).
    pub fn with_composer_model_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.composer_model_items = items;
        self.composer_model_menu_open = open;
        self.on_select_model_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the composer's harness dropdown (items, open flag, select callback).
    pub fn with_composer_harness_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.composer_harness_items = items;
        self.composer_harness_menu_open = open;
        self.on_select_harness_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the composer's working-directory dropdown.
    pub fn with_composer_dir_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.composer_dir_items = items;
        self.composer_dir_menu_open = open;
        self.on_select_dir_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the composer's git-branch dropdown.
    pub fn with_composer_branch_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.composer_branch_items = items;
        self.composer_branch_menu_open = open;
        self.on_select_branch_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired when the composer draft begins with `/` (slash command).
    pub fn with_composer_on_slash<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_composer_slash = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the pending ask rendered inline at the end of the transcript. When
    /// `Some`, the chat renders an `AskUserCard` (warp-new model) in the
    /// message stream instead of pinning a question to the composer.
    pub fn with_pending_ask(mut self, ask: Option<AskUserUi>) -> Self {
        self.pending_ask = ask;
        self
    }

    /// Fired with the composed response when the user submits the inline ask.
    pub fn with_on_answer_ask<F: FnMut(String, Option<(String, String)>) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_answer_ask = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired when the user skips the inline ask.
    pub fn with_on_skip_ask<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_skip_ask = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Show a command the harness suspended on for approval (A6/R1) in the
    /// composer's proposal card.
    pub fn with_command_proposal(mut self, proposal: Option<CommandProposalUi>) -> Self {
        self.composer_proposal = proposal;
        self
    }

    /// The app-owned selected candidate index for the proposal card.
    pub fn with_command_proposal_selection(mut self, selected: Rc<RefCell<usize>>) -> Self {
        self.composer_proposal_selection = selected;
        self
    }

    /// Fired with `(proposal id, decision)` when the user approves, edits or
    /// rejects a proposed command, so the host can resume the suspended turn.
    pub fn with_on_command_decision<F: FnMut(String, CommandDecision) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_command_decision = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Whether the agent auto-approves `ask_user` questions (skip the ask and
    /// continue). Shown as a toggle in the controls strip above the composer.
    pub fn with_auto_approve(mut self, enabled: bool) -> Self {
        self.auto_approve = enabled;
        self
    }

    /// Fired when the user toggles the auto-approve switch.
    pub fn with_on_toggle_auto_approve<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_auto_approve = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// A prompt queued while the agent was busy, shown inline as a pending block.
    pub fn with_queued_prompt(mut self, prompt: Option<String>) -> Self {
        self.queued_prompt = prompt;
        self
    }

    /// Fired when the user clicks "Send now" on a queued prompt (sends it
    /// immediately and interrupts the in-flight turn).
    pub fn with_on_send_queued<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_send_queued = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired when the user dismisses a queued prompt.
    pub fn with_on_dismiss_queued<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_dismiss_queued = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set a live remote-desktop frame to render inline at the end of the
    /// transcript, marking the harness's handoff of control (computer use).
    pub fn with_inline_screen(
        mut self,
        source: impl Into<String>,
        frame_seq: u64,
        width: u32,
        height: u32,
        data: Arc<[u8]>,
    ) -> Self {
        self.inline_screen = Some(InlineScreen {
            source: source.into(),
            frame_seq,
            width,
            height,
            data,
        });
        self
    }

    /// Fired when the user closes the inline remote-desktop block.
    pub fn with_on_close_inline_screen<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_close_inline_screen = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// A detected BYOH desktop-handoff URI, rendered as an open-roremote action.
    pub fn with_screen_link(mut self, link: Option<String>) -> Self {
        self.screen_link = link;
        self
    }

    /// Fired when the user clicks the detected handoff link to open the desktop.
    pub fn with_on_open_screen_link<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_open_screen_link = Some(Rc::new(RefCell::new(callback)));
        self
    }
}
