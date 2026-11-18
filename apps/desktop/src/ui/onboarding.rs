//! First-run onboarding: three steps over the whole window.
//!
//! The shape is the reference's: progress dashes centred at the top, a text
//! column on the left that says what the step is for, and the cards or field
//! the step acts on on the right. Step one asks what the app will be used for,
//! step two asks for a friendly name, and step three asks whether anonymous
//! crash reports and usage counts may be sent. The answers are written to the
//! same store keys the Profile settings page edits, and the flow only appears
//! again when a dev build asks for it.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Border, Button, ButtonVariant, ConstrainedBox, Container, CrossAxisAlignment,
    EdgeInsets, Element, Empty, Expanded, Fill, Flex, Icon, KeyHandler, MainAxisAlignment,
    MainAxisSize, Spacer, Text, TextInput,
};
use goble_ui::platform::text_atlas::FontWeight;
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::vec2f;

use crate::state::{ONBOARDING_STEPS, ONBOARDING_USAGE};

use super::{OnboardingAction, UiActions, UiSnapshot};

/// Widest the two-column content gets, so a wide window keeps the text and the
/// cards together instead of throwing them to opposite edges.
const CONTENT_WIDTH: f32 = 760.0;
/// The left column's fixed width; the right column takes the rest.
const TEXT_COLUMN: f32 = 330.0;
const COLUMN_GAP: f32 = 56.0;
const PROGRESS_WIDTH: f32 = 44.0;
const PROGRESS_HEIGHT: f32 = 3.0;

pub fn build_onboarding(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let step = state.onboarding_step.min(ONBOARDING_STEPS - 1);
    let body = match step {
        0 => build_usage_step(app, state, actions),
        1 => build_name_step(app, state, actions),
        _ => build_data_step(app, state, actions),
    };

    let column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(progress(app, step))
        .with_child(
            Expanded::new(
                Flex::column()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::Center)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_child(
                        // A full-width row so `Center` has the window to
                        // center the fixed-width content in; the column alone
                        // would shrink to the content and sit on the left.
                        Flex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_main_axis_alignment(MainAxisAlignment::Center)
                            .with_child(
                                ConstrainedBox::new(body)
                                    .with_width(CONTENT_WIDTH)
                                    .with_min_width(CONTENT_WIDTH)
                                    .finish(),
                            )
                            .finish(),
                    )
                    .finish(),
            )
            .finish(),
        )
        .with_child(footer(app, state, actions, step));

    let panel = Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .with_padding(EdgeInsets::new(24.0, 32.0, 24.0, 32.0))
        .finish();
    // `Enter` advances and `Esc` goes back. The name step keeps its letters:
    // the text field is the only child that reads them.
    let on_key = actions.on_onboarding.clone();
    KeyHandler::new(panel, move |key, modifiers| {
        if modifiers.ctrl || modifiers.command || modifiers.alt {
            return false;
        }
        match key {
            "Enter" => {
                (on_key.borrow_mut())(OnboardingAction::Next);
                true
            }
            "Escape" => {
                (on_key.borrow_mut())(OnboardingAction::Back);
                true
            }
            // The steps without a field have nothing to type into, so they
            // swallow the rest rather than leaking it to the shell behind.
            _ if step != 1 => true,
            _ => false,
        }
    })
    .finish()
}

/// Three short rules; the current step is the accent one.
fn progress(app: &AppContext, step: usize) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::Center)
        .with_spacing(6.0);
    for index in 0..ONBOARDING_STEPS {
        let color = if index == step {
            ColorToken::Accent
        } else {
            ColorToken::SurfaceRaised
        };
        row = row.with_child(
            Container::new(Box::new(
                Empty::new().with_size(vec2f(PROGRESS_WIDTH, PROGRESS_HEIGHT)),
            ))
            .with_background(Fill::Solid(app.theme.color(color)))
            .with_corner_radius(PROGRESS_HEIGHT * 0.5)
            .finish(),
        );
    }
    row.finish()
}

fn step_text(app: &AppContext, eyebrow: &str, title: &str, description: &str) -> Box<dyn Element> {
    Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        // Stretch so the text measures against the column's own width and
        // wraps there; `Start` would hand the runs an unbounded width.
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(12.0)
        .with_child(
            Text::new(eyebrow)
                .with_theme_color(ColorToken::Accent, app)
                .with_font_size(10.0)
                .with_weight(FontWeight::Bold)
                .finish(),
        )
        .with_child(
            Text::new(title)
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(26.0)
                .with_weight(FontWeight::Bold)
                .finish(),
        )
        .with_child(
            Text::new(description)
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(13.0)
                .finish(),
        )
        .finish()
}

fn step_frame(left: Box<dyn Element>, right: Box<dyn Element>) -> Box<dyn Element> {
    Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(COLUMN_GAP)
        .with_child(
            ConstrainedBox::new(left)
                .with_width(TEXT_COLUMN)
                .with_min_width(TEXT_COLUMN)
                .finish(),
        )
        .with_child(Expanded::new(right).finish())
        .finish()
}

/// One selectable card: an icon, a label, and a check when it is the answer.
fn choice_card(
    app: &AppContext,
    icon: &'static str,
    icon_color: ColorToken,
    label: &str,
    selected: bool,
    on_click: Rc<RefCell<dyn FnMut()>>,
) -> Box<dyn Element> {
    let md = app.theme.spacing_px(SpacingToken::Md);
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(md)
        .with_child(
            Icon::new(icon)
                .with_size(22.0)
                .with_theme_color(icon_color, app)
                .finish(),
        )
        .with_child(
            Text::new(label)
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(14.0)
                .finish(),
        )
        .with_child(Spacer::new().finish());
    row = if selected {
        row.with_child(
            Icon::new("check")
                .with_size(16.0)
                .with_theme_color(ColorToken::Accent, app)
                .finish(),
        )
    } else {
        row.with_child(Empty::new().with_size(vec2f(16.0, 16.0)).finish())
    };
    let content = ConstrainedBox::new(row.finish())
        .with_min_height(44.0)
        .finish();
    let border = if selected {
        app.theme.color(ColorToken::Accent)
    } else {
        app.theme.color(ColorToken::Border)
    };
    Button::new(content)
        .with_variant(ButtonVariant::Default)
        .with_corner_radius(10.0)
        .with_border_color(border)
        .with_on_click(move || (on_click.borrow_mut())())
        .finish()
}

fn build_usage_step(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let md = app.theme.spacing_px(SpacingToken::Md);
    let icons = ["terminal", "user", "sparkle"];
    let colors = [ColorToken::Focus, ColorToken::Success, ColorToken::Warning];

    let mut cards = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(md);
    for (index, (value, label)) in ONBOARDING_USAGE.iter().enumerate() {
        let on_select = actions.on_onboarding.clone();
        let value = value.to_string();
        cards = cards.with_child(choice_card(
            app,
            icons[index],
            colors[index],
            label,
            state.onboarding_usage == *value,
            Rc::new(RefCell::new(move || {
                (on_select.borrow_mut())(OnboardingAction::SelectUsage(value.clone()))
            })),
        ));
    }

    let left = step_text(
        app,
        "CUSTOMIZE YOUR EXPERIENCE",
        "What will you use Goble for?",
        "Pick the one that best describes your usage. You can change your mind later.",
    );
    step_frame(left, cards.finish())
}

fn build_name_step(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let on_change = actions.on_onboarding.clone();
    let on_submit = actions.on_onboarding.clone();
    let field = TextInput::new()
        .with_value(state.onboarding_name.clone())
        .with_placeholder("Your name")
        .with_focused(true)
        .with_plain(true)
        .with_full_width(true)
        .with_on_change(move |value| (on_change.borrow_mut())(OnboardingAction::NameChanged(value)))
        .with_on_submit(move || (on_submit.borrow_mut())(OnboardingAction::Next))
        .finish();
    // The step's only field always holds the caret, so the wrapper is its
    // chrome: one accent outline instead of the input's own focus ring nested
    // inside a second border.
    let field = Container::new(field)
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Accent)))
        .with_corner_radius(10.0)
        .with_padding(EdgeInsets::uniform(app.theme.spacing_px(SpacingToken::Md)))
        .finish();

    let left = step_text(
        app,
        "MAKE IT YOURS",
        "What should we call you?",
        "A friendly name for the agent to greet you by. You can change it later in Settings.",
    );
    step_frame(left, field)
}

fn build_data_step(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let md = app.theme.spacing_px(SpacingToken::Md);
    let mut cards = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(md);
    for (share, icon, label) in [
        (true, "sparkle", "Share anonymous data"),
        (false, "cloud-off", "Don't share"),
    ] {
        let on_select = actions.on_onboarding.clone();
        cards = cards.with_child(choice_card(
            app,
            icon,
            if share {
                ColorToken::Accent
            } else {
                ColorToken::Muted
            },
            label,
            state.onboarding_share == share,
            Rc::new(RefCell::new(move || {
                (on_select.borrow_mut())(OnboardingAction::SetDataSharing(share))
            })),
        ));
    }
    cards = cards.with_child(
        Container::new(
            Text::new("You can change this later in Settings → Profile.")
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(11.0)
                .finish(),
        )
        .with_padding(EdgeInsets::new(md, 0.0, 0.0, 0.0))
        .finish(),
    );

    let left = step_text(
        app,
        "PRIVACY",
        "Help us improve Goble?",
        "Crash reports and usage counts only. Prompts, code and file names never leave your machine.",
    );
    step_frame(left, cards.finish())
}

fn footer(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    step: usize,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm);
    if step > 0 {
        let on_back = actions.on_onboarding.clone();
        row = row.with_child(
            Button::new(
                Text::new("Back")
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_variant(ButtonVariant::Outline)
            .with_border_color(app.theme.color(ColorToken::Text))
            .with_corner_radius(10.0)
            .with_on_click(move || (on_back.borrow_mut())(OnboardingAction::Back))
            .finish(),
        );
    }
    row = row.with_child(Spacer::new().finish());

    let last = step + 1 >= ONBOARDING_STEPS;
    let enabled = state.onboarding_can_advance();
    let on_next = actions.on_onboarding.clone();
    // A disabled `Button` ignores events but keeps its enabled paint, so the
    // accent fill would claim the step is ready when it is not. The label and
    // variant carry the state instead: muted text on the secondary fill.
    let (label_color, variant) = if enabled {
        (ColorToken::Bg, ButtonVariant::Primary)
    } else {
        (ColorToken::Muted, ButtonVariant::Default)
    };
    row = row.with_child(
        Button::new(
            Text::new(if last { "Get started" } else { "Continue" })
                .with_theme_color(label_color, app)
                .with_font_size(12.0)
                .finish(),
        )
        .with_variant(variant)
        .with_disabled(!enabled)
        .with_on_click(move || (on_next.borrow_mut())(OnboardingAction::Next))
        .finish(),
    );
    row.finish()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::EventContext;
    use goble_ui::event::{DispatchedEvent, ModifiersState};
    use goble_ui::geometry::Vector2F;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;

    use crate::root_view::RootView;
    use crate::state::UiState;

    use super::*;

    const WINDOW: (f32, f32) = (1100.0, 760.0);

    /// The tree, its live state, the store the answers land in, the directory
    /// keeping that store alive, and the app context the tree paints with.
    type FirstRun = (
        Box<dyn Element>,
        Rc<RefCell<UiState>>,
        Arc<DesktopState>,
        tempfile::TempDir,
        AppContext,
    );

    /// The real tree with the first-run overlay forced up, plus the store the
    /// answers land in.
    fn first_run_root() -> FirstRun {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        );
        let app = AppContext::default();
        let root = RootView::new(&app, &desktop, None);
        let state = root.state_rc();
        {
            let mut s = state.borrow_mut();
            s.onboarding_open = true;
            s.onboarding_step = 0;
        }
        (Box::new(root) as Box<dyn Element>, state, desktop, dir, app)
    }

    fn frame(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
        render_element(root, vec2f(WINDOW.0, WINDOW.1), app)
    }

    fn drawn(commands: &[RenderCommand], text: &str) -> bool {
        commands
            .iter()
            .any(|c| matches!(c, RenderCommand::DrawText { text: run, .. } if run == text))
    }

    fn text_origin(commands: &[RenderCommand], text: &str) -> Vector2F {
        commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::DrawText {
                    origin, text: run, ..
                } if run == text => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the frame draws {text}"))
    }

    fn key(root: &mut Box<dyn Element>, app: &AppContext, name: &str) -> bool {
        let mut ctx = EventContext;
        root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: name.to_string(),
                modifiers: ModifiersState::none(),
            },
            &mut ctx,
            app,
        )
    }

    /// A click on a drawn label: down then up at the same point, the way a
    /// pointer completes a press on a card.
    fn click(root: &mut Box<dyn Element>, app: &AppContext, at: Vector2F) {
        let mut ctx = EventContext;
        let down = DispatchedEvent::MouseDown {
            position: at,
            button: 0,
        };
        root.dispatch_event(&down, &mut ctx, app);
        let up = DispatchedEvent::MouseUp {
            position: at,
            button: 0,
        };
        root.dispatch_event(&up, &mut ctx, app);
    }

    #[test]
    fn the_three_steps_draw_their_own_question_and_answers() {
        let (mut root, state, _desktop, _dir, app) = first_run_root();

        let steps: [(usize, &str, &[&str]); 3] = [
            (
                0,
                "What will you use Goble for?",
                &["Work", "Personal", "Learning"],
            ),
            (
                1,
                "What should we call you?",
                &["Your name", "Continue", "Back"],
            ),
            (
                2,
                "Help us improve Goble?",
                &["Share anonymous data", "Get started"],
            ),
        ];
        for (step, title, labels) in steps {
            state.borrow_mut().onboarding_step = step;
            let commands = frame(&mut root, &app);
            assert!(drawn(&commands, title), "step {step} asks its question");
            for label in labels {
                assert!(drawn(&commands, label), "step {step} draws {label}");
            }
        }
    }

    #[test]
    fn a_card_click_answers_and_enter_walks_the_flow_to_the_end() {
        let (mut root, state, desktop, _dir, app) = first_run_root();

        // Step one: no answer, so `Enter` cannot advance.
        let commands = frame(&mut root, &app);
        assert!(key(&mut root, &app, "Enter"));
        assert_eq!(
            state.borrow().onboarding_step,
            0,
            "the question is required"
        );

        // Pick the Work card with the pointer.
        let at = text_origin(&commands, "Work");
        click(&mut root, &app, at);
        assert_eq!(state.borrow().onboarding_usage, "work");

        // `Enter` now advances to the name.
        let _ = frame(&mut root, &app);
        assert!(key(&mut root, &app, "Enter"));
        assert_eq!(state.borrow().onboarding_step, 1);

        // The name field takes letters; `Enter` moves on.
        let _ = frame(&mut root, &app);
        for c in ["a", "d", "a"] {
            key(&mut root, &app, c);
        }
        assert_eq!(state.borrow().onboarding_name, "ada");
        let _ = frame(&mut root, &app);
        key(&mut root, &app, "Enter");
        assert_eq!(state.borrow().onboarding_step, 2);

        // `Escape` goes back, then `Enter` returns and finishes the flow.
        key(&mut root, &app, "Escape");
        assert_eq!(state.borrow().onboarding_step, 1);
        let _ = frame(&mut root, &app);
        key(&mut root, &app, "Enter");
        let _ = frame(&mut root, &app);
        key(&mut root, &app, "Enter");
        let s = state.borrow();
        assert!(!s.onboarding_open, "the overlay closes");
        assert!(s.onboarding_done);
        assert_eq!(desktop.get_friendly_name(), "ada");
        assert_eq!(desktop.get_usage(), "work");
    }
}
