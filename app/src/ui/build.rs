use goble_ui::elements::{
    AppContext, Container, CrossAxisAlignment, Divider, Element, Empty, Expanded, Fill, Flex,
    LayoutContext, MainAxisSize, PaintContext, Point, SizeConstraint, Stack,
};
use goble_ui::geometry::{PointF, RectF, Size2F, Vector2F};
use goble_ui::theme::ColorToken;
use goble_ui::{vec2f, ColorU, Dialog, Sheet, DIALOG_DEFAULT_WIDTH, SHEET_DEFAULT_WIDTH};

use super::actions::{AiActions, MediaActions, ProjectsActions, UiActions};
use super::snapshot::{AiSnapshot, MediaSnapshot, ProjectsSnapshot, UiSnapshot};
use super::types::AppTab;
use super::{
    agent_dashboard, chat, connectors, crons, hosts, inbox, media, model_form, onboarding, settings,
    shell, shortcuts_help, sidebar, vault, CONNECTORS_WIDTH,
};

/// The thickness of the rule that wraps the whole window, warp-new's window
/// border (`app/src/root_view.rs:117-131`).
pub const WINDOW_BORDER_WIDTH: f32 = 1.0;

/// The radius of the window's own corners while it is windowed. macOS rounds
/// the corners of a standard window and clips the content to that shape, so the
/// rule follows them instead of being cut off at each corner; a full-screen
/// window has square corners, and so does every other platform's.
#[cfg(target_os = "macos")]
const WINDOW_CORNER_RADIUS: f32 = 10.0;
#[cfg(not(target_os = "macos"))]
const WINDOW_CORNER_RADIUS: f32 = 0.0;

/// The radius the window's rule is drawn at: [`WINDOW_CORNER_RADIUS`] while the
/// window is windowed, square once it is full screen (macOS hides the rounded
/// frame then, and the window reaches the screen's own square corners).
pub fn window_corner_radius(fullscreen: bool) -> f32 {
    if fullscreen {
        0.0
    } else {
        WINDOW_CORNER_RADIUS
    }
}

/// The snapshots and callbacks one frame is built from.
pub struct BuildContext<'a> {
    pub app: &'a AppContext,
    pub state: &'a UiSnapshot,
    pub actions: &'a UiActions,
    pub ai: &'a AiSnapshot,
    pub ai_actions: &'a AiActions,
    pub projects: &'a ProjectsSnapshot,
    pub projects_actions: &'a ProjectsActions,
    pub media: &'a MediaSnapshot,
    pub media_actions: &'a MediaActions,
}

/// Build the complete app UI: topbar, sidebar, main content, and the
/// auxiliary sheets (crons, connectors, vault) stacked on top.
pub fn build_ui(context: &BuildContext<'_>) -> Box<dyn Element> {
    let BuildContext {
        app,
        state,
        actions,
        ai,
        ai_actions,
        projects: _,
        projects_actions: _,
        media,
        media_actions,
    } = *context;
    let topbar = shell::build_topbar(app, state, actions, media, media_actions);
    let sidebar = sidebar::build_sidebar(app, state, actions, &media.selected_medium);
    // The agent-management dashboard takes the panes' place beside the tools
    // sidebar: the sidebar and the toolbar stay around it, and the panel gets
    // the whole pane area.
    let main: Box<dyn Element> = if state.agent_dashboard.open {
        agent_dashboard::build_agent_dashboard(app, state, actions)
    } else {
        shell::build_main(context)
    };
    let body: Box<dyn Element> = if state.sidebar_visible {
        let on_drag_start = actions.on_sidebar_drag_start.clone();
        let on_drag_move = actions.on_sidebar_drag_move.clone();
        let on_drag_end = actions.on_sidebar_drag_end.clone();
        shell::SidebarLayout::new(sidebar, main, state.sidebar_width)
            .with_dragging(state.sidebar_dragging)
            .with_on_drag_start(move |x| (on_drag_start.borrow_mut())(x))
            .with_on_drag_move(move |x| (on_drag_move.borrow_mut())(x))
            .with_on_drag_end(move || (on_drag_end.borrow_mut())())
            .finish()
    } else {
        // With the sidebar retracted the main area (and its resizable divider)
        // is gone, so the chat/terminal column fills the whole window.
        drop(sidebar);
        main
    };

    let mut shell_col = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    // The toolbar is layered *above* the shell body (see the stack below) so the
    // trays it opens — the "+ ▾" environment menu above all — paint over the
    // sidebar and the pane surface instead of being covered by them. This
    // placeholder reserves the toolbar's height in the body column so the
    // content still starts below it.
    shell_col = shell_col.with_child(
        Empty::new()
            .with_size(vec2f(0.0, shell::TOPBAR_HEIGHT))
            .finish(),
    );
    // Separator below the topbar, like the sidebar separators. The topbar is
    // a fixed height and is not resizable, so this is purely a visual rule.
    shell_col = shell_col.with_child(Divider::horizontal().finish());
    // First-run: a compact getting-started hint on the chat workspace, shown
    // after the workspace choice is made, until dismissed.
    if state.current_tab == AppTab::Chat && state.show_onboarding_tip {
        shell_col = shell_col.with_child(chat::build_onboarding_tip(app, actions));
        shell_col = shell_col.with_child(Divider::horizontal().finish());
    }
    // The body must consume only the *remaining* height below the topbar, so
    // wrap it in `Expanded`; otherwise the chat composer would be pushed past
    // the bottom edge of the window.
    shell_col = shell_col.with_child(Expanded::new(body).finish());

    let on_close_crons = actions.on_close_crons.clone();
    let crons_sheet = Sheet::new(crons::build_crons_drawer(app, state, actions))
        .with_expanded(state.crons_open)
        .with_width(SHEET_DEFAULT_WIDTH)
        .with_on_close(move || (on_close_crons.borrow_mut())())
        .finish();

    // The global inbox: approvals and questions from every pane, plus the
    // terminal reports of turns and sub-agents. It is a popover hung from the
    // topbar's inbox control (see [`inbox::build_inbox_popover`]) rather than a
    // dialog: nothing is dimmed, the workspace underneath stays live, and a
    // press anywhere else closes it.
    let inbox_popover = inbox::build_inbox_popover(app, state, actions);

    let on_close_connectors = ai_actions.on_close_connectors.clone();
    let connectors_sheet = Sheet::new(connectors::build_connectors_sheet(app, ai, ai_actions))
        .with_expanded(ai.connectors_open)
        .with_width(CONNECTORS_WIDTH)
        .with_on_close(move || (on_close_connectors.borrow_mut())())
        .finish();

    let on_close_vault = ai_actions.on_close_vault.clone();
    let vault_sheet = Sheet::new(vault::build_vault_sheet(app, ai, ai_actions))
        .with_expanded(ai.vault_open)
        .with_width(SHEET_DEFAULT_WIDTH)
        .with_on_close(move || (on_close_vault.borrow_mut())())
        .finish();

    // The agent panel (routines + scheduled tasks) floats over the workspace
    // as a right-anchored sheet, with an explicit X close button plus the
    // standard backdrop-click-to-close. `right_sidebar_open` is its open flag.
    let on_close_chat_panel = actions.on_toggle_right_sidebar.clone();
    let chat_panel_sheet = Sheet::new(chat::build_chat_panel_overlay(app, state, actions))
        .with_expanded(state.right_sidebar_open)
        .with_width(SHEET_DEFAULT_WIDTH)
        .with_on_close(move || (on_close_chat_panel.borrow_mut())())
        .finish();

    // Settings is not a sheet: it is a tab — a space whose root is a settings
    // leaf — so its surface is drawn by the pane tree with the other tabs, and
    // it is closed by its tab's ✕ (see `UiState::open_settings_tab`).

    // The keyboard shortcuts panel: a centered sheet over the workspace, the
    // shape grok-build's cheatsheet has. Ctrl+. toggles it; it is stacked last
    // (see the stack's children) so it is the top-most overlay while open.
    let on_close_shortcuts = actions.on_close_shortcuts_help.clone();
    let shortcuts_help_dialog =
        Dialog::new(shortcuts_help::build_shortcuts_help(app, state, actions))
            .with_open(state.shortcuts_help_open)
            .with_width(shortcuts_help::SHORTCUTS_PANEL_WIDTH)
            .with_on_close(move || (on_close_shortcuts.borrow_mut())())
            .finish();

    // Cmd+Shift+N's saved-host list, and the offer shown when a saved host's
    // session ends. Both are dialogs over the workspace; the picker is modal
    // while it is up, the reconnect offer only takes the click it is given.
    let host_picker_dialog = hosts::build_host_picker(app, state, actions);
    let ssh_reconnect_dialog = hosts::build_reconnect_prompt(app, state, actions);
    // Closing a file pane with unsaved edits asks here, above the workspace.
    let close_prompt_dialog = super::file_view::build_close_prompt(app, state, actions);

    let mut stack = Stack::new().with_children(vec![
        shell_col.finish(),
        // The toolbar paints after the body so the popups/trays anchored in it
        // (the "+ ▾" environment menu) sit above the sidebar and the pane
        // surface, and it is dispatched first within the shell for the same
        // reason. Sheets and dialogs below stay above the toolbar.
        topbar,
        crons_sheet,
        connectors_sheet,
        vault_sheet,
        chat_panel_sheet,
        host_picker_dialog,
        ssh_reconnect_dialog,
        close_prompt_dialog,
        // The shortcuts panel goes last: Ctrl+. is a global chord, so wherever
        // the keyboard is, the panel it opens is the top-most surface and the
        // first to see Escape and a backdrop click.
        shortcuts_help_dialog,
    ]);

    // A finished turn is announced here, at the window's top layer, so the
    // banner reads the same over the workspace, over settings and over every
    // other page, and shifts nothing it is drawn above.
    if let Some(banner) = inbox::build_announcement(app, state, actions) {
        stack = stack.with_overlay(banner, vec2f(0.0, 0.0));
    }
    // The inbox panel is anchored, so it is placed by its own element against
    // the rect the topbar's control recorded. It is the last of the two, so it
    // takes the keyboard before the banner does.
    if let Some(popover) = inbox_popover {
        stack = stack.with_overlay(popover, vec2f(0.0, 0.0));
    }

    // First-run onboarding overlays: the local/remote workspace choice. The
    // missing-API-key error is no longer a dialog — it renders inline at the
    // top of the chat area (see `ChatView::with_notice`), so it stays next to
    // the conversation it blocks.
    let on_dismiss_workspace = actions.on_dismiss_workspace_choice.clone();
    if state.show_workspace_choice && !state.show_llm_key_banner {
        stack = stack.with_overlay(
            Dialog::new(chat::build_workspace_choice(app, actions))
                .with_open(true)
                .with_width(DIALOG_DEFAULT_WIDTH)
                .with_on_close(move || (on_dismiss_workspace.borrow_mut())())
                .finish(),
            vec2f(0.0, 0.0),
        );
    }

    // Settings' default-model menu hangs from the field that opens it. The
    // settings page scrolls inside a viewport, so the menu is drawn here, at
    // the window's top layer, where neither the viewport nor the rows below it
    // can clip or cover it.
    if let Some((menu, offset)) = settings::build_model_menu_overlay(app, state, actions) {
        stack = stack.with_overlay(menu, offset);
    }

    // The model-provider dialog floats above everything (including the sheets).
    stack = stack.with_overlay(
        model_form::build_llm_dialog(app, state, actions),
        vec2f(0.0, 0.0),
    );    // "Add a new environment" dialog, opened from the topbar "+" menu. It only
    // paints/intercepts events while open (see [`super::media::build_add_medium_dialog`]).
    stack = stack.with_overlay(
        media::build_add_medium_dialog(app, state, media_actions, actions),
        vec2f(0.0, 0.0),
    );
    // First-run onboarding is the top-most surface: it fills the window, so
    // nothing behind it takes a click or a key until the flow is finished.
    if state.onboarding_open {
        stack = stack.with_overlay(
            Dialog::new(onboarding::build_onboarding(app, state, actions))
                .with_open(true)
                .with_inset(0.0)
                .finish(),
            vec2f(0.0, 0.0),
        );
    }
    // The window's own edge: a 1 px rule around the whole window, drawn above
    // every page. It is mounted last so it is the topmost thing painted — and
    // the first offered an event, which it never takes (see [`WindowBorder`]).
    let fullscreen = app
        .window_control
        .window_fullscreen()
        .unwrap_or(state.fullscreen);
    stack = stack.with_overlay(build_window_border(app, fullscreen), vec2f(0.0, 0.0));

    Container::new(stack.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish()
}

/// The rule around the window's edge, in the theme's outline colour.
fn build_window_border(app: &AppContext, fullscreen: bool) -> Box<dyn Element> {
    WindowBorder {
        color: app.theme.color(ColorToken::Border),
        corner_radius: window_corner_radius(fullscreen),
        size: None,
        origin: None,
    }
    .finish()
}

/// The 1 px rule wrapping the whole window, warp-new's `unthemed_window_border`
/// (`app/src/root_view.rs:117-131`) and the themed stroke over the same edge
/// (`app/src/workspace/view.rs:1921`).
///
/// It is drawn at the window's outer edge on every page — the workspace,
/// settings, the dashboards, first-run onboarding — because it is the topmost
/// overlay of [`build_ui`]'s stack. It is a decoration, not a surface: it takes
/// up no layout room, and it answers no event at all (the trait's default
/// `dispatch_event` is `false`), so every press, move, scroll and key passes
/// through to whatever is drawn beneath it, the 1 px band included.
///
/// `Renderer::stroke_rect` strokes *inside* the rectangle it is given (its
/// shader keeps the band between the edge and `width` inside it), so the rule
/// is drawn on the window's own rect and fills its outermost pixel: the line is
/// wholly visible inside the window, with nothing of it off screen.
struct WindowBorder {
    color: ColorU,
    corner_radius: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

/// One side of the window: the constraint's own, or nothing when it is
/// unbounded — an infinite rule would be no rule at all.
fn window_extent(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

impl Element for WindowBorder {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        // The stack hands the root's constraint down, and that is the window.
        let size = vec2f(
            window_extent(constraint.max.x),
            window_extent(constraint.max.y),
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.stroke_rect(
                RectF::new(PointF::new(origin.x, origin.y), Size2F::new(size.x, size.y)),
                self.color,
                WINDOW_BORDER_WIDTH,
                self.corner_radius,
            );
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::{Element, EventContext};
    use goble_ui::event::{DispatchedEvent, ModifiersState};
    use goble_ui::geometry::{rectf, RectF};
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;

    use crate::root_view::RootView;
    use crate::state::UiState;

    fn window() -> Vector2F {
        vec2f(1024.0, 768.0)
    }

    /// The whole app over a real store, with the first-run overlays out of the
    /// way — the tree [`build_ui`] is asked for every frame.
    fn app_root() -> (Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let app = AppContext::default();
        let view = RootView::new(&app, &desktop, None);
        let state = view.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.show_onboarding_tip = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
        }
        (Box::new(view), state, dir)
    }

    /// Every rule a frame draws in the theme's outline colour, in draw order.
    fn rules(commands: &[RenderCommand], app: &AppContext) -> Vec<(RectF, f32, f32)> {
        let outline = app.theme.color(ColorToken::Border);
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::StrokeRect {
                    rect,
                    color,
                    width,
                    corner_radius,
                } if *color == outline => Some((*rect, *width, *corner_radius)),
                _ => None,
            })
            .collect()
    }

    /// The rule that spans the window, when the frame drew one.
    fn window_rule(commands: &[RenderCommand], app: &AppContext) -> Option<(RectF, f32, f32)> {
        let window = window();
        let spanning: Vec<_> = rules(commands, app)
            .into_iter()
            .filter(|(rect, _, _)| rect.width() == window.x && rect.height() == window.y)
            .collect();
        assert!(
            spanning.len() <= 1,
            "the window wears one rule, not a stack of them: {spanning:?}"
        );
        spanning.first().copied()
    }

    /// The window wears one 1 px rule around all of it, in the theme's outline
    /// colour, drawn on the window's own rect: `stroke_rect` strokes inside its
    /// rect, so the line fills the window's outermost pixel and nothing of it
    /// hangs off screen.
    #[test]
    fn the_window_wears_one_rule_around_all_of_it() {
        let app = AppContext::default();
        let (mut root, _state, _dir) = app_root();
        let commands = render_element(&mut root, window(), &app);

        let (rect, width, radius) = window_rule(&commands, &app).expect("the window's rule");
        assert_eq!(
            rect,
            rectf(0.0, 0.0, window().x, window().y),
            "the rule covers the whole window"
        );
        assert_eq!(width, WINDOW_BORDER_WIDTH, "it is one pixel thick");
        assert_eq!(
            radius,
            window_corner_radius(false),
            "a windowed window's rule follows the window's own corners"
        );

        // Painted last of everything: it reads over the bar, the panes, the
        // sheets and the dialogs alike.
        assert!(
            matches!(
                commands.last(),
                Some(RenderCommand::StrokeRect { rect: drawn, width: drawn_width, .. })
                    if *drawn == rect && *drawn_width == WINDOW_BORDER_WIDTH
            ),
            "the rule is the frame's last paint: {:?}",
            commands.last()
        );
    }

    /// The rule is the window's, not the workspace's: settings wears it too,
    /// and so does the first-run overlay that fills the window.
    #[test]
    fn every_full_window_page_wears_the_rule() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();

        state.borrow_mut().open_settings_tab(None);
        let commands = render_element(&mut root, window(), &app);
        assert!(
            window_rule(&commands, &app).is_some(),
            "the settings page wears the window's rule"
        );

        let (mut first_run, state, _dir) = app_root();
        state.borrow_mut().show_workspace_choice = true;
        let commands = render_element(&mut first_run, window(), &app);
        assert!(
            window_rule(&commands, &app).is_some(),
            "the first-run overlay wears the window's rule as well"
        );
    }

    /// A full-screen window has square corners, so the rule squares up with it.
    #[test]
    fn the_rule_is_square_in_full_screen() {
        let app = AppContext::default();
        app.window_control.note_window_fullscreen(true);
        let (mut root, _state, _dir) = app_root();
        let commands = render_element(&mut root, window(), &app);

        let (_, width, radius) = window_rule(&commands, &app).expect("the window's rule");
        assert_eq!(width, WINDOW_BORDER_WIDTH);
        assert_eq!(radius, 0.0, "a full-screen window has square corners");
    }

    /// The rule is a decoration: it takes the whole window and no event at all,
    /// so a press, move, scroll or key lands on whatever is drawn under it.
    #[test]
    fn the_rule_takes_no_room_and_no_event() {
        let app = AppContext::default();
        let mut rule = build_window_border(&app, false);
        let size = rule.layout(
            SizeConstraint::loose(window()),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size, window(), "the rule is the window's own size");

        // Points on the 1 px band itself, and points well inside it.
        let points = [
            vec2f(0.5, 0.5),
            vec2f(window().x - 0.5, window().y - 0.5),
            vec2f(window().x / 2.0, 0.5),
            vec2f(0.5, window().y / 2.0),
        ];
        let mut ctx = EventContext;
        let mut events: Vec<DispatchedEvent> = Vec::new();
        for position in points {
            events.push(DispatchedEvent::MouseDown {
                position,
                button: 0,
            });
            events.push(DispatchedEvent::MouseUp {
                position,
                button: 0,
            });
            events.push(DispatchedEvent::MouseMove { position });
        }
        events.push(DispatchedEvent::Scroll {
            delta: vec2f(0.0, -12.0),
            precise: false,
        });
        events.push(DispatchedEvent::KeyDown {
            key: "Escape".to_string(),
            modifiers: ModifiersState::none(),
        });
        for event in &events {
            assert!(
                !rule.dispatch_event(event, &mut ctx, &app),
                "the rule swallows nothing: {event:?}"
            );
        }
    }

    /// The rule is mounted above everything, so a press goes through it to the
    /// control it covers: the Settings icon still opens the settings tab.
    #[test]
    fn a_press_through_the_rule_still_reaches_the_control() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let commands = render_element(&mut root, window(), &app);
        let settings = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    name, origin, size, ..
                } if name == "settings" => {
                    Some(vec2f(origin.x + size / 2.0, origin.y + size / 2.0))
                }
                _ => None,
            })
            .expect("the settings control");

        let mut ctx = EventContext;
        for event in [
            DispatchedEvent::MouseDown {
                position: settings,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: settings,
                button: 0,
            },
        ] {
            root.dispatch_event(&event, &mut ctx, &app);
        }
        assert!(
            state.borrow().settings_pane().is_some(),
            "the press reached the Settings control under the rule"
        );
    }
}
