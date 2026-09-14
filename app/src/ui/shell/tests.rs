use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::{AppContext, Element, Icon, Text, TopbarButton};
use goble_ui::event::{DispatchedEvent, ModifiersState};
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::theme::ColorToken;

use super::topbar::{build_live_indicator, insertion_index, WorkspaceChip};

use super::*;
use crate::root_view::RootView;
use crate::state::{PaneSession, UiState, NEW_CONVERSATION_TITLE};
use crate::ui::{Pane, PaneKind, Space, SplitDir};
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
/// spinner frame and the diamond that marks the count.
#[test]
fn the_cue_shows_the_live_count_and_a_spinner() {
    let app = AppContext::default();
    let mut element = cue(3);
    let commands = render_element(&mut element, vec2f(200.0, 36.0), &app);
    let drawn = drawn_texts(&commands);

    assert!(
        drawn.iter().any(|t| t == "3"),
        "the live count is drawn: {drawn:?}"
    );
    assert!(
        commands.iter().any(|c| matches!(
            c,
            RenderCommand::DrawIcon { name, .. } if name == "diamond"
        )),
        "the count is marked with the diamond icon: {commands:?}"
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

/// One frame of a chip with the pointer at `cursor`. Hover is read at paint, so
/// the pointer is what decides whether the tab offers its close control.
fn render_with_pointer(
    element: &mut Box<dyn Element>,
    size: Vector2F,
    app: &AppContext,
    cursor: Option<Vector2F>,
) -> Vec<RenderCommand> {
    let _ = element.layout(
        SizeConstraint::loose(size),
        &mut LayoutContext::default(),
        app,
    );
    let mut ctx = PaintContext::new(goble_ui::render::Renderer::new());
    if let Some(position) = cursor {
        ctx.cursor_inside = true;
        ctx.cursor_position = position;
    }
    element.paint(vec2f(0.0, 0.0), &mut ctx, app);
    ctx.renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default()
}

/// The centre of the close "x" a frame drew, when it drew one at all.
fn close_point(commands: &[RenderCommand]) -> Option<Vector2F> {
    commands.iter().find_map(|c| match c {
        RenderCommand::DrawIcon {
            origin, name, size, ..
        } if name == "x-close" => Some(vec2f(origin.x + size / 2.0, origin.y + size / 2.0)),
        _ => None,
    })
}

/// R33: the trailing "x" is drawn only while the pointer is over the tab, and a
/// click on it still closes that space — and only that one.
#[test]
fn the_close_control_appears_on_hover_and_closes_its_space() {
    let app = AppContext::default();
    let selected: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let closed: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let on_click: Rc<RefCell<dyn FnMut(usize)>> = {
        let selected = selected.clone();
        Rc::new(RefCell::new(move |i: usize| {
            selected.borrow_mut().push(i)
        }))
    };
    let on_close: Rc<RefCell<dyn FnMut(usize)>> = {
        let closed = closed.clone();
        Rc::new(RefCell::new(move |i: usize| {
            closed.borrow_mut().push(i)
        }))
    };
    let mut element = tab(&app, true, on_click, close_button(&app, on_close));

    // The pointer away from the tab: the control is not drawn, so an unhovered
    // tab carries no invisible hit target.
    let away = render_with_pointer(
        &mut element,
        vec2f(400.0, TOPBAR_HEIGHT),
        &app,
        Some(vec2f(380.0, TOPBAR_HEIGHT / 2.0)),
    );
    assert!(
        close_point(&away).is_none(),
        "an unhovered tab draws no close control"
    );

    // Over the tab: the control appears, and the click that lands on it closes
    // that space without selecting it.
    let hovered = render_with_pointer(
        &mut element,
        vec2f(400.0, TOPBAR_HEIGHT),
        &app,
        Some(vec2f(20.0, TOPBAR_HEIGHT / 2.0)),
    );
    let point = close_point(&hovered).expect("a hovered tab draws its close control");

    let mut ctx = EventContext::default();
    for event in [
        DispatchedEvent::MouseDown {
            position: point,
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: point,
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

/// The window the shell tests lay the whole app out in.
fn window() -> Vector2F {
    vec2f(1024.0, 768.0)
}

/// The whole app over a real store, with the first-run overlays out of the way,
/// plus its live state, the service handle and the temp dir the store owns. The
/// tab labels are read off a real frame (`RootView::layout` rebuilds the tree
/// the topbar draws from), so these tests assert the drawn label, not a
/// helper's return value.
fn app_root() -> (
    Box<dyn Element>,
    Rc<RefCell<UiState>>,
    Arc<DesktopState>,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    );
    let app = AppContext::default();
    let view = RootView::new(&app, &desktop, None);
    let state = view.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.settings_overlay_open = false;
        s.right_sidebar_open = false;
    }
    (Box::new(view), state, desktop, dir)
}

/// A pane session at `path` that owns no conversation of its own.
fn pane_at(path: &str) -> PaneSession {
    PaneSession {
        conversation_id: String::new(),
        draft: String::new(),
        path: path.to_string(),
    }
}

/// Every text run drawn in the topbar band, which is where the tab labels live.
/// Scoped to the bar because the same string is also drawn lower down (the
/// composer's working-directory pill shows the same shortened path).
fn topbar_texts(commands: &[RenderCommand]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { text, origin, .. } if origin.y < TOPBAR_HEIGHT => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

/// The origin of the topbar run that reads exactly `label`.
fn topbar_label_origin(commands: &[RenderCommand], label: &str) -> Option<Vector2F> {
    commands.iter().find_map(|c| match c {
        RenderCommand::DrawText { text, origin, .. }
            if text == label && origin.y < TOPBAR_HEIGHT =>
        {
            Some(*origin)
        }
        _ => None,
    })
}

/// One key through the whole app tree, after a frame — so the field that reads
/// its own value was built from the last frame's state, as it is in the app.
fn press(root: &mut Box<dyn Element>, app: &AppContext, name: &str) {
    let _ = root.layout(
        SizeConstraint::loose(window()),
        &mut LayoutContext::default(),
        app,
    );
    let mut paint_ctx = PaintContext::default();
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    let mut ctx = EventContext::default();
    root.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: name.to_string(),
            modifiers: ModifiersState::none(),
        },
        &mut ctx,
        app,
    );
}

/// A full press+release at `pos`, with a frame between the two events — the way
/// the running app rebuilds the tree between them. The double-click window is
/// therefore read across frames, which is where it has to hold.
fn click(root: &mut Box<dyn Element>, app: &AppContext, pos: Vector2F) {
    let mut ctx = EventContext::default();
    let _ = root.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: pos,
            button: 0,
        },
        &mut ctx,
        app,
    );
    let _ = render_element(root, window(), app);
    let _ = root.dispatch_event(
        &DispatchedEvent::MouseUp {
            position: pos,
            button: 0,
        },
        &mut ctx,
        app,
    );
}

/// A tab holding one terminal pane is named by that pane's working directory,
/// read the way the `~` symbol means it — the same shortening the composer's
/// path pill uses — and follows the pane when it moves. A tab opened on a path
/// of its own reads that path.
#[test]
fn a_terminal_tab_is_named_by_its_working_directory() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = app_root();
    let home = std::env::var("HOME").expect("HOME is set");
    {
        let mut s = state.borrow_mut();
        s.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Terminal,
        })];
        s.active_space = 0;
        s.active_pane_id = 7;
        s.pane_sessions.insert(7, pane_at(&home));
    }

    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "~"),
        "a terminal in the home directory is labelled ~: {drawn:?}"
    );

    state
        .borrow_mut()
        .set_pane_path(7, format!("{home}/Projects/goble"));
    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "~/Projects/goble"),
        "a directory under the home directory is labelled under ~: {drawn:?}"
    );

    state
        .borrow_mut()
        .set_pane_path(7, "/tmp/elsewhere".to_string());
    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "/tmp/elsewhere"),
        "a tab opened on another path reads that path: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|t| t == "~"),
        "the old directory is gone from the tab: {drawn:?}"
    );
}

/// A tab holding an agent pane reads `New Agent` while its conversation has no
/// subject, and the conversation's subject — the chat's title — once it has one.
#[test]
fn an_agent_tab_reads_new_agent_until_its_conversation_has_a_subject() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = app_root();
    let fresh = desktop
        .create_chat(NEW_CONVERSATION_TITLE, None, None)
        .expect("a conversation nobody has named");
    let subject = desktop
        .create_chat("Demo · Hot reload", None, None)
        .expect("a conversation with a subject");
    {
        let mut s = state.borrow_mut();
        s.refresh_conversations(&desktop);
        s.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Chat,
        })];
        s.active_space = 0;
        s.active_pane_id = 7;
        let mut session = pane_at("");
        session.conversation_id = fresh.clone();
        s.pane_sessions.insert(7, session);
    }

    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "New Agent"),
        "an agent with no conversation subject reads New Agent: {drawn:?}"
    );

    state
        .borrow_mut()
        .pane_sessions
        .get_mut(&7)
        .expect("the pane's session")
        .conversation_id = subject.clone();
    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "Demo · Hot reload"),
        "the tab takes the conversation's subject: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|t| t == "New Agent"),
        "the placeholder is gone once there is a subject: {drawn:?}"
    );
}

/// A tab holding several panes — a terminal and an agent split — reads as its
/// focused pane, so the tab names whatever the user is working in.
#[test]
fn a_tab_with_several_panes_reads_as_its_focused_pane() {
    let app = AppContext::default();
    let (mut root, state, desktop, _dir) = app_root();
    let subject = desktop
        .create_chat("Demo · Hot reload", None, None)
        .expect("a conversation with a subject");
    let home = std::env::var("HOME").expect("HOME is set");
    {
        let mut s = state.borrow_mut();
        s.refresh_conversations(&desktop);
        s.spaces = vec![Space::unnamed(Pane::Split {
            id: 9,
            dir: SplitDir::Horizontal,
            ratio: 0.5,
            first: Box::new(Pane::Leaf {
                id: 7,
                kind: PaneKind::Terminal,
            }),
            second: Box::new(Pane::Leaf {
                id: 8,
                kind: PaneKind::Chat,
            }),
        })];
        s.active_space = 0;
        s.active_pane_id = 7;
        s.pane_sessions.insert(7, pane_at(&home));
        let mut session = pane_at("");
        session.conversation_id = subject.clone();
        s.pane_sessions.insert(8, session);
    }

    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "~"),
        "with the terminal focused the tab names its directory: {drawn:?}"
    );

    state.borrow_mut().active_pane_id = 8;
    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "Demo · Hot reload"),
        "with the agent focused the tab names its subject: {drawn:?}"
    );
}

/// A tab the user renamed by hand keeps the name they typed, for good: the
/// label derived from what the tab holds never takes it back.
#[test]
fn a_tab_the_user_renamed_keeps_the_typed_name() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = app_root();
    let home = std::env::var("HOME").expect("HOME is set");
    {
        let mut s = state.borrow_mut();
        s.spaces = vec![Space::unnamed(Pane::Leaf {
            id: 7,
            kind: PaneKind::Terminal,
        })];
        s.active_space = 0;
        s.active_pane_id = 7;
        s.pane_sessions.insert(7, pane_at(&home));
    }

    let commands = render_element(&mut root, window(), &app);
    let chip = topbar_label_origin(&commands, "~").expect("the tab reads its directory");

    // Double-click the tab: the inline field opens on the label it draws now.
    // Both clicks go through a real frame, which is where the app's
    // double-click window is read.
    let spot = vec2f(chip.x + 4.0, chip.y + 6.0);
    click(&mut root, &app, spot);
    click(&mut root, &app, spot);
    {
        let s = state.borrow();
        assert!(
            s.space_rename_editing,
            "the second click opens the rename field"
        );
        assert_eq!(
            s.space_rename_draft, "~",
            "the field opens on the label the tab draws"
        );
    }

    // Clear the seeded label, type the user's name and commit with Return.
    press(&mut root, &app, "Backspace");
    for key in ["p", "r", "o", "i", "e", "c", "t"] {
        press(&mut root, &app, key);
    }
    press(&mut root, &app, "Enter");

    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "proiect"),
        "the typed name is drawn: {drawn:?}"
    );

    // The pane moving elsewhere does not take the name back.
    state
        .borrow_mut()
        .set_pane_path(7, "/tmp/elsewhere".to_string());
    let drawn = topbar_texts(&render_element(&mut root, window(), &app));
    assert!(
        drawn.iter().any(|t| t == "proiect"),
        "the typed name survives a move: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|t| t == "/tmp/elsewhere"),
        "the derived label does not come back over the typed name: {drawn:?}"
    );
}

/// Three tabs as the strip's own registry reports them after a paint: 60 pt
/// wide, side by side, so their centers are 130, 190 and 250.
fn three_drawn_tabs() -> Vec<RectF> {
    [
        rectf(100.0, 0.0, 60.0, 20.0),
        rectf(160.0, 0.0, 60.0, 20.0),
        rectf(220.0, 0.0, 60.0, 20.0),
    ]
    .to_vec()
}

/// The insertion index a pane dropped on the strip resolves to is counted off
/// the drawn tabs, the way the tab reorder counts them: left of the strip is
/// before the first tab, right of it is after the last one, and a pointer
/// between two tabs names the boundary it has passed.
#[test]
fn the_drop_index_counts_the_drawn_tab_centers_left_of_the_pointer() {
    let tabs = three_drawn_tabs();

    assert_eq!(
        insertion_index(&tabs, 0.0),
        0,
        "left of the strip: before the first tab"
    );
    assert_eq!(
        insertion_index(&tabs, 140.0),
        1,
        "just past the first tab's center"
    );
    assert_eq!(
        insertion_index(&tabs, 200.0),
        2,
        "past the second tab's center, before the third"
    );
    assert_eq!(
        insertion_index(&tabs, 400.0),
        3,
        "right of the strip: after the last tab"
    );
    assert_eq!(
        insertion_index(&tabs, 99.0),
        0,
        "just left of the strip is still before every tab"
    );
    assert_eq!(insertion_index(&[], 42.0), 0, "no tabs, no other place to go");
}
