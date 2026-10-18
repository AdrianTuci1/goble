//! The global Inbox: approvals and questions the agent is waiting on, plus the
//! terminal reports of turns and sub-agents.
//!
//! Items are event-derived (see [`super::agents`]); nothing here invents an
//! item for a state no event reported. The list is app-wide — one pane's
//! approval is reachable while another pane is on screen — and
//! [`UiState::jump_to_inbox_item`] is the action that takes the user back to the
//! pane the item belongs to.

use chrono::{DateTime, Utc};

use super::*;

/// What an [`InboxItem`] is. The kind decides the row's label and colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InboxKind {
    /// The harness suspended on a command and is waiting for a decision.
    Approval,
    /// The agent asked the user a question.
    Question,
    /// A turn completed successfully.
    Done,
    /// A turn ended in an error.
    Error,
    /// A sub-agent failed.
    Agent,
}

impl InboxKind {
    /// The word the overlay draws beside the row.
    pub fn label(self) -> &'static str {
        match self {
            Self::Approval => "APPROVAL",
            Self::Question => "QUESTION",
            Self::Done => "DONE",
            Self::Error => "ERROR",
            Self::Agent => "AGENT",
        }
    }
}

/// One thing the global Inbox holds. `pane_id` is what [`UiState::jump_to_inbox_item`]
/// focuses; `space_id` is the space the pane was in when the item was raised (a
/// jump re-locates the pane, because spaces reorder).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InboxItem {
    pub id: String,
    pub pane_id: u64,
    pub space_id: usize,
    pub conversation_id: String,
    pub kind: InboxKind,
    pub title: String,
    pub detail: String,
    pub created_at: DateTime<Utc>,
    pub acknowledged: bool,
}

impl InboxItem {
    /// Build an unacknowledged item. The caller supplies the id when it has one
    /// worth deduping on; [`UiState::raise_inbox`] allocates one otherwise.
    pub fn new(
        id: impl Into<String>,
        pane_id: u64,
        space_id: usize,
        conversation_id: impl Into<String>,
        kind: InboxKind,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            pane_id,
            space_id,
            conversation_id: conversation_id.into(),
            kind,
            title: title.into(),
            detail: detail.into(),
            created_at: Utc::now(),
            acknowledged: false,
        }
    }
}

impl UiState {
    /// Add an item to the inbox, newest first. Returns whether it was added.
    ///
    /// Dedupe is two-fold: an item with the same id is never added twice, and
    /// an unacknowledged item already covering the same conversation and kind
    /// is not duplicated — one approval per conversation, one question, and so
    /// on. An acknowledged item does not suppress the next report of its kind.
    pub fn push_inbox(&mut self, item: InboxItem) -> bool {
        if self.inbox.iter().any(|existing| existing.id == item.id) {
            return false;
        }
        if !item.conversation_id.is_empty()
            && self.inbox.iter().any(|existing| {
                !existing.acknowledged
                    && existing.kind == item.kind
                    && existing.conversation_id == item.conversation_id
            })
        {
            return false;
        }
        self.inbox.insert(0, item);
        self.inbox_index = 0;
        self.inbox_error = None;
        true
    }

    /// Raise an event-derived item on a pane, allocating its id. The title and
    /// detail are the words the overlay draws; they come from the event, never
    /// from a guess about what the event probably meant.
    pub fn raise_inbox(
        &mut self,
        pane_id: u64,
        conversation_id: impl Into<String>,
        kind: InboxKind,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> String {
        let id = format!("inbox-{}", self.next_inbox_id);
        self.next_inbox_id += 1;
        let space_id = self
            .space_index_of_pane(pane_id)
            .unwrap_or(self.active_space);
        self.push_inbox(InboxItem {
            id: id.clone(),
            pane_id,
            space_id,
            conversation_id: conversation_id.into(),
            kind,
            title: title.into(),
            detail: detail.into(),
            created_at: Utc::now(),
            acknowledged: false,
        });
        id
    }

    /// Acknowledge one item. Returns whether the item existed and was unread.
    pub fn ack_inbox(&mut self, id: &str) -> bool {
        let Some(item) = self.inbox.iter_mut().find(|item| item.id == id) else {
            return false;
        };
        if item.acknowledged {
            return false;
        }
        item.acknowledged = true;
        true
    }

    /// Acknowledge every item. Returns how many were unread.
    pub fn ack_all(&mut self) -> usize {
        let mut acked = 0;
        for item in &mut self.inbox {
            if !item.acknowledged {
                item.acknowledged = true;
                acked += 1;
            }
        }
        acked
    }

    /// How many items have not been acknowledged.
    pub fn unread_inbox_count(&self) -> usize {
        self.inbox.iter().filter(|item| !item.acknowledged).count()
    }

    /// Move to the space and pane an item belongs to. `index` is a position in
    /// the list as the overlay draws it (newest first), which is the same order
    /// `self.inbox` holds. Returns whether the jump landed; a pane that has
    /// since been closed leaves the overlay open and records why.
    pub fn jump_to_inbox_item(&mut self, index: usize) -> bool {
        let Some(item) = self.inbox.get(index) else {
            self.inbox_error = Some("No inbox item is selected.".to_string());
            return false;
        };
        let pane_id = item.pane_id;
        let Some(space_id) = self.space_index_of_pane(pane_id) else {
            self.inbox_error = Some(format!(
                "Pane {pane_id} is no longer open; the item has nothing to jump to."
            ));
            return false;
        };
        self.inbox_error = None;
        if let Some(item) = self.inbox.get_mut(index) {
            item.space_id = space_id;
        }
        self.active_space = space_id;
        self.spaces[space_id].recompute_layout();
        self.active_pane_id = pane_id;
        self.current_tab = AppTab::Chat;
        self.sync_active_view();
        true
    }

    /// Drop every item a pane owns when that pane closes.
    pub fn prune_inbox_for_pane(&mut self, pane_id: u64) {
        let before = self.inbox.len();
        self.inbox.retain(|item| item.pane_id != pane_id);
        if self.inbox.len() != before {
            self.clamp_inbox_index();
            self.inbox_error = None;
        }
    }

    /// The space holding `pane_id`, if any.
    pub fn space_index_of_pane(&self, pane_id: u64) -> Option<usize> {
        self.spaces
            .iter()
            .position(|space| space.root.contains_leaf(pane_id))
    }

    /// Keep the selection inside the list after items leave it.
    pub fn clamp_inbox_index(&mut self) {
        self.inbox_index = if self.inbox.is_empty() {
            0
        } else {
            self.inbox_index.min(self.inbox.len() - 1)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Pane, PaneAgentState, PaneSession, Space, TurnOutcome};
    use crate::terminal::{TerminalMode, TuiAgent};

    use super::super::agents::tui_agent_state;

    fn proposal(chat_id: &str, id: &str, command: &str) -> goble_desktop_service::CommandProposedEvent {
        goble_desktop_service::CommandProposedEvent {
            chat_id: chat_id.to_string(),
            id: id.to_string(),
            candidates: vec![command.to_string()],
            cwd: "/workspace".to_string(),
        }
    }

    #[test]
    fn begin_turn_reports_working() {
        let mut state = UiState::mock();
        state.begin_turn(1);
        assert_eq!(state.pane_agent_state(1), PaneAgentState::Working);
    }

    #[test]
    fn a_command_proposal_reports_needs_input_and_raises_an_approval() {
        let mut state = UiState::mock();
        state.apply_command_proposed(&proposal("c1", "call-1", "git status"));
        assert_eq!(state.pane_agent_state(1), PaneAgentState::NeedsInput);
        assert_eq!(state.inbox.len(), 1);
        let item = &state.inbox[0];
        assert_eq!(item.kind, InboxKind::Approval);
        assert_eq!(item.pane_id, 1);
        assert_eq!(item.conversation_id, "c1");
        assert_eq!(item.detail, "git status");
        assert!(!item.acknowledged);
    }

    #[test]
    fn an_ask_reports_needs_input_and_raises_a_question() {
        let mut state = UiState::mock();
        state.apply_ask_user("c1", "Deploy?", vec!["yes".to_string()]);
        assert_eq!(state.pane_agent_state(1), PaneAgentState::NeedsInput);
        assert_eq!(
            state
                .pane_runtime
                .get(&1)
                .and_then(|rt| rt.pending_ask.as_ref())
                .map(|ask| ask.question.as_str()),
            Some("Deploy?")
        );
        assert_eq!(state.inbox[0].kind, InboxKind::Question);
        assert_eq!(state.inbox[0].detail, "Deploy?");
    }

    #[test]
    fn a_successful_finish_reports_done_and_raises_a_done_item() {
        let mut state = UiState::mock();
        state.begin_turn(1);
        state.finish_turn(1, TurnOutcome::Success);
        assert_eq!(state.pane_agent_state(1), PaneAgentState::Done);
        assert_eq!(state.inbox[0].kind, InboxKind::Done);
    }

    #[test]
    fn a_failed_finish_reports_errored_and_raises_an_error_item() {
        let mut state = UiState::mock();
        state.begin_turn(1);
        state.finish_turn(1, TurnOutcome::Failed(Some("boom".to_string())));
        assert_eq!(state.pane_agent_state(1), PaneAgentState::Errored);
        assert_eq!(state.inbox[0].kind, InboxKind::Error);
        assert_eq!(state.inbox[0].detail, "boom");
    }

    #[test]
    fn a_terminal_error_reports_errored() {
        let mut state = UiState::mock();
        state.begin_turn(1);
        state.apply_turn_error("c1", "model overloaded");
        assert_eq!(state.pane_agent_state(1), PaneAgentState::Errored);
        assert_eq!(state.inbox[0].kind, InboxKind::Error);
        assert_eq!(state.inbox[0].detail, "model overloaded");
    }

    #[test]
    fn a_finish_does_not_erase_a_pending_approval() {
        let mut state = UiState::mock();
        state.apply_command_proposed(&proposal("c1", "call-1", "rm -rf build"));
        state.finish_turn(1, TurnOutcome::Success);
        let rt = state.pane_runtime.get(&1).expect("the pane's runtime");
        assert!(
            rt.pending_command.is_some(),
            "the approval is still waiting for the user"
        );
        assert_eq!(rt.agent_state, PaneAgentState::NeedsInput);
        assert_eq!(
            state.inbox.iter().filter(|item| item.kind == InboxKind::Approval).count(),
            1,
            "no Done item is invented while the approval is pending"
        );
        assert!(!state.inbox.iter().any(|item| item.kind == InboxKind::Done));
    }

    #[test]
    fn approving_a_command_immediately_reports_working() {
        let mut state = UiState::mock();
        state.begin_turn(1);
        state.apply_command_proposed(&proposal("c1", "call-1", "git status"));
        assert_eq!(state.pane_agent_state(1), PaneAgentState::NeedsInput);

        state.resume_command_decision(1, "call-1");

        let rt = state.pane_runtime.get(&1).expect("the pane's runtime");
        assert!(
            rt.pending_command.is_none() && rt.command_selection.is_none(),
            "the decision clears the card"
        );
        assert!(rt.busy, "the resumed turn is running");
        assert_eq!(rt.agent_state, PaneAgentState::Working);
    }

    #[test]
    fn skipping_a_question_immediately_reports_working() {
        let mut state = UiState::mock();
        state.apply_ask_user("c1", "Deploy?", vec!["yes".to_string()]);
        assert_eq!(state.pane_agent_state(1), PaneAgentState::NeedsInput);

        state.skip_pending_ask(1);

        let rt = state.pane_runtime.get(&1).expect("the pane's runtime");
        assert!(rt.pending_ask.is_none(), "the skipped card is gone");
        assert!(rt.busy, "the resumed turn is running");
        assert_eq!(rt.agent_state, PaneAgentState::Working);
    }

    #[test]
    fn stopping_a_turn_immediately_reports_idle() {
        let mut state = UiState::mock();
        state.begin_turn(1);
        state.apply_command_proposed(&proposal("c1", "call-1", "git status"));

        state.stop_turn(1);

        let rt = state.pane_runtime.get(&1).expect("the pane's runtime");
        assert!(!rt.busy, "the cancelled turn is no longer running");
        assert!(rt.turn_started_at.is_none(), "the elapsed clock is dropped");
        assert!(
            rt.pending_command.is_none(),
            "the suspended approval belongs to the cancelled turn"
        );
        assert_eq!(rt.agent_state, PaneAgentState::Idle);
    }

    #[test]
    fn a_failed_subagent_raises_an_agent_item() {
        let mut state = UiState::mock();
        state.apply_subagent_finished(&goble_desktop_service::SubAgentFinishedEvent {
            chat_id: "c1".to_string(),
            subagent_id: "child-1".to_string(),
            status: "failed".to_string(),
            output: None,
            error: Some("budget exhausted".to_string()),
            duration_ms: 1200,
            turns: 2,
            tool_calls: 3,
            tokens: 100,
        });
        assert_eq!(state.inbox[0].kind, InboxKind::Agent);
        assert_eq!(state.inbox[0].detail, "budget exhausted");
    }

    #[test]
    fn push_dedupes_by_id_and_by_conversation_kind() {
        let mut state = UiState::mock();
        let first = state.raise_inbox(1, "c1", InboxKind::Approval, "one", "git status");
        assert_eq!(state.inbox.len(), 1);
        state.raise_inbox(1, "c1", InboxKind::Approval, "two", "git diff");
        assert_eq!(state.inbox.len(), 1, "the same conversation and kind is one item");
        state.push_inbox(InboxItem::new(
            first,
            1,
            0,
            "c2",
            InboxKind::Error,
            "same id",
            "detail",
        ));
        assert_eq!(state.inbox.len(), 1, "the same id is never added twice");
        // Once acknowledged, a new report of the same kind is a new item.
        state.ack_all();
        state.raise_inbox(1, "c1", InboxKind::Approval, "three", "git show");
        assert_eq!(state.inbox.len(), 2);
    }

    #[test]
    fn ack_marks_one_and_ack_all_marks_the_rest() {
        let mut state = UiState::mock();
        let first = state.raise_inbox(1, "c1", InboxKind::Approval, "a", "a");
        state.raise_inbox(1, "c2", InboxKind::Question, "b", "b");
        assert_eq!(state.unread_inbox_count(), 2);
        assert!(state.ack_inbox(&first));
        assert!(!state.ack_inbox(&first), "the same item is acknowledged once");
        assert_eq!(state.unread_inbox_count(), 1);
        assert_eq!(state.ack_all(), 1);
        assert_eq!(state.unread_inbox_count(), 0);
    }

    #[test]
    fn jump_switches_to_the_items_space_and_pane() {
        let mut state = UiState::mock();
        state.spaces = vec![
            Space::new("A", Pane::Leaf { id: 1, kind: crate::ui::PaneKind::Chat }),
            Space::new("B", Pane::Leaf { id: 2, kind: crate::ui::PaneKind::Chat }),
        ];
        state.active_space = 0;
        state.active_pane_id = 1;
        state.pane_sessions.insert(
            2,
            PaneSession {
                conversation_id: "c2".to_string(),
                draft: String::new(),
                path: "/b".to_string(),
                daemon_session_id: String::new(),
            },
        );
        state.raise_inbox(2, "c2", InboxKind::Approval, "approve", "git status");
        assert_eq!(state.inbox[0].space_id, 1);
        assert!(state.jump_to_inbox_item(0));
        assert_eq!(state.active_space, 1);
        assert_eq!(state.active_pane_id, 2);
        assert_eq!(state.current_tab, AppTab::Chat);
    }

    #[test]
    fn a_jump_to_a_closed_pane_reports_the_failure() {
        let mut state = UiState::mock();
        state.raise_inbox(99, "gone", InboxKind::Approval, "approve", "git status");
        assert!(!state.jump_to_inbox_item(0));
        assert!(state.inbox_error.is_some(), "the overlay can say why it did not land");
    }

    #[test]
    fn closing_a_pane_prunes_its_inbox_items() {
        let mut state = UiState::mock();
        state.raise_inbox(1, "c1", InboxKind::Approval, "approve", "git status");
        state.raise_inbox(2, "c2", InboxKind::Question, "ask", "which branch?");
        assert_eq!(state.inbox.len(), 2);
        state.forget_pane(1);
        assert_eq!(state.inbox.len(), 1);
        assert_eq!(state.inbox[0].pane_id, 2);
        state.forget_pane(2);
        assert!(state.inbox.is_empty());
        assert_eq!(state.inbox_index, 0);
    }

    #[test]
    fn a_tui_agent_is_working_while_a_command_runs_and_idle_at_a_prompt() {
        let mode = TerminalMode::Agent(TuiAgent::Codex);
        assert_eq!(tui_agent_state(&mode, true), Some(PaneAgentState::Working));
        assert_eq!(tui_agent_state(&mode, false), Some(PaneAgentState::Idle));
        assert_eq!(tui_agent_state(&TerminalMode::Shell, true), None);
    }

    #[test]
    fn a_tui_agent_pane_folds_its_terminal_mode_into_the_stored_state() {
        let mut state = UiState::mock();
        state
            .terminal
            .borrow_mut()
            .set_mode(1, TerminalMode::Agent(TuiAgent::Codex));
        state.refresh_tui_agent_states();
        assert_eq!(
            state.pane_agent_state(1),
            PaneAgentState::Idle,
            "an agent mode with no command under way is at its prompt"
        );
    }
}
