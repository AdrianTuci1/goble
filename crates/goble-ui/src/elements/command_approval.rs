//! The inline command-approval card.
//!
//! When the harness suspends on a command it wants to run, the request is part
//! of the conversation: the card closes the agent's output, exactly where the
//! command would run, and the turn stays suspended until the user decides. It
//! scrolls with the transcript instead of pinning to the input, so a long
//! command or a list of variants is read like any other output.
//!
//! The card is warp-new's requested-command block: the question and the
//! decision controls share the header, the command itself is a full-width
//! editable field under them, and a quiet hint names the gestures. `Run`
//! approves the selected candidate verbatim; editing it first and then running
//! sends the edited line instead, and nothing runs before that decision
//! arrives at the harness.

use std::cell::RefCell;
use std::rc::Rc;

use goble_core::harness::CommandDecision;

use crate::elements::{
    AppContext, Axis, Border, Button, ButtonVariant, ComposerButton, ConstrainedBox, Container,
    CrossAxisAlignment, Divider, EdgeInsets, Element, Expanded, Flex, Icon, KeyHandler,
    MainAxisAlignment, Padding, ScrollState, Scrollable, Text, TextArea,
};
use crate::event::ModifiersState;
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, SpacingToken};

use super::chat_composer::CommandProposalUi;

/// The tallest the candidate list grows before it scrolls. A long list of
/// variants is read in place without growing the card past a terminal pane.
pub const APPROVAL_BODY_MAX_HEIGHT: f32 = 180.0;

/// The tallest the editable command grows before it scrolls. A script with a
/// few lines still reads whole; a longer one keeps the card's height stable.
pub const APPROVAL_EDITOR_MAX_HEIGHT: f32 = 140.0;

/// Where a decision on the proposal goes: the proposal id and the harness's own
/// [`CommandDecision`].
pub type DecisionHandler = Rc<RefCell<dyn FnMut(String, CommandDecision) + 'static>>;

/// The app-owned state the card edits through.
///
/// Every handle is owned outside the element, because the card is rebuilt every
/// frame: the selected candidate, the two viewports, the command text and where
/// its caret sits all have to survive that rebuild.
#[derive(Clone)]
pub struct CommandApprovalState {
    /// The candidate the card opened on.
    pub selection: Rc<RefCell<usize>>,
    /// The candidate list's viewport.
    pub scroll: Rc<RefCell<ScrollState>>,
    /// The editable command's viewport.
    pub editor_scroll: Rc<RefCell<ScrollState>>,
    /// The command the user will run, seeded from the selected candidate and
    /// changed by typing.
    pub draft: Rc<RefCell<String>>,
    /// The insertion beam's character index into `draft`.
    pub caret: Rc<RefCell<usize>>,
    /// Whether the command field holds the keyboard. Off while the rich input
    /// is focused: the user is typing a follow-up into the composer, and the
    /// card's command must not take those keys. Clicks on Run/Reject still
    /// decide either way.
    pub field_focused: bool,
}

impl CommandApprovalState {
    pub fn new(
        selection: Rc<RefCell<usize>>,
        scroll: Rc<RefCell<ScrollState>>,
        editor_scroll: Rc<RefCell<ScrollState>>,
        draft: Rc<RefCell<String>>,
        caret: Rc<RefCell<usize>>,
    ) -> Self {
        Self {
            selection,
            scroll,
            editor_scroll,
            draft,
            caret,
            field_focused: true,
        }
    }
}

impl Default for CommandApprovalState {
    fn default() -> Self {
        Self {
            selection: Rc::new(RefCell::new(0)),
            scroll: Rc::new(RefCell::new(ScrollState::default())),
            editor_scroll: Rc::new(RefCell::new(ScrollState::default())),
            draft: Rc::new(RefCell::new(String::new())),
            caret: Rc::new(RefCell::new(0)),
            field_focused: true,
        }
    }
}

/// The approval card for `proposal`: the question and the decision controls in
/// the header, the command or its variants, and the hint that names the keys.
///
/// `state` is the app-owned editing state, so a variant picked before the
/// per-frame rebuild is still the picked one and an edit survives the next
/// frame. Without a decision handler the buttons draw but settle nothing, which
/// is the shape a host that has not wired the harness gets.
pub fn command_approval_card(
    proposal: &CommandProposalUi,
    state: &CommandApprovalState,
    on_decision: Option<DecisionHandler>,
    app: &AppContext,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let md = app.theme.spacing_px(SpacingToken::Md);

    let id = proposal.id.clone();
    let candidates = proposal.candidates.clone();
    let selection = state.selection.clone();
    let draft = state.draft.clone();
    let caret = state.caret.clone();
    let editor_scroll = state.editor_scroll.clone();

    // Run reads the live draft when the click (or Enter) lands, so an edit made
    // in this same frame is the command that gets approved. An unchanged draft
    // is an approval of the selected candidate; an edited one is an edit.
    let decide: Rc<RefCell<dyn FnMut()>> = {
        let handler = on_decision.clone();
        let id = id.clone();
        let candidates = candidates.clone();
        let selection = selection.clone();
        let draft = draft.clone();
        Rc::new(RefCell::new(move || {
            let Some(cb) = handler.as_ref() else {
                return;
            };
            let text = draft.borrow().clone();
            if text.trim().is_empty() {
                return;
            }
            let chosen = match candidates.len() {
                0 => String::new(),
                len => candidates[*selection.borrow() % len].clone(),
            };
            let decision = if text == chosen {
                CommandDecision::Approve(text)
            } else {
                CommandDecision::Edit(text)
            };
            (cb.borrow_mut())(id.clone(), decision);
        }))
    };

    let reject: Rc<RefCell<dyn FnMut()>> = {
        let handler = on_decision.clone();
        let id = id.clone();
        Rc::new(RefCell::new(move || {
            if let Some(cb) = handler.as_ref() {
                (cb.borrow_mut())(id.clone(), CommandDecision::Reject(String::new()));
            }
        }))
    };

    // The options sit in the header, on the question's own line: the user reads
    // the question and decides without moving down the card first. Each button
    // carries the key that answers it, so the gesture is written where it acts.
    let run = {
        let decide = decide.clone();
        Button::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(5.0)
                .with_child(
                    Text::new("Run")
                        .with_theme_color(ColorToken::Bg, app)
                        .with_font_size(12.0)
                        .finish(),
                )
                .with_child(
                    Icon::new("key-return")
                        .with_size(11.0)
                        .with_theme_color(ColorToken::Bg, app)
                        .finish(),
                )
                .finish(),
        )
        .with_variant(ButtonVariant::Primary)
        .with_on_click(move || (decide.borrow_mut())())
        .finish()
    };
    let reject_button = {
        let reject = reject.clone();
        ComposerButton::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(6.0)
                .with_child(
                    Text::new("Reject")
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(12.0)
                        .finish(),
                )
                .with_child(
                    Text::new("esc")
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(10.0)
                        .finish(),
                )
                .finish(),
        )
        .with_height(28.0)
        .with_h_pad(10.0)
        .with_outline(false)
        .with_on_click(move || (reject.borrow_mut())())
        .finish()
    };

    // The question leads the header on its own: no icon in front of it, so the
    // words and the two controls are the whole band.
    let heading = Text::new("OK if I run this command and read the output?")
        .with_theme_color(ColorToken::Text, app)
        .with_font_size(12.0)
        .with_weight(FontWeight::SemiBold)
        .with_max_lines(1)
        .finish();

    let header = Flex::row()
        .with_main_axis_size(crate::elements::MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(Expanded::new(heading).finish())
        .with_child(reject_button)
        .with_child(run)
        .finish();
    // The header keeps its inset; the command below it does not (see the
    // editor), so the separator spans the card's whole width.
    let header = Padding::new(header, EdgeInsets::new(md, md, md, sm)).finish();

    let selected = selected_index(proposal, &state.selection);

    // The variants are a reading list, not an editor: each row picks the
    // command the editor below it opens. A single candidate draws no list.
    let variants: Option<Box<dyn Element>> = (proposal.candidates.len() > 1).then(|| {
        let mut list = Flex::column()
            .with_main_axis_size(crate::elements::MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(2.0);
        for (index, candidate) in proposal.candidates.iter().enumerate() {
            list = list.with_child(candidate_row(
                candidate,
                index,
                index == selected,
                &state.selection,
                &draft,
                &caret,
                &editor_scroll,
                app,
            ));
        }
        Padding::new(
            ConstrainedBox::new(
                Scrollable::new(list.finish(), Axis::Vertical)
                    .with_state(state.scroll.clone())
                    .with_fit_content(true)
                    .finish(),
            )
            .with_min_height(24.0)
            .with_max_height(APPROVAL_BODY_MAX_HEIGHT)
            .finish(),
            EdgeInsets::new(md, sm, md, 0.0),
        )
        .finish()
    });

    // The command is a full-width field the user can change before running it.
    // It holds the keyboard while the rich input does not: once the user clicks
    // the composer to write a follow-up, these keys are the editor's.
    let field = TextArea::new()
        .with_value(draft.borrow().clone())
        .with_placeholder("Type a command to run…")
        .with_multiline(true)
        .with_line_height(1.4)
        .with_min_height(0.0)
        .with_full_width(true)
        .with_focused(state.field_focused)
        .with_blur_on_outside_click(false)
        .with_caret(caret.clone())
        .with_scroll_state(editor_scroll.clone())
        .with_on_change({
            let draft = draft.clone();
            move |text| *draft.borrow_mut() = text
        })
        .with_on_submit({
            let decide = decide.clone();
            move |_modifiers: ModifiersState| (decide.borrow_mut())()
        })
        .finish();
    // The command is the card's content: full width, no inset, no fill, no box
    // of its own. The card's border and the separator over it are the only
    // edges it needs.
    let editor = ConstrainedBox::new(
        Scrollable::new(field, Axis::Vertical)
            .with_state(editor_scroll)
            .with_fit_content(true)
            .finish(),
    )
    .with_min_height(26.0)
    .with_max_height(APPROVAL_EDITOR_MAX_HEIGHT)
    .finish();

    // The gestures live on the controls they trigger, so the hint keeps only
    // the one affordance the controls cannot show: the field is editable.
    let footer = Padding::new(
        Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new("click to edit")
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(10.0)
                    .with_max_lines(1)
                    .finish(),
            )
            .finish(),
        EdgeInsets::new(md, sm, md, sm),
    )
    .finish();

    // The header is a band of its own: the separator under it is the only
    // boundary between the question and the content, so the column itself
    // needs no spacing.
    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(header)
        .with_child(Divider::horizontal().finish());
    if let Some(variants) = variants {
        column = column.with_child(variants);
    }
    column = column
        .with_child(editor)
        .with_child(footer);

    // Escape rejects from anywhere in the card; ↑/↓ switch variants while the
    // command is one line, and stay the editor's own keys once it is not.
    let key_handler = {
        let field_focused = state.field_focused;
        let reject = reject.clone();
        let selection = state.selection.clone();
        let draft = draft.clone();
        let caret = caret.clone();
        let editor_scroll = state.editor_scroll.clone();
        let count = proposal.candidates.len();
        let candidates = proposal.candidates.clone();
        move |key: &str, modifiers: ModifiersState| -> bool {
            // The composer holds the keys while it is focused; the card's
            // Escape and arrow keys would otherwise reject or rewrite a
            // command while the user is typing a follow-up.
            if !field_focused {
                return false;
            }
            match key {
                "Escape" if !modifiers.command && !modifiers.ctrl => {
                    (reject.borrow_mut())();
                    true
                }
                "ArrowUp" | "ArrowDown"
                    if count > 1
                        && !modifiers.command
                        && !modifiers.ctrl
                        && !modifiers.alt
                        && !draft.borrow().contains('\n') =>
                {
                    let step = if key == "ArrowUp" { -1 } else { 1 };
                    let next = (*selection.borrow() as isize + step).rem_euclid(count as isize)
                        as usize;
                    *selection.borrow_mut() = next;
                    let text = candidates[next].clone();
                    *caret.borrow_mut() = text.chars().count();
                    *draft.borrow_mut() = text;
                    *editor_scroll.borrow_mut() = ScrollState::default();
                    true
                }
                _ => false,
            }
        }
    };

    // The card is an outline, not a panel: the transcript's own background
    // shows through, the light border is the only edge, and the content sits
    // flush against it.
    KeyHandler::new(
        Container::new(column.finish())
            .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
            .with_corner_radius(app.theme.radius_px())
            .finish(),
        key_handler,
    )
    .finish()
}

fn selected_index(proposal: &CommandProposalUi, selection: &Rc<RefCell<usize>>) -> usize {
    match proposal.candidates.len() {
        0 => 0,
        len => *selection.borrow() % len,
    }
}

/// One variant row: the index, the command, and a mark on the picked one. The
/// row is the hit target: a click picks the variant and loads it into the
/// editor, so what Run approves is what the field shows.
#[allow(clippy::too_many_arguments)]
fn candidate_row(
    candidate: &str,
    index: usize,
    is_selected: bool,
    selection: &Rc<RefCell<usize>>,
    draft: &Rc<RefCell<String>>,
    caret: &Rc<RefCell<usize>>,
    editor_scroll: &Rc<RefCell<ScrollState>>,
    app: &AppContext,
) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0);
    if is_selected {
        row = row.with_child(
            Icon::new("chevron-right")
                .with_size(10.0)
                .with_theme_color(ColorToken::Accent, app)
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
                Text::new(candidate.to_string())
                    .with_theme_color(
                        if is_selected {
                            ColorToken::Accent
                        } else {
                            ColorToken::Text
                        },
                        app,
                    )
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish(),
        );
    let selection = selection.clone();
    let draft = draft.clone();
    let caret = caret.clone();
    let editor_scroll = editor_scroll.clone();
    let text = candidate.to_string();
    ComposerButton::new(row.finish())
        .with_height(24.0)
        .with_outline(false)
        .with_on_click(move || {
            *selection.borrow_mut() = index;
            *caret.borrow_mut() = text.chars().count();
            *draft.borrow_mut() = text.clone();
            *editor_scroll.borrow_mut() = ScrollState::default();
        })
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{AppContext, EventContext};
    use crate::event::DispatchedEvent;
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;

    fn proposal(candidates: &[&str]) -> CommandProposalUi {
        CommandProposalUi::new(
            "call-1",
            candidates.iter().map(|c| c.to_string()).collect(),
            "~/Projects/goble",
        )
    }

    fn state_for(proposal: &CommandProposalUi) -> CommandApprovalState {
        let draft = proposal.candidates.first().cloned().unwrap_or_default();
        CommandApprovalState {
            caret: Rc::new(RefCell::new(draft.chars().count())),
            draft: Rc::new(RefCell::new(draft)),
            ..CommandApprovalState::default()
        }
    }

    fn texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn decisions() -> (
        Rc<RefCell<Vec<(String, CommandDecision)>>>,
        DecisionHandler,
    ) {
        let decisions: Rc<RefCell<Vec<(String, CommandDecision)>>> = Rc::default();
        let sink = decisions.clone();
        let handler: DecisionHandler = Rc::new(RefCell::new(move |id, decision| {
            sink.borrow_mut().push((id, decision));
        }));
        (decisions, handler)
    }

    #[test]
    fn the_card_names_the_command_and_offers_the_decision_in_the_header() {
        let proposal = proposal(&["git status"]);
        let state = state_for(&proposal);
        let app = AppContext::default();
        let mut element = command_approval_card(&proposal, &state, None, &app);
        let commands = render_element(&mut element, vec2f(640.0, 300.0), &app);
        let drawn = texts(&commands);

        for expected in [
            "OK if I run this command and read the output?",
            "git status",
            "Run",
            "Reject",
            "click to edit",
        ] {
            assert!(
                drawn.iter().any(|text| text == expected),
                "{expected:?} is drawn: {drawn:?}"
            );
        }
        // The question and the two controls share the header's own band: the
        // options live above the command, not under it.
        let y = |text: &str| {
            commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawText { text: drawn, origin, .. } if drawn == text => {
                        Some(origin.y)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{text:?} is drawn"))
        };
        let title = y("OK if I run this command and read the output?");
        let run = y("Run");
        let command = y("git status");
        assert!(
            title < command && run < command,
            "the header sits over the command"
        );
    }

    #[test]
    fn running_approves_the_selected_candidate_and_rejecting_refuses() {
        let proposal = proposal(&["git status", "git diff --stat"]);
        let state = state_for(&proposal);
        *state.selection.borrow_mut() = 1;
        *state.draft.borrow_mut() = "git diff --stat".to_string();
        let (decisions, handler) = decisions();
        let app = AppContext::default();
        let mut element =
            command_approval_card(&proposal, &state, Some(handler), &app);
        let commands = render_element(&mut element, vec2f(640.0, 300.0), &app);

        click_text(&mut element, &commands, "Run", &app);
        assert_eq!(
            decisions.borrow().as_slice(),
            [(
                "call-1".to_string(),
                CommandDecision::Approve("git diff --stat".to_string())
            )]
        );

        let commands = render_element(&mut element, vec2f(640.0, 300.0), &app);
        click_text(&mut element, &commands, "Reject", &app);
        assert_eq!(
            decisions.borrow().last(),
            Some(&(
                "call-1".to_string(),
                CommandDecision::Reject(String::new())
            ))
        );
    }

    #[test]
    fn picking_a_variant_loads_it_into_the_editor_and_run_approves_it() {
        let proposal = proposal(&["git status", "git diff --stat"]);
        let state = state_for(&proposal);
        let (decisions, handler) = decisions();
        let app = AppContext::default();
        let mut element =
            command_approval_card(&proposal, &state, Some(handler), &app);
        let commands = render_element(&mut element, vec2f(640.0, 300.0), &app);
        click_text(&mut element, &commands, "2.", &app);
        assert_eq!(*state.selection.borrow(), 1, "the row click picks the variant");
        assert_eq!(
            state.draft.borrow().as_str(),
            "git diff --stat",
            "the editor opens the picked variant"
        );

        let commands = render_element(&mut element, vec2f(640.0, 300.0), &app);
        click_text(&mut element, &commands, "Run", &app);
        assert_eq!(
            decisions.borrow().last(),
            Some(&(
                "call-1".to_string(),
                CommandDecision::Approve("git diff --stat".to_string())
            )),
            "Run approves the variant the editor shows"
        );
    }

    #[test]
    fn an_edited_command_runs_as_an_edit() {
        let proposal = proposal(&["git status"]);
        let state = state_for(&proposal);
        let (decisions, handler) = decisions();
        let app = AppContext::default();
        let mut element =
            command_approval_card(&proposal, &state, Some(handler), &app);
        let commands = render_element(&mut element, vec2f(640.0, 300.0), &app);
        click_text(&mut element, &commands, "git status", &app);

        // Typing lands in the focused command field, at its end.
        element.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: " ".to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut EventContext::default(),
            &app,
        );
        for ch in ["-", "-", "s", "h", "o", "r", "t"] {
            element.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: ch.to_string(),
                    modifiers: ModifiersState::none(),
                },
                &mut EventContext::default(),
                &app,
            );
        }
        assert_eq!(state.draft.borrow().as_str(), "git status --short");

        let commands = render_element(&mut element, vec2f(640.0, 300.0), &app);
        click_text(&mut element, &commands, "Run", &app);
        assert_eq!(
            decisions.borrow().last(),
            Some(&(
                "call-1".to_string(),
                CommandDecision::Edit("git status --short".to_string())
            )),
            "an edited command is reported as an edit"
        );
    }

    #[test]
    fn enter_runs_and_escape_rejects_from_the_editor() {
        let proposal = proposal(&["git status"]);
        let state = state_for(&proposal);
        let (decisions, handler) = decisions();
        let app = AppContext::default();
        let mut element = command_approval_card(&proposal, &state, Some(handler), &app);
        render_element(&mut element, vec2f(640.0, 300.0), &app);

        element.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Enter".to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut EventContext::default(),
            &app,
        );
        assert_eq!(
            decisions.borrow().last(),
            Some(&(
                "call-1".to_string(),
                CommandDecision::Approve("git status".to_string())
            ))
        );

        element.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Escape".to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut EventContext::default(),
            &app,
        );
        assert_eq!(
            decisions.borrow().last(),
            Some(&(
                "call-1".to_string(),
                CommandDecision::Reject(String::new())
            ))
        );
    }

    #[test]
    fn a_narrow_card_keeps_the_decision_controls_visible() {
        let proposal = proposal(&["git status"]);
        let state = state_for(&proposal);
        let app = AppContext::default();
        let mut element = command_approval_card(&proposal, &state, None, &app);
        let commands = render_element(&mut element, vec2f(260.0, 400.0), &app);
        for label in ["Reject", "Run"] {
            let origin = commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawText { text, origin, .. } if text == label => Some(*origin),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{label:?} is drawn in a narrow card"));
            assert!(
                origin.x < 260.0,
                "{label:?} stays inside the card: {origin:?}"
            );
        }
    }

    #[test]
    fn a_long_list_scrolls_inside_the_card() {
        let candidates: Vec<String> = (0..24).map(|i| format!("run variant {i}")).collect();
        let proposal = CommandProposalUi::new("call-1", candidates, "~/Projects/goble");
        let state = state_for(&proposal);
        let app = AppContext::default();
        let mut element = command_approval_card(&proposal, &state, None, &app);
        let commands = render_element(&mut element, vec2f(640.0, 500.0), &app);

        assert!(
            commands
                .iter()
                .any(|command| matches!(command, RenderCommand::ClipRect { .. })),
            "the command body is a clipped viewport"
        );
        assert!(
            state.scroll.borrow().max_offset() > 0.0,
            "the list has travel"
        );
    }

    /// A click on the run of `text`: the button or row the run sits in takes it.
    fn click_text(
        element: &mut Box<dyn Element>,
        commands: &[RenderCommand],
        text: &str,
        app: &AppContext,
    ) {
        let origin = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text: drawn, origin, .. } if drawn == text => {
                    Some(*origin)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{text:?} is drawn"));
        let mut ctx = EventContext::default();
        element.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: vec2f(origin.x + 2.0, origin.y + 4.0),
                button: 0,
            },
            &mut ctx,
            app,
        );
    }
}
