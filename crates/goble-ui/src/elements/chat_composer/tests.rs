use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::AppContext;
use crate::elements::Element;
use crate::elements::LayoutContext;
use crate::elements::PopupMenuItem;
use crate::elements::SizeConstraint;
use crate::elements::COMPOSER_CONTROL_RADIUS;
use crate::event::DispatchedEvent;
use crate::event::ModifiersState;
use crate::geometry::vec2f;
use crate::theme::ColorToken;
use goble_core::harness::CommandDecision;
use super::*;

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

/// Every control the rich input owns sits in one footer row under the editor:
/// the context pill and attach, then the model and stop, then the mode badge,
/// left to right. Nothing but the draft is above the editor.
#[test]
fn composer_puts_every_control_in_the_footer_below_the_editor() {
    use crate::elements::PaintContext;
    use crate::render::{RenderCommand, Renderer};

    let app = AppContext::default();
    let mut composer = ChatComposer::new()
        .with_value("hi")
        .with_model_label("gpt-4o")
        .with_path_label("~/Projects/goble")
        .with_on_attach(|| {})
        .with_on_select_model(|| {})
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

    let icon_y = |name: &str| -> Option<f32> {
        commands.iter().find_map(|c| match c {
            RenderCommand::DrawIcon { origin, name: n, .. } if n == name => Some(origin.y),
            _ => None,
        })
    };
    let text_y = |text: &str| -> Option<f32> {
        commands.iter().find_map(|c| match c {
            RenderCommand::DrawText { origin, text: t, .. } if t == text => Some(origin.y),
            _ => None,
        })
    };

    let plus = icon_y("plus").expect("attach pill icon");
    let sparkle = icon_y("sparkle").expect("model pill icon");
    let editor = text_y("hi").expect("editor text");
    let folder = icon_y("folder").expect("directory pill icon");

    assert!(
        editor < folder,
        "the directory pill sits below the editor: {folder} vs {editor}"
    );
    assert!(
        editor < plus,
        "the attach button sits below the editor: {plus} vs {editor}"
    );
    assert!(
        editor < sparkle,
        "the model pill sits below the editor: {sparkle} vs {editor}"
    );
    // One row: the directory pill, attach and the model share the footer's
    // line, within the few pixels their different heights account for.
    assert!(
        (plus - folder).abs() < 20.0,
        "attach is on the directory pill's line: {folder} vs {plus}"
    );
    assert!(
        (sparkle - folder).abs() < 20.0,
        "the model is on the directory pill's line: {folder} vs {sparkle}"
    );

    // The account button is gone from the rich input.
    assert!(icon_y("user").is_none(), "no account icon in the rich input");

    // Every control in the row is a card: the directory pill, attach, the model
    // and stop each outline their own box in the theme's grey, at the theme's
    // corner radius, and none of them fills it while the pointer is away.
    let borders: Vec<(crate::color::ColorU, f32, f32)> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::StrokeRect {
                color,
                width,
                corner_radius,
                ..
            } => Some((*color, *width, *corner_radius)),
            _ => None,
        })
        .collect();
    assert_eq!(
        borders.len(),
        4,
        "the directory pill, attach, the model and stop are four cards, got {borders:?}"
    );
    for (color, width, radius) in &borders {
        assert_eq!(
            *color,
            app.theme.color(ColorToken::Border),
            "a card is outlined in the theme's grey"
        );
        assert_eq!(*width, 1.0, "the outline is 1px");
        assert_eq!(*radius, COMPOSER_CONTROL_RADIUS, "at the input's own corner radius");
    }
    assert!(
        !commands.iter().any(|c| matches!(
            c,
            RenderCommand::FillRect { color, .. } if *color == app.theme.color(ColorToken::Hover)
        )),
        "a card draws no fill until the pointer is over it: {commands:?}"
    );
}

#[test]
fn composer_renders_dir_and_branch_pills_without_the_environment_pill() {
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
    // folder + git-branch are the two context pills the rich input still draws.
    for expected in ["folder", "git-branch"] {
        assert!(icons.iter().any(|n| n == expected), "missing icon {expected}");
    }
    // The environment pill (`computer` -> the agentmode glyph) is gone, even
    // though the host still sets its label and its menu.
    assert!(
        !icons.iter().any(|n| n == "agentmode"),
        "the environment pill is not drawn: {icons:?}"
    );
    assert!(
        !drawn_texts(&commands).iter().any(|t| t == "grok build"),
        "and its label is not drawn either: {:?}",
        drawn_texts(&commands)
    );

    // The directory and branch pills are two cards: each outlines its own box
    // in the theme's grey, at the theme's corner radius.
    let borders: Vec<(crate::color::ColorU, f32, f32)> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::StrokeRect {
                color,
                width,
                corner_radius,
                ..
            } => Some((*color, *width, *corner_radius)),
            _ => None,
        })
        .collect();
    assert_eq!(borders.len(), 2, "one card per pill, got {borders:?}");
    for (color, width, radius) in &borders {
        assert_eq!(
            *color,
            app.theme.color(ColorToken::Border),
            "a card is outlined in the theme's grey"
        );
        assert_eq!(*width, 1.0, "the outline is 1px");
        assert_eq!(*radius, COMPOSER_CONTROL_RADIUS, "at the input's own corner radius");
    }
}

/// A rich-input control is a card: the row's own box outlined in the theme's
/// grey border while the pointer is away, and the theme's hover fill once the
/// pointer is over it. The outline is all it draws at rest.
#[test]
fn a_rich_input_control_is_a_card_that_fills_on_hover() {
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let border = app.theme.color(ColorToken::Border);
    let hover = app.theme.color(ColorToken::Hover);
    let mut composer = ChatComposer::new().with_path_label("~/Projects/goble");

    let rest = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0)).1;
    let card = card_of(&rest, "folder");
    let (color, width, radius) = rest
        .iter()
        .find_map(|c| match c {
            RenderCommand::StrokeRect {
                rect,
                color,
                width,
                corner_radius,
            } if *rect == card => Some((*color, *width, *corner_radius)),
            _ => None,
        })
        .expect("the directory pill outlines its own box");
    assert_eq!(color, border, "the card's outline is the theme's grey");
    assert_eq!(width, 1.0, "the outline is 1px");
    assert_eq!(radius, COMPOSER_CONTROL_RADIUS, "at the input's own corner radius");
    assert!(
        !rest.iter().any(|c| matches!(
            c,
            RenderCommand::FillRect { color, .. } if *color == hover
        )),
        "a card fills only under the pointer: {rest:?}"
    );

    // The pointer onto the card: the same box fills with the hover grey, inside
    // the outline that is still drawn around it.
    let cursor = vec2f(
        (card.min_x() + card.max_x()) / 2.0,
        (card.min_y() + card.max_y()) / 2.0,
    );
    let over = paint_composer_with_pointer(&app, &mut composer, vec2f(600.0, 500.0), Some(cursor));
    assert!(
        over.iter().any(|c| matches!(
            c,
            RenderCommand::FillRect { rect, color, corner_radius }
                if *rect == card && *color == hover && *corner_radius == COMPOSER_CONTROL_RADIUS
        )),
        "the pointer over the card fills it with the hover grey: {over:?}"
    );
}

/// The rich input's controls carry no drop-down chevron: the card itself is the
/// trigger, and the tray still opens from a click on it. Each control is still
/// drawn by its own glyph — only the chevron is gone.
#[test]
fn the_rich_input_controls_draw_no_drop_down_chevron() {
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let mut composer = ChatComposer::new()
        .with_harness_label("grok build")
        .with_path_label("~/Projects/goble")
        .with_branch_label("main")
        .with_model_label("gpt-4o")
        .with_on_attach(|| {})
        .with_harness_menu(
            vec![PopupMenuItem::new("grok build")],
            Rc::new(RefCell::new(false)),
            |_| {},
        )
        .with_dir_menu(
            vec![PopupMenuItem::new("~/Projects/goble")],
            Rc::new(RefCell::new(false)),
            |_| {},
        )
        .with_branch_menu(
            vec![PopupMenuItem::new("main")],
            Rc::new(RefCell::new(false)),
            |_| {},
        )
        .with_model_menu(
            vec![PopupMenuItem::new("gpt-4o")],
            Rc::new(RefCell::new(false)),
            |_| {},
        );

    let commands = paint_composer_in(&app, &mut composer, vec2f(760.0, 500.0)).1;
    let mut icons: Vec<String> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    icons.sort();
    assert_eq!(
        icons,
        [
            "folder",     // the working-directory pill
            "git-branch", // the branch pill
            "plus",       // attach
            "sparkle",    // the model
        ],
        "every control draws its own glyph and none of them a chevron, \
         the environment pill included"
    );
    assert!(
        !icons.iter().any(|name| name == "agentmode"),
        "the environment pill is gone from the rich input"
    );
}

#[test]
fn composer_context_menu_opens_and_paints_panel() {
    use crate::elements::PaintContext;
    use crate::render::{RenderCommand, Renderer};

    let app = AppContext::default();
    let open = Rc::new(RefCell::new(true));
    let mut composer = ChatComposer::new()
        .with_path_label("/work/project")
        .with_dir_menu(
            vec![
                PopupMenuItem::new("/work/project"),
                PopupMenuItem::new("/work/other"),
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
    paint_composer_in(app, composer, vec2f(600.0, 500.0)).1
}

/// The same, at a chosen width: the composer's own size and what it painted.
fn paint_composer_in(
    app: &AppContext,
    composer: &mut ChatComposer,
    window: crate::geometry::Vector2F,
) -> (crate::geometry::Vector2F, Vec<crate::render::RenderCommand>) {
    let size = composer.layout(
        SizeConstraint::loose(window),
        &mut LayoutContext::default(),
        app,
    );
    let mut paint_ctx = crate::elements::PaintContext::new(crate::render::Renderer::new());
    composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    let commands = paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default();
    (size, commands)
}

/// The same, with the pointer at `cursor` for the frame that paints: the tree
/// is rebuilt every frame, so an element reads the cursor at paint (see
/// `views::chat_view::tests`). `None` puts the pointer outside the window.
fn paint_composer_with_pointer(
    app: &AppContext,
    composer: &mut ChatComposer,
    window: crate::geometry::Vector2F,
    cursor: Option<crate::geometry::Vector2F>,
) -> Vec<crate::render::RenderCommand> {
    let _ = composer.layout(
        SizeConstraint::loose(window),
        &mut LayoutContext::default(),
        app,
    );
    let mut paint_ctx = crate::elements::PaintContext::new(crate::render::Renderer::new());
    if let Some(position) = cursor {
        paint_ctx.cursor_inside = true;
        paint_ctx.cursor_position = position;
    }
    composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default()
}

/// The box the control that drew `icon` outlines: the border around the icon's
/// own origin. Every rich-input control is a card of its own, so the border
/// that encloses the glyph is the control's.
fn card_of(commands: &[crate::render::RenderCommand], icon: &str) -> crate::geometry::RectF {
    use crate::render::RenderCommand;
    let origin = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawIcon { origin, name, .. } if name == icon => Some(*origin),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the {icon} control is drawn"));
    commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::StrokeRect { rect, .. }
                if rect.min_x() <= origin.x
                    && origin.x <= rect.max_x()
                    && rect.min_y() <= origin.y
                    && origin.y <= rect.max_y() =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("the {icon} control outlines its own box"))
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
    // The proposal is a band of native rows, so it draws no border at all.
    let strokes = commands
        .iter()
        .filter(|c| matches!(c, crate::render::RenderCommand::StrokeRect { .. }))
        .count();
    assert_eq!(strokes, 0, "the proposal draws no border");

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

#[test]
fn a_press_anywhere_in_the_rich_input_focuses_the_editor() {
    let app = AppContext::default();
    let focused: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let log = focused.clone();
    let mut composer = ChatComposer::new()
        .with_value("hi")
        .with_path_label("~/Projects/goble")
        .with_branch_label("main")
        .with_model_label("gpt-4o")
        .with_on_attach(|| {})
        .with_on_select_model(|| {})
        .with_on_focus_change(move |focused| log.borrow_mut().push(focused));
    let size = composer.layout(
        SizeConstraint::loose(vec2f(600.0, 500.0)),
        &mut LayoutContext::default(),
        &app,
    );
    composer.paint(
        vec2f(0.0, 0.0),
        &mut crate::elements::PaintContext::new(crate::render::Renderer::new()),
        &app,
    );

    // The bar's bottom-right gutter: inside the rich input's own bounds, below
    // and right of every child the tree holds.
    let gutter = vec2f(size.x - 2.0, size.y - 2.0);
    let mut ctx = crate::elements::EventContext::default();
    let handled = composer.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: gutter,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    assert!(handled, "a press in the bar belongs to the bar");
    assert_eq!(
        *focused.borrow(),
        vec![true],
        "the bar's own gutter focuses the editor"
    );

    // A press outside the bar is not the bar's to take.
    let outside = vec2f(size.x + 8.0, size.y + 8.0);
    let handled = composer.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: outside,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    assert!(!handled, "a press outside the bar is left to the pane");
    assert_eq!(*focused.borrow(), vec![true], "and it does not re-report focus");
}

/// Narrowed, the footer's controls move onto a second line instead of being
/// drawn past the input's own edge — the warp-new behaviour at resize, where
/// the input column grows downwards rather than sideways.
#[test]
fn the_footer_wraps_instead_of_overflowing_the_input() {
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let composer = || {
        ChatComposer::new()
            .with_path_label("~/Projects/goble")
            .with_branch_label("main")
            .with_model_label("gpt-4o")
            .with_stop_visible(true)
            .with_on_stop(|| {})
            .with_on_select_model(|| {})
            .with_vim(Rc::new(RefCell::new(crate::vim::VimState::new())))
    };

    // Wide enough for the whole footer: one line.
    let (wide, wide_commands) = paint_composer_in(&app, &mut composer(), vec2f(700.0, 500.0));
    let model_y = |commands: &[RenderCommand]| -> f32 {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "gpt-4o" => Some(origin.y),
                _ => None,
            })
            .expect("the model label is drawn")
    };

    // Narrow: the same controls, wrapped, and nothing drawn past the input's
    // own width.
    let (narrow, narrow_commands) = paint_composer_in(&app, &mut composer(), vec2f(260.0, 500.0));
    assert!(
        model_y(&narrow_commands) > model_y(&wide_commands),
        "the model label moved to a second line: {} then {}",
        model_y(&wide_commands),
        model_y(&narrow_commands)
    );
    assert!(
        narrow.y > wide.y,
        "the wrapped footer is taller: {wide:?} then {narrow:?}"
    );
    for command in &narrow_commands {
        let max_x = match command {
            RenderCommand::DrawText { origin, .. } => origin.x,
            RenderCommand::FillRect { rect, .. } => rect.max_x(),
            RenderCommand::StrokeRect { rect, .. } => rect.max_x(),
            RenderCommand::DrawIcon { origin, .. } => origin.x,
            _ => continue,
        };
        assert!(
            max_x <= narrow.x + 0.5,
            "nothing is drawn past the input's width ({}): {command:?}",
            narrow.x
        );
    }
}

/// The instructions the host hands the composer are drawn in the input's own
/// bottom row, under the editor and the action row: the one entry a surface
/// keeps inside its input rather than over the separator above it. The composer
/// hugs its content either way — the strip adds its own height and nothing else.
#[test]
fn the_instructions_close_the_input_under_the_action_row() {
    use crate::elements::ShortcutHint;
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let line_of = |commands: &[RenderCommand], text: &str| -> Option<f32> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawText { text: run, origin, .. } if run == text => Some(origin.y),
            _ => None,
        })
    };
    // The shell bar: its directory pill over the editor, its one instruction
    // under it, and no model or stop control in between.
    let bare = paint_composer_in(
        &app,
        &mut ChatComposer::new()
            .with_context_above_editor(true)
            .with_path_label("~/Projects/goble"),
        vec2f(600.0, 500.0),
    );
    let with_hints = paint_composer_in(
        &app,
        &mut ChatComposer::new()
            .with_context_above_editor(true)
            .with_path_label("~/Projects/goble")
            .with_hints(vec![ShortcutHint::new(&["⌘", "↵"], "new conversation")]),
        vec2f(600.0, 500.0),
    );

    let (bare_size, bare_commands) = bare;
    let (size, commands) = with_hints;
    assert!(
        line_of(&bare_commands, "new conversation").is_none(),
        "an input handed no instructions draws none"
    );
    let editor = line_of(&commands, "Ask anything...").expect("the editor is drawn");
    let hint = line_of(&commands, "new conversation").expect("the instruction is drawn");
    assert!(
        hint > editor,
        "the instruction is under the editor, inside the input ({hint} against {editor})"
    );
    // The strip's own height is all it costs.
    let bare_editor = line_of(&bare_commands, "Ask anything...").expect("the editor is drawn");
    assert!(
        (bare_editor - editor).abs() < 1.0,
        "the editor does not move for the strip ({editor} against {bare_editor})"
    );
    assert!(
        size.y > bare_size.y,
        "the input grows by the strip's height ({} against {})",
        bare_size.y,
        size.y
    );
}

/// The composer's editor draws its insertion beam in the focus blue — the
/// colour the reference's editor cursor carries, not the UI's neutral accent —
/// and a blurred composer draws no beam.
#[test]
fn the_focused_editor_shows_the_focus_blue_caret() {
    use crate::render::RenderCommand;

    let app = AppContext::default();
    let focus = app.theme.color(ColorToken::Focus);

    let mut focused = ChatComposer::new().with_value("hi").with_focused(true);
    let commands = paint_composer(&app, &mut focused);
    assert!(
        commands.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { color, .. } if *color == focus
        )),
        "the focused editor's beam is the focus blue: {commands:?}"
    );

    let mut blurred = ChatComposer::new().with_value("hi");
    let commands = paint_composer(&app, &mut blurred);
    assert!(
        !commands.iter().any(|command| matches!(
            command,
            RenderCommand::FillRect { color, .. } if *color == focus
        )),
        "a blurred composer draws no caret: {commands:?}"
    );
}

/// The rich input hugs its own rows: the card's vertical padding is `xs` and
/// the block carries no vertical margin, so the first content row is `xs` under
/// the composer's top edge and the last one is `xs` above its bottom edge —
/// about 4pt where it used to be `md` + `xs`, 16pt. The horizontal stays `md`
/// on the card, so the editor's text keeps the conversation's text column.
#[test]
fn the_rich_input_keeps_only_xs_above_and_below_its_rows() {
    use crate::render::RenderCommand;
    use crate::theme::SpacingToken;

    let app = AppContext::default();
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    // The context row rides above the editor here, so the first card drawn is
    // the top row and the model card is the bottom one. The composer paints
    // from its own origin: its top edge is 0 and its bottom edge is `size.y`.
    let mut composer = ChatComposer::new()
        .with_context_above_editor(true)
        .with_path_label("~/Projects/goble")
        .with_model_label("gpt-4o")
        .with_on_select_model(|| {});
    let (size, commands) = paint_composer_in(&app, &mut composer, vec2f(600.0, 500.0));

    let cards: Vec<crate::geometry::RectF> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::StrokeRect { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect();
    let top = cards
        .iter()
        .map(|rect| rect.min_y())
        .fold(f32::INFINITY, f32::min);
    let bottom = cards
        .iter()
        .map(|rect| rect.max_y())
        .fold(f32::NEG_INFINITY, f32::max);

    assert!(
        (top - xs).abs() < 0.5,
        "the first row is {xs} under the composer's top edge, got {top}"
    );
    assert!(
        (size.y - bottom - xs).abs() < 0.5,
        "the last row is {xs} above the composer's bottom edge, got {} of {}",
        size.y - bottom,
        size.y
    );
}

/// `⌘⌥⏎` is the cloud gesture (warp-new's `CMD+ALT+ENTER`): the host's own
/// callback fires with the draft, and the ordinary send path — what a plain
/// `↵` takes — is left alone.
#[test]
fn the_cloud_chord_submits_to_the_cloud_and_plain_enter_still_sends() {
    let app = AppContext::default();

    // A plain Enter sends the draft the usual way and never routes it to the
    // cloud, even on a composer that carries both callbacks.
    let sent = Rc::new(RefCell::new(Vec::new()));
    let sent_cb = sent.clone();
    let cloud_for_plain = Rc::new(RefCell::new(Vec::new()));
    let cloud_for_plain_cb = cloud_for_plain.clone();
    let mut plain = ChatComposer::new()
        .with_focused(true)
        .with_value("a local turn")
        .with_on_send(move |text| sent_cb.borrow_mut().push(text))
        .with_on_send_to_cloud(move |text| cloud_for_plain_cb.borrow_mut().push(text));
    paint_composer(&app, &mut plain);
    let _ = press(&mut plain, &app, "Enter");
    assert_eq!(*sent.borrow(), vec!["a local turn".to_string()]);
    assert!(
        cloud_for_plain.borrow().is_empty(),
        "a plain Enter is not the cloud chord"
    );
    assert_eq!(plain.value(), "", "the submitted draft is spent");

    // Command/Ctrl+Alt+Enter is the cloud chord, in both spellings the OS
    // sends (command on macOS, ctrl elsewhere).
    for modifiers in [
        ModifiersState {
            alt: true,
            command: true,
            ..Default::default()
        },
        ModifiersState {
            alt: true,
            ctrl: true,
            ..Default::default()
        },
    ] {
        let cloud = Rc::new(RefCell::new(Vec::new()));
        let cloud_cb = cloud.clone();
        let local = Rc::new(RefCell::new(Vec::new()));
        let local_cb = local.clone();
        let mut composer = ChatComposer::new()
            .with_focused(true)
            .with_value("ship it")
            .with_on_send(move |text| local_cb.borrow_mut().push(text))
            .with_on_send_to_cloud(move |text| cloud_cb.borrow_mut().push(text));
        paint_composer(&app, &mut composer);

        let handled = composer.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Enter".to_string(),
                modifiers,
            },
            &mut crate::elements::EventContext::default(),
            &app,
        );
        assert!(handled, "the cloud chord belongs to the composer");
        assert_eq!(*cloud.borrow(), vec!["ship it".to_string()]);
        assert!(local.borrow().is_empty(), "the cloud chord is not a local send");
        assert_eq!(composer.value(), "", "the submitted draft is spent");
    }

    // Shift is not part of the gesture, so the editor keeps the key.
    let cloud = Rc::new(RefCell::new(Vec::new()));
    let cloud_cb = cloud.clone();
    let mut shifted = ChatComposer::new()
        .with_focused(true)
        .with_value("typed")
        .with_on_send_to_cloud(move |text| cloud_cb.borrow_mut().push(text));
    paint_composer(&app, &mut shifted);
    let _ = shifted.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: "Enter".to_string(),
            modifiers: ModifiersState {
                alt: true,
                command: true,
                shift: true,
                ..Default::default()
            },
        },
        &mut crate::elements::EventContext::default(),
        &app,
    );
    assert!(cloud.borrow().is_empty(), "⌘⌥⇧↵ is not the cloud chord");
}
