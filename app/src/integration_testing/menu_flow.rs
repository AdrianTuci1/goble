//! Regression coverage for popup/tray menus actually opening.
//!
//! Two bugs historically kept every popup menu from opening:
//!   * The topbar `SpaceBar` swallowed *every* `MouseDown`/`MouseUp` in the
//!     window (its hit-test returned the nearest tab for any x and its release
//!     handler returned `true` unconditionally), so no pointer event below the
//!     topbar ever reached the body — no 3-dots menu, no composer pill.
//!   * Because the element tree is rebuilt every frame, menu open state must be
//!     app-owned (`Rc<RefCell<bool>>`); a fresh flag per frame would be wiped
//!     on the rebuild before the panel can render.
//!
//! These tests mount the real `RootView` and drive a real click, plus compose a
//! `ChatComposer` directly to cover the rich-input pills, asserting the open
//! flag flips.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::root_view::RootView;
use goble_desktop_service::DesktopState;
use goble_ui::elements::{
    AppContext, ChatComposer, EventContext, LayoutContext, PaintContext, PopupMenuItem,
    SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::vec2f;
use goble_ui::render::{RenderCommand, Renderer};
use goble_ui::Element;

const W: f32 = 1024.0;
const H: f32 = 768.0;

/// Mount the real `RootView` over a live `DesktopState` (shows the chat pane
/// with the agent header 3-dots menu). The temp dir is kept alive so the
/// thread-store path the state owns is not removed mid-test.
fn build_root() -> (RootView, Arc<DesktopState>, tempfile::TempDir) {
    let (desktop, dir) = common::desktop_state();
    let view = RootView::new(&AppContext::default(), &desktop, None);
    (view, desktop, dir)
}

/// One layout+paint pass (the per-frame rebuild). `RootView.layout` calls
/// `rebuild`, so each call replaces the element tree exactly like a frame.
fn render(root: &mut RootView, app: &AppContext) -> Vec<RenderCommand> {
    render_with_pointer(root, app, None)
}

/// The same pass with the pointer placed at `pos`, which is where hover lives:
/// the tree is rebuilt every frame, so an element reads the cursor at paint.
fn render_with_pointer(
    root: &mut RootView,
    app: &AppContext,
    pos: Option<(f32, f32)>,
) -> Vec<RenderCommand> {
    let _ = root.layout(SizeConstraint::loose(vec2f(W, H)), &mut LayoutContext::default(), app);
    let renderer = Renderer::new();
    let mut paint_ctx = PaintContext::new(renderer);
    if let Some((x, y)) = pos {
        paint_ctx.cursor_position = vec2f(x, y);
        paint_ctx.cursor_inside = true;
    }
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    paint_ctx
        .renderer
        .take()
        .map(|r| r.commands().to_vec())
        .unwrap_or_default()
}

/// Center of the first `name` icon drawn.
fn icon_center(cmds: &[RenderCommand], name: &str) -> Option<(f32, f32)> {
    cmds.iter().find_map(|c| match c {
        RenderCommand::DrawIcon { origin, name: n, size, .. } if n == name => {
            Some((origin.x + size / 2.0, origin.y + size / 2.0))
        }
        _ => None,
    })
}

/// Center of the top-most `name` icon drawn (smallest y), so a glyph that also
/// appears lower in the window (the composer pills reuse `chevron-down`) still
/// resolves to the toolbar control.
fn topmost_icon_center(cmds: &[RenderCommand], name: &str) -> Option<(f32, f32)> {
    cmds.iter()
        .filter_map(|c| match c {
            RenderCommand::DrawIcon { origin, name: n, size, .. } if n == name => {
                Some((origin.x + size / 2.0, origin.y + size / 2.0))
            }
            _ => None,
        })
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

/// A point inside the row that drew `text`: its own left end, a few px in.
fn text_center(cmds: &[RenderCommand], text: &str) -> Option<(f32, f32)> {
    cmds.iter().find_map(|c| match c {
        RenderCommand::DrawText { text: run, origin, .. } if run == text => {
            Some((origin.x + 8.0, origin.y + 6.0))
        }
        _ => None,
    })
}

/// Every place `text` was drawn, in paint order.
fn text_positions(cmds: &[RenderCommand], text: &str) -> Vec<(f32, f32)> {
    cmds.iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { text: run, origin, .. } if run == text => {
                Some((origin.x + 8.0, origin.y + 6.0))
            }
            _ => None,
        })
        .collect()
}

/// The top-most place `text` was drawn (smallest y), so a label the pane's
/// composer also carries (the environment pill) still resolves to the topbar
/// surface above it.
fn topmost_text_position(cmds: &[RenderCommand], text: &str) -> Option<(f32, f32)> {
    text_positions(cmds, text)
        .into_iter()
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
}

/// Dispatch a full press+release at `pos`. Between the two, we re-render so the
/// tree is rebuilt (matching the real about_to_wait redraw between down and up).
fn click(root: &mut RootView, app: &AppContext, pos: (f32, f32)) {
    let mut ctx = EventContext::default();
    let down = DispatchedEvent::MouseDown { position: vec2f(pos.0, pos.1), button: 0 };
    let _ = root.dispatch_event(&down, &mut ctx, app);
    // Rebuild the tree (a redraw happens between down and up in the real app).
    let _ = render(root, app);
    let up = DispatchedEvent::MouseUp { position: vec2f(pos.0, pos.1), button: 0 };
    let _ = root.dispatch_event(&up, &mut ctx, app);
}

/// The same click with the pointer held at `pos` across the rebuild. A surface
/// that only exists while the pointer is over it (a menu's hover tray) is drawn
/// by the frame between press and release, so that frame must see the pointer —
/// the real one does, because the pointer has not moved.
fn click_with_pointer(root: &mut RootView, app: &AppContext, pos: (f32, f32)) {
    let mut ctx = EventContext::default();
    let down = DispatchedEvent::MouseDown { position: vec2f(pos.0, pos.1), button: 0 };
    let _ = root.dispatch_event(&down, &mut ctx, app);
    let _ = render_with_pointer(root, app, Some(pos));
    let up = DispatchedEvent::MouseUp { position: vec2f(pos.0, pos.1), button: 0 };
    let _ = root.dispatch_event(&up, &mut ctx, app);
}

#[test]
fn topbar_environment_menu_opens() {
    let (mut root, _desktop, _dir) = build_root();
    let app = AppContext::default();
    let _ = render(&mut root, &app);
    let cmds = render(&mut root, &app);
    // The new topbar's "+ ▾" control opens the environment menu from its
    // chevron: the top-most chevron in the window (the toolbar paints last now,
    // and the composer pills use the same glyph lower down).
    let pos = topmost_icon_center(&cmds, "chevron-down").expect("topbar environment menu trigger");
    click(&mut root, &app, pos);

    let open = root.state_rc().borrow().env_selector_open.clone();
    assert!(*open.borrow(), "the environment trigger should open its menu");
}

/// The environment menu's first entry launches a terminal: choosing it opens a
/// new space whose root pane is a plain PTY — the same workspace "+" opens, and
/// the one thing the menu could not do before (it only listed environments).
#[test]
fn topbar_environment_menu_launches_a_terminal() {
    let (mut root, _desktop, _dir) = build_root();
    let app = AppContext::default();
    let _ = render(&mut root, &app);
    let cmds = render(&mut root, &app);
    let pos = topmost_icon_center(&cmds, "chevron-down").expect("topbar environment menu trigger");
    click(&mut root, &app, pos);

    let spaces_before = root.state_rc().borrow().spaces.len();
    let cmds = render(&mut root, &app);
    let item = text_center(&cmds, "New terminal").expect("the menu lists the terminal entry");
    click(&mut root, &app, item);

    let state_rc = root.state_rc();
    let state = state_rc.borrow();
    assert_eq!(
        state.spaces.len(),
        spaces_before + 1,
        "choosing the entry opens a new space"
    );
    let space = state.spaces.last().expect("the new space");
    match &space.root {
        goble_app::ui::Pane::Leaf { kind, .. } => assert_eq!(
            *kind,
            goble_app::ui::PaneKind::Terminal,
            "the new space's pane is a terminal"
        ),
        other => panic!("the new space's root should be one leaf, got {other:?}"),
    }
    assert!(
        !*state_rc.borrow().env_selector_open.borrow(),
        "choosing an entry closes the menu"
    );
}

/// Choosing an environment in the same menu opens a new space that runs on it
/// (rather than only switching the active environment): the tab lands at the
/// end of the top strip, exactly where "+" puts one.
#[test]
fn topbar_environment_menu_opens_a_space_in_that_environment() {
    let (mut root, _desktop, _dir) = build_root();
    let app = AppContext::default();
    let _ = render(&mut root, &app);
    let cmds = render(&mut root, &app);
    let pos = topmost_icon_center(&cmds, "chevron-down").expect("topbar environment menu trigger");
    click(&mut root, &app, pos);

    let (spaces_before, last_before) = {
        let state_rc = root.state_rc();
        let state = state_rc.borrow();
        (state.spaces.len(), state.spaces.last().map(|s| s.name.clone()))
    };
    let cmds = render(&mut root, &app);
    let item = text_center(&cmds, "Remote (xrdp)").expect("the menu lists the environments");
    click(&mut root, &app, item);

    let new_name = {
        let state_rc = root.state_rc();
        let state = state_rc.borrow();
        assert_eq!(
            state.spaces.len(),
            spaces_before + 1,
            "choosing an environment opens a new space"
        );
        let space = state.spaces.last().expect("the new space");
        assert_eq!(
            space.medium, "remote-xrdp",
            "the new space runs on the chosen environment"
        );
        assert_eq!(
            state.active_space,
            state.spaces.len() - 1,
            "the new space is the active one"
        );
        match &space.root {
            goble_app::ui::Pane::Leaf { kind, .. } => {
                assert_eq!(*kind, goble_app::ui::PaneKind::Terminal, "the new space is a plain PTY")
            }
            other => panic!("the new space's root should be one leaf, got {other:?}"),
        }
        assert!(
            !*state_rc.borrow().env_selector_open.borrow(),
            "choosing an entry closes the menu"
        );
        space.name.clone()
    };
    assert_ne!(
        Some(new_name.clone()),
        last_before,
        "the new tab is added next to the existing ones, not renamed onto the last one"
    );

    // The tab is drawn in the top strip, above the body, next to the old ones.
    let cmds = render(&mut root, &app);
    let topbar_height = goble_app::ui::shell::TOPBAR_HEIGHT;
    let (_, tab_y) = text_center(&cmds, &new_name).expect("the new space's tab is drawn");
    assert!(
        tab_y < topbar_height,
        "the new space's tab sits in the top strip (y={tab_y}, bar is {topbar_height} tall)"
    );
}

/// Hovering an environment row shows the tray warp-new draws beside a hovered
/// menu item: the environment's own name and a "Make default" button. It makes
/// that environment the one new spaces start in — the pick "+" uses — without
/// opening anything, which is what clicking the row itself does.
#[test]
fn hovering_an_environment_row_offers_make_default_in_a_tray() {
    let (mut root, _desktop, _dir) = build_root();
    let app = AppContext::default();
    let _ = render(&mut root, &app);
    let cmds = render(&mut root, &app);
    let trigger = topmost_icon_center(&cmds, "chevron-down").expect("topbar environment menu trigger");
    click(&mut root, &app, trigger);

    let cmds = render(&mut root, &app);
    let row = topmost_text_position(&cmds, "Remote (xrdp)").expect("the menu lists the environments");
    assert!(
        text_center(&cmds, "Make default").is_none(),
        "no tray is drawn before the pointer is over a row"
    );

    // The pointer lands on the row: that frame records it, and the next frame —
    // the pointer still there — draws the tray it calls for.
    let _ = render_with_pointer(&mut root, &app, Some(row));
    let cmds = render_with_pointer(&mut root, &app, Some(row));

    let names = text_positions(&cmds, "Remote (xrdp)");
    assert_eq!(
        names.len(),
        2,
        "the row and the tray beside it both name the environment"
    );
    assert!(
        names[1].0 > names[0].0,
        "the tray is drawn beside the row, not over it: {:?}",
        names
    );
    let button = text_center(&cmds, "Make default").expect("the tray offers Make default");
    assert!(
        names[1].0 < W && button.0 < W,
        "the tray is drawn inside the window: {names:?}, button at {button:?}"
    );

    let spaces_before = root.state_rc().borrow().spaces.len();
    assert_eq!(
        root.media_state_rc().borrow().selected_medium_id(),
        "local",
        "local is the default to begin with"
    );

    click_with_pointer(&mut root, &app, button);

    assert_eq!(
        root.media_state_rc().borrow().selected_medium_id(),
        "remote-xrdp",
        "\"Make default\" makes that environment the one new spaces start in"
    );
    let state_rc = root.state_rc();
    assert_eq!(
        state_rc.borrow().spaces.len(),
        spaces_before,
        "and opens no space, unlike clicking the row"
    );
    assert!(
        !*state_rc.borrow().env_selector_open.borrow(),
        "the menu closes once the choice is made"
    );

    // The choice shows where it was made: the composer's environment pill reads
    // the environment that is now the default, and the tray is gone with the
    // menu that showed it.
    let cmds = render(&mut root, &app);
    assert!(
        text_center(&cmds, "Make default").is_none(),
        "the tray closes with the menu"
    );
    assert_eq!(
        text_positions(&cmds, "Remote (xrdp)").len(),
        1,
        "the environment pill now reads the environment that is the default"
    );
}

/// The tray's button is inert for the environment that already is the default:
/// it stays drawn (so the tray looks the same on every row) but neither changes
/// the default nor closes the menu.
#[test]
fn the_tray_s_make_default_does_nothing_for_the_environment_already_default() {
    let (mut root, _desktop, _dir) = build_root();
    let app = AppContext::default();
    // The active environment is the one new spaces already start in.
    root.media_state_rc()
        .borrow_mut()
        .select_medium("remote-xrdp");

    let _ = render(&mut root, &app);
    let cmds = render(&mut root, &app);
    let trigger = topmost_icon_center(&cmds, "chevron-down").expect("topbar environment menu trigger");
    click(&mut root, &app, trigger);

    let cmds = render(&mut root, &app);
    let row = topmost_text_position(&cmds, "Remote (xrdp)").expect("the menu lists the environments");
    let _ = render_with_pointer(&mut root, &app, Some(row));
    let cmds = render_with_pointer(&mut root, &app, Some(row));
    let button = text_center(&cmds, "Make default").expect("the tray still offers the button");

    let spaces_before = root.state_rc().borrow().spaces.len();
    click_with_pointer(&mut root, &app, button);

    assert_eq!(
        root.media_state_rc().borrow().selected_medium_id(),
        "remote-xrdp",
        "the environment that is already the default is left as it is"
    );
    let state_rc = root.state_rc();
    assert_eq!(
        state_rc.borrow().spaces.len(),
        spaces_before,
        "and no space is opened"
    );
    assert!(
        *state_rc.borrow().env_selector_open.borrow(),
        "an inert button does not close the menu either"
    );
}

#[test]
fn agent_header_3_dots_opens_menu() {
    let (mut root, _desktop, _dir) = build_root();
    let app = AppContext::default();
    // Two render passes: first establishes origins/sizes, second is the frame
    // that actually receives the click.
    let _ = render(&mut root, &app);
    let cmds = render(&mut root, &app);
    let pos = topmost_icon_center(&cmds, "dots-horizontal").expect("agent 3-dots icon");
    click(&mut root, &app, pos);

    let state_rc = root.state_rc();
    let open = {
        let s = state_rc.borrow();
        s.agent_header_menus
            .get(&s.active_pane_id)
            .cloned()
            .expect("an app-owned agent-header menu flag for the active pane")
    };
    assert!(*open.borrow(), "3-dots menu should open after clicking it");
}

/// Mount a `ChatComposer` that carries a `PopupMenu` pill, click the pill's
/// trigger icon, and assert the app-owned open flag flips.
///
/// This is the rich-input regression: the model, directory, branch and harness
/// pills all share the same dispatch path (`ChatComposer` → `self.root` →
/// `Padding`), so proving each one opens proves the `Padding`/`PopupMenu`
/// wiring the topbar swallow bug used to break.
fn assert_composer_pill_opens(mut composer: Box<dyn Element>, trigger_icon: &str, open: Rc<RefCell<bool>>) {
    let app = AppContext::default();

    // Layout + paint so the trigger's bounds are known.
    let _ = composer.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let renderer = Renderer::new();
    let mut paint_ctx = PaintContext::new(renderer);
    composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let cmds = paint_ctx.renderer.take().map(|r| r.commands().to_vec()).unwrap_or_default();

    let (x, y) = icon_center(&cmds, trigger_icon).expect("composer pill trigger icon");
    let mut ctx = EventContext::default();
    let down = DispatchedEvent::MouseDown { position: vec2f(x, y), button: 0 };
    let up = DispatchedEvent::MouseUp { position: vec2f(x, y), button: 0 };
    // The composer is rebuilt each frame too, so re-lay it out + paint it
    // between down and up (a real redraw), otherwise the rebuilt trigger has no
    // origin yet and its bounds are unknown when the release arrives.
    let _ = composer.dispatch_event(&down, &mut ctx, &app);
    let _ = composer.layout(
        SizeConstraint::loose(vec2f(600.0, 400.0)),
        &mut LayoutContext::default(),
        &app,
    );
    let renderer = Renderer::new();
    let mut paint_ctx = PaintContext::new(renderer);
    composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
    let _ = composer.dispatch_event(&up, &mut ctx, &app);

    assert!(*open.borrow(), "composer pill ({trigger_icon}) should open its dropdown");
}

#[test]
fn composer_model_pill_opens_menu() {
    let open = Rc::new(RefCell::new(false));
    let open_menu = open.clone();
    let composer = ChatComposer::new()
        .with_model_label("gpt-4o")
        .with_model_menu(
            vec![
                PopupMenuItem::new("gpt-4o"),
                PopupMenuItem::new("gpt-4o-mini"),
            ],
            open_menu,
            |_| {},
        )
        .finish();
    assert_composer_pill_opens(composer, "sparkle", open);
}

#[test]
fn composer_dir_pill_opens_menu() {
    let open = Rc::new(RefCell::new(false));
    let open_menu = open.clone();
    let composer = ChatComposer::new()
        .with_path_label("/Users/me/project")
        .with_dir_menu(
            vec![
                PopupMenuItem::new("/Users/me/project"),
                PopupMenuItem::new("/Users/me/other"),
            ],
            open_menu,
            |_| {},
        )
        .finish();
    assert_composer_pill_opens(composer, "folder", open);
}

#[test]
fn composer_branch_pill_opens_menu() {
    let open = Rc::new(RefCell::new(false));
    let open_menu = open.clone();
    let composer = ChatComposer::new()
        .with_branch_label("main")
        .with_branch_menu(
            vec![
                PopupMenuItem::new("main"),
                PopupMenuItem::new("dev"),
            ],
            open_menu,
            |_| {},
        )
        .finish();
    assert_composer_pill_opens(composer, "git-branch", open);
}

#[test]
fn composer_harness_pill_opens_menu() {
    let open = Rc::new(RefCell::new(false));
    let open_menu = open.clone();
    let composer = ChatComposer::new()
        .with_harness_label("Local")
        .with_harness_menu(
            vec![
                PopupMenuItem::new("Local"),
                PopupMenuItem::new("Remote"),
            ],
            open_menu,
            |_| {},
        )
        .finish();
    // The composer harness pill uses the "computer" glyph, which renders as the
    // registered "agentmode" asset (they share a glyph in `icon.rs`).
    assert_composer_pill_opens(composer, "agentmode", open);
}

/// The rich input carries no account button any more. The pills around its
/// editor describe the surface (the harness and the directory above, the model
/// below); nothing account-shaped is drawn in the pane's input at all, so the
/// app must not wire one.
#[test]
fn the_rich_input_has_no_account_button() {
    let (mut root, _desktop, _dir) = build_root();
    let app = AppContext::default();
    let _ = render(&mut root, &app);
    let cmds = render(&mut root, &app);
    assert!(
        icon_center(&cmds, "user").is_none(),
        "the account button is gone from the rich input"
    );
    assert!(icon_center(&cmds, "plus").is_some(), "the attach pill is still drawn");
}

/// The turn-status footer's still-running line is information the user needs —
/// how much work is running — so it stays drawn while a worker execution is in
/// flight, and it is not a hit target: nothing opens from it.
#[test]
fn the_status_footer_names_the_work_still_running() {
    let (mut root, _desktop, _dir) = build_root();
    let app = AppContext::default();
    // A worker execution is running while the pane's own turn is idle, so the
    // footer is in its still-running state.
    root.state_rc().borrow_mut().apply_agent_started(
        "worker-a",
        "trace-1",
        "agent-1",
        "2026-09-11T10:00:00Z",
    );
    assert!(
        root.state_rc()
            .borrow()
            .pane_chat_snapshot()
            .get(&1)
            .expect("the active pane's snapshot")
            .turn_status
            .is_in_flight(),
        "the footer reads the still-running execution"
    );

    let _ = render(&mut root, &app);
    let cmds = render(&mut root, &app);
    let (line, origin) = cmds
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. } if text.contains("still running") => {
                Some((text.clone(), *origin))
            }
            _ => None,
        })
        .expect("the still-running footer line is drawn");
    assert!(
        line.contains("1 execution still running"),
        "the footer names how much work is running: {line:?}"
    );

    click(&mut root, &app, (origin.x + 4.0, origin.y + 4.0));

    let state = root.state_rc();
    let s = state.borrow();
    assert!(
        !s.settings_overlay_open && !s.crons_open && !s.right_sidebar_open,
        "clicking the footer line opens nothing"
    );
}
