use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::AppContext;
use crate::elements::Element;
use crate::elements::LayoutContext;
use crate::elements::PopupMenuItem;
use crate::elements::SizeConstraint;
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

#[test]
fn composer_puts_the_context_row_above_the_editor_and_the_model_below_it() {
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

    assert!(plus < editor, "the attach pill sits above the editor: {plus} vs {editor}");
    assert!(
        sparkle > editor,
        "the model pill sits below the editor: {sparkle} vs {editor}"
    );

    // The account button is gone from the rich input.
    assert!(icon_y("user").is_none(), "no account icon in the rich input");

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
