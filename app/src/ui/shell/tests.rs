use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{AppContext, Element, Icon, Text, TopbarButton};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::vec2f;
use goble_ui::theme::ColorToken;

use super::topbar::{build_live_indicator, WorkspaceChip};

use super::*;
use goble_ui::elements::running_indicator::SPINNER_FRAMES;
use goble_ui::elements::{EventContext, LayoutContext, PaintContext, SizeConstraint};
use goble_ui::render::RenderCommand;
use goble_ui::test_util::render_element;

fn drawn_texts(commands: &[RenderCommand]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// A cue at `count`.
fn cue(count: usize) -> Box<dyn Element> {
    build_live_indicator(&AppContext::default(), count, 0.0)
}

/// C3: while work is in flight the topbar cue shows the live count next to a
/// spinner frame.
#[test]
fn the_cue_shows_the_live_count_and_a_spinner() {
    let app = AppContext::default();
    let mut element = cue(3);
    let commands = render_element(&mut element, vec2f(200.0, 36.0), &app);
    let drawn = drawn_texts(&commands);

    assert!(
        drawn.iter().any(|t| t == "◆ 3"),
        "the live count is drawn: {drawn:?}"
    );
    assert!(
        drawn.iter().any(|t| SPINNER_FRAMES.contains(&t.as_str())),
        "a spinner frame is drawn: {drawn:?}"
    );
}

/// C3: at zero the cue is inert — it draws nothing and takes no room.
#[test]
fn the_cue_is_inert_at_zero() {
    let app = AppContext::default();
    let mut element = cue(0);
    let commands = render_element(&mut element, vec2f(200.0, 36.0), &app);

    assert!(commands.is_empty(), "zero work draws nothing: {commands:?}");
    assert_eq!(
        element.size(),
        Some(vec2f(0.0, 0.0)),
        "zero work takes no topbar room"
    );

    let mut ctx = EventContext::default();
    for event in [
        DispatchedEvent::MouseDown {
            position: vec2f(5.0, 5.0),
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: vec2f(5.0, 5.0),
            button: 0,
        },
    ] {
        assert!(
            !element.dispatch_event(&event, &mut ctx, &app),
            "an inert cue takes no pointer event"
        );
    }
}

/// C3: the cue shows the count and is not a hit target — a click passes
/// through it.
#[test]
fn the_cue_takes_no_pointer_event() {
    let app = AppContext::default();
    let mut element = cue(2);
    element.layout(
        SizeConstraint::loose(vec2f(200.0, 36.0)),
        &mut LayoutContext::default(),
        &app,
    );
    element.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

    let mut ctx = EventContext::default();
    for event in [
        DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        },
    ] {
        assert!(
            !element.dispatch_event(&event, &mut ctx, &app),
            "the live indicator takes no pointer event"
        );
    }
}

/// C3: the indicator slot does not change the established bar height.
#[test]
fn the_topbar_height_is_unchanged() {
    #[cfg(target_os = "macos")]
    assert_eq!(TOPBAR_HEIGHT, 36.0);
    #[cfg(not(target_os = "macos"))]
    assert_eq!(TOPBAR_HEIGHT, 40.0);
}

/// A tab with the given close control, the shape `build_workspace_chip`
/// builds.
fn tab(
    app: &AppContext,
    active: bool,
    on_click: Rc<RefCell<dyn FnMut(usize)>>,
    close: Box<dyn Element>,
) -> Box<dyn Element> {
    Box::new(WorkspaceChip::new(
        Text::new("Space 1")
            .with_theme_color(ColorToken::Text, app)
            .with_font_size(12.0)
            .finish(),
        1,
        active,
        on_click,
        close,
    ))
}

fn close_button(app: &AppContext, on_close: Rc<RefCell<dyn FnMut(usize)>>) -> Box<dyn Element> {
    let index = 1;
    TopbarButton::new(
        Icon::new("x")
            .with_size(10.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(16.0)
    .with_on_click(move || (on_close.borrow_mut())(index))
    .finish()
}

/// R2: a workspace tab is a browser-style tab — square-cornered, no border,
/// and as tall as the toolbar.
#[test]
fn a_workspace_tab_fills_the_bar_with_square_corners() {
    let app = AppContext::default();
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));
    let mut element = tab(&app, true, noop.clone(), close_button(&app, noop));
    let commands = render_element(&mut element, vec2f(400.0, TOPBAR_HEIGHT), &app);

    assert_eq!(
        element.size(),
        Some(vec2f(element.size().unwrap().x, TOPBAR_HEIGHT)),
        "the tab runs the full height of the toolbar"
    );
    let fills: Vec<_> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::FillRect {
                rect,
                corner_radius,
                ..
            } => Some((*rect, *corner_radius)),
            _ => None,
        })
        .collect();
    assert_eq!(
        fills.len(),
        1,
        "the active tab is the one filled surface: {fills:?}"
    );
    let (rect, radius) = fills[0];
    assert_eq!(radius, 0.0, "a tab has square corners: {rect:?}");
    assert_eq!(rect.height(), TOPBAR_HEIGHT, "the tab's fill spans the bar");
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, RenderCommand::StrokeRect { .. })),
        "a tab never strokes a border (that would double the neighbour's rule)"
    );
}

/// R2: an inactive tab draws no background at all, so the active one stays
/// the only filled surface.
#[test]
fn only_the_active_tab_is_filled() {
    let app = AppContext::default();
    let noop: Rc<RefCell<dyn FnMut(usize)>> = Rc::new(RefCell::new(|_: usize| {}));
    let mut element = tab(&app, false, noop.clone(), close_button(&app, noop));
    let commands = render_element(&mut element, vec2f(400.0, TOPBAR_HEIGHT), &app);

    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, RenderCommand::FillRect { .. })),
        "an inactive tab has no idle background: {commands:?}"
    );
}

/// R2: clicking the tab body still selects that space.
#[test]
fn a_click_on_the_tab_selects_its_space() {
    let app = AppContext::default();
    let selected: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let closed: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let on_click: Rc<RefCell<dyn FnMut(usize)>> = {
        let selected = selected.clone();
        Rc::new(RefCell::new(move |i: usize| selected.borrow_mut().push(i)))
    };
    let on_close: Rc<RefCell<dyn FnMut(usize)>> = {
        let closed = closed.clone();
        Rc::new(RefCell::new(move |i: usize| closed.borrow_mut().push(i)))
    };
    let mut element = tab(&app, true, on_click, close_button(&app, on_close));
    element.layout(
        SizeConstraint::loose(vec2f(400.0, TOPBAR_HEIGHT)),
        &mut LayoutContext::default(),
        &app,
    );
    element.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

    let mut ctx = EventContext::default();
    for event in [
        DispatchedEvent::MouseDown {
            position: vec2f(5.0, TOPBAR_HEIGHT / 2.0),
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: vec2f(5.0, TOPBAR_HEIGHT / 2.0),
            button: 0,
        },
    ] {
        element.dispatch_event(&event, &mut ctx, &app);
    }

    assert_eq!(
        *selected.borrow(),
        vec![1],
        "the tab body selects its space"
    );
    assert!(
        closed.borrow().is_empty(),
        "the tab body is not the close control"
    );
}

/// R2: the trailing "x" still closes that space, and only that one.
#[test]
fn a_click_on_the_close_control_closes_its_space() {
    let app = AppContext::default();
    let selected: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let closed: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let on_click: Rc<RefCell<dyn FnMut(usize)>> = {
        let selected = selected.clone();
        Rc::new(RefCell::new(move |i: usize| selected.borrow_mut().push(i)))
    };
    let on_close: Rc<RefCell<dyn FnMut(usize)>> = {
        let closed = closed.clone();
        Rc::new(RefCell::new(move |i: usize| closed.borrow_mut().push(i)))
    };
    let mut element = tab(&app, true, on_click, close_button(&app, on_close));
    let commands = render_element(&mut element, vec2f(400.0, TOPBAR_HEIGHT), &app);
    let (x, y) = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawIcon {
                origin, name, size, ..
            } if name == "x-close" => Some((origin.x + size / 2.0, origin.y + size / 2.0)),
            _ => None,
        })
        .expect("the close \"x\" is drawn");

    let mut ctx = EventContext::default();
    for event in [
        DispatchedEvent::MouseDown {
            position: vec2f(x, y),
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: vec2f(x, y),
            button: 0,
        },
    ] {
        element.dispatch_event(&event, &mut ctx, &app);
    }

    assert_eq!(*closed.borrow(), vec![1], "the \"x\" closes its space");
    assert!(
        selected.borrow().is_empty(),
        "a click on the close control must not also select"
    );
}
