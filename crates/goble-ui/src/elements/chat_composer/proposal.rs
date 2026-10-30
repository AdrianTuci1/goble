use std::cell::RefCell;
use std::rc::Rc;

use goble_core::harness::CommandDecision;

use super::composer::ChatComposer;

/// The renderable state of a command the harness proposed and is waiting on
/// (A6's `CommandProposed`): the call it suspended, the candidate command lines
/// and the directory they would run in. Candidate generation is a model call
/// and stays in the harness; the card only renders what it is handed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandProposalUi {
    pub id: String,
    pub candidates: Vec<String>,
    pub cwd: String,
}

impl CommandProposalUi {
    pub fn new(id: impl Into<String>, candidates: Vec<String>, cwd: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            candidates,
            cwd: cwd.into(),
        }
    }
}

impl ChatComposer {
    /// Hold the command the harness is waiting on, so the composer answers the
    /// proposal's keys (↑/↓ between candidates, Enter to run, Esc to reject)
    /// while its normal surface stays put. The approval itself is a card at the
    /// end of the transcript; the input is the keyboard the decision arrives
    /// through.
    pub fn with_proposal(mut self, proposal: CommandProposalUi) -> Self {
        self.proposal = Some(proposal);
        self
    }

    /// Set the app-owned selection index. Like the footer menus' `open` flags
    /// this state must live outside the element, which is rebuilt every frame.
    pub fn with_proposal_selection(mut self, selected: Rc<RefCell<usize>>) -> Self {
        self.proposal_selected = selected;
        self
    }

    /// Report the user's decision on a proposal as `(proposal id, decision)`,
    /// in the harness's own [`CommandDecision`] form so the host can resume the
    /// suspended turn with it verbatim.
    pub fn with_on_decision<F: FnMut(String, CommandDecision) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_decision = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn proposal(&self) -> Option<&CommandProposalUi> {
        self.proposal.as_ref()
    }

    /// The candidate Enter would approve, as the proposal currently stands.
    pub fn selected_candidate(&self) -> Option<String> {
        self.proposal_handles()?.selected_candidate()
    }

    /// Proposal-mode keys, taken before the editor sees them: the candidates
    /// cycle and Enter/Esc decide. Typing still edits the draft, so the line a
    /// follow-up carries is the user's own.
    pub(super) fn handle_proposal_key(&mut self, event: &crate::event::DispatchedEvent) -> bool {
        if !self.focused || self.proposal.is_none() {
            return false;
        }
        // A typed follow-up and a queued one both take the keys: the card
        // decides only while the input is empty and nothing waits to be sent.
        if !self.value.borrow().is_empty() || self.queued_prompt_present {
            return false;
        }
        let crate::event::DispatchedEvent::KeyDown { key, .. } = event else {
            return false;
        };
        match key.as_str() {
            "ArrowUp" => self.cycle_candidate(-1),
            "ArrowDown" => self.cycle_candidate(1),
            "Escape" => self.reject_decision(),
            "Enter" => self.submit_decision(),
            _ => false,
        }
    }

    fn cycle_candidate(&mut self, step: isize) -> bool {
        match self.proposal_handles() {
            Some(handles) => {
                handles.cycle(step);
                true
            }
            None => false,
        }
    }

    fn submit_decision(&mut self) -> bool {
        self.proposal_handles()
            .map(|h| h.approve())
            .unwrap_or(false)
    }

    fn reject_decision(&mut self) -> bool {
        self.proposal_handles().map(|h| h.reject()).unwrap_or(false)
    }

    /// The shared handles a proposal gesture acts on, or `None` in ordinary
    /// composer mode. Both the keyboard path and the transcript card's
    /// closures act on the same app-owned selection and callback, so a click
    /// and a key produce the same payload.
    pub(super) fn proposal_handles(&self) -> Option<ProposalHandles> {
        let proposal = self.proposal.as_ref()?;
        Some(ProposalHandles {
            id: proposal.id.clone(),
            candidates: proposal.candidates.clone(),
            selected: self.proposal_selected.clone(),
            on_decision: self.on_decision.clone(),
        })
    }
}

/// The state a proposal gesture acts on: the selected candidate and the
/// callback the decision is reported through. The shared `Rc`s are why a
/// selection made in one frame is still the selection in the next.
#[derive(Clone)]
pub(super) struct ProposalHandles {
    id: String,
    candidates: Vec<String>,
    selected: Rc<RefCell<usize>>,
    on_decision: Option<Rc<RefCell<dyn FnMut(String, CommandDecision) + 'static>>>,
}

impl ProposalHandles {
    fn selected_index(&self) -> usize {
        match self.candidates.len() {
            0 => 0,
            len => *self.selected.borrow() % len,
        }
    }

    pub(super) fn selected_candidate(&self) -> Option<String> {
        self.candidates.get(self.selected_index()).cloned()
    }

    fn cycle(&self, step: isize) {
        let len = self.candidates.len();
        if len == 0 {
            return;
        }
        let current = *self.selected.borrow();
        *self.selected.borrow_mut() =
            (current as isize + step).rem_euclid(len as isize) as usize;
    }

    /// Approve the selection verbatim. Returns false when the host wired no
    /// decision handler, so the key is not swallowed.
    fn approve(&self) -> bool {
        let Some(cb) = self.on_decision.clone() else {
            return false;
        };
        let Some(candidate) = self.selected_candidate() else {
            return false;
        };
        (cb.borrow_mut())(self.id.clone(), CommandDecision::Approve(candidate));
        true
    }

    /// Reject the proposal; the harness turns it into a failed tool call.
    fn reject(&self) -> bool {
        let Some(cb) = self.on_decision.clone() else {
            return false;
        };
        (cb.borrow_mut())(self.id.clone(), CommandDecision::Reject(String::new()));
        true
    }
}
