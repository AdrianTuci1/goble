use goble_ui::elements::{
    AppContext, Border, ConstrainedBox, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, Expanded, Fill, Flex, LayoutContext, MainAxisSize, Padding, PaintContext, Point,
    SizeConstraint, Stack,
};
use goble_ui::geometry::{rectf, PointF, RectF, Size2F, Vector2F};
use goble_ui::platform::{CursorKind, WindowControl};
use goble_ui::theme::ColorToken;
use goble_ui::{vec2f, ColorU, Dialog, Sheet, DIALOG_DEFAULT_WIDTH, SHEET_DEFAULT_WIDTH};

use super::actions::{AiActions, MediaActions, ProjectsActions, UiActions};
use super::snapshot::{AiSnapshot, MediaSnapshot, ProjectsSnapshot, UiSnapshot};
use super::types::AppTab;
use super::{
    agent_dashboard, chat, chrome, connectors, crons, hosts, inbox, media, model_form,
    onboarding, panes, session_menu, settings, shell, shortcuts_help, sidebar, vault,
    CONNECTORS_WIDTH,
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
    let sidebar = sidebar::build_sidebar(app, state, actions, &media.selected_medium);
    // The agent-management dashboard takes the panes' place beside the tools
    // sidebar: the sidebar and the toolbar stay around it, and the panel gets
    // the whole pane area.
    // The responsive layout's second step: below the secondary-panel
    // breakpoint (or when the panes cannot keep their minimum width), the
    // dashboard leaves the view and the active pane stays visible — the
    // DeepSeek Harness rule that the centre column never gives up its minimum.
    let dashboard_shown = state.agent_dashboard.open && !state.responsive.secondary_hidden;
    let main: Box<dyn Element> = if dashboard_shown {
        agent_dashboard::build_agent_dashboard(app, state, actions)
    } else {
        let main = shell::build_main(context);
        if state.current_tab == AppTab::Chat {
            // The panel surface — the whole area the panes and panels are
            // rendered on — is the sidebar's own `Surface`, painted over the
            // pane area's WHOLE rectangle: the [`panes::PANE_AREA_INSET`] frame
            // itself (the 4 px between the chrome and the tab slots), the
            // gutters between and around the tab slots, and every edge. The
            // chrome above (the bar block) keeps the window's `Bg`; inside the
            // panel area only the pane cards are the darker material (see
            // `panes::pane_surface`). The inset must therefore be the GROUND of
            // the pane area painted `Surface`, not a margin left for the
            // window's `Bg` to show through: the padding is inside the surface
            // container, so no dark strip is left anywhere around the panes.
            // The tab slots tile the padded area, so every pane keeps its own
            // gutter geometry relative to its slot.
            let panels = Padding::new(main, EdgeInsets::uniform(panes::PANE_AREA_INSET)).finish();
            Container::new(panels)
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .finish()
        } else {
            // Every other page is not a panel surface: it takes the whole main
            // area, the way it always has.
            main
        }
    };
    // The pane area takes the main column; its slots are inset from the chrome
    // by `panes::PANE_AREA_INSET`, and the surface paints that inset frame too
    // (see above). The content lane is PER PANE: each
    // chat pane's content column is centred inside the pane's own width by the
    // `ContentLane` the view mounts (`ChatView::with_content_lane`), with its
    // per-pane width preference, while the pane's own header spans the full
    // pane. The app topbar and the sidebar stay full width, and the dashboard
    // and the non-chat pages take the whole main area (they are not
    // conversation content, so they are not a panel surface).
    // The responsive layout's first step: below the breakpoint (measured from
    // the pane tree's own minimum plus the sidebar) the sidebar auto-collapses,
    // and the panes take the whole window. A manual re-expand below the
    // breakpoint keeps the sidebar on (a later step then narrows the panes).
    let main: Box<dyn Element> = if state.side_panel_maximized && state.right_sidebar_open {
        chrome::build_side_panel(app, state, actions)
    } else if state.right_sidebar_open {
        let panel = chrome::build_side_panel(app, state, actions);
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Expanded::new(main).finish())
            .with_child(
                ConstrainedBox::new(panel)
                    .with_width(chrome::OVERVIEW_WIDTH)
                    .finish(),
            )
            .finish()
    } else {
        main
    };
    let main = Container::new(main)
        .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
        .with_corner_radius(12.0)
        .finish();
    let sidebar_shown = state.responsive.sidebar_shown(state.sidebar_visible);
    let body: Box<dyn Element> = if sidebar_shown && !state.side_panel_maximized {
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
    // sidebar and the pane surface instead of being covered by them. This first
    // child reserves the toolbar's own height plus the band under it
    // ([`shell::TOPBAR_BOTTOM_BAND`]) and paints that band full width in the
    // window's own `Bg`, so the bar block reads as one dark piece whose bottom
    // edge is the top of the content sheet: the floating pills end above it and
    // the selected tab's fill crosses it into the surface below.
    // The general topbar is retired: the sidebar and the panel own the top
    // edge, so the shell body starts at the window's first pixel.
    // No rule under the toolbar: Chrome has none between its tab strip and
    // the toolbar, and in Goble the two-tone does the separating — the bar and
    // its band are the window frame's `Bg`, and the content sheet's `Surface`
    // begins exactly on the band's bottom edge. The selected workspace tab's
    // fill runs through the band and into that sheet, so a rule here would be
    // the very seam that merge is meant to erase. The body therefore starts at
    // `TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND`, and the panel surface starts there
    // too — its `PANE_AREA_INSET` frame is part of the painted `Surface`, so no
    // dark strip is left under the band. The tab slots' `PANE_INSET` gutter
    // sits below that edge, not inside the band.
    // First-run: a compact getting-started hint on the chat workspace, shown
    // after the workspace choice is made, until dismissed.
    if state.current_tab == AppTab::Chat && state.show_onboarding_tip {
        shell_col = shell_col.with_child(chat::build_onboarding_tip(app, actions));
        shell_col = shell_col.with_child(Divider::horizontal().finish());
    }
    // The body must consume only the *remaining* height below the topbar, so
    // wrap it in `Expanded`; otherwise the chat composer would be pushed past
    // the bottom edge of the window.
    shell_col = shell_col.with_child(chrome::build_chrome_row(app, state, actions));
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
    // The agent panel is a secondary surface: below the responsive breakpoint
    // it leaves the view with the dashboard, so the panes keep their width.
    let chat_panel_sheet = Sheet::new(chat::build_chat_panel_overlay(app, state, actions))
        .with_expanded(state.right_sidebar_open && !state.responsive.secondary_hidden)
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
    let settings_overlay = settings::overlay::build_settings_overlay(app, state, actions);
    let host_picker_dialog = hosts::build_host_picker(app, state, actions);
    let ssh_target_dialog = hosts::build_ssh_target_overlay(app, state, actions);
    let ssh_reconnect_dialog = hosts::build_reconnect_prompt(app, state, actions);
    // Closing a file pane with unsaved edits asks here, above the workspace.
    let close_prompt_dialog = super::file_view::build_close_prompt(app, state, actions);

    let mut stack = Stack::new().with_children(vec![
        shell_col.finish(),
        // The toolbar paints after the body so the popups/trays anchored in it
        // (the "+ ▾" environment menu) sit above the sidebar and the pane
        // surface, and it is dispatched first within the shell for the same
        // reason. Sheets and dialogs below stay above the toolbar.
        crons_sheet,
        connectors_sheet,
        vault_sheet,
        host_picker_dialog,
        ssh_target_dialog,
        ssh_reconnect_dialog,
        close_prompt_dialog,
        // The shortcuts panel goes last: Ctrl+. is a global chord, so wherever
        // the keyboard is, the panel it opens is the top-most surface and the
        // first to see Escape and a backdrop click.
        shortcuts_help_dialog,
        // The session menu's own surfaces stay above the sheets: the Rename
        // dialog and the stop-and-archive confirmation take the keyboard while
        // they are open (each an inert zero-size dialog otherwise).
        session_menu::build_rename_dialog_surface(app, state, actions),
        session_menu::build_archive_confirm_surface(app, state, actions),
        // The Workspaces list's own Rename dialog (workspace rows' "Rename
        // workspace"): same casing, its own copy and callbacks.
        session_menu::build_workspace_rename_dialog_surface(app, state, actions),
        // Settings floats above every workspace surface; it is a sheet, not a
        // pane, so it never takes a tab.
        settings_overlay,
    ]);

    // A finished turn is announced here, at the window's top layer, so the
    // banner reads the same over the workspace, over settings and over every
    // other page, and shifts nothing it is drawn above.
    if let Some(banner) = inbox::build_announcement(app, state, actions) {
        stack = stack.with_overlay(banner, vec2f(0.0, 0.0));
    }
    // The session menu's notice hangs in the same top layer: a copy or archive
    // result reads over the workspace, over settings and over the sheets, and
    // shifts nothing it is drawn above.
    if let Some(toast) = session_menu::build_toast_surface(app, state, actions) {
        stack = stack.with_overlay(toast, vec2f(0.0, 0.0));
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
    // The window's own edge: a neutral 1 px rule around the whole window, drawn
    // above every page. The panel does not outline its tabs — the focused pane
    // wears its accent corner marker instead — so the window edge stays
    // neutral, topbar included. The rule is mounted last so it is the topmost
    // thing painted — and the first offered an event, which it never takes (see
    // [`WindowBorder`]).
    let fullscreen = app
        .window_control
        .window_fullscreen()
        .unwrap_or(state.fullscreen);
    stack = stack.with_overlay(build_window_border(app, fullscreen), vec2f(0.0, 0.0));

    Container::new(stack.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
        .finish()
}

/// The window's own neutral rule: 1 px around the edge, in the theme's
/// `Border`. The accent lives on the tabs (see
/// [`crate::ui::panes::build_slot_outline_layer`]), never on the window edge.
fn build_window_border(app: &AppContext, fullscreen: bool) -> Box<dyn Element> {
    WindowBorder {
        color: app.theme.color(ColorToken::Border),
        corner_radius: window_corner_radius(fullscreen),
        size: None,
        origin: None,
    }
    .finish()
}

/// A zero-height first child of the shell column: it resets the window cursor
/// to the platform default before anything paints, so a resize arrow a content
/// lane's edge asked for on an earlier frame cannot outlive the pointer leaving
/// that lane. A lane owns the cursor only while the pointer is inside it, so a
/// pointer moving from an edge onto a surface with no lane (a terminal pane,
/// the sidebar, the topbar) would otherwise keep the stale arrow. Elements that
/// own a cursor refine it later in the same frame (the lanes, over their
/// strips); `WindowControl::set_cursor` only calls the platform when the value
/// actually changes, so this costs nothing while the arrow is already showing.
struct CursorBaseline {
    window: WindowControl,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

fn build_cursor_baseline(app: &AppContext) -> Box<dyn Element> {
    CursorBaseline {
        window: app.window_control.clone(),
        size: None,
        origin: None,
    }
    .finish()
}

impl Element for CursorBaseline {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(window_extent(constraint.max.x), 0.0);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, _ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.window.set_cursor(CursorKind::Default);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// The full-window-width foot of the topbar: the dark band under the bar's row.
///
/// It reserves [`shell::TOPBAR_HEIGHT`] + [`shell::TOPBAR_BOTTOM_BAND`] of the
/// shell column — the whole bar block — and paints the band's few rows
/// (`TOPBAR_HEIGHT` … `TOPBAR_HEIGHT + TOPBAR_BOTTOM_BAND`) in the window's own
/// `Bg`. There is no lighter lip between the bar and the content: the bar's
/// darker background therefore fills every pixel down to the content sheet, and
/// the sheet's `Surface` (the sidebar, and the panes' full-bleed header band)
/// begins exactly on the band's bottom edge. The band runs the whole window
/// width, under the sidebar as well as beside the panes. The selected workspace
/// tab is painted above it by the topbar: its body is the same `Surface` as the
/// sheet and crosses the band into it, and its inverted corners are cut out of
/// the fill in the band's `Bg`, which is what makes the merge read.
struct TopbarBand {
    color: ColorU,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

fn build_topbar_band(app: &AppContext) -> Box<dyn Element> {
    TopbarBand {
        color: app.theme.color(ColorToken::Bg),
        size: None,
        origin: None,
    }
    .finish()
}

impl Element for TopbarBand {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(
            window_extent(constraint.max.x),
            shell::TOPBAR_HEIGHT + shell::TOPBAR_BOTTOM_BAND,
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let band_top = origin.y + shell::TOPBAR_HEIGHT;
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rect(
                rectf(origin.x, band_top, size.x, shell::TOPBAR_BOTTOM_BAND),
                self.color,
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

/// The 1 px rule wrapping the whole window, warp-new's `unthemed_window_border`
/// (`app/src/root_view.rs:117-131`) and the themed stroke over the same edge
/// (`app/src/workspace/view.rs:1921`), in the theme's neutral `Border`.
///
/// It is drawn at the window's outer edge on every page — the workspace,
/// settings, the dashboards, first-run onboarding — because it is the topmost
/// overlay of [`build_ui`]'s stack. The panel does not wear an accent outline:
/// the selected pane is marked by its accent corner triangle instead, so the
/// window's own edge stays neutral — topbar included.
/// It is a decoration, not a surface: it takes up no layout room, and it answers
/// no event at all (the trait's default `dispatch_event` is `false`), so every
/// press, move, scroll and key passes through to whatever is drawn beneath it,
/// the 1 px band included.
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

/// The shared CPU rasterizer, so the pane-card corner tests can read the
/// rendered pixels: a command list can show the cut and the Border were emitted,
/// but only a rendered frame shows that no square corner pixel survives.
/// Test-only: the app itself never rasterizes a frame.
#[cfg(test)]
#[path = "../../../../crates/goble-ui/examples/composer_preview/raster.rs"]
mod raster;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::Path;
    use std::rc::Rc;
    use std::sync::Arc;

    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::{Element, EventContext};
    use goble_ui::event::{DispatchedEvent, ModifiersState};
    use goble_ui::geometry::{rectf, RectF};
    use goble_ui::platform::CursorKind;
    use goble_ui::render::{RenderCommand, Renderer};
    use goble_ui::test_util::render_element;

    use crate::root_view::RootView;
    use crate::state::{PaneSession, UiState};
    use crate::ui::{Pane, PaneKind, Space, SplitDir};
    use goble_ui::elements::{lane_column, resolve_content_width, LaneEdge};

    fn window() -> Vector2F {
        vec2f(1024.0, 768.0)
    }

    /// The whole app over a real store, with the first-run overlays out of the
    /// way — the tree [`build_ui`] is asked for every frame.
    fn app_root() -> (Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir) {
        app_root_for(&AppContext::default())
    }

    /// The same tree mounted over `app`'s theme, so a light-theme frame can be
    /// rendered (the root rebuilds with the `AppContext` each frame).
    fn app_root_for(
        app: &AppContext,
    ) -> (Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let view = RootView::new(app, &desktop, None);
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

    /// Every rule a frame draws in the window's own outline colour (the
    /// theme's neutral `Border`), in draw order.
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

    /// The window wears one 1 px rule around all of it, in the theme's neutral
    /// `Border`, drawn on the window's own rect: `stroke_rect` strokes inside
    /// its rect, so the line fills the window's outermost pixel and nothing of
    /// it hangs off screen.
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

    /// The bar block's reservation is the bar's own height plus its band, and
    /// every surface below starts on that one line: the sidebar, and the panes'
    /// full-bleed header band bleeding back up to it. The pane card keeps its
    /// `PANE_INSET` gutter below that line — the bar, the band and the body
    /// origin agree with no leftover 3–4 px mismatch.
    #[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
    #[test]
    fn the_bar_block_and_the_body_origin_are_the_same_line() {
        let app = AppContext::default();
        let sheet_top = shell::TOPBAR_HEIGHT + shell::TOPBAR_BOTTOM_BAND;

        // The band element reserves the whole bar block and paints only the
        // band's rows.
        let mut band = build_topbar_band(&app);
        let size = band.layout(
            SizeConstraint::loose(window()),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(
            size,
            vec2f(window().x, sheet_top),
            "the bar block is the bar plus its band"
        );

        let (mut root, state, _dir) = app_root();
        let commands = render_element(&mut root, window(), &app);
        let surface = app.theme.color(ColorToken::Surface);
        let sidebar = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == surface
                        && rect.min_x() == 0.0
                        && rect.height() > window().y / 2.0 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the sidebar paints its surface");
        assert_eq!(
            sidebar.min_y(),
            sheet_top,
            "the content sheet begins on the band's bottom edge: {sidebar:?}"
        );

        // The panes' header bands start on the panel surface's top edge, which
        // is the body origin plus the panel inset; the pane cards keep their own
        // gutter below it.
        let frames = pane_frames(&state);
        assert!(!frames.is_empty(), "the frame draws its panes");
        let inset = crate::ui::panes::PANE_INSET;
        let panel_top = sheet_top + crate::ui::panes::PANE_AREA_INSET;
        for frame in frames {
            assert_eq!(
                frame.min_y(),
                panel_top + inset,
                "the pane card is the panel surface plus its gutter: {frame:?}"
            );
            let header = header_band(&commands, &app, frame).expect("the pane's header band");
            assert_eq!(
                header.min_y(),
                panel_top,
                "the pane's header band starts on the panel surface's top edge: {header:?}"
            );
            assert_eq!(
                header.max_y(),
                frame.min_y() + shell::TOPBAR_HEIGHT,
                "and it is one bar tall against the card: {header:?}"
            );
        }
    }

    /// The panel does not outline its tabs: no accent (and no extra neutral)
    /// rule runs around a pane's slot — only the pane card's own `Border` edge
    /// does. The selected cue is the focused pane's accent corner triangle,
    /// drawn over the panel card at its top-left corner. Solo and in a split.
    #[test]
    fn the_panel_wears_no_tab_outline_only_the_focused_corner() {
        let app = AppContext::default();
        let accent = app.theme.color(ColorToken::Accent);

        // One pane: the window rule is neutral, no accent stroke anywhere, and
        // one accent corner marker on the focused pane's card.
        let (mut root, state, _dir) = app_root();
        let commands = render_element(&mut root, window(), &app);
        let (_, width, _) = window_rule(&commands, &app).expect("the window's neutral rule");
        assert_eq!(width, WINDOW_BORDER_WIDTH);
        assert!(
            !has_accent_window_rule(&commands, &app),
            "no accent rule on the window's own rect"
        );
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, RenderCommand::StrokeRect { color, .. } if *color == accent)),
            "no accent outline stroke anywhere in the panel"
        );
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 1, "one pane: {frames:?}");
        assert_eq!(
            rounded_border_strokes(&commands, &app),
            1,
            "only the pane card's own Border edge frames the pane"
        );
        let card = frames[0];
        let markers = corner_markers(&commands, &app);
        assert_eq!(markers.len(), 1, "one focused corner: {markers:?}");
        assert!(
            (markers[0].x - card.min_x()).abs() < 0.01
                && (markers[0].y - card.min_y()).abs() < 0.01,
            "the accent corner sits on the focused pane's top-left corner: {:?} against {card:?}",
            markers[0]
        );
        // It is painted after the card's fill, so it reads on the darker panel.
        let card_fill = commands
            .iter()
            .position(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *rect == card && *color == app.theme.color(ColorToken::Bg) =>
                {
                    true
                }
                _ => false,
            })
            .expect("the panel card paints its darker background");
        let marker = commands
            .iter()
            .position(|command| matches!(command, RenderCommand::DrawIcon { name, color, .. } if name == "upper-left-triangle" && *color == accent))
            .expect("the accent corner marker");
        assert!(
            marker > card_fill,
            "the panel paints the marker above the card: {marker} against {card_fill}"
        );

        // Two panes: still no outlines; the two cards keep their own Border
        // edges and only the focused (left) pane wears the accent corner.
        let (mut root, state, _dir) = app_root();
        install_two_panes(&state, 0.5);
        let commands = render_element(&mut root, window(), &app);
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, RenderCommand::StrokeRect { color, .. } if *color == accent)),
            "no accent outline stroke around either tab"
        );
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 2, "two panes: {frames:?}");
        assert_eq!(
            rounded_border_strokes(&commands, &app),
            2,
            "one card edge per pane, no outline around the slots"
        );
        let markers = corner_markers(&commands, &app);
        assert_eq!(markers.len(), 1, "only the focused tab is marked: {markers:?}");
        assert!(
            (markers[0].x - frames[0].min_x()).abs() < 0.01
                && (markers[0].y - frames[0].min_y()).abs() < 0.01,
            "the corner sits on the focused (left) pane: {:?} against {:?}",
            markers[0],
            frames[0]
        );
    }

    /// Every pane card is one rounded shape: a rounded `Bg` fill on all four
    /// corners at the pane card radius, and a `Border` edge traced on the same
    /// rounded rect over the content, so the card's own edge rounds with it. One
    /// pane and two, dark and light. The frame is common to every pane kind
    /// (`build_leaf` builds it around whatever the leaf holds), so the kind does
    /// not change the card's shape.
    #[test]
    fn every_pane_card_is_rounded_on_all_four_corners() {
        fn assert_cards_are_rounded(app: &AppContext, two_panes: bool) {
            let (mut root, state, _dir) = app_root_for(app);
            if two_panes {
                install_two_panes(&state, 0.5);
            }
            let commands = render_element(&mut root, window(), app);
            let frames = pane_frames(&state);
            assert_eq!(
                frames.len(),
                if two_panes { 2 } else { 1 },
                "the frame draws every pane: {frames:?}"
            );
            let bg = app.theme.color(ColorToken::Bg);
            let border = app.theme.color(ColorToken::Border);
            for card in &frames {
                let radius = crate::ui::panes::card_radius(vec2f(card.width(), card.height()));
                assert!(radius > 0.0, "the pane card has a corner radius: {radius}");
                // The card's fill: one rounded rect at the card's own rect. The
                // renderer's single `corner_radius` rounds all four corners.
                let fill = commands.iter().find_map(|command| match command {
                    RenderCommand::FillRect {
                        rect,
                        color,
                        corner_radius,
                    } if *rect == *card && *color == bg => Some(*corner_radius),
                    _ => None,
                });
                assert_eq!(
                    fill,
                    Some(radius),
                    "the pane card's fill is rounded on every corner: {card:?}"
                );
            }
            // The card's own Border edge: one rounded rect per card, over the
            // content and the corner cut, so the visible card edge rounds with
            // the card.
            let edges: Vec<f32> = commands
                .iter()
                .filter_map(|command| match command {
                    RenderCommand::StrokeRect {
                        rect,
                        color,
                        width,
                        corner_radius,
                    } if *color == border
                        && (*width - 1.0).abs() < 0.01
                        && frames.contains(rect) =>
                    {
                        Some(*corner_radius)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                edges.len(),
                frames.len(),
                "one rounded Border edge per pane card, over its content"
            );
            for (edge, card) in edges.iter().zip(&frames) {
                let radius = crate::ui::panes::card_radius(vec2f(card.width(), card.height()));
                assert_eq!(
                    *edge, radius,
                    "the card's Border edge rounds at the card radius: {card:?}"
                );
            }
        }

        // One pane, both themes.
        assert_cards_are_rounded(&AppContext::default(), false);
        let mut light = AppContext::default();
        light.theme = goble_ui::theme::Theme::light();
        assert_cards_are_rounded(&light, false);
        // Two panes: each card is its own rounded shape, both themes.
        assert_cards_are_rounded(&AppContext::default(), true);
        assert_cards_are_rounded(&light, true);
    }

    /// The focused pane's accent triangle is clipped to the card's rounded
    /// top-left corner: the card paints the panel-surface band outside its
    /// rounded path after the content and after the marker — the cut — and then
    /// draws the rounded `Border` edge over the cut, so the corner reads as one
    /// rounded card with the coloured corner inside it. Without that order the
    /// marker's square corner would sit in the card's corner spandrel, outside
    /// the radius: the "square bite" the owner reported.
    #[test]
    fn the_focus_triangle_is_clipped_to_the_cards_rounded_corner() {
        let app = AppContext::default();
        let accent = app.theme.color(ColorToken::Accent);
        let surface = app.theme.color(ColorToken::Surface);
        let border = app.theme.color(ColorToken::Border);

        for two_panes in [false, true] {
            let (mut root, state, _dir) = app_root();
            if two_panes {
                install_two_panes(&state, 0.5);
            }
            let commands = render_element(&mut root, window(), &app);
            let frames = pane_frames(&state);
            assert_eq!(frames.len(), if two_panes { 2 } else { 1 }, "{frames:?}");
            let card = frames[0];
            let radius = crate::ui::panes::card_radius(vec2f(card.width(), card.height()));
            let band = crate::ui::panes::CARD_EDGE_BAND;

            let marker = commands
                .iter()
                .position(|command| {
                    matches!(
                        command,
                        RenderCommand::DrawIcon { origin, name, color, .. }
                            if name == "upper-left-triangle"
                                && *color == accent
                                && (origin.x - card.min_x()).abs() < 0.01
                                && (origin.y - card.min_y()).abs() < 0.01
                    )
                })
                .expect("the focused pane's accent corner marker on its card corner");
            let cut = commands
                .iter()
                .position(|command| {
                    matches!(
                        command,
                        RenderCommand::StrokeRect {
                            rect,
                            color,
                            width,
                            corner_radius,
                        } if *color == surface
                            && (*width - band).abs() < 0.01
                            && (*corner_radius - (radius + band)).abs() < 0.01
                            && (rect.min_x() - (card.min_x() - band)).abs() < 0.01
                            && (rect.min_y() - (card.min_y() - band)).abs() < 0.01
                            && (rect.width() - (card.width() + band * 2.0)).abs() < 0.01
                            && (rect.height() - (card.height() + band * 2.0)).abs() < 0.01
                    )
                })
                .expect("the card cuts its content back to the rounded path");
            let edge = commands
                .iter()
                .position(|command| {
                    matches!(
                        command,
                        RenderCommand::StrokeRect {
                            rect,
                            color,
                            width,
                            corner_radius,
                        } if *rect == card
                            && *color == border
                            && (*width - 1.0).abs() < 0.01
                            && (*corner_radius - radius).abs() < 0.01
                    )
                })
                .expect("the card's rounded Border edge over the cut");
            assert!(
                cut > marker,
                "the surface cut is painted after the focus marker, so it clips the marker's \
                 square corner: cut {cut} against marker {marker}"
            );
            assert!(
                edge > cut,
                "the rounded Border edge is painted over the cut, so the card's corner closes: \
                 edge {edge} against cut {cut}"
            );
            // The cut's inner edge is the card's own rounded path: the stroked
            // band starts on the expanded rect's edge and reaches in by `band`,
            // and the radius shrinks by the same amount.
            assert!(
                (radius + band - band - radius).abs() < 0.01,
                "the cut's inner edge is the card's rounded path"
            );
        }
    }

    /// No pane card leaves a square pixel outside its rounded corner: the
    /// card's four corner spandrels are the panel surface, the pixels just
    /// inside the path are the card content, and the focused pane's accent
    /// triangle has no pixel outside the rounded top-left corner. This is the
    /// rendered-pixel counterpart of the command-order tests — it is read off
    /// the real `RootView` rasterized at one device pixel per point.
    #[test]
    fn no_square_pixels_survive_at_a_pane_cards_corners() {
        let mut light = AppContext::default();
        light.theme = goble_ui::theme::Theme::light();

        for app in [&AppContext::default(), &light] {
            for two_panes in [false, true] {
                let (pixels, frames) = raster_frame(app, two_panes);
                assert_eq!(frames.len(), if two_panes { 2 } else { 1 });
                let surface = app.theme.color(ColorToken::Surface);
                let bg = app.theme.color(ColorToken::Bg);
                let accent = app.theme.color(ColorToken::Accent);

                for (index, card) in frames.iter().enumerate() {
                    let radius = crate::ui::panes::card_radius(vec2f(card.width(), card.height()));
                    let focused = index == 0;
                    let corner = |x: f32, y: f32| pixel_at(&pixels, x, y);
                    let near = |at: [u8; 3], color: goble_ui::ColorU| near_color(at, color);

                    // The four corner spandrels — outside the rounded path,
                    // inside the card's rectangle — are the panel surface the
                    // card sits on, whatever the card's content paints there.
                    for (what, x, y) in [
                        ("top-left", card.min_x(), card.min_y()),
                        ("top-right", card.max_x() - 1.0, card.min_y()),
                        ("bottom-left", card.min_x(), card.max_y() - 1.0),
                        ("bottom-right", card.max_x() - 1.0, card.max_y() - 1.0),
                    ] {
                        let at = corner(x, y);
                        assert!(
                            near(at, surface),
                            "the {what} spandrel of a pane card is the panel surface, \
                             not a square card pixel: {at:?} against {surface:?}"
                        );
                    }

                    // Just inside the rounded path (the corner's arc centre) the
                    // card content still paints: the card fill is Bg.
                    for (what, x, y) in [
                        ("bottom-left", card.min_x() + radius, card.max_y() - radius),
                        ("bottom-right", card.max_x() - radius, card.max_y() - radius),
                    ] {
                        let at = corner(x, y);
                        assert!(
                            near(at, bg),
                            "the {what} corner inside the path is the card's own fill: {at:?}"
                        );
                    }

                    // The focus marker is inside the rounded top-left corner: it
                    // paints there, and no accent pixel lies in the spandrel
                    // outside the path (the square bite would show here).
                    let inside = corner(card.min_x() + radius * 0.5, card.min_y() + radius * 0.5);
                    let outside = accent_pixels_outside_corner(&pixels, *card, radius, accent);
                    if focused {
                        assert!(
                            near(inside, accent),
                            "the focused pane's accent triangle paints inside the card's \
                             rounded top-left corner: {inside:?} against {accent:?}"
                        );
                        assert_eq!(
                            outside, 0,
                            "no accent pixel may sit outside the card's rounded top-left \
                             corner (the marker is clipped to the radius)"
                        );
                    } else {
                        assert!(
                            !near(inside, accent),
                            "an unfocused pane wears no accent corner: {inside:?}"
                        );
                        assert_eq!(outside, 0);
                    }
                }
            }
        }
    }

    /// The real app frame rasterized to RGBA pixels at one device pixel per
    /// point, for the corner probes no command list can prove.
    fn raster_frame(app: &AppContext, two_panes: bool) -> (image::RgbaImage, Vec<RectF>) {
        let (mut root, state, _dir) = app_root_for(app);
        if two_panes {
            install_two_panes(&state, 0.5);
        }
        let assets = raster::PreviewAssets::load(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../crates/goble-ui/assets")
                .canonicalize()
                .expect("the bundled icon and font assets"),
        )
        .expect("the preview assets load");
        let dir = tempfile::tempdir().expect("temp frame dir");
        let out = dir.path().join("pane-corners.png");
        raster::render_element_to_png(
            &assets,
            app,
            &mut root,
            window().x,
            window().y,
            1.0,
            None,
            &out,
        )
        .expect("the frame rasterizes");
        let pixels = image::open(&out)
            .expect("the rendered frame reads back")
            .to_rgba8();
        let frames = pane_frames(&state);
        (pixels, frames)
    }

    /// The RGB at the pixel the point falls in.
    fn pixel_at(pixels: &image::RgbaImage, x: f32, y: f32) -> [u8; 3] {
        let px = pixels
            .get_pixel(x.floor().max(0.0) as u32, y.floor().max(0.0) as u32)
            .0;
        [px[0], px[1], px[2]]
    }

    /// Whether the drawn pixel matches the theme colour, within the renderer's
    /// anti-aliasing and linear-light blend tolerance.
    fn near_color(pixel: [u8; 3], color: goble_ui::ColorU) -> bool {
        pixel[0].abs_diff(color.r) <= 6
            && pixel[1].abs_diff(color.g) <= 6
            && pixel[2].abs_diff(color.b) <= 6
    }

    /// The accent pixels of `card`'s top-left corner square that lie outside the
    /// card's rounded path (the corner's arc centre is the square's far corner).
    fn accent_pixels_outside_corner(
        pixels: &image::RgbaImage,
        card: RectF,
        radius: f32,
        accent: goble_ui::ColorU,
    ) -> usize {
        let mut outside = 0;
        let reach = radius.ceil() as u32 + 4;
        for dy in 0..reach {
            for dx in 0..reach {
                let x = card.min_x().floor() as u32 + dx;
                let y = card.min_y().floor() as u32 + dy;
                let px = pixels.get_pixel(x, y).0;
                if !near_color([px[0], px[1], px[2]], accent) {
                    continue;
                }
                // Pixel centre, measured from the rounded corner's arc centre.
                let cx = card.min_x() + radius;
                let cy = card.min_y() + radius;
                let distance =
                    ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                if distance > radius {
                    outside += 1;
                }
            }
        }
        outside
    }

    /// The 1 pt `Border` rules rounded at the pane card's own radius: one per
    /// pane card. No extra rule may frame a tab slot.
    fn rounded_border_strokes(commands: &[RenderCommand], app: &AppContext) -> usize {
        let border = app.theme.color(ColorToken::Border);
        commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    RenderCommand::StrokeRect {
                        color,
                        width,
                        corner_radius,
                        ..
                    } if *color == border
                        && (*width - 1.0).abs() < 0.01
                        && (*corner_radius - crate::ui::panes::WINDOW_CORNER_RADIUS).abs() < 0.01
                )
            })
            .count()
    }

    /// The focused panes' accent corner markers (the upper-left triangle icon),
    /// by their drawn origin.
    fn corner_markers(commands: &[RenderCommand], app: &AppContext) -> Vec<Vector2F> {
        let accent = app.theme.color(ColorToken::Accent);
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon {
                    origin, name, color, ..
                } if name == "upper-left-triangle" && *color == accent => Some(*origin),
                _ => None,
            })
            .collect()
    }

    /// Whether the frame draws an accent rule on the window's own rect.
    fn has_accent_window_rule(commands: &[RenderCommand], app: &AppContext) -> bool {
        let accent = app.theme.color(ColorToken::Accent);
        commands.iter().any(|command| match command {
            RenderCommand::StrokeRect { rect, color, .. } => {
                *color == accent && rect.width() == window().x && rect.height() == window().y
            }
            _ => false,
        })
    }

    /// The colour of the last fill covering `point`, i.e. what the frame leaves
    /// visible there.
    fn topmost_fill(commands: &[RenderCommand], point: Vector2F) -> Option<goble_ui::ColorU> {
        commands.iter().rev().find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if point.x >= rect.min_x()
                    && point.x <= rect.max_x()
                    && point.y >= rect.min_y()
                    && point.y <= rect.max_y() =>
            {
                Some(*color)
            }
            _ => None,
        })
    }

    /// Every pane card in a rendered frame, in window order: the rectangles the
    /// panes recorded while painting (`UiSnapshot::pane_bounds`). The card fill
    /// itself is no longer a reliable marker — an agent pane's card is the same
    /// `Bg` as the window behind it — so the frame geometry is read from the
    /// bounds the panes actually painted at, which is also what a drop resolves
    /// against.
    fn pane_frames(state: &Rc<RefCell<UiState>>) -> Vec<RectF> {
        let s = state.borrow();
        let mut frames: Vec<RectF> = s.pane_bounds.borrow().values().copied().collect();
        frames.sort_by(|a, b| a.min_x().total_cmp(&b.min_x()));
        frames
    }

    /// The pane's own header band: the approved full-bleed, square `Surface`
    /// fill over the pane's top edge. It bleeds `PANE_INSET` past the pane card
    /// on the top, left and right, and its bottom meets the pane frame's top
    /// plus `TOPBAR_HEIGHT` (so the band is one bar tall against the card, not
    /// inset). A header inside the content lane would be lane-wide instead.
    fn header_band(commands: &[RenderCommand], app: &AppContext, frame: RectF) -> Option<RectF> {
        let surface = app.theme.color(ColorToken::Surface);
        let inset = crate::ui::panes::PANE_INSET;
        commands.iter().find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } if *color == surface
                && corner_radius.abs() < 0.01
                && (rect.min_y() - (frame.min_y() - inset)).abs() < 0.5
                && (rect.height() - (shell::TOPBAR_HEIGHT + inset)).abs() < 0.5
                && (rect.min_x() - (frame.min_x() - inset)).abs() < 0.5
                && (rect.width() - (frame.width() + 2.0 * inset)).abs() < 0.5 =>
            {
                Some(*rect)
            }
            _ => None,
        })
    }

    /// The pane's content column: the widest `Bg` fill inside the pane frame,
    /// excluding the card itself. With the lane active it is the centred column;
    /// on a narrow pane it is the pane's full width. An agent pane's card is the
    /// same `Bg` as its content column (that is the point of the uniform agent
    /// surface), so the card's own fill — the frame rect itself — is skipped and
    /// the column is the tall fill under the pane's chrome.
    fn content_column(commands: &[RenderCommand], app: &AppContext, frame: RectF) -> RectF {
        let bg = app.theme.color(ColorToken::Bg);
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == bg
                        && rect.min_x() >= frame.min_x() - 0.5
                        && rect.max_x() <= frame.max_x() + 0.5
                        && rect.min_y() >= frame.min_y() - 0.5
                        && rect.max_y() <= frame.max_y() + 0.5
                        && rect.width() > 100.0
                        && !((rect.width() - frame.width()).abs() < 0.5
                            && (rect.height() - frame.height()).abs() < 0.5) =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .max_by(|a, b| (a.width() * a.height()).total_cmp(&(b.width() * b.height())))
            .expect("the pane's content column is painted")
    }

    /// Every pane in `frames` centres its own content column in its own frame
    /// and its header spans the whole frame: the core per-pane-lane invariant.
    fn assert_pane_geometry(
        commands: &[RenderCommand],
        app: &AppContext,
        frames: &[RectF],
        pref: Option<f32>,
    ) {
        assert!(!frames.is_empty(), "the frame draws its panes");
        for frame in frames {
            let column = content_column(commands, app, *frame);
            let (gutter, width) = lane_column(frame.width(), pref);
            assert!(
                (column.width() - width).abs() < 0.5,
                "the pane's content column is the lane's column: {} against {width} (frame {:?})",
                column.width(),
                frame
            );
            assert!(
                (column.min_x() - (frame.min_x() + gutter)).abs() < 0.5,
                "the content column is centred in its own pane: {} against {} (frame {:?})",
                column.min_x(),
                frame.min_x() + gutter,
                frame
            );
            let inset = crate::ui::panes::PANE_INSET;
            let header = header_band(commands, app, *frame).unwrap_or_else(|| {
                panic!(
                    "the pane's header is the full-bleed square band over its top edge: no \
                     {TOPBAR}+{inset}-tall Surface fill bleeding {inset} past {:?}",
                    frame,
                    TOPBAR = shell::TOPBAR_HEIGHT
                )
            });
            assert!(
                (header.min_x() - (frame.min_x() - inset)).abs() < 0.5,
                "the header band bleeds PANE_INSET past the pane's left edge: {:?} against {:?}",
                header,
                frame
            );
            assert!(
                (header.width() - (frame.width() + 2.0 * inset)).abs() < 0.5,
                "the header band spans the pane slot (pane width + 2×PANE_INSET), not the content \
                 lane: {:?} against {:?}",
                header,
                frame
            );
            assert!(
                (header.max_y() - (frame.min_y() + shell::TOPBAR_HEIGHT)).abs() < 0.5,
                "the band's bottom is the pane card's top + TOPBAR_HEIGHT: {:?} against {:?}",
                header,
                frame
            );
            if width < frame.width() - 0.5 {
                assert!(
                    column.width() < frame.width() - 0.5,
                    "an active lane is narrower than its pane: {} against {:?}",
                    column.width(),
                    frame
                );
            }
        }
    }

    fn chat_pane_session() -> PaneSession {
        PaneSession {
            conversation_id: String::new(),
            draft: String::new(),
            path: String::new(),
            daemon_session_id: String::new(),
        }
    }

    fn chat_leaf(id: u64) -> Box<Pane> {
        Box::new(Pane::Leaf {
            id,
            kind: PaneKind::Chat,
        })
    }

    /// Install a two-pane horizontal split (panes 7 and 8, active on 7) with a
    /// per-pane session each, so both draw a chat surface.
    fn install_two_panes(state: &Rc<RefCell<UiState>>, ratio: f32) {
        let mut s = state.borrow_mut();
        s.spaces = vec![Space::unnamed(Pane::Split {
            id: 9,
            dir: SplitDir::Horizontal,
            ratio,
            first: chat_leaf(7),
            second: chat_leaf(8),
        })];
        s.active_space = 0;
        s.active_pane_id = 7;
        for id in [7u64, 8] {
            s.pane_sessions.insert(id, chat_pane_session());
        }
    }

    /// Install a three-pane split with equal slots (outer 2/3, inner 1/2):
    /// panes 7, 8, 11, active on 7.
    fn install_three_panes(state: &Rc<RefCell<UiState>>) {
        let mut s = state.borrow_mut();
        s.spaces = vec![Space::unnamed(Pane::Split {
            id: 9,
            dir: SplitDir::Horizontal,
            ratio: 2.0 / 3.0,
            first: Box::new(Pane::Split {
                id: 10,
                dir: SplitDir::Horizontal,
                ratio: 0.5,
                first: chat_leaf(7),
                second: chat_leaf(8),
            }),
            second: chat_leaf(11),
        })];
        s.active_space = 0;
        s.active_pane_id = 7;
        for id in [7u64, 8, 11] {
            s.pane_sessions.insert(id, chat_pane_session());
        }
    }

    /// The panes span the pane area (not one window-wide lane column), and each
    /// pane centres its own content in its own width while its header spans the
    /// full pane: 1, 2 (different widths) and 3 panes.
    #[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
    #[test]
    fn each_pane_centres_its_own_content_and_its_header_spans_the_pane() {
        let app = AppContext::default();

        // One pane on a wide window: the pane fills the pane area and its
        // content column is the ratio column, centred in the pane.
        let (mut root, state, _dir) = app_root();
        let commands = render_element(&mut root, vec2f(1600.0, 900.0), &app);
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 1, "one pane: {frames:?}");
        // Main 1300, panel inset 4 each side (1292), leaf inset 4 each side:
        // the pane is 1284 wide.
        assert!((frames[0].width() - 1284.0).abs() < 0.5, "{:?}", frames[0]);
        assert_pane_geometry(&commands, &app, &frames, None);
        let (gutter, width) = lane_column(frames[0].width(), None);
        assert!(
            (width - 821.76).abs() < 0.5 && (gutter - 231.12).abs() < 0.5,
            "64% of 1284 centred: {width} at gutter {gutter}"
        );

        // Two panes of DIFFERENT widths (ratio 0.6 at 2600): each centres its
        // own content inside its own pane, not in a shared window-wide column.
        let (mut root, state, _dir) = app_root();
        install_two_panes(&state, 0.6);
        let commands = render_element(&mut root, vec2f(2600.0, 900.0), &app);
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 2, "two panes: {frames:?}");
        // Main 2300, panel inset 4 each side: the pane area is 2292. Split 0.6:
        // slots 1375.2 and 916.8, so the frames are 1367.2 and 908.8 (both above
        // the 856 activation floor).
        assert!((frames[0].width() - 1367.2).abs() < 0.5, "{:?}", frames[0]);
        assert!((frames[1].width() - 908.8).abs() < 0.5, "{:?}", frames[1]);
        assert_pane_geometry(&commands, &app, &frames, None);
        let left = content_column(&commands, &app, frames[0]);
        let right = content_column(&commands, &app, frames[1]);
        assert!(
            (left.width() - 875.008).abs() < 0.5,
            "64% of the left pane's 1367.2: {left:?}"
        );
        assert!(
            (right.width() - 680.0).abs() < 0.5,
            "64% of the right pane's 908.8 clamps to the 680 floor: {right:?}"
        );
        assert!(
            (left.width() - right.width()).abs() > 1.0,
            "the two panes resolve their own columns: {left:?} against {right:?}"
        );

        // Three panes on a 3000 window: each pane has its own lane.
        let (mut root, state, _dir) = app_root();
        install_three_panes(&state);
        let commands = render_element(&mut root, vec2f(3000.0, 900.0), &app);
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 3, "three panes: {frames:?}");
        for frame in &frames {
            // 3000 - sidebar 300 = 2700; panel inset 4 each side → 2692; two
            // equal nested halves → 897.333 slots; minus 8 of card inset each.
            assert!((frame.width() - 889.333).abs() < 0.5, "equal slots: {frame:?}");
        }
        assert_pane_geometry(&commands, &app, &frames, None);

        // The app topbar and the sidebar stay full width: the selected
        // workspace tab starts at the bar's own leading inset, not inside any
        // pane's lane.
        let leading = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == app.theme.color(ColorToken::Surface)
                        && rect.min_y() > 0.0
                        && rect.min_y() < shell::TOPBAR_HEIGHT
                        && rect.max_y() > shell::TOPBAR_HEIGHT
                        // The foot band is the merge line's widening; the
                        // body is the tall one.
                        && rect.height() > 20.0 =>
                {
                    Some(rect.min_x())
                }
                _ => None,
            })
            .min_by(f32::total_cmp)
            .expect("the selected workspace tab is painted in the topbar");
        assert!(
            leading < crate::ui::SIDEBAR_WIDTH,
            "the topbar is not inside a pane's lane: {leading}"
        );
    }

    /// A narrow pane (and a narrow window) stays a full-width pass-through with
    /// equal gutters of zero: today's small-screen behaviour, per pane.
    #[ignore = "chrome-panels: asserted the retired per-pane header/general-topbar structure"]
    #[test]
    fn a_narrow_pane_stays_full_width() {
        let app = AppContext::default();

        // 1024 window: the pane is 708 wide (1024 - 300 sidebar - 8 panel
        // inset - 8 card inset), under the 856 activation floor.
        let (mut root, state, _dir) = app_root();
        let commands = render_element(&mut root, window(), &app);
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 1);
        assert_pane_geometry(&commands, &app, &frames, None);
        let column = content_column(&commands, &app, frames[0]);
        assert!(
            (column.width() - frames[0].width()).abs() < 0.5
                && (column.min_x() - frames[0].min_x()).abs() < 0.5,
            "at 1024 the pane is full width: {column:?} against {:?}",
            frames[0]
        );

        // 2 panes at 1600: each pane is 638 wide (1292 pane area / 2 - 8), so
        // both lanes are inactive.
        let (mut root, state, _dir) = app_root();
        install_two_panes(&state, 0.5);
        let commands = render_element(&mut root, vec2f(1600.0, 900.0), &app);
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 2);
        for frame in &frames {
            assert!((frame.width() - 638.0).abs() < 0.5, "{frame:?}");
        }
        assert_pane_geometry(&commands, &app, &frames, None);
        for frame in &frames {
            let column = content_column(&commands, &app, *frame);
            assert!(
                (column.width() - frame.width()).abs() < 0.5,
                "a narrow pane stays full width: {column:?} against {frame:?}"
            );
        }
    }

    /// The app's own actions over a fresh state, without building a whole
    /// view: enough to drive the lane's drag callbacks the way the element
    /// does.
    fn lane_actions(state: &Rc<RefCell<UiState>>) -> crate::ui::UiActions {
        crate::actions::make_actions(
            Rc::clone(state),
            None,
            Rc::new(RefCell::new(crate::media::MediaState::mock())),
            goble_ui::platform::WindowControl::default(),
            Rc::new(RefCell::new(1.0)),
        )
    }

    /// The drag math lives in the app: symmetric about the centre, with the
    /// same `resolve_content_width` the lane lays out with, and it writes only
    /// the dragged pane's preference.
    #[test]
    fn the_lane_drag_is_symmetric_per_pane_and_clamped() {
        let state = Rc::new(RefCell::new(UiState::mock()));
        let actions = lane_actions(&state);
        let pref = |pane: u64| state.borrow().content_lane_prefs.get(&pane).copied();

        // Pane 7, right edge, column 1600, content 920: 50 px outward widens
        // by 100.
        (actions.on_content_lane_drag_start.borrow_mut())(
            7,
            1289.0,
            LaneEdge::Right,
            920.0,
            1600.0,
        );
        {
            let s = state.borrow();
            assert!(s.content_lane_dragging, "a press on the handle starts the drag");
            assert_eq!(s.content_lane_drag_pane, Some(7), "the drag names its pane");
            assert_eq!(s.content_lane_drag_edge, Some(LaneEdge::Right));
        }
        (actions.on_content_lane_drag_move.borrow_mut())(1339.0);
        assert_eq!(pref(7), Some(1020.0));
        assert_eq!(pref(8), None, "another pane's width is never touched");
        // Past the margin budget: the pane's column margin wins.
        (actions.on_content_lane_drag_move.borrow_mut())(1800.0);
        assert_eq!(pref(7), Some(1424.0));
        // Past the floor: 640 wins.
        (actions.on_content_lane_drag_move.borrow_mut())(100.0);
        assert_eq!(pref(7), Some(640.0));
        (actions.on_content_lane_drag_end.borrow_mut())();
        {
            let s = state.borrow();
            assert_eq!(
                s.content_lane_prefs.get(&7),
                Some(&640.0),
                "the last resolved width is the committed preference"
            );
            assert!(!s.content_lane_dragging && s.content_lane_drag_pane.is_none());
            assert_eq!(s.content_lane_drag_edge, None);
        }

        // The left edge is mirrored: outward is pointer-left. Pane 8's drag
        // leaves pane 7's committed width alone.
        (actions.on_content_lane_drag_start.borrow_mut())(
            8,
            311.0,
            LaneEdge::Left,
            920.0,
            1600.0,
        );
        (actions.on_content_lane_drag_move.borrow_mut())(261.0);
        assert_eq!(
            pref(8),
            Some(1020.0),
            "dragging the left edge left widens, like the right edge right"
        );
        assert_eq!(pref(7), Some(640.0), "the sibling pane keeps its own width");
        (actions.on_content_lane_drag_move.borrow_mut())(411.0);
        assert_eq!(pref(8), Some(720.0), "dragging the left edge inward narrows");
        (actions.on_content_lane_drag_end.borrow_mut())();

        // A narrow column is a full-width pass-through whatever is dragged.
        (actions.on_content_lane_drag_start.borrow_mut())(
            7,
            400.0,
            LaneEdge::Right,
            800.0,
            800.0,
        );
        (actions.on_content_lane_drag_move.borrow_mut())(500.0);
        assert_eq!(
            pref(7),
            Some(800.0),
            "a narrow lane stays full width"
        );
        (actions.on_content_lane_drag_end.borrow_mut())();
    }

    /// A drag in one pane resizes only that pane: with two panes carrying
    /// DIFFERENT committed widths, the sibling's content column is
    /// byte-identical before and after the drag.
    #[test]
    fn a_lane_drag_resizes_only_the_pane_it_is_in() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        install_two_panes(&state, 0.6);
        let size = vec2f(2600.0, 900.0);
        // Learn each pane's width first, then give the two panes different
        // committed widths (844 and 680) — the shape the owner drags in.
        let commands = render_element(&mut root, size, &app);
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 2);
        let expected_left = lane_column(frames[0].width(), None).1;
        let expected_right = lane_column(frames[1].width(), None).1;
        {
            let mut s = state.borrow_mut();
            s.content_lane_prefs.insert(7, expected_left - 34.0);
            s.content_lane_prefs.insert(8, expected_right);
        }
        let commands = render_element(&mut root, size, &app);
        let frames = pane_frames(&state);
        let before_left = content_column(&commands, &app, frames[0]);
        let before_right = content_column(&commands, &app, frames[1]);
        assert!(
            (before_left.width() - (expected_left - 34.0)).abs() < 0.5,
            "the left pane starts on its own committed width: {before_left:?}"
        );
        assert!(
            (before_right.width() - expected_right).abs() < 0.5,
            "the right pane starts on its own (different) committed width: {before_right:?}"
        );

        // The LEFT pane's right strip: 24 px outside its content column, 10 px
        // wide, under the pane header.
        let column_right = frames[0].min_x() + (frames[0].width() - before_left.width()) / 2.0
            + before_left.width();
        let press = vec2f(column_right + 24.0 + 5.0, 400.0);
        let mut ctx = EventContext::default();
        assert!(
            root.dispatch_event(
                &DispatchedEvent::MouseDown {
                    position: press,
                    button: 0,
                },
                &mut ctx,
                &app,
            ),
            "the press lands on the left pane's invisible edge"
        );
        {
            let s = state.borrow();
            assert_eq!(s.active_pane_id, 7, "the press focuses the pane it lands in");
            assert_eq!(s.content_lane_drag_pane, Some(7), "only the left pane owns the drag");
            let left = s.content_lane_prefs.get(&7).copied().expect("the dragged pane freezes its width");
            assert!(
                (left - (expected_left - 34.0)).abs() < 0.5,
                "frozen at the pane's committed width: {left}"
            );
            assert_eq!(
                s.content_lane_prefs.get(&8),
                Some(&expected_right),
                "the right pane keeps its own committed width"
            );
        }

        // 50 px outward widens the left pane's content by 2*50 = 100.
        let moved = vec2f(press.x + 50.0, 400.0);
        assert!(root.dispatch_event(
            &DispatchedEvent::MouseMove { position: moved },
            &mut ctx,
            &app,
        ));
        let dragged =
            resolve_content_width(frames[0].width(), Some(expected_left - 34.0 + 100.0));
        {
            let s = state.borrow();
            let left = s.content_lane_prefs.get(&7).copied().unwrap();
            assert!((left - dragged).abs() < 0.5, "the left pane follows the pointer: {left} against {dragged}");
            assert_eq!(
                s.content_lane_prefs.get(&8),
                Some(&expected_right),
                "the right pane's committed width is untouched"
            );
        }
        assert!(root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: moved,
                button: 0,
            },
            &mut ctx,
            &app,
        ));
        {
            let s = state.borrow();
            assert!(!s.content_lane_dragging && s.content_lane_drag_pane.is_none());
            assert!((s.content_lane_prefs.get(&7).copied().unwrap() - dragged).abs() < 0.5);
        }

        // The next frame lays the left pane out at the committed width; the
        // right pane's content column is exactly where it was.
        let commands = render_element(&mut root, size, &app);
        let frames = pane_frames(&state);
        let left = content_column(&commands, &app, frames[0]);
        let right = content_column(&commands, &app, frames[1]);
        assert!(
            (left.width() - dragged).abs() < 0.5,
            "the dragged pane's content follows its preference: {left:?} against {dragged}"
        );
        assert!(
            left.width() > before_left.width() + 99.0,
            "the dragged pane widened: {left:?} against {before_left:?}"
        );
        assert_eq!(
            right, before_right,
            "the sibling pane's content column did not move or change size"
        );
        assert!((right.width() - expected_right).abs() < 0.5);
    }

    /// Grabbing a pane's handle and letting go without moving anything does not
    /// switch that pane from its ratio to a committed preference (and never
    /// touches a sibling's).
    #[test]
    fn a_lane_click_without_motion_leaves_the_panes_ratio_alone() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        install_two_panes(&state, 0.6);
        let commands = render_element(&mut root, vec2f(2600.0, 900.0), &app);
        let frames = pane_frames(&state);
        let (gutter, width) = lane_column(frames[0].width(), None);
        let press = vec2f(frames[0].min_x() + gutter + width + 24.0 + 5.0, 400.0);
        let mut ctx = EventContext::default();

        let _ = root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: press,
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert!(
            state.borrow().content_lane_prefs.contains_key(&7),
            "the pressed pane freezes its width while held"
        );
        let _ = root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: press,
                button: 0,
            },
            &mut ctx,
            &app,
        );
        {
            let s = state.borrow();
            assert_eq!(
                s.content_lane_prefs.get(&7),
                None,
                "no motion: the pane's ratio is restored"
            );
            assert_eq!(s.content_lane_prefs.get(&8), None, "the sibling was never touched");
        }
    }

    /// Closing a pane drops its committed lane width and keeps every other
    /// pane's: the preference map is pruned with the pane (`forget_pane`).
    #[ignore = "chrome-panels: asserted the retired per-pane header/general-topbar structure"]
    #[test]
    fn closing_a_pane_drops_its_lane_preference() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        install_two_panes(&state, 0.6);
        let _ = render_element(&mut root, vec2f(2600.0, 900.0), &app);
        {
            let mut s = state.borrow_mut();
            s.content_lane_prefs.insert(7, 950.0);
            s.content_lane_prefs.insert(8, 700.0);
            s.content_lane_drag_pane = Some(7);
            s.content_lane_dragging = true;
            s.close_pane(7, None);
        }
        {
            let s = state.borrow();
            assert!(
                !s.content_lane_prefs.contains_key(&7),
                "the closed pane's width is dropped"
            );
            assert_eq!(
                s.content_lane_prefs.get(&8),
                Some(&700.0),
                "the surviving pane keeps its own width"
            );
            assert!(
                !s.content_lane_dragging && s.content_lane_drag_pane.is_none(),
                "the drag the closed pane owned is gone"
            );
        }
        // The surviving pane fills the pane area and lays its own committed
        // width out inside itself.
        let commands = render_element(&mut root, vec2f(2600.0, 900.0), &app);
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 1, "the split collapses to the surviving leaf");
        assert_pane_geometry(&commands, &app, &frames, Some(700.0));
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
    #[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
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

    /// Every thin one-axis rule a frame paints below the bar: a fill no thicker
    /// than 1.5 px and at least 40 px long. These are the divider rules a
    /// resizable boundary must not draw.
    fn thin_rules(commands: &[RenderCommand]) -> Vec<RectF> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, .. }
                    if rect.min_y() >= shell::TOPBAR_HEIGHT
                        && ((rect.width() <= 1.5 && rect.height() >= 40.0)
                            || (rect.height() <= 1.5 && rect.width() >= 40.0)) =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .collect()
    }

    /// Paint the already-built frame once more with the pointer at `pointer`, so
    /// the cursor the tree asks the platform for is observable.
    fn paint_with_pointer(root: &mut Box<dyn Element>, app: &AppContext, pointer: Vector2F) {
        let mut ctx = goble_ui::elements::PaintContext::new(Renderer::new());
        ctx.cursor_inside = true;
        ctx.cursor_position = pointer;
        root.paint(vec2f(0.0, 0.0), &mut ctx, app);
    }

    /// The tab slots (the pane area the tabs tile) are inset 4 px from the
    /// window's chrome on every side, and the pane cards keep their own
    /// `PANE_INSET` gutter inside them. The SURFACE behind them, though, paints
    /// the body's whole rectangle: the `PANE_AREA_INSET` frame is part of the
    /// panel surface, so no dark strip is left around the panes. The per-tab
    /// outline keeps its geometry relative to its slot
    /// (`every_tab_wears_its_own_outline_not_the_window`).
    #[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
    #[test]
    fn the_tab_slots_are_inset_four_px_and_the_surface_fills_the_body() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let window = window();
        let sheet_top = shell::TOPBAR_HEIGHT + shell::TOPBAR_BOTTOM_BAND;
        let commands = render_element(&mut root, window, &app);

        let surface = app.theme.color(ColorToken::Surface);
        let band = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == surface
                    && *corner_radius == 0.0
                    && (rect.height() - (shell::TOPBAR_HEIGHT + crate::ui::panes::PANE_INSET))
                        .abs()
                        < 0.5
                    && rect.min_y() > shell::TOPBAR_HEIGHT
                    && rect.width() > window.x / 2.0 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the pane's full-bleed header band is painted");
        assert_eq!(
            band.min_y(),
            sheet_top + crate::ui::panes::PANE_AREA_INSET,
            "the first tab slot starts 4 px below the bar block: {band:?}"
        );
        assert_eq!(
            band.min_x(),
            crate::ui::SIDEBAR_WIDTH + crate::ui::panes::PANE_AREA_INSET,
            "and 4 px right of the sidebar: {band:?}"
        );
        assert_eq!(
            band.max_x(),
            window.x - crate::ui::panes::PANE_AREA_INSET,
            "and 4 px inside the window's right edge: {band:?}"
        );

        // The pane card keeps its own gutter inside the tab slot, and the tab
        // slot keeps the 4 px inset from the window's bottom edge.
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 1, "one pane: {frames:?}");
        assert_eq!(
            frames[0].min_x(),
            band.min_x() + crate::ui::panes::PANE_INSET,
            "the card sits PANE_INSET inside its tab slot: {:?}",
            frames[0]
        );
        assert_eq!(
            window.y - (frames[0].max_y() + crate::ui::panes::PANE_INSET),
            crate::ui::panes::PANE_AREA_INSET,
            "and 4 px above the window's bottom edge: {:?}",
            frames[0]
        );

        // The owner's layering: the surface the panels are rendered on is the
        // SIDEBAR's `Surface` — the pane area's whole backdrop, its inset frame
        // included, and the gutters between and around the tab slots — and the
        // panel card itself is the darker `Bg`.
        let sidebar = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == surface && rect.min_x() == 0.0 && rect.height() > window.y / 2.0 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the sidebar paints its surface");
        let panel_surface = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == surface
                    && *corner_radius == 0.0
                    && rect.width() > window.x / 2.0
                    && rect.height() > window.y / 2.0 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the panel area paints its surface");
        // The surface paints the body's WHOLE rectangle, the `PANE_AREA_INSET`
        // frame included: the tab slots are the 4 px inset area inside it, but
        // the surface behind them is not inset at all — no dark strip is left
        // between the chrome and the panes.
        assert_eq!(
            panel_surface,
            rectf(
                crate::ui::SIDEBAR_WIDTH,
                sheet_top,
                window.x - crate::ui::SIDEBAR_WIDTH,
                window.y - sheet_top
            ),
            "the panel surface fills the body from the chrome to the window edges: \
             {panel_surface:?}"
        );
        assert_eq!(
            sidebar.min_y(),
            shell::TOPBAR_HEIGHT + shell::TOPBAR_BOTTOM_BAND,
            "the sidebar still starts at the body origin: {sidebar:?}"
        );

        // The gutters around the card show the panel surface (the sidebar's
        // colour), and the card fill itself is the darker `Bg`.
        let mid_y = frames[0].min_y() + frames[0].height() / 2.0;
        let in_gutter = vec2f(
            frames[0].min_x() - crate::ui::panes::PANE_INSET / 2.0,
            mid_y,
        );
        assert_eq!(
            topmost_fill(&commands, in_gutter),
            Some(app.theme.color(ColorToken::Surface)),
            "the gutter around the panel is the panel surface: {in_gutter:?}"
        );
        let card_fill = commands.iter().find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. } if *rect == frames[0] => Some(*color),
            _ => None,
        });
        assert_eq!(
            card_fill,
            Some(app.theme.color(ColorToken::Bg)),
            "the panel card is the darker background: {:?}",
            frames[0]
        );
    }

    /// The owner's corrected panel surface ("the panel surface still isn't the
    /// sidebar's grey; a 3 px space remained"): EVERY pixel of the panel area
    /// that is not a pane card is the sidebar's `Surface` — the
    /// `PANE_AREA_INSET` frame itself, the gutters, all four edges — and only
    /// the pane cards are the darker `Bg`.
    ///
    /// The test scans pixels the way the frame leaves them (the last fill
    /// covering the point): the panel area's top/bottom/left/right edges (the
    /// rows and columns of the 4 px inset frame), and the gutter between two
    /// neighbouring tab slots, in both themes.
    #[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
    #[test]
    fn the_whole_panel_surface_is_the_sidebar_grey_with_no_dark_strip() {
        fn assert_theme(app: &AppContext, two_panes: bool) {
            // Two panes need a window wide enough that the responsive layout
            // keeps the sidebar shown beside them, so the sidebar reference
            // pixel is the sidebar's own.
            let window = if two_panes {
                vec2f(1600.0, 900.0)
            } else {
                window()
            };
            let sheet_top = shell::TOPBAR_HEIGHT + shell::TOPBAR_BOTTOM_BAND;
            let (mut root, state, _dir) = app_root_for(app);
            if two_panes {
                install_two_panes(&state, 0.5);
            }
            let commands = render_element(&mut root, window, app);
            let surface = app.theme.color(ColorToken::Surface);
            let bg = app.theme.color(ColorToken::Bg);
            assert_ne!(surface, bg, "the surface and the card are distinct materials");

            // The sidebar's own pixel is the reference the panel surface has to
            // match: both are the theme's `Surface`.
            let mid_y = (sheet_top + window.y) / 2.0;
            let sidebar_pixel = vec2f(crate::ui::SIDEBAR_WIDTH / 2.0, mid_y);

            assert_eq!(
                topmost_fill(&commands, sidebar_pixel),
                Some(surface),
                "the sidebar paints the Surface grey: {sidebar_pixel:?}"
            );

            // The panel area is the body's own rectangle: from the sidebar to
            // the window's right and bottom edges.
            let left = crate::ui::SIDEBAR_WIDTH;
            let top = sheet_top;
            let right = window.x;
            let bottom = window.y;
            let mut scanned = 0usize;
            let mut check = |point: Vector2F, what: &str| {
                let pixel = topmost_fill(&commands, point);
                assert_eq!(
                    pixel,
                    Some(surface),
                    "the panel surface's {what} is the sidebar's Surface, got {pixel:?} \
                     at {point:?} (window {window:?})"
                );
                scanned += 1;
            };
            // Every edge of the panel area, over the whole 4 px inset frame:
            // three pixel-rows/columns inside each edge. Before the fix these
            // were the window's `Bg` (4 px at the top/left, the 3 px strip the
            // owner saw at the right/bottom).
            let mut x = left;
            while x <= right {
                for offset in [1.5, 2.5, 3.5] {
                    check(vec2f(x, top + offset), "top edge");
                    check(vec2f(x, bottom - offset), "bottom edge");
                }
                x += 1.0;
            }
            // The side scans start 1 px below the band's bottom edge and stop
            // 1 px above the window's bottom edge: the body's top row and the
            // window's rule are chrome boundaries, not panel-surface pixels.
            let mut y = top + 1.0;
            while y <= bottom - 1.0 {
                for offset in [1.5, 2.5, 3.5] {
                    check(vec2f(left + offset, y), "left edge");
                    check(vec2f(right - offset, y), "right edge");
                }
                y += 1.0;
            }
            assert!(scanned > 4_000, "the edges were scanned: {scanned} pixels");

            // Only the pane cards are the darker `Bg`: every card paints its
            // own rect in `Bg` …
            let frames = pane_frames(&state);
            assert!(!frames.is_empty(), "the frame draws its panes");
            for frame in &frames {
                let card_fill = commands.iter().find_map(|command| match command {
                    RenderCommand::FillRect { rect, color, .. } if *rect == *frame => Some(*color),
                    _ => None,
                });
                assert_eq!(
                    card_fill,
                    Some(bg),
                    "the pane card {frame:?} is the darker Bg, got {card_fill:?}"
                );
            }
            // … and no `Bg` fill is painted over the panel surface outside a
            // card: there is no stray dark strip anywhere in the pane area.
            let surface_index = commands
                .iter()
                .position(|command| match command {
                    RenderCommand::FillRect { rect, color, .. } =>
                        *color == surface
                            && rect.min_x() == left
                            && rect.max_x() == right
                            && rect.min_y() == top
                            && rect.max_y() == bottom,
                    _ => false,
                })
                .expect("the panel surface paints the body's whole rectangle");
            let stray: Vec<RectF> = commands
                .iter()
                .skip(surface_index + 1)
                .filter_map(|command| match command {
                    RenderCommand::FillRect { rect, color, .. } if *color == bg => {
                        let inside_card = frames.iter().any(|frame| {
                            rect.min_x() >= frame.min_x() - 0.5
                                && rect.max_x() <= frame.max_x() + 0.5
                                && rect.min_y() >= frame.min_y() - 0.5
                                && rect.max_y() <= frame.max_y() + 0.5
                        });
                        // The chrome above the pane area (the tab fillets) is
                        // not a panel-surface pixel.
                        let above_surface = rect.max_y() <= top + 0.5;
                        (!inside_card && !above_surface).then_some(*rect)
                    }
                    _ => None,
                })
                .collect();
            assert!(
                stray.is_empty(),
                "no Bg strip is painted over the panel surface outside the pane cards: {stray:?}"
            );

            // The gutter between two neighbouring tab slots — the shared split
            // boundary — is the panel surface, not a dark seam: the panel does
            // not outline its tabs.
            if frames.len() == 2 {
                let boundary = (frames[0].max_x() + frames[1].min_x()) * 0.5;
                let mut gutter_y = top + 6.0;
                while gutter_y <= bottom - 6.0 {
                    assert_eq!(
                        topmost_fill(&commands, vec2f(boundary, gutter_y)),
                        Some(surface),
                        "the gutter between the tab slots is the panel surface,                          not a dark seam, at y {gutter_y}"
                    );
                    gutter_y += 7.0;
                }
            }
        }

        // Dark: the window `Bg` is #0e0e0e, the sidebar/surface #161616.
        assert_theme(&AppContext::default(), false);
        assert_theme(&AppContext::default(), true);
        // Light: surface and card are their own palette's materials; no
        // hardcoded colours.
        let mut light = AppContext::default();
        light.theme = goble_ui::theme::Theme::light();
        assert_theme(&light, false);
        assert_theme(&light, true);
    }

    /// No divider line is drawn at the split boundary or at the sidebar edge —
    /// neither idle nor while either resize drag is in flight — and the
    /// boundary's whole affordance is the resize cursor over its grab band.
    #[test]
    fn the_resize_boundaries_draw_no_line_and_show_the_resize_cursor() {
        let app = AppContext::default();
        for (split_dragging, sidebar_dragging) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let (mut root, state, _dir) = app_root();
            install_two_panes(&state, 0.5);
            {
                let mut s = state.borrow_mut();
                s.dragging_pane_id = split_dragging.then_some(9);
                s.sidebar_dragging = sidebar_dragging;
            }
            let command = || {
                format!("split dragging {split_dragging}, sidebar dragging {sidebar_dragging}")
            };
            // Wide enough that the responsive sidebar (this lane's breakpoint
            // work) keeps it shown beside the two panes: the sidebar edge must
            // be probeable, and its own collapse has its own test.
            let size = vec2f(1600.0, 900.0);
            let commands = render_element(&mut root, size, &app);

            let rules = thin_rules(&commands);
            assert!(
                rules.is_empty(),
                "{}: no divider rule at a resizable boundary: {rules:?}",
                command()
            );

            // The split's boundary: half-way between the two neighbouring pane
            // cards, in the middle of the grab band.
            let frames = pane_frames(&state);
            assert_eq!(frames.len(), 2, "two panes: {frames:?}");
            let boundary_x = (frames[0].max_x() + frames[1].min_x()) / 2.0;
            let mid_y = (sheet_top() + size.y) / 2.0;
            paint_with_pointer(&mut root, &app, vec2f(boundary_x, mid_y));
            assert_eq!(
                app.window_control.cursor(),
                CursorKind::EwResize,
                "{}: the split boundary shows the resize cursor",
                command()
            );

            // The sidebar edge.
            paint_with_pointer(
                &mut root,
                &app,
                vec2f(crate::ui::SIDEBAR_WIDTH, mid_y),
            );
            assert_eq!(
                app.window_control.cursor(),
                CursorKind::EwResize,
                "{}: the sidebar edge shows the resize cursor",
                command()
            );

            // Off the bands the cursor is the ordinary arrow — unless a drag is
            // in flight, which captures it as a resize arrow wherever it is.
            let over_content = vec2f(
                frames[0].min_x() + frames[0].width() / 2.0,
                frames[0].min_y() + frames[0].height() / 2.0,
            );
            paint_with_pointer(&mut root, &app, over_content);
            let expected = if split_dragging || sidebar_dragging {
                CursorKind::EwResize
            } else {
                CursorKind::Default
            };
            assert_eq!(
                app.window_control.cursor(),
                expected,
                "{}: off the grab bands",
                command()
            );
        }
    }

    fn sheet_top() -> f32 {
        shell::TOPBAR_HEIGHT + shell::TOPBAR_BOTTOM_BAND
    }

    /// The sidebar's `Surface` fill — it starts at the window's left edge and
    /// runs the body's height — when the sidebar is drawn in this frame. The
    /// panel surface (the body's own fill) also starts at x=0 when the sidebar
    /// is collapsed, so the sidebar is the `Surface` fill that stops at the
    /// sidebar's width instead of reaching the window's right edge.
    fn sidebar_surface(
        commands: &[RenderCommand],
        app: &AppContext,
        size: Vector2F,
    ) -> Option<RectF> {
        let surface = app.theme.color(ColorToken::Surface);
        commands.iter().find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. }
                if *color == surface
                    && rect.min_x() == 0.0
                    && rect.height() > size.y / 2.0
                    && rect.max_x() < size.x - 1.0 =>
            {
                Some(*rect)
            }
            _ => None,
        })
    }

    /// The left edge of the first pane card: the panel surface's inset plus the
    /// card's own gutter from the body's left edge.
    fn first_card_left() -> f32 {
        crate::ui::panes::PANE_AREA_INSET + crate::ui::panes::PANE_INSET
    }

    /// The dashboard's roster search field names it; the dashboard is the
    /// secondary surface this lane hides at the narrow breakpoint.
    fn dashboard_drawn(commands: &[RenderCommand]) -> bool {
        commands.iter().any(|command| {
            matches!(
                command,
                RenderCommand::DrawText { text, .. }
                    if text.contains("Search agents by name")
            )
        })
    }

    fn click(root: &mut Box<dyn Element>, app: &AppContext, at: Vector2F) {
        let mut ctx = EventContext;
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
            root.dispatch_event(&event, &mut ctx, app);
        }
    }

    /// The sidebar auto-collapses below its responsive breakpoint and the
    /// preference restores it when the window grows: with the sidebar gone the
    /// panes take the frame from the panel inset.
    #[test]
    fn the_sidebar_auto_collapses_below_the_breakpoint_and_restores() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let wide = vec2f(1600.0, 900.0);
        let narrow = vec2f(900.0, 900.0);

        let commands = render_element(&mut root, wide, &app);
        assert!(
            sidebar_surface(&commands, &app, wide).is_some(),
            "a wide window shows the sidebar"
        );
        assert!(!state.borrow().responsive.sidebar_collapsed);

        let commands = render_element(&mut root, narrow, &app);
        assert!(
            sidebar_surface(&commands, &app, narrow).is_none(),
            "below the breakpoint the sidebar leaves the view"
        );
        assert!(state.borrow().responsive.sidebar_collapsed);
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 1, "the active pane keeps the frame: {frames:?}");
        assert_eq!(
            frames[0].min_x(),
            first_card_left(),
            "with the sidebar collapsed the panes start at the panel inset: {:?}",
            frames[0]
        );

        // Growing the window restores the sidebar from the preference.
        let commands = render_element(&mut root, wide, &app);
        assert!(
            sidebar_surface(&commands, &app, wide).is_some(),
            "growing back restores the sidebar"
        );
        assert!(!state.borrow().responsive.sidebar_collapsed);
        assert!(state.borrow().sidebar_visible, "the preference was never rewritten");
    }

    /// A wide pane tree collapses the sidebar earlier than a single pane: the
    /// breakpoint is the measured pane minimum plus the sidebar, not only the
    /// DSH constant.
    #[test]
    fn a_wide_pane_tree_collapses_the_sidebar_earlier_than_the_floor() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        install_two_panes(&state, 0.5);
        let breakpoint = state.borrow().responsive_breakpoint();
        assert!(
            breakpoint > crate::state::SIDEBAR_AUTO_COLLAPSE,
            "two panes need more than the DSH floor: {breakpoint}"
        );

        let kept = render_element(&mut root, vec2f(1100.0, 900.0), &app);
        assert!(
            !state.borrow().responsive.sidebar_collapsed,
            "1100 still fits two panes beside the sidebar"
        );
        assert!(sidebar_surface(&kept, &app, vec2f(1100.0, 900.0)).is_some());

        let collapsed = render_element(&mut root, vec2f(1020.0, 900.0), &app);
        assert!(
            state.borrow().responsive.sidebar_collapsed,
            "1000 is below the measured breakpoint {breakpoint}"
        );
        assert!(sidebar_surface(&collapsed, &app, vec2f(1020.0, 900.0)).is_none());
        assert_eq!(pane_frames(&state).len(), 2, "both panes stay visible");
    }

    /// Below the second breakpoint the secondary surfaces leave the view: the
    /// dashboard (which would otherwise replace the panes) and the agent panel
    /// sheet are hidden, and the panes with the active one stay.
    #[test]
    fn the_secondary_panels_leave_the_view_below_the_second_breakpoint() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        install_two_panes(&state, 0.5);
        {
            let mut s = state.borrow_mut();
            s.agent_dashboard.open = true;
            s.right_sidebar_open = true;
        }
        let wide = vec2f(1600.0, 900.0);
        let commands = render_element(&mut root, wide, &app);
        assert!(
            dashboard_drawn(&commands),
            "a wide window keeps the dashboard"
        );
        assert!(!state.borrow().responsive.secondary_hidden);

        let narrow = vec2f(760.0, 900.0);
        let commands = render_element(&mut root, narrow, &app);
        assert!(
            !dashboard_drawn(&commands),
            "below 768 the dashboard leaves the view"
        );
        assert!(state.borrow().responsive.secondary_hidden);
        assert!(!state.borrow().responsive.focused_only, "two panes still fit 760");
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 2, "the panes and the active one stay: {frames:?}");
        assert!(
            frames.iter().any(|f| {
                let point = vec2f(400.0, 500.0);
                point.x >= f.min_x()
                    && point.x <= f.max_x()
                    && point.y >= f.min_y()
                    && point.y <= f.max_y()
            }),
            "the active pane is visible"
        );

        // Growing restores the secondary surfaces in reverse order: the
        // dashboard comes back with the width.
        let commands = render_element(&mut root, wide, &app);
        assert!(
            dashboard_drawn(&commands),
            "growing restores the dashboard"
        );
        assert!(!state.borrow().responsive.secondary_hidden);
    }

    /// The last responsive step: when even the collapsed window cannot give
    /// every pane its minimum width, only the active pane is drawn (over the
    /// whole pane area), and the rest restore when the window grows.
    #[test]
    fn only_the_active_pane_is_drawn_below_the_panes_minimum() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        install_two_panes(&state, 0.5);
        let active = state.borrow().active_pane_id;

        let size = vec2f(700.0, 900.0);
        let _ = render_element(&mut root, size, &app);
        assert!(
            state.borrow().responsive.focused_only,
            "700 cannot give two 360 pt panes their minimum"
        );
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 1, "only the active pane is drawn: {frames:?}");
        assert!(
            state.borrow().pane_bounds.borrow().contains_key(&active),
            "the active pane is the one kept"
        );
        assert_eq!(frames[0].min_x(), first_card_left());

        let _ = render_element(&mut root, vec2f(900.0, 900.0), &app);
        assert!(!state.borrow().responsive.focused_only);
        assert_eq!(pane_frames(&state).len(), 2, "both panes come back");
    }

    /// A manual toggle below the breakpoint re-expands the auto-collapsed
    /// sidebar; the next breakpoint crossing drops the override, so growing and
    /// shrinking again restores the auto behaviour.
    #[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
    #[test]
    fn a_narrow_sidebar_toggle_reexpands_until_the_window_grows() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let size = vec2f(900.0, 900.0);
        let commands = render_element(&mut root, size, &app);
        assert!(sidebar_surface(&commands, &app, size).is_none());
        let toggle = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    name, origin, size, ..
                } if name == "menu-01" => {
                    Some(vec2f(origin.x + size / 2.0, origin.y + size / 2.0))
                }
                _ => None,
            })
            .expect("the topbar draws the sidebar toggle");
        click(&mut root, &app, toggle);

        let commands = render_element(&mut root, size, &app);
        assert!(
            sidebar_surface(&commands, &app, size).is_some(),
            "a narrow toggle re-expands the auto-collapsed sidebar"
        );
        assert!(state.borrow().sidebar_visible, "the preference is untouched");
        assert!(state.borrow().sidebar_narrow_expanded);

        // Growing crosses the breakpoint: the override is dropped and the
        // preference (visible) keeps the sidebar.
        let _ = render_element(&mut root, vec2f(1600.0, 900.0), &app);
        assert!(!state.borrow().sidebar_narrow_expanded);
        assert!(!state.borrow().responsive.sidebar_collapsed);

        // Narrowing again crosses back: the sidebar auto-collapses again.
        let commands = render_element(&mut root, size, &app);
        assert!(sidebar_surface(&commands, &app, size).is_none());
        assert!(state.borrow().responsive.sidebar_collapsed);
    }

    /// A split drag stops at the pane minimum: the dragged pane keeps its 360 pt
    /// (the split floor includes the panes' frame gutters) and the neighbour
    /// keeps the rest of the pane area.
    #[test]
    fn a_split_drag_stops_at_the_pane_minimum_width() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        install_two_panes(&state, 0.5);
        let size = vec2f(900.0, 900.0);
        let _ = render_element(&mut root, size, &app);
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 2);
        let boundary = (frames[0].max_x() + frames[1].min_x()) / 2.0;
        let mid_y = (sheet_top() + size.y) / 2.0;
        let mut ctx = EventContext::default();
        assert!(root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(boundary, mid_y),
                button: 0,
            },
            &mut ctx,
            &app,
        ));
        assert_eq!(state.borrow().dragging_pane_id, Some(9));
        // The drag is live in the tree rebuilt after the press; move the
        // boundary to the window's left edge.
        let _ = render_element(&mut root, size, &app);
        assert!(root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: vec2f(4.0, mid_y),
            },
            &mut ctx,
            &app,
        ));
        let _ = render_element(&mut root, size, &app);
        let frames = pane_frames(&state);
        assert_eq!(frames.len(), 2);
        assert!(
            frames[0].width() >= crate::state::PANE_MIN_WIDTH - 0.5,
            "the dragged pane stops at its minimum, got {:?}",
            frames[0]
        );
        assert!(
            frames[1].width() > frames[0].width(),
            "the neighbour keeps the rest: {:?}",
            frames[1]
        );
        let _ = root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: vec2f(4.0, mid_y),
                button: 0,
            },
            &mut ctx,
            &app,
        );
    }

    /// The general topbar is gone: the sidebar's traffic-light row is the top
    /// edge and the panel header starts on the same line, with no tab strip or
    /// global bar above either.
    #[test]
    fn the_general_topbar_is_gone_and_the_panel_owns_the_top_edge() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let commands = render_element(&mut root, window(), &app);
        let new_conversation = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "New conversation" && origin.x < 250.0 =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the sidebar starts with New conversation");
        assert!(
            new_conversation.y < 60.0,
            "the sidebar owns the top edge, got {new_conversation:?}"
        );
        let title = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "New conversation" && origin.x > 250.0 =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the chrome row names the conversation");
        assert!(
            title.y < 40.0,
            "the panel header is at the window's top, got {title:?}"
        );
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, .. }
                    if text == "Goble" || text == "Workspaces"
            )),
            "no general-topbar/crumb text survives"
        );
        let _ = state;
    }

    /// The panel header's sidebar control is a real toggle now that the
    /// general topbar no longer carries one.
    #[test]
    fn the_panel_header_toggles_the_sidebar() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let commands = render_element(&mut root, window(), &app);
        let toggle = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { origin, name, size, .. }
                    if name == "left-panel-close" =>
                {
                    Some(*origin + vec2f(*size / 2.0, *size / 2.0))
                }
                _ => None,
            })
            .expect("the header draws the hide-sidebar toggle");
        assert!(state.borrow().sidebar_visible);
        let mut ctx = EventContext;
        for event in [
            DispatchedEvent::MouseDown {
                position: toggle,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: toggle,
                button: 0,
            },
        ] {
            let _ = root.dispatch_event(&event, &mut ctx, &app);
        }
        assert!(
            !state.borrow().sidebar_visible,
            "the header toggle hides the sidebar"
        );
    }

    /// The Overview control opens the popover with its real rows and counts.
    #[test]
    fn the_overview_control_opens_the_side_tab() {
        let app = AppContext::default();
        let (mut root, _state, _dir) = app_root();
        let commands = render_element(&mut root, window(), &app);
        let trigger = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { origin, name, size, .. } if name == "flat-list" => {
                    Some(*origin + vec2f(*size / 2.0, *size / 2.0))
                }
                _ => None,
            })
            .expect("the chrome row draws the Overview control");
        let mut ctx = EventContext;
        for event in [
            DispatchedEvent::MouseDown { position: trigger, button: 0 },
            DispatchedEvent::MouseUp { position: trigger, button: 0 },
        ] {
            let _ = root.dispatch_event(&event, &mut ctx, &app);
        }
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for expected in [
            "Overview",
            "Search or enter a URL",
            "Review",
            "Open File",
            "Terminal",
            "Extensions",
            "Subagents",
            "Background Tasks",
        ] {
            assert!(
                texts.iter().any(|text| *text == expected),
                "the Overview side tab draws {expected:?}: {texts:?}"
            );
        }
    }

    /// One continuous chrome row: the title sits on it and the sidebar starts
    /// below it; the right Overview dock lists real rows/actions.
    #[test]
    fn the_chrome_row_owns_the_title_and_the_overview_panel_lists_real_rows() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let commands = render_element(&mut root, window(), &app);
        let y = |needle: &str| {
            commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawText { text, origin, .. } if text == needle => Some(origin.y),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the frame draws {needle:?}"))
        };
        let title = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "New conversation" && origin.x > 250.0 =>
                {
                    Some(origin.y)
                }
                _ => None,
            })
            .expect("the chrome row names the conversation");
        assert!(title < crate::ui::chrome::CHROME_HEIGHT);
        state.borrow_mut().right_sidebar_open = true;
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for expected in [
            "Overview",
            "Search or enter a URL",
            "Review",
            "Open File",
            "Terminal",
            "Browser",
            "Subagents",
            "Background Tasks",
            "0",
        ] {
            assert!(
                texts.iter().any(|text| *text == expected),
                "the overview dock draws {expected:?}: {texts:?}"
            );
        }
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "fullscreen-enter"
            )),
            "the dock offers maximize"
        );
    }

    /// The panel's More menu carries the DSH actions with their shortcuts, and
    /// the Split right row really splits the focused pane.
    #[test]
    fn the_panel_more_menu_offers_split_replace_and_their_shortcuts() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let commands = render_element(&mut root, window(), &app);
        let more = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { origin, name, size, .. } if name == "ellipsis" => {
                    Some(*origin + vec2f(*size / 2.0, *size / 2.0))
                }
                _ => None,
            })
            .expect("the panel header draws its More control");
        let mut ctx = EventContext;
        for event in [
            DispatchedEvent::MouseDown { position: more, button: 0 },
            DispatchedEvent::MouseUp { position: more, button: 0 },
        ] {
            let _ = root.dispatch_event(&event, &mut ctx, &app);
        }
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for expected in [
            "Split right",
            "Split down",
            "Replace with new",
            "⌘ Space",
            "⌘ ⇧ D",
        ] {
            assert!(
                texts.iter().any(|text| *text == expected),
                "the More menu draws {expected:?}: {texts:?}"
            );
        }

        let before = state.borrow().spaces[state.borrow().active_space].root.leaf_count();
        let split = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "Split right" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the Split right row");
        for event in [
            DispatchedEvent::MouseDown {
                position: split + vec2f(6.0, 6.0),
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: split + vec2f(6.0, 6.0),
                button: 0,
            },
        ] {
            let _ = root.dispatch_event(&event, &mut ctx, &app);
        }
        let after = state.borrow().spaces[state.borrow().active_space].root.leaf_count();
        assert_eq!(after, before + 1, "Split right splits the focused pane");
    }

    /// The settings surface is an overlay: wide it floats with its rail; narrow
    /// it fills and offers the hamburger. Closing returns to the workspace.
    #[ignore = "shell-settings-2: retired topbar/settings-pane structure"]
    #[test]
    fn settings_is_a_wide_sheet_with_a_rail_and_a_narrow_sheet_with_a_hamburger() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.settings_overlay_open = true;
            s.settings_overlay_wide = true;
        }
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for expected in ["Settings", "General", "Friendly name", "Appearance"] {
            assert!(
                texts.iter().any(|text| *text == expected),
                "the wide sheet draws {expected:?}: {texts:?}"
            );
        }

        {
            let mut s = state.borrow_mut();
            s.settings_overlay_wide = false;
        }
        let commands = render_element(&mut root, vec2f(700.0, 900.0), &app);
        let menu = commands.iter().any(|command| matches!(
            command,
            RenderCommand::DrawIcon { name, .. } if name == "menu-01"
        ));
        assert!(menu, "the narrow sheet draws its hamburger");
        state.borrow_mut().settings_overlay_open = false;
        let commands = render_element(&mut root, vec2f(700.0, 900.0), &app);
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, .. } if *text == "Friendly name"
            )),
            "the closed overlay leaves the workspace"
        );
    }
}
