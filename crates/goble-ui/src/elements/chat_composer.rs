use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Border, Button, ButtonVariant, Chip, Clipped, ComposerButton, ConstrainedBox,
    Container, ContextPill, CrossAxisAlignment, EdgeInsets, Element, Expanded, Fill, Flex, Icon,
    LayoutContext, MainAxisAlignment, MainAxisSize, Padding, PaintContext, PillTraySide, Point,
    PopupMenu, PopupMenuItem, PopupMenuPosition, SizeConstraint, Text, TextArea, Tooltip,
    TooltipPosition,
};
use crate::event::{DispatchedEvent, ModifiersState};
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, FontFamily, SpacingToken};
use goble_core::harness::CommandDecision;

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

pub struct ChatComposer {
    value: Rc<RefCell<String>>,
    placeholder: String,
    attachments: Vec<String>,
    model_label: Option<String>,
    path_label: Option<String>,
    focused: bool,
    stop_visible: bool,
    /// warp-new style context pills: the agent harness, the working directory
    /// and the git branch, each with its own dropdown selector. The working
    /// directory reuses `path_label` (set via `with_path_label`) as its label.
    harness_label: Option<String>,
    harness_menu_items: Vec<PopupMenuItem>,
    harness_menu_open: Rc<RefCell<bool>>,
    on_select_harness_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    dir_menu_items: Vec<PopupMenuItem>,
    dir_menu_open: Rc<RefCell<bool>>,
    on_select_dir_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    branch_label: Option<String>,
    branch_menu_items: Vec<PopupMenuItem>,
    branch_menu_open: Rc<RefCell<bool>>,
    on_select_branch_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    /// The command the harness is waiting on an approval for (A6). While set,
    /// the composer shows the proposal card and the editor holds the selected
    /// candidate so it can be run as-is or edited in place.
    proposal: Option<CommandProposalUi>,
    /// Which candidate is selected. App-owned, like the footer menus' `open`
    /// flags, because the composer element is rebuilt every frame.
    proposal_selected: Rc<RefCell<usize>>,
    on_decision: Option<Rc<RefCell<dyn FnMut(String, CommandDecision) + 'static>>>,
    on_slash: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_send: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    /// Cmd/Ctrl+Enter submit: start a NEW agent conversation (warp-new). The
    /// host decides what "new conversation" means; the composer only reports
    /// the keybinding.
    on_cmd_enter: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_attach: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_model: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_key: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_variant: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_voice: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_image: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_code: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_link: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_stop: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_focus_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    on_profile: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    model_menu_items: Vec<PopupMenuItem>,
    model_menu_open: Rc<RefCell<bool>>,
    on_select_model_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    profile_menu_items: Vec<PopupMenuItem>,
    profile_menu_open: Rc<RefCell<bool>>,
    on_select_profile_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatComposer {
    pub fn new() -> Self {
        Self {
            value: Rc::new(RefCell::new(String::new())),
            placeholder: String::from("Ask anything..."),
            attachments: Vec::new(),
            model_label: None,
            path_label: None,
            focused: false,
            stop_visible: false,
            on_change: None,
            on_send: None,
            on_cmd_enter: None,
            on_attach: None,
            on_select_model: None,
            on_select_key: None,
            on_select_variant: None,
            on_voice: None,
            on_image: None,
            on_code: None,
            on_link: None,
            on_stop: None,
            on_focus_change: None,
            on_profile: None,
            model_menu_items: Vec::new(),
            model_menu_open: Rc::new(RefCell::new(false)),
            on_select_model_item: None,
            profile_menu_items: Vec::new(),
            profile_menu_open: Rc::new(RefCell::new(false)),
            on_select_profile_item: None,
            harness_label: None,
            harness_menu_items: Vec::new(),
            harness_menu_open: Rc::new(RefCell::new(false)),
            on_select_harness_item: None,
            dir_menu_items: Vec::new(),
            dir_menu_open: Rc::new(RefCell::new(false)),
            on_select_dir_item: None,
            branch_label: None,
            branch_menu_items: Vec::new(),
            branch_menu_open: Rc::new(RefCell::new(false)),
            on_select_branch_item: None,
            proposal: None,
            proposal_selected: Rc::new(RefCell::new(0)),
            on_decision: None,
            on_slash: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_value(self, value: impl Into<String>) -> Self {
        *self.value.borrow_mut() = value.into();
        self
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn with_attachments(mut self, attachments: Vec<String>) -> Self {
        self.attachments = attachments;
        self
    }

    pub fn with_model_label(mut self, label: impl Into<String>) -> Self {
        self.model_label = Some(label.into());
        self
    }

    pub fn with_path_label(mut self, path: impl Into<String>) -> Self {
        self.path_label = Some(path.into());
        self
    }

    /// Label for the warp-new "harness" context pill (the agent/harness the
    /// current turn runs on). Shown as the left-most pill in the footer.
    pub fn with_harness_label(mut self, label: impl Into<String>) -> Self {
        self.harness_label = Some(label.into());
        self
    }

    /// Set the harness dropdown: items, the app-owned `open` flag, and a
    /// select callback (same contract as `with_model_menu`).
    pub fn with_harness_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.harness_menu_items = items;
        self.harness_menu_open = open;
        self.on_select_harness_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the working-directory dropdown (items, open flag, select callback).
    pub fn with_dir_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.dir_menu_items = items;
        self.dir_menu_open = open;
        self.on_select_dir_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Label for the git-branch context pill.
    pub fn with_branch_label(mut self, label: impl Into<String>) -> Self {
        self.branch_label = Some(label.into());
        self
    }

    /// Set the git-branch dropdown (items, open flag, select callback).
    pub fn with_branch_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.branch_menu_items = items;
        self.branch_menu_open = open;
        self.on_select_branch_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired when the user begins typing a slash command (the draft starts
    /// with `/`), so the host can open the command palette (grok-build style).
    pub fn with_on_slash<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_slash = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_stop_visible(mut self, visible: bool) -> Self {
        self.stop_visible = visible;
        self
    }

    pub fn with_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_send<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_send = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Cmd/Ctrl+Enter submits the draft as a NEW agent conversation (warp-new).
    pub fn with_on_cmd_enter<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_cmd_enter = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_attach<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_attach = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_model<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_model = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_key<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_key = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_variant<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_variant = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_voice<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_voice = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_image<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_image = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_code<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_code = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_link<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_link = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_stop<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_stop = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_focus_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_focus_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_profile<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_profile = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the model dropdown: `items` to show, the app-owned `open` flag (so
    /// open state survives the per-frame rebuild), and a select callback.
    pub fn with_model_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.model_menu_items = items;
        self.model_menu_open = open;
        self.on_select_model_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the account/profile dropdown (same contract as `with_model_menu`).
    pub fn with_profile_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.profile_menu_items = items;
        self.profile_menu_open = open;
        self.on_select_profile_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

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

    pub fn value(&self) -> String {
        self.value.borrow().clone()
    }

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    pub fn attachments(&self) -> &[String] {
        &self.attachments
    }

    /// Build a warp-new context pill (icon + label + chevron) for the harness
    /// and branch selectors. Content-sized; when `items` is non-empty it is
    /// wrapped in the shared [`PopupMenu`] so the app-owned `open` flag survives
    /// the per-frame rebuild, otherwise it renders as a plain pill.
    /// The agent's footer pills are the shared [`ContextPill`] (a terminal pane
    /// shows the same pill in its topbar), so the two surfaces stay identical.
    fn context_pill(
        &self,
        app: &AppContext,
        icon: &'static str,
        label: &str,
        items: &[PopupMenuItem],
        open: Rc<RefCell<bool>>,
        on_select: &Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
        tooltip: &str,
    ) -> Box<dyn Element> {
        let mut pill = ContextPill::new(icon, label.to_string())
            .with_tooltip(tooltip)
            .with_tray_side(PillTraySide::Above);
        if !items.is_empty() {
            let cb = on_select
                .clone()
                .unwrap_or_else(|| Rc::new(RefCell::new(|_: usize| {})));
            pill = pill.with_menu(items.to_vec(), open, move |idx| (cb.borrow_mut())(idx));
        }
        pill.finish(app)
    }

    /// Proposal-mode keys, taken before the editor sees them: the candidates
    /// cycle and Enter/Esc decide. Typing is left to the textarea, so only these
    /// keys are consumed and only while the composer is focused with a proposal
    /// on screen.
    fn handle_proposal_key(&mut self, event: &DispatchedEvent) -> bool {
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
    fn proposal_handles(&self) -> Option<ProposalHandles> {
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

    /// The proposal card: the candidate lines with the selected one marked, the
    /// directory they would run in, and the approve/reject actions plus the
    /// keyboard affordances.
    fn proposal_card(&self, app: &AppContext) -> Option<Box<dyn Element>> {
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
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                .with_border(
                    Border::all(1.0)
                        .with_border_fill(Fill::Solid(app.theme.color(ColorToken::Border))),
                )
                .with_padding(EdgeInsets::uniform(md))
                .with_corner_radius(8.0)
                .finish(),
        )
    }

    /// Rebuild the rich-input tree. The composer hugs its content: the textarea
    /// grows with the draft up to a cap, and the footer pills sit just below it,
    /// so the rich input never fills the whole pane. This keeps the message
    /// transcript visible and lets the user see the terminal/agent surface
    /// instead of a tall input box.
    fn rebuild(&mut self, app: &AppContext) {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let md = app.theme.spacing_px(SpacingToken::Md);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Min)
            .with_spacing(sm);

        if !self.attachments.is_empty() {
            let mut attachment_row = Flex::row().with_spacing(sm);
            for attachment in &self.attachments {
                let chip = Chip::new(
                    Text::new(attachment.clone())
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .finish();
                attachment_row = attachment_row.with_child(chip);
            }
            column = column.with_child(attachment_row.finish());
        }

        // Command-proposal mode (A6). The editor below the card is the in-place
        // edit surface, so an empty draft is seeded with the selected candidate;
        // a draft the user typed or cycled to is left alone.
        if let Some(handles) = self.proposal_handles() {
            if self.value.borrow().trim().is_empty() {
                if let Some(seed) = handles.selected_candidate() {
                    *self.value.borrow_mut() = seed;
                }
            }
        }
        if let Some(card) = self.proposal_card(app) {
            column = column.with_child(card);
        }

        // Send closure shared between Enter-to-submit and the (removed) send
        // button path; keeps Enter-to-send working. The submit carries the
        // Enter key's modifiers so the host can route a plain Enter (terminal
        // command) vs Cmd/Ctrl+Enter (a NEW agent conversation, warp-new).
        let value_for_send = self.value.clone();
        let on_send = self.on_send.clone();
        let on_cmd_enter = self.on_cmd_enter.clone();
        let send = Rc::new(RefCell::new(move |modifiers: ModifiersState| {
            let text = value_for_send.borrow().clone();
            if !text.is_empty() {
                let agent_submit = (modifiers.command || modifiers.ctrl) && !modifiers.shift;
                if agent_submit {
                    if let Some(cb) = on_cmd_enter.as_ref() {
                        (cb.borrow_mut())(text.clone());
                    }
                } else if let Some(cb) = on_send.as_ref() {
                    (cb.borrow_mut())(text.clone());
                }
                *value_for_send.borrow_mut() = String::new();
            }
        }));

        // The textarea fills the whole composer width and is visually part of
        // the rich-input bar (no separate box).
        let value = self.value.clone();
        let on_change = self.on_change.clone();
        let on_focus_change = self.on_focus_change.clone();
        let on_slash = self.on_slash.clone();
        let send_for_submit = send.clone();
        let textarea = TextArea::new()
            .with_value(self.value.borrow().clone())
            .with_placeholder(self.placeholder.clone())
            .with_min_height(48.0)
            .with_focused(self.focused)
            .with_on_change(move |text| {
                *value.borrow_mut() = text.clone();
                if text.trim_start().starts_with('/') {
                    if let Some(cb) = on_slash.as_ref() {
                        (cb.borrow_mut())();
                    }
                }
                if let Some(cb) = on_change.as_ref() {
                    (cb.borrow_mut())(text);
                }
            })
            .with_on_focus_change(move |focused| {
                if let Some(cb) = on_focus_change.as_ref() {
                    (cb.borrow_mut())(focused);
                }
            })
            .with_on_submit(move |mods| (send_for_submit.borrow_mut())(mods))
            .finish();
        // Cap the textarea so the rich input hugs its content (a few lines of
        // draft + the footer pills) instead of expanding to fill the pane.
        column = column.with_child(
            ConstrainedBox::new(textarea)
                .with_min_height(48.0)
                .with_max_height(160.0)
                .finish(),
        );

        // Footer: attach (+) on the left; model, profile (and stop while
        // streaming) on the right. No send button. The footer is bounded to the
        // composer width so the left group can flex and the path label clips.
        let mut footer = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);

        let mut left_group = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm);
        // Harness context pill (the agent/harness the turn runs on).
        if let Some(label) = self.harness_label.clone() {
            left_group = left_group.with_child(self.context_pill(
                app,
                "computer",
                &label,
                &self.harness_menu_items,
                self.harness_menu_open.clone(),
                &self.on_select_harness_item,
                "Select environment",
            ));
        }
        // Working-directory pill: fills the remaining left-group width so a
        // long path ellipsizes instead of overflowing the composer.
        if let Some(path) = self.path_label.clone() {
            let dir_child = || {
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_spacing(6.0)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Icon::new("folder")
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        Expanded::new(
                            Clipped::new(
                                Text::new(path.clone())
                                    .with_theme_color(ColorToken::Muted, app)
                                    .with_font_size(11.0)
                                    .with_max_lines(1)
                                    .finish(),
                            )
                            .finish(),
                        )
                        .finish(),
                    )
                    .with_child(
                        Icon::new("chevron-down")
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .finish()
            };
            let trigger = Tooltip::new(
                ComposerButton::new(dir_child()).with_height(28.0).finish(),
                "Select working directory",
            )
            .with_position(TooltipPosition::Above)
            .finish();
            let dir = if !self.dir_menu_items.is_empty() {
                let mut menu = PopupMenu::new(trigger, self.dir_menu_items.clone())
                    .with_open(self.dir_menu_open.clone())
                    .with_position(PopupMenuPosition::Above);
                if let Some(cb) = self.on_select_dir_item.clone() {
                    menu = menu.with_on_select(move |idx| (cb.borrow_mut())(idx));
                }
                menu.finish()
            } else {
                trigger
            };
            left_group = left_group.with_child(Expanded::new(dir).finish());
        }
        // Git-branch context pill.
        if let Some(label) = self.branch_label.clone() {
            left_group = left_group.with_child(self.context_pill(
                app,
                "git-branch",
                &label,
                &self.branch_menu_items,
                self.branch_menu_open.clone(),
                &self.on_select_branch_item,
                "Select branch",
            ));
        }
        if let Some(cb) = self.on_attach.clone() {
            let attach = ComposerButton::new(
                Icon::new("plus")
                    .with_size(16.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_height(28.0)
            .with_on_click(move || (cb.borrow_mut())())
            .finish();
            left_group = left_group.with_child(
                Tooltip::new(attach, "Attach")
                    .with_position(TooltipPosition::Above)
                    .finish(),
            );
        }
        footer = footer.with_child(Expanded::new(left_group.finish()).finish());

        let mut right_group = Flex::row().with_spacing(sm);
        if let Some(label) = self.model_label.clone() {
            let model_child = || {
                Flex::row()
                    .with_spacing(6.0)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Icon::new("sparkle")
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        Text::new(label.clone())
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_child(
                        Icon::new("chevron-down")
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .finish()
            };
            if !self.model_menu_items.is_empty() {
                let trigger = Tooltip::new(
                    ComposerButton::new(model_child()).with_height(28.0).finish(),
                    "Select model",
                )
                .with_position(TooltipPosition::Above)
                .finish();
                let mut menu = PopupMenu::new(trigger, self.model_menu_items.clone())
                    .with_open(self.model_menu_open.clone())
                    .with_position(PopupMenuPosition::Above);
                if let Some(cb) = self.on_select_model_item.clone() {
                    menu = menu.with_on_select(move |idx| (cb.borrow_mut())(idx));
                }
                right_group = right_group.with_child(menu.finish());
            } else if let Some(cb) = self.on_select_model.clone() {
                let model = ComposerButton::new(model_child())
                    .with_height(28.0)
                    .with_on_click(move || (cb.borrow_mut())())
                    .finish();
                right_group = right_group.with_child(
                    Tooltip::new(model, "Select model")
                        .with_position(TooltipPosition::Above)
                        .finish(),
                );
            }
        }
        let profile_child = || {
            Icon::new("user")
                .with_size(16.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish()
        };
        if !self.profile_menu_items.is_empty() {
            let trigger = Tooltip::new(
                ComposerButton::new(profile_child()).with_height(28.0).finish(),
                "Account",
            )
            .with_position(TooltipPosition::Above)
            .finish();
            let mut menu = PopupMenu::new(trigger, self.profile_menu_items.clone())
                .with_open(self.profile_menu_open.clone())
                .with_position(PopupMenuPosition::Above);
            if let Some(cb) = self.on_select_profile_item.clone() {
                menu = menu.with_on_select(move |idx| (cb.borrow_mut())(idx));
            }
            right_group = right_group.with_child(menu.finish());
        } else if let Some(cb) = self.on_profile.clone() {
            let profile = ComposerButton::new(profile_child())
                .with_height(28.0)
                .with_on_click(move || (cb.borrow_mut())())
                .finish();
            right_group = right_group.with_child(
                Tooltip::new(profile, "Account")
                    .with_position(TooltipPosition::Above)
                    .finish(),
            );
        }
        if self.stop_visible {
            if let Some(cb) = self.on_stop.clone() {
                let stop = ComposerButton::new(
                    Icon::new("stop")
                        .with_size(16.0)
                        .with_theme_color(ColorToken::Error, app)
                        .finish(),
                )
                .with_height(28.0)
                .with_on_click(move || (cb.borrow_mut())())
                .finish();
                right_group = right_group.with_child(
                    Tooltip::new(stop, "Stop")
                        .with_position(TooltipPosition::Above)
                        .finish(),
                );
            }
        }
        footer = footer.with_child(right_group.finish());
        column = column.with_child(footer.finish());

        // The composer card keeps only its gutters/padding; the raised
        // background and 1px border are dropped so the rich input has no gray
        // inset box behind the textarea and pills.
        let card = Container::new(column.finish())
            .with_padding(EdgeInsets::new(md, md, md, sm))
            .with_corner_radius(8.0)
            .finish();
        self.root = Some(Padding::new(card, EdgeInsets::new(md, sm, md, md)).finish());
    }
}

impl Default for ChatComposer {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for ChatComposer {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.as_mut().unwrap().paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        if self.handle_proposal_key(event) {
            return true;
        }
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}

/// The state a proposal gesture acts on, cloned into the card's row and button
/// closures: the selected candidate, the composer's draft (the editor below the
/// card, which is where a candidate is edited in place) and the callbacks the
/// change and the decision are reported through. The shared `Rc`s are why a
/// selection made in one frame is still the selection in the next.
#[derive(Clone)]
struct ProposalHandles {
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

    fn selected_candidate(&self) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::LayoutContext;
    use crate::geometry::vec2f;

    #[test]
    fn composer_layouts_non_zero() {
        let app = AppContext::default();
        let mut composer = ChatComposer::new();
        let size = composer.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn composer_keeps_value_and_attachments() {
        let app = AppContext::default();
        let mut composer = ChatComposer::new()
            .with_value("hello")
            .with_attachments(vec!["doc.md".to_string()]);

        let size = composer.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );

        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
        assert_eq!(composer.value(), "hello");
        assert_eq!(composer.attachments(), &["doc.md".to_string()]);
    }

    #[test]
    fn composer_renders_model_profile_attach_and_stop_pills() {
        use crate::elements::PaintContext;
        use crate::render::{RenderCommand, Renderer};

        let app = AppContext::default();
        let mut composer = ChatComposer::new()
            .with_model_label("gpt-4o")
            .with_on_attach(|| {})
            .with_on_select_model(|| {})
            .with_on_profile(|| {})
            .with_on_stop(|| {})
            .with_stop_visible(true);

        let size = composer.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);

        let mut paint_ctx = PaintContext::new(Renderer::new());
        composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();

        let icons: Vec<String> = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        for expected in ["sparkle", "user", "plus", "stop"] {
            assert!(icons.iter().any(|n| n == expected), "missing icon {expected}");
        }

        // Each pill draws a surface + a 1px border.
        // Rich-input pills are flat (no gray inset box), so no borders are drawn.
        let strokes = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        assert_eq!(strokes, 0, "expected flat pills without border boxes, got {strokes}");
    }

    #[test]
    fn composer_renders_harness_dir_branch_pills() {
        use crate::elements::PaintContext;
        use crate::render::{RenderCommand, Renderer};

        let app = AppContext::default();
        let mut composer = ChatComposer::new()
            .with_harness_label("grok build")
            .with_path_label("/work/project")
            .with_branch_label("main")
            .with_harness_menu(
                vec![PopupMenuItem::new("grok build")],
                Rc::new(RefCell::new(false)),
                |_| {},
            )
            .with_dir_menu(
                vec![PopupMenuItem::new("/work/project")],
                Rc::new(RefCell::new(false)),
                |_| {},
            )
            .with_branch_menu(
                vec![PopupMenuItem::new("main")],
                Rc::new(RefCell::new(false)),
                |_| {},
            );

        let size = composer.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);

        let mut paint_ctx = PaintContext::new(Renderer::new());
        composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();

        let icons: Vec<String> = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        // `computer` renders the agentmode glyph; folder + git-branch are the
        // new atlas entries.
        for expected in ["agentmode", "folder", "git-branch"] {
            assert!(icons.iter().any(|n| n == expected), "missing icon {expected}");
        }

        // Rich-input pills are flat (no gray inset box), so no borders are drawn.
        let strokes = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        assert_eq!(strokes, 0, "expected flat pills without border boxes, got {strokes}");
    }

    #[test]
    fn composer_context_menu_opens_and_paints_panel() {
        use crate::elements::PaintContext;
        use crate::render::{RenderCommand, Renderer};

        let app = AppContext::default();
        let open = Rc::new(RefCell::new(true));
        let mut composer = ChatComposer::new()
            .with_harness_label("grok build")
            .with_harness_menu(
                vec![
                    PopupMenuItem::new("grok build"),
                    PopupMenuItem::new("claude"),
                ],
                open,
                |_| {},
            );

        let size = composer.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);

        let mut paint_ctx = PaintContext::new(Renderer::new());
        composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();

        // An opened context selector draws its dropdown panel (a bordered
        // surface), confirming the rich-input pills actually open.
        let strokes = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        assert!(strokes >= 1, "opened context menu should paint its panel border");
    }

    fn drawn_texts(commands: &[crate::render::RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|c| match c {
                crate::render::RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn text_color(
        commands: &[crate::render::RenderCommand],
        text: &str,
    ) -> Vec<crate::color::ColorU> {
        commands
            .iter()
            .filter_map(|c| match c {
                crate::render::RenderCommand::DrawText {
                    text: drawn, color, ..
                } if drawn == text => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// Lay the composer out and paint it, returning the draw commands.
    fn paint_composer(
        app: &AppContext,
        composer: &mut ChatComposer,
    ) -> Vec<crate::render::RenderCommand> {
        composer.layout(
            SizeConstraint::loose(vec2f(600.0, 500.0)),
            &mut LayoutContext::default(),
            app,
        );
        let mut paint_ctx = crate::elements::PaintContext::new(crate::render::Renderer::new());
        composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
        paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default()
    }

    /// Dispatch a key the way the window does.
    fn press(composer: &mut ChatComposer, app: &AppContext, name: &str) -> bool {
        composer.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: name.to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut crate::elements::EventContext::default(),
            app,
        )
    }

    fn proposal(candidates: &[&str]) -> CommandProposalUi {
        CommandProposalUi::new(
            "call-1",
            candidates.iter().map(|c| c.to_string()).collect(),
            "/work/project",
        )
    }

    type DecisionLog = Rc<RefCell<Vec<(String, CommandDecision)>>>;

    fn decision_log() -> DecisionLog {
        Rc::new(RefCell::new(Vec::new()))
    }

    fn record_into(log: DecisionLog) -> impl FnMut(String, CommandDecision) + 'static {
        move |id, decision| log.borrow_mut().push((id, decision))
    }

    #[test]
    fn proposal_card_shows_the_candidates_and_selects_the_first() {
        let app = AppContext::default();
        let decisions = decision_log();
        let mut composer = ChatComposer::new()
            .with_focused(true)
            .with_proposal(proposal(&["git status", "git diff --stat"]))
            .with_on_decision(record_into(decisions.clone()));
        let commands = paint_composer(&app, &mut composer);

        let texts = drawn_texts(&commands);
        for expected in [
            "Agent proposes a command",
            "git status",
            "git diff --stat",
            "/work/project",
            "Approve",
            "Reject",
        ] {
            assert!(texts.iter().any(|t| t == expected), "missing {expected:?}");
        }
        // The card is a bordered raised surface, like the transcript's blocks.
        let strokes = commands
            .iter()
            .filter(|c| matches!(c, crate::render::RenderCommand::StrokeRect { .. }))
            .count();
        assert!(strokes >= 1, "proposal card should draw its border");

        // The first candidate is selected: its row is the accent one and the
        // editor was seeded with it, so Enter has something to run.
        assert_eq!(composer.selected_candidate().as_deref(), Some("git status"));
        assert_eq!(composer.value(), "git status");
        let accent = app.theme.color(ColorToken::Accent);
        assert!(
            text_color(&commands, "git status").contains(&accent),
            "the selected candidate should be the accent row"
        );
        let unselected = text_color(&commands, "git diff --stat");
        assert!(!unselected.contains(&accent));
    }

    #[test]
    fn arrow_keys_cycle_the_candidates_and_follow_the_draft() {
        let app = AppContext::default();
        let changes = Rc::new(RefCell::new(Vec::new()));
        let for_cb = changes.clone();
        let mut composer = ChatComposer::new()
            .with_focused(true)
            .with_proposal(proposal(&["git status", "git diff --stat"]))
            .with_on_change(move |text| for_cb.borrow_mut().push(text));
        paint_composer(&app, &mut composer);

        assert!(press(&mut composer, &app, "ArrowDown"));
        assert_eq!(
            composer.selected_candidate().as_deref(),
            Some("git diff --stat")
        );
        assert_eq!(composer.value(), "git diff --stat");
        // The cycle is a draft change too, so the host's own draft (which is
        // what the editor is rebuilt from) does not undo the selection.
        assert_eq!(*changes.borrow(), vec!["git diff --stat".to_string()]);

        // Wraps past the end back to the first, and the other way from the top.
        assert!(press(&mut composer, &app, "ArrowDown"));
        assert_eq!(composer.value(), "git status");
        assert!(press(&mut composer, &app, "ArrowUp"));
        assert_eq!(composer.value(), "git diff --stat");
    }

    #[test]
    fn a_host_owned_selection_survives_the_rebuild() {
        let app = AppContext::default();
        let selection = Rc::new(RefCell::new(1));
        let mut composer = ChatComposer::new()
            .with_focused(true)
            .with_proposal(proposal(&["git status", "git diff --stat"]))
            .with_proposal_selection(selection.clone());
        paint_composer(&app, &mut composer);

        assert_eq!(composer.value(), "git diff --stat");
        assert_eq!(
            composer.selected_candidate().as_deref(),
            Some("git diff --stat")
        );

        assert!(press(&mut composer, &app, "ArrowUp"));
        assert_eq!(*selection.borrow(), 0);
        assert_eq!(composer.value(), "git status");
    }

    #[test]
    fn editing_a_candidate_submits_the_edit() {
        let app = AppContext::default();
        let decisions = decision_log();
        let mut composer = ChatComposer::new()
            .with_focused(true)
            .with_proposal(proposal(&["git status"]))
            .with_on_decision(record_into(decisions.clone()));
        paint_composer(&app, &mut composer);

        // Type into the editor below the card — the in-place edit.
        for ch in " --short".chars() {
            assert!(press(&mut composer, &app, &ch.to_string()));
        }
        assert_eq!(composer.value(), "git status --short");

        assert!(press(&mut composer, &app, "Enter"));
        assert_eq!(
            *decisions.borrow(),
            vec![(
                "call-1".to_string(),
                CommandDecision::Edit("git status --short".to_string())
            )]
        );
    }

    #[test]
    fn enter_approves_the_selected_candidate() {
        let app = AppContext::default();
        let decisions = decision_log();
        let mut composer = ChatComposer::new()
            .with_focused(true)
            .with_proposal(proposal(&["git status", "git diff --stat"]))
            .with_on_decision(record_into(decisions.clone()));
        paint_composer(&app, &mut composer);

        assert!(press(&mut composer, &app, "ArrowDown"));
        assert!(press(&mut composer, &app, "Enter"));
        assert_eq!(
            *decisions.borrow(),
            vec![(
                "call-1".to_string(),
                CommandDecision::Approve("git diff --stat".to_string())
            )]
        );
    }

    #[test]
    fn escape_rejects_the_proposal() {
        let app = AppContext::default();
        let decisions = decision_log();
        let mut composer = ChatComposer::new()
            .with_focused(true)
            .with_proposal(proposal(&["git status"]))
            .with_on_decision(record_into(decisions.clone()));
        paint_composer(&app, &mut composer);

        assert!(press(&mut composer, &app, "Escape"));
        assert_eq!(
            *decisions.borrow(),
            vec![("call-1".to_string(), CommandDecision::Reject(String::new()))]
        );
    }

    #[test]
    fn proposal_keys_are_only_taken_with_focus_and_a_proposal() {
        let app = AppContext::default();
        let decisions = decision_log();
        let first_log = decisions.clone();
        let second_log = decisions.clone();

        // No proposal: the keys belong to the ordinary editor.
        let mut plain = ChatComposer::new()
            .with_focused(true)
            .with_on_decision(record_into(first_log))
            .with_value("a draft");
        paint_composer(&app, &mut plain);
        assert!(!press(&mut plain, &app, "ArrowDown"));
        assert!(!press(&mut plain, &app, "Escape"));

        // A proposal on an unfocused composer does not swallow keys either.
        let mut blurred = ChatComposer::new()
            .with_proposal(proposal(&["git status"]))
            .with_on_decision(record_into(second_log));
        paint_composer(&app, &mut blurred);
        assert!(!press(&mut blurred, &app, "ArrowDown"));
        assert!(!press(&mut blurred, &app, "Enter"));
        assert!(decisions.borrow().is_empty());
    }
}
