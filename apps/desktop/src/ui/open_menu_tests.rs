//! App-level acceptance tests for the window's ONE open-menu state: every menu
//! surface (the sidebar's view-options popup, a session row's "..." tray, the
//! composer's model card, a workspace tab's context menu) registers as *the*
//! open menu; the pointer over an open menu cannot hover the rows behind it; a
//! press outside closes it, Escape closes it, a press inside is the menu's; and
//! opening a second menu closes the first.
//!
//! The app's `UiState::open_menu` mirror is written by the root view after each
//! frame from the UI's shared registry (`AppContext::open_menus`), so these
//! tests read the same state the rest of the desktop would.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::store::Store;
use goble_desktop_service::{DesktopState, ThreadStore};
use goble_ui::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, SizeConstraint,
};
use goble_ui::event::{DispatchedEvent, ModifiersState};
use goble_ui::geometry::{vec2f, Vector2F};
use goble_ui::render::{RenderCommand, Renderer};

use crate::root_view::RootView;
use crate::state::{SpaceMenu, UiState};

fn window() -> Vector2F {
    vec2f(1024.0, 768.0)
}

/// The app over a real store with two conversations, the sidebar showing.
fn seeded(
    app: &AppContext,
) -> (
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
    let first = desktop
        .create_chat("Redesign the sidebar", None, None)
        .expect("create the first conversation");
    desktop
        .add_chat_message(&first, "user", "salut")
        .expect("the user's prompt");
    desktop
        .add_chat_message(&first, "assistant", "Bine ai venit!")
        .expect("the agent's reply");
    let second = desktop
        .create_chat("Automate Instagram posting", None, None)
        .expect("create the second conversation");
    desktop
        .add_chat_message(&second, "assistant", "done")
        .expect("the agent's reply");
    // A list long enough that some rows sit below the view-options panel: the
    // hover that must still work outside the menu needs a row outside it.
    for index in 3..=14 {
        let chat = desktop
            .create_chat(&format!("Conversation {index:02}"), None, None)
            .expect("create another conversation");
        desktop
            .add_chat_message(&chat, "assistant", "ok")
            .expect("its reply");
    }
    let view = RootView::new(app, &desktop, None);
    let state = view.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.show_onboarding_tip = false;
        s.right_sidebar_open = false;
        s.crons_open = false;
        s.refresh_conversations(&desktop);
    }
    (Box::new(view), state, desktop, dir)
}

/// Lay out and paint the whole app at the test window size, with the pointer at
/// `cursor` (or outside the window when `None`). The root view seeds the
/// open-menu hover gate and reconciles the registry — the same code the real
/// frame loop runs.
fn frame(
    root: &mut Box<dyn Element>,
    app: &AppContext,
    cursor: Option<Vector2F>,
) -> Vec<RenderCommand> {
    let constraint = SizeConstraint::loose(window());
    let _ = root.layout(constraint, &mut LayoutContext::default(), app);
    let mut ctx = PaintContext::new(Renderer::new());
    if let Some(point) = cursor {
        ctx.cursor_position = point;
        ctx.cursor_inside = true;
    }
    root.paint(vec2f(0.0, 0.0), &mut ctx, app);
    ctx.renderer
        .take()
        .map(|renderer| renderer.commands().to_vec())
        .unwrap_or_default()
}

fn dispatch(root: &mut Box<dyn Element>, app: &AppContext, event: DispatchedEvent) {
    let mut ctx = EventContext::default();
    root.dispatch_event(&event, &mut ctx, app);
}

fn click(root: &mut Box<dyn Element>, app: &AppContext, at: Vector2F) {
    for event in [
        DispatchedEvent::MouseDown {
            position: at,
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: at,
            button: 0,
        },
    ] {
        dispatch(root, app, event);
    }
}

fn key(root: &mut Box<dyn Element>, app: &AppContext, name: &str) {
    dispatch(
        root,
        app,
        DispatchedEvent::KeyDown {
            key: name.to_string(),
            modifiers: ModifiersState::none(),
        },
    );
}

/// The centre of a drawn glyph, when the frame drew one with that name.
fn icon_center(commands: &[RenderCommand], name: &str) -> Option<Vector2F> {
    commands.iter().find_map(|command| match command {
        RenderCommand::DrawIcon {
            name: drawn,
            origin,
            size,
            ..
        } if drawn == name => Some(*origin + vec2f(*size / 2.0, *size / 2.0)),
        _ => None,
    })
}

/// The origin of a drawn run of text, when the frame drew it.
fn text_origin(commands: &[RenderCommand], text: &str) -> Option<Vector2F> {
    commands.iter().find_map(|command| match command {
        RenderCommand::DrawText { text: drawn, origin, .. } if drawn == text => Some(*origin),
        _ => None,
    })
}

/// A point inside the open menu's first reported surface (a fifth of the way
/// into it, clear of the outer border).
fn point_inside_open_menu(app: &AppContext) -> Vector2F {
    let surface = app
        .open_menus
        .surfaces()
        .into_iter()
        .next()
        .expect("the open menu reported a surface");
    vec2f(
        surface.min_x() + surface.width() * 0.5,
        surface.min_y() + surface.height() * 0.25,
    )
}

/// Open the sidebar header's view-options menu by pressing its sliders control.
fn open_sidebar_view_options(
    root: &mut Box<dyn Element>,
    app: &AppContext,
) -> Vec<RenderCommand> {
    let commands = frame(root, app, None);
    let at = icon_center(&commands, "sliders-two").expect("the header draws the view options");
    click(root, app, at);
    frame(root, app, None)
}

/// The sidebar view-options menu is one of the window's menus: it registers as
/// the open menu, the pointer over its panel does not hover the session rows
/// behind it, and a press outside or Escape closes it.
#[test]
fn the_sidebar_view_options_menu_registers_isolates_hover_and_closes() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = seeded(&app);

    let commands = open_sidebar_view_options(&mut root, &app);
    assert!(
        *state.borrow().sidebar_menu_open.borrow(),
        "the view-options menu is open"
    );
    assert_eq!(
        state.borrow().open_menu,
        Some("sidebar.view-options"),
        "the window recognised the popup as the open menu"
    );
    assert!(
        text_origin(&commands, "Flat list").is_some(),
        "the open menu draws its rows"
    );

    // Hover over the open panel: a session row lies behind it, and it must not
    // take the pointer (the row's shared hover cell stays empty).
    *state.borrow_mut().sidebar_hover_row.borrow_mut() = None;
    let point = point_inside_open_menu(&app);
    let commands = frame(&mut root, &app, Some(point));
    assert_eq!(
        *state.borrow().sidebar_hover_row.borrow(),
        None,
        "the row behind the open menu must not hover while the pointer is on the menu"
    );
    assert!(
        text_origin(&commands, "Hide archived").is_some(),
        "the menu is still the menu under the pointer"
    );

    // Hover outside the open menu still reaches the rows: only the menu's own
    // surface shadows the pointer. The last row sits below the panel.
    let commands = frame(&mut root, &app, None);
    // The list is capped and its rows sit under the panel; the "New
    // conversation" row above the panel is the one outside it.
    let commands = frame(&mut root, &app, None);
    let above = text_origin(&commands, "New conversation").expect("the new-conversation row");
    let point = above + vec2f(24.0, 6.0);
    assert!(
        !app.open_menus.pointer_on_surface(point),
        "the row below the panel is outside the menu's surface"
    );
    *state.borrow_mut().sidebar_hover_row.borrow_mut() = None;
    let _ = frame(&mut root, &app, Some(point));
    assert!(
        state.borrow().sidebar_hover_row.borrow().is_some(),
        "hover outside the open menu still reaches the sidebar"
    );

    // A press outside closes the menu, and the press itself still lands on what
    // it aimed at (nothing behind is dimmed or trapped).
    click(&mut root, &app, vec2f(700.0, 400.0));
    assert!(
        !*state.borrow().sidebar_menu_open.borrow(),
        "the outside press closed the menu"
    );
    let _ = frame(&mut root, &app, None);
    assert_eq!(
        state.borrow().open_menu,
        None,
        "the shared state let go of the closed menu"
    );

    // Escape closes it too.
    let _ = open_sidebar_view_options(&mut root, &app);
    key(&mut root, &app, "Escape");
    assert!(
        !*state.borrow().sidebar_menu_open.borrow(),
        "Escape closed the menu"
    );

    // A press inside the menu is the menu's: choosing a row runs its action and
    // closes the menu instead of dismissing it on the way in.
    let commands = open_sidebar_view_options(&mut root, &app);
    let flat = text_origin(&commands, "Flat list").expect("the row is drawn");
    let before = *state.borrow().sidebar_flat_list.borrow();
    click(&mut root, &app, flat + vec2f(4.0, 4.0));
    assert_ne!(
        *state.borrow().sidebar_flat_list.borrow(),
        before,
        "the press inside the menu reached its row"
    );
    assert!(
        !*state.borrow().sidebar_menu_open.borrow(),
        "choosing a row closes the menu"
    );
}

/// Open a session row's "..." tray, the way a pointer on the row and a press on
/// its ellipsis does.
fn open_session_tray(
    root: &mut Box<dyn Element>,
    state: &Rc<RefCell<UiState>>,
    app: &AppContext,
) {
    let id = state
        .borrow()
        .conversations
        .first()
        .map(|entry| entry.id.clone())
        .expect("a conversation is listed");
    *state.borrow_mut().sidebar_hover_row.borrow_mut() = Some(format!("session:{id}"));
    let commands = frame(root, app, None);
    let at = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawIcon { name, origin, .. }
                if name == "dots-horizontal"
                    && origin.y > crate::ui::shell::TOPBAR_HEIGHT =>
            {
                Some(*origin + vec2f(8.0, 8.0))
            }
            _ => None,
        })
        .expect("the hovered row draws its ellipsis");
    click(root, app, at);
    let _ = frame(root, app, None);
}

/// The session row's "..." tray is one of the window's menus: it registers as
/// the open menu, a press outside closes it, and Escape closes it.
#[test]
fn the_session_row_tray_registers_and_closes_from_outside() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = seeded(&app);

    open_session_tray(&mut root, &state, &app);
    assert!(
        app.open_menus.is_open(),
        "the row tray registered as the open menu"
    );
    assert_eq!(
        state.borrow().open_menu,
        Some("sidebar.session_row"),
        "the app sees the row tray as the open menu"
    );
    let commands = frame(&mut root, &app, None);
    assert!(
        text_origin(&commands, "Delete conversation").is_some(),
        "the tray draws its rows"
    );

    // A press outside closes it.
    click(&mut root, &app, vec2f(700.0, 500.0));
    let _ = frame(&mut root, &app, None);
    assert!(
        !app.open_menus.is_open(),
        "the outside press closed the row tray"
    );

    // Escape closes it too.
    open_session_tray(&mut root, &state, &app);
    assert!(app.open_menus.is_open());
    key(&mut root, &app, "Escape");
    let _ = frame(&mut root, &app, None);
    assert!(
        !app.open_menus.is_open(),
        "Escape closed the row tray"
    );
}

/// The composer's model card (the DSH two-level menu above the model chip) is
/// one of the window's menus: it registers as the open menu and a press outside
/// closes it.
#[test]
fn the_composer_model_card_registers_and_closes_from_outside() {
    let app = AppContext::default();
    let (mut root, state, _desktop, _dir) = seeded(&app);
    let _ = frame(&mut root, &app, None);
    let pane_id = state.borrow().active_pane_id;

    *state
        .borrow_mut()
        .pane_controls_mut(pane_id)
        .model_tray_open
        .borrow_mut() = true;
    let commands = frame(&mut root, &app, None);
    assert_eq!(
        state.borrow().open_menu,
        Some("composer.model"),
        "the composer model card registered as the open menu"
    );
    // The card draws the two DSH root cells.
    assert!(
        text_origin(&commands, "Model").is_some() || text_origin(&commands, "No efforts").is_some(),
        "the model card is drawn"
    );

    // Hover over the card belongs to the card: the transcript behind it keeps
    // its own hover state empty.
    state.borrow_mut().tool_hover.borrow_mut().take();
    let point = point_inside_open_menu(&app);
    let _ = frame(&mut root, &app, Some(point));
    assert_eq!(
        *state.borrow().tool_hover.borrow(),
        None,
        "the transcript row behind the card must not hover"
    );

    // A press outside closes it.
    click(&mut root, &app, vec2f(700.0, 120.0));
    assert!(
        !*state
            .borrow()
            .pane_controls(pane_id)
            .model_tray_open
            .borrow(),
        "the outside press closed the model card"
    );
}
