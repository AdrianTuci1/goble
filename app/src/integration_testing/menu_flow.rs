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

use std::cell::RefCell;
use std::rc::Rc;

use goble_app::root_view::RootView;
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

/// Mount the real `RootView` over mock state (shows the chat pane with the
/// agent header 3-dots menu).
fn build_root() -> RootView {
    RootView::new(&AppContext::default(), None, None)
}

/// One layout+paint pass (the per-frame rebuild). `RootView.layout` calls
/// `rebuild`, so each call replaces the element tree exactly like a frame.
fn render(root: &mut RootView, app: &AppContext) -> Vec<RenderCommand> {
    let _ = root.layout(SizeConstraint::loose(vec2f(W, H)), &mut LayoutContext::default(), app);
    let renderer = Renderer::new();
    let mut paint_ctx = PaintContext::new(renderer);
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

#[test]
fn topbar_medium_selector_opens_menu() {
    let mut root = build_root();
    let app = AppContext::default();
    let _ = render(&mut root, &app);
    let cmds = render(&mut root, &app);
    // The topbar's compact medium selector uses the "computer" glyph; it is the
    // top-most one (the composer harness pill uses the same glyph lower down).
    let pos = icon_center(&cmds, "agentmode").expect("topbar medium selector icon");
    click(&mut root, &app, pos);

    let open = root.state_rc().borrow().env_selector_open.clone();
    assert!(*open.borrow(), "topbar medium selector should open its menu");
}

#[test]
fn agent_header_3_dots_opens_menu() {
    let mut root = build_root();
    let app = AppContext::default();
    // Two render passes: first establishes origins/sizes, second is the frame
    // that actually receives the click.
    let _ = render(&mut root, &app);
    let cmds = render(&mut root, &app);
    let pos = icon_center(&cmds, "dots-horizontal").expect("agent 3-dots icon");
    click(&mut root, &app, pos);

    let open = root.state_rc().borrow().agent_header_menu_open.clone();
    assert!(*open.borrow(), "3-dots menu should open after clicking it");
}

/// Mount a `ChatComposer` that carries a `PopupMenu` pill, click the pill's
/// trigger icon, and assert the app-owned open flag flips.
///
/// This is the rich-input regression: the model, directory, branch, profile and
/// harness pills all share the same dispatch path (`ChatComposer` → `self.root`
/// → `Padding`), so proving each one opens proves the `Padding`/`PopupMenu`
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

#[test]
fn composer_profile_pill_opens_menu() {
    let open = Rc::new(RefCell::new(false));
    let open_menu = open.clone();
    let composer = ChatComposer::new()
        .with_profile_menu(
            vec![
                PopupMenuItem::new("Settings"),
                PopupMenuItem::new("Log out"),
            ],
            open_menu,
            |_| {},
        )
        .finish();
    assert_composer_pill_opens(composer, "user", open);
}
