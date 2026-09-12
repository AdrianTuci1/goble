use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{AppContext, Button, ButtonVariant, Clipped, ComposerButton, Container, CrossAxisAlignment, EdgeInsets, Element, Expanded, Fill, Flex, Icon, MainAxisAlignment, Text};
use crate::event::DispatchedEvent;
use crate::theme::{ColorToken, FontFamily, SpacingToken};
use goble_core::harness::CommandDecision;
use super::composer::ChatComposer;

/// The renderable state of a command the harness proposed and is waiting on
/// (A6's `CommandProposed`): the call it suspended, the candidate command lines
/// and the directory they would run in. Candidate generation is a model call
/// and stays in the harness; the composer only renders what it is handed.
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
    /// Show the command the harness proposed and is waiting on (A6). The card
    /// renders the candidate list above the editor, and an empty draft is
    /// seeded from the selected candidate, so Enter always runs what the editor
    /// shows whether it was cycled or edited.
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
    /// cycle and Enter/Esc decide. Typing is left to the textarea, so only these
    /// keys are consumed and only while the composer is focused with a proposal
    /// on screen.
    pub(super) fn handle_proposal_key(&mut self, event: &DispatchedEvent) -> bool {
        if !self.focused || self.proposal.is_none() {
            return false;
        }
        let DispatchedEvent::KeyDown { key, .. } = event else {
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
    /// composer mode. Both the keyboard path and the card's row/button
    /// closures go through this, so a click and a key produce the same payload.
    pub(super) fn proposal_handles(&self) -> Option<ProposalHandles> {
        let proposal = self.proposal.as_ref()?;
        Some(ProposalHandles {
            id: proposal.id.clone(),
            candidates: proposal.candidates.clone(),
            selected: self.proposal_selected.clone(),
            value: self.value.clone(),
            on_change: self.on_change.clone(),
            on_decision: self.on_decision.clone(),
        })
    }

    /// The proposal: the candidate lines with the selected one marked, the
    /// directory they would run in, and the approve/reject actions plus the
    /// keyboard affordances. It is a full-width band of rows — no border, no
    /// rounded card — so it reads as part of the agent surface.
    pub(super) fn proposal_card(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let proposal = self.proposal.as_ref()?;
        let handles = self.proposal_handles()?;
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let md = app.theme.spacing_px(SpacingToken::Md);
        let selected = handles.selected_index();

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm);

        let title = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.0)
            .with_child(
                Icon::new("terminal")
                    .with_size(14.0)
                    .with_theme_color(ColorToken::Warning, app)
                    .finish(),
            )
            .with_child(
                Text::new("Agent proposes a command")
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish();
        let cwd = Clipped::new(
            Text::new(proposal.cwd.clone())
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .with_max_lines(1)
                .finish(),
        )
        .finish();
        column = column.with_child(
            Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(title)
                .with_child(cwd)
                .finish(),
        );

        for (index, candidate) in proposal.candidates.iter().enumerate() {
            let is_selected = index == selected;
            let mut row = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.0);
            if is_selected {
                row = row.with_child(
                    Text::new("▸")
                        .with_theme_color(ColorToken::Accent, app)
                        .with_font_size(11.0)
                        .finish(),
                );
            }
            row = row
                .with_child(
                    Text::new(format!("{}.", index + 1))
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(11.0)
                        .finish(),
                )
                .with_child(
                    Expanded::new(
                        Clipped::new(
                            Text::new(candidate.clone())
                                .with_theme_color(
                                    if is_selected {
                                        ColorToken::Accent
                                    } else {
                                        ColorToken::Text
                                    },
                                    app,
                                )
                                .with_font_size(12.0)
                                .with_font_family(FontFamily::Mono)
                                .with_max_lines(1)
                                .finish(),
                        )
                        .finish(),
                    )
                    .finish(),
                );
            let handles_for_click = handles.clone();
            column = column.with_child(
                ComposerButton::new(row.finish())
                    .with_height(24.0)
                    .with_on_click(move || handles_for_click.select(index))
                    .finish(),
            );
        }

        let hint = Expanded::new(
            Clipped::new(
                Text::new("↑/↓ switch · Enter run · Esc reject")
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(10.0)
                    .with_max_lines(1)
                    .finish(),
            )
            .finish(),
        )
        .finish();
        let approve_handles = handles.clone();
        let approve = Button::new(
            Text::new("Approve")
                .with_theme_color(ColorToken::Bg, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_variant(ButtonVariant::Primary)
        .with_on_click(move || {
            approve_handles.approve();
        })
        .finish();
        let reject_handles = handles.clone();
        let reject = Button::new(
            Text::new("Reject")
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || {
            reject_handles.reject();
        })
        .finish();
        column = column.with_child(
            Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(hint)
                .with_child(
                    Flex::row()
                        .with_spacing(sm)
                        .with_child(reject)
                        .with_child(approve)
                        .finish(),
                )
                .finish(),
        );

        Some(
            // A full-width band of native rows, like the rest of the agent
            // surface: no border and no rounded card around it.
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                .with_padding(EdgeInsets::uniform(md))
                .finish(),
        )
    }
}

/// The state a proposal gesture acts on, cloned into the card's row and button
/// closures: the selected candidate, the composer's draft (the editor below the
/// card, which is where a candidate is edited in place) and the callbacks the
/// change and the decision are reported through. The shared `Rc`s are why a
/// selection made in one frame is still the selection in the next.
#[derive(Clone)]
pub(super) struct ProposalHandles {
    id: String,
    candidates: Vec<String>,
    selected: Rc<RefCell<usize>>,
    value: Rc<RefCell<String>>,
    on_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
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

    /// Select a candidate and load it into the draft, so the editor shows the
    /// line Enter would run while still allowing it to be edited in place. The
    /// new draft is reported as a change so the host's own draft stays in step.
    fn select(&self, index: usize) {
        let len = self.candidates.len();
        if len == 0 {
            return;
        }
        let index = index % len;
        *self.selected.borrow_mut() = index;
        let text = self.candidates[index].clone();
        *self.value.borrow_mut() = text.clone();
        if let Some(cb) = self.on_change.as_ref() {
            (cb.borrow_mut())(text);
        }
    }

    fn cycle(&self, step: isize) {
        let len = self.candidates.len();
        if len == 0 {
            return;
        }
        let next = (*self.selected.borrow() as isize + step).rem_euclid(len as isize) as usize;
        self.select(next);
    }

    /// Approve the selection: an untouched candidate is run verbatim, while a
    /// draft that was edited is submitted as the edit. Returns false when the
    /// host wired no decision handler, so the key is not swallowed.
    fn approve(&self) -> bool {
        let Some(cb) = self.on_decision.clone() else {
            return false;
        };
        let text = self.value.borrow().trim().to_string();
        let decision = match self.selected_candidate() {
            Some(candidate) if candidate.trim() == text => CommandDecision::Approve(candidate),
            _ => CommandDecision::Edit(text),
        };
        (cb.borrow_mut())(self.id.clone(), decision);
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
