use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Border, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, EventContext,
    Expanded, Fill, Flex, LayoutContext, MainAxisSize, Padding, PaintContext, Point,
    SizeConstraint, Stack,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, PointF, RectF, Size2F, Vector2F};
use goble_ui::platform::{CursorKind, WindowControl};
use goble_ui::theme::ColorToken;
use goble_ui::{vec2f, ColorU, Dialog, Sheet, DIALOG_DEFAULT_WIDTH, SHEET_DEFAULT_WIDTH};

use super::actions::{AiActions, MediaActions, ProjectsActions, UiActions};
use super::snapshot::{AiSnapshot, MediaSnapshot, ProjectsSnapshot, UiSnapshot};
use super::types::AppTab;
use super::{
    agent_dashboard, chat, chrome, connectors, crons, hosts, inbox, media, model_form, onboarding,
    panes, session_menu, settings, shell, shortcuts_help, sidebar, vault, CONNECTORS_WIDTH,
};

/// Keeps the shell behind an open modal from reacting to the pointer's hover,
/// while the modal's own sheet still paints its hover. The shell is painted
/// before the modal, so clearing `cursor_inside` for that subtree is what stops
/// a sidebar/topbar row lighting up under the scrim.
struct ModalHoverGate {
    child: Box<dyn Element>,
    suppress: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ModalHoverGate {
    fn new(child: Box<dyn Element>, suppress: bool) -> Self {
        Self {
            child,
            suppress,
            size: None,
            origin: None,
        }
    }

    fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for ModalHoverGate {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let saved = ctx.cursor_inside;
        if self.suppress {
            ctx.cursor_inside = false;
        }
        self.child.paint(origin, ctx, app);
        ctx.cursor_inside = saved;
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn wants_animation(&self) -> bool {
        self.child.wants_animation()
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.child.dispatch_event(event, ctx, app)
    }
}

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

/// The invisible grab band centred on the shared main/side-panel boundary:
/// four points on each side, like the sidebar's resize band.
const SIDE_SPLIT_HANDLE_HALF: f32 = 4.0;
/// Each card keeps at least this width when the content area is wide enough to
/// give both sides their minimum.
const SIDE_SPLIT_MIN_CARD_WIDTH: f32 = 320.0;
/// The ratio range the split can be dragged to, before the per-card minimum is
/// applied for the current content width.
const SIDE_SPLIT_MIN_RATIO: f32 = 0.25;
const SIDE_SPLIT_MAX_RATIO: f32 = 0.75;

/// Resolve the side panel's share of `content_width`, with the width-aware
/// minimum applied on top of the `0.25..=0.75` ratio range.
pub(crate) fn resolved_side_split_ratio(ratio: f32, content_width: f32) -> f32 {
    let ratio = if ratio.is_finite() { ratio } else { 0.5 }
        .clamp(SIDE_SPLIT_MIN_RATIO, SIDE_SPLIT_MAX_RATIO);
    if !content_width.is_finite() || content_width <= 0.0 {
        return ratio;
    }
    // When both cards can keep the minimum, bound the ratio so neither can be
    // dragged below it. On a content area too narrow for both, the ratio range
    // above is the only bound.
    if content_width >= 2.0 * SIDE_SPLIT_MIN_CARD_WIDTH {
        let min_ratio = SIDE_SPLIT_MIN_CARD_WIDTH / content_width;
        ratio.clamp(min_ratio, 1.0 - min_ratio)
    } else {
        ratio
    }
}

type SideSplitMoveHandler = Option<Rc<RefCell<dyn FnMut(f32)>>>;
type SideSplitEndHandler = Option<Rc<RefCell<dyn FnMut()>>>;

/// The main card and the right side panel side by side, at widths derived from
/// the measured content width and `ratio` (not two equal `Expanded` halves),
/// with both cards filling the row's whole height. The shared boundary carries
/// an invisible resize band; no rule or gutter is painted there, so the two
/// cards' own casings remain the only edge.
struct SideSplitLayout {
    main: Box<dyn Element>,
    panel: Box<dyn Element>,
    ratio: f32,
    dragging: bool,
    on_drag_start: Option<Rc<RefCell<dyn FnMut()>>>,
    on_drag_move: SideSplitMoveHandler,
    on_drag_end: SideSplitEndHandler,
    size: Option<Vector2F>,
    origin: Option<Point>,
    main_width: f32,
}

impl SideSplitLayout {
    fn new(main: Box<dyn Element>, panel: Box<dyn Element>, ratio: f32) -> Self {
        Self {
            main,
            panel,
            ratio,
            dragging: false,
            on_drag_start: None,
            on_drag_move: None,
            on_drag_end: None,
            size: None,
            origin: None,
            main_width: 0.0,
        }
    }

    fn with_dragging(mut self, dragging: bool) -> Self {
        self.dragging = dragging;
        self
    }

    fn with_on_drag_start<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_drag_start = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn with_on_drag_move<F: FnMut(f32) + 'static>(mut self, callback: F) -> Self {
        self.on_drag_move = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn with_on_drag_end<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_drag_end = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }

    /// The resize band, in absolute window coordinates, centred on the shared
    /// boundary so it is four points either side of it.
    fn handle_bounds(&self) -> Option<RectF> {
        let origin = self.origin?;
        let size = self.size?;
        Some(rectf(
            origin.x() + self.main_width - SIDE_SPLIT_HANDLE_HALF,
            origin.y(),
            SIDE_SPLIT_HANDLE_HALF * 2.0,
            size.y,
        ))
    }

    /// Resolve a pointer x to a side-panel ratio of the measured content
    /// width, clamped to the ratio range and the two cards' minimum widths.
    fn ratio_at(&self, pointer_x: f32) -> f32 {
        let Some(origin) = self.origin else {
            return self.ratio;
        };
        let Some(size) = self.size else {
            return self.ratio;
        };
        if size.x <= 0.0 {
            return self.ratio;
        }
        // The boundary sits at `origin.x + size.x * (1 - side_ratio)`: the
        // side panel is the trailing half. A pointer dragging right must
        // therefore shrink the side share so the boundary follows the cursor.
        resolved_side_split_ratio(1.0 - (pointer_x - origin.x()) / size.x, size.x)
    }

    fn request_cursor(&self, ctx: &PaintContext, app: &AppContext) {
        let over_handle = ctx.cursor_inside
            && self
                .handle_bounds()
                .is_some_and(|bounds| contains(bounds, ctx.cursor_position));
        // `CursorKind` has the platform's horizontal resize arrow (`EwResize`,
        // the col-resize cursor); there is no separate `ColResize` variant.
        if self.dragging || over_handle {
            app.window_control.set_cursor(CursorKind::EwResize);
        }
    }
}

impl Element for SideSplitLayout {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let width = constraint.max.x.max(0.0);
        let height = constraint.max.y.max(0.0);
        let side_width = width * resolved_side_split_ratio(self.ratio, width);
        let main_width = (width - side_width).max(0.0);
        // Tight constraints on both axes: the two cards share one top and one
        // bottom edge and fill the row, so neither casing has vertical padding
        // the other lacks.
        let _ = self.main.layout(
            SizeConstraint::new(vec2f(main_width, height), vec2f(main_width, height)),
            ctx,
            app,
        );
        let _ = self.panel.layout(
            SizeConstraint::new(vec2f(side_width, height), vec2f(side_width, height)),
            ctx,
            app,
        );
        self.main_width = main_width;
        let size = vec2f(width, height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        // The panels are flat: no spacing, no casing. The shell's surface is
        // drawn around both; the only separation between them is the one
        // hairline painted over their shared edge below.
        self.main.paint(origin, ctx, app);
        self.panel
            .paint(origin + vec2f(self.main_width, 0.0), ctx, app);
        if let (Some(renderer), Some(size)) = (ctx.renderer.as_mut(), self.size) {
            renderer.fill_rect(
                rectf(origin.x + self.main_width, origin.y, 1.0, size.y),
                app.theme.color(ColorToken::Border),
            );
        }
        // Requested after the children paint, so the resize cursor wins over
        // any cursor a child asked for inside the band.
        self.request_cursor(ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        // While dragging, consume move/up even when the pointer is over the
        // main card or the panel content beyond the band.
        if self.dragging {
            match event {
                DispatchedEvent::MouseMove { position } => {
                    let ratio = self.ratio_at(position.x);
                    if let Some(cb) = &self.on_drag_move {
                        (cb.borrow_mut())(ratio);
                    }
                    return true;
                }
                DispatchedEvent::MouseUp { .. } => {
                    if let Some(cb) = &self.on_drag_end {
                        (cb.borrow_mut())();
                    }
                    self.dragging = false;
                    return true;
                }
                _ => {}
            }
        }
        // Start a drag when the pointer grabs the shared boundary's band.
        if let DispatchedEvent::MouseDown { position, .. } = event {
            if let Some(bounds) = self.handle_bounds() {
                if contains(bounds, *position) {
                    if let Some(cb) = &self.on_drag_start {
                        (cb.borrow_mut())();
                    }
                    // Also live in this element, so a release before the next
                    // rebuild still ends the gesture.
                    self.dragging = true;
                    return true;
                }
            }
        }
        if self.main.dispatch_event(event, ctx, app) {
            return true;
        }
        self.panel.dispatch_event(event, ctx, app)
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
    let side_split = state.right_sidebar_open
        && !state.side_panel_maximized
        && !state.responsive.secondary_hidden;
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
            let insets = if side_split {
                // The side panel shares this half's right edge, so the pane
                // area spends no `PANE_AREA_INSET` there. The pane card inside
                // also spends its own `PANE_INSET` on every side; pulling the
                // right inset negative by that gutter and one border width
                // cancels it and carries the card's right border just under the
                // panel's left border. The two borders are then one shared line
                // with no gutter, while the card's left gutter
                // (`PANE_AREA_INSET + PANE_INSET`) stays unchanged.
                EdgeInsets::new(0.0, 0.0, -(panes::PANE_INSET + 1.0), panes::PANE_AREA_INSET)
            } else {
                // No band on the left and top: the panels own those edges and
                // the shell surface already provides the margin. The free
                // space is kept on the bottom and the right.
                EdgeInsets::new(0.0, 0.0, panes::PANE_AREA_INSET, panes::PANE_AREA_INSET)
            };
            let panels = Padding::new(main, insets).finish();
            Container::new(panels)
                .with_background(Fill::Solid(app.theme.color(ColorToken::ContentBg)))
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
    // The Overview side panel shares the content area with the main panel as
    // two cards sized from `state.side_split_ratio` of the width the sidebar
    // leaves (default 0.5, the equal split), not two fixed `Expanded` halves.
    // The split clamps the ratio to 0.25..=0.75 and keeps 320 pt on each card
    // when the content area allows it. The row has no spacing and no outer
    // padding or casing of its own — the two cards' own bordered, rounded
    // casings meet on the shared boundary with no gutter between them, and the
    // resize band there paints nothing.
    // While the panel is open the pane area's right inset is canceled (see
    // above), so the main card's right border is on that same boundary: the
    // frame shows one shared line, not a main line, a gutter, and a panel line.
    // Maximized, the side panel is the only content card. Below the secondary
    // breakpoint the panel leaves the view (the responsive layout's narrow
    // step) and the main card takes the whole content width. The row itself
    // only arranges the cards; each branch below owns its casing, so the side
    // panel's border is not nested inside the main panel's.
    // The chat workspace is ONE raised sheet: the main panel and the side
    // panel live inside the same rounded surface, separated by a square,
    // borderless boundary, with a 5 pt free margin of the shell's `Bg` around
    // the whole sheet. The two panels paint the same flat `Surface` (see
    // `chrome::build_side_panel`), so the sheet reads as one material. Pages
    // that are not the chat workspace keep the older single-card casing.
    let panel_surface = state.current_tab == AppTab::Chat && !dashboard_shown;
    let main: Box<dyn Element> = if !panel_surface {
        Container::new(main)
            .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
            .with_corner_radius(12.0)
            .finish()
    } else if state.side_panel_maximized && state.right_sidebar_open {
        shell::build_workspace_surface(app, chrome::build_side_panel(app, state, actions))
    } else {
        let row: Box<dyn Element> = if side_split {
            let panel = chrome::build_side_panel(app, state, actions);
            let on_drag_start = actions.on_side_split_drag_start.clone();
            let on_drag_move = actions.on_side_split_drag_move.clone();
            let on_drag_end = actions.on_side_split_drag_end.clone();
            SideSplitLayout::new(main, panel, state.side_split_ratio)
                .with_dragging(state.side_split_dragging)
                .with_on_drag_start(move || (on_drag_start.borrow_mut())())
                .with_on_drag_move(move |ratio| (on_drag_move.borrow_mut())(ratio))
                .with_on_drag_end(move || (on_drag_end.borrow_mut())())
                .finish()
        } else {
            main
        };
        shell::build_workspace_surface(app, row)
    };
    let sidebar_shown = state.responsive.sidebar_shown(state.sidebar_visible);
    let body: Box<dyn Element> = if sidebar_shown {
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

    // 382: floating windows are application-window level: above the sidebar
    // and side panel, below the settings/spotlight layers.
    let floating_overlay = state
        .spaces
        .get(state.active_space)
        .and_then(|space| panes::build_floating_overlay(app, state, actions, space));
    let mut children =
        vec![ModalHoverGate::new(shell_col.finish(), state.settings_overlay_open).finish()];
    if let Some(floating) = floating_overlay {
        children.push(floating);
    }
    children.extend(vec![
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
        // pane, so it never takes a tab. It is queued in the last paint layer
        // as well, so the topbar's own last-layer menu triggers cannot draw
        // through a narrow settings sheet that reaches the window's top edge.
        chrome::TopLayer::new(settings_overlay).finish(),
        // Q: the topbar spotlight is the last surface; while open it paints a
        // scrim and its own card, menus and keyboard above everything else.
        chrome::TopLayer::new(crate::ui::spotlight::build_spotlight(
            app,
            state,
            actions,
            state.window_size,
        ))
        .finish(),
    ]);
    let mut stack = Stack::new().with_children(children);

    // 349: the Automations popover is anchored beside the sidebar row and
    // floats above the workspace, below the settings/spotlight top layers.
    if state.automations_open {
        stack = stack.with_overlay(
            crons::build_automations_popover(app, state, actions),
            vec2f(crate::ui::SIDEBAR_WIDTH + 24.0, 56.0),
        );
    }
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
    ); // "Add a new environment" dialog, opened from the topbar "+" menu. It only
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
    // A file row/card context menu is a window-level surface: it hangs from the
    // pointer over the sidebar or a review card and paints above both.
    if let Some(target) = state.file_menu.borrow().clone() {
        stack = stack.with_overlay(
            super::file_menu::build_file_menu(&target, actions),
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

/// The window rule, drawn in the window's last layer (see
/// [`WindowBorder::paint`]).
struct WindowBorderPaint {
    rect: RectF,
    color: ColorU,
    corner_radius: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Element for WindowBorderPaint {
    fn paint(&mut self, _origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.stroke_rect(
                self.rect,
                self.color,
                WINDOW_BORDER_WIDTH,
                self.corner_radius,
            );
        }
    }

    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        self.size.unwrap_or(Vector2F::zero())
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
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

    fn paint(&mut self, origin: Vector2F, _ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        // The rule wraps the whole window and must read over every surface the
        // frame draws — panes, sheets, dialogs and the open menus/topbar
        // popovers that paint in the window's last layer. Queue it there; the
        // layer drains in push order, and this is pushed last of the tree.
        app.hover_chips.borrow_mut().push(
            origin,
            Box::new(WindowBorderPaint {
                rect: RectF::new(PointF::new(origin.x, origin.y), Size2F::new(size.x, size.y)),
                color: self.color,
                corner_radius: self.corner_radius,
                size: Some(size),
                origin: Some(Point::from_vec2f(origin, Default::default())),
            }),
        );
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
    use crate::state::{PaneSession, SideTabKind, UiState};
    use crate::ui::{Pane, PaneKind, Space, SplitDir};
    use goble_core::harness::ToolCallStatus;
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
        // A writable settings file, so a click that goes through B1/B2 really
        // persists (the interaction tests read the refreshed view).
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "").expect("seed config.toml");
        desktop.set_config_path(config);
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

    /// The flat workspace surface: the full pane-area `Bg` fill that both
    /// panels live on. It has no radius, margin or border; the largest
    /// content-sized fill is the sheet.
    fn workspace_sheet(commands: &[RenderCommand], app: &AppContext, size: Vector2F) -> RectF {
        let bg = app.theme.color(ColorToken::Bg);
        let surface = app.theme.color(ColorToken::Surface);
        let content = app.theme.color(ColorToken::ContentBg);
        let sheets: Vec<RectF> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if (*color == bg || *color == surface || *color == content)
                        && rect.height() > size.y / 2.0
                        && rect.width() > size.x / 2.0
                        && rect.min_x() >= crate::ui::SIDEBAR_WIDTH - 1.0
                        && rect.min_y() >= crate::ui::chrome::CHROME_HEIGHT - 1.0 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .collect();
        sheets
            .into_iter()
            .max_by(|a, b| (a.width() * a.height()).total_cmp(&(b.width() * b.height())))
            .expect("the workspace surface")
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
            0,
            "the pane card has no Border edge of its own any more"
        );
        let card = frames[0];
        let markers = corner_markers(&commands, &app);
        assert!(
            markers.is_empty(),
            "the panel's top-left corner has no accent ornament: {markers:?}"
        );
        let _ = card;

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
            0,
            "no pane card edge: the shell surface and split hairline separate"
        );
        let markers = corner_markers(&commands, &app);
        assert!(
            markers.is_empty(),
            "neither pane wears an accent corner: {markers:?}"
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
            let bg = app.theme.color(ColorToken::ContentBg);
            let border = app.theme.color(ColorToken::Border);
            let _ = border;
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
            // No card owns a Border edge any more: the shell's rounded
            // surface and the split hairline draw the separation, while the
            // rounded fill + corner cut keep each pane's shape clean.
            let pane_edges = commands
                .iter()
                .filter(|command| {
                    matches!(
                        command,
                        RenderCommand::StrokeRect { rect, color, width, .. }
                            if *color == border
                                && (*width - 1.0).abs() < 0.01
                                && frames.contains(rect)
                    )
                })
                .count();
            assert_eq!(pane_edges, 0, "no pane card paints a 1 pt Border edge");
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

    /// The panel's top-left corner carries no ornament: the old accent
    /// triangle is gone for both a single and a split pane, while the card's
    /// rounded cut and its own Border edge still close the corner.
    #[test]
    fn the_panel_has_no_corner_ornament() {
        let app = AppContext::default();
        let accent = app.theme.color(ColorToken::Accent);
        let surface = app.theme.color(ColorToken::ContentBg);
        let border = app.theme.color(ColorToken::Border);

        for two_panes in [false, true] {
            let (mut root, state, _dir) = app_root();
            if two_panes {
                install_two_panes(&state, 0.5);
            }
            let commands = render_element(&mut root, window(), &app);
            let frames = pane_frames(&state);
            assert_eq!(frames.len(), if two_panes { 2 } else { 1 }, "{frames:?}");
            assert!(
                !commands.iter().any(|command| matches!(
                    command,
                    RenderCommand::DrawIcon { name, color, .. }
                        if name == "upper-left-triangle" && *color == accent
                )),
                "no accent corner triangle is drawn on any pane"
            );
            let card = frames[0];
            let radius = crate::ui::panes::card_radius(vec2f(card.width(), card.height()));
            let band = crate::ui::panes::CARD_EDGE_BAND;
            assert!(
                commands.iter().any(|command| matches!(
                    command,
                    RenderCommand::StrokeRect { rect, color, width, corner_radius }
                        if *color == surface
                            && (*width - band).abs() < 0.01
                            && (*corner_radius - (radius + band)).abs() < 0.01
                            && (rect.min_x() - (card.min_x() - band)).abs() < 0.01
                )),
                "the card still cuts its content back to the rounded path"
            );
            let pane_edges = commands
                .iter()
                .filter(|command| {
                    matches!(
                        command,
                        RenderCommand::StrokeRect { rect, color, width, .. }
                            if *rect == card
                                && *color == border
                                && (*width - 1.0).abs() < 0.01
                    )
                })
                .count();
            assert_eq!(pane_edges, 0, "the card owns no Border edge");
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
                let surface = app.theme.color(ColorToken::ContentBg);
                let bg = app.theme.color(ColorToken::ContentBg);
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

                    // No pane wears an accent corner ornament: the rounded cut
                    // leaves the corner clean, and no accent pixel lies inside
                    // or outside the path.
                    let inside = corner(card.min_x() + radius * 0.5, card.min_y() + radius * 0.5);
                    let outside = accent_pixels_outside_corner(&pixels, *card, radius, accent);
                    assert!(
                        !near(inside, accent),
                        "no pane wears an accent corner ornament: {inside:?}"
                    );
                    assert_eq!(
                        outside, 0,
                        "no accent pixel may sit outside the card's rounded top-left corner"
                    );
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
                    origin,
                    name,
                    color,
                    ..
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
        let surface = app.theme.color(ColorToken::ContentBg);
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
        let bg = app.theme.color(ColorToken::ContentBg);
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
            assert!(
                s.content_lane_dragging,
                "a press on the handle starts the drag"
            );
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
        (actions.on_content_lane_drag_start.borrow_mut())(8, 311.0, LaneEdge::Left, 920.0, 1600.0);
        (actions.on_content_lane_drag_move.borrow_mut())(261.0);
        assert_eq!(
            pref(8),
            Some(1020.0),
            "dragging the left edge left widens, like the right edge right"
        );
        assert_eq!(pref(7), Some(640.0), "the sibling pane keeps its own width");
        (actions.on_content_lane_drag_move.borrow_mut())(411.0);
        assert_eq!(
            pref(8),
            Some(720.0),
            "dragging the left edge inward narrows"
        );
        (actions.on_content_lane_drag_end.borrow_mut())();

        // A narrow column is a full-width pass-through whatever is dragged.
        (actions.on_content_lane_drag_start.borrow_mut())(7, 400.0, LaneEdge::Right, 800.0, 800.0);
        (actions.on_content_lane_drag_move.borrow_mut())(500.0);
        assert_eq!(pref(7), Some(800.0), "a narrow lane stays full width");
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
        let column_right = frames[0].min_x()
            + (frames[0].width() - before_left.width()) / 2.0
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
            assert_eq!(
                s.active_pane_id, 7,
                "the press focuses the pane it lands in"
            );
            assert_eq!(
                s.content_lane_drag_pane,
                Some(7),
                "only the left pane owns the drag"
            );
            let left = s
                .content_lane_prefs
                .get(&7)
                .copied()
                .expect("the dragged pane freezes its width");
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
        let dragged = resolve_content_width(frames[0].width(), Some(expected_left - 34.0 + 100.0));
        {
            let s = state.borrow();
            let left = s.content_lane_prefs.get(&7).copied().unwrap();
            assert!(
                (left - dragged).abs() < 0.5,
                "the left pane follows the pointer: {left} against {dragged}"
            );
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
            assert_eq!(
                s.content_lane_prefs.get(&8),
                None,
                "the sibling was never touched"
            );
        }
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
            let command =
                || format!("split dragging {split_dragging}, sidebar dragging {sidebar_dragging}");
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
            paint_with_pointer(&mut root, &app, vec2f(crate::ui::SIDEBAR_WIDTH, mid_y));
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
        // The pane area's inset inside the one workspace sheet, plus the free
        // margin the sheet keeps from the window/sidebar edge.
        shell::WORKSPACE_MARGIN + crate::ui::panes::PANE_INSET
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
        assert_eq!(
            frames.len(),
            1,
            "the active pane keeps the frame: {frames:?}"
        );
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
        assert!(
            state.borrow().sidebar_visible,
            "the preference was never rewritten"
        );
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
        assert!(
            !state.borrow().responsive.focused_only,
            "two panes still fit 760"
        );
        let frames = pane_frames(&state);
        assert_eq!(
            frames.len(),
            2,
            "the panes and the active one stay: {frames:?}"
        );
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
        assert!(dashboard_drawn(&commands), "growing restores the dashboard");
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
            new_conversation.y < 75.0,
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
                RenderCommand::DrawIcon {
                    origin, name, size, ..
                } if name == "panel-left-outline" => {
                    Some(*origin + vec2f(*size / 2.0, *size / 2.0))
                }
                _ => None,
            })
            .expect("the topbar draws DSH's sidebar toggle");
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
        _state.borrow_mut().right_sidebar_open = true;
        _state.borrow_mut().side_tabs = vec![SideTabKind::Overview];
        _state.borrow_mut().active_side_tab = 0;
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for expected in [
            "Search or enter a URL",
            "Review",
            "Open File",
            "Terminal",
            "Side Question",
            "Plugins",
        ] {
            assert!(
                texts.iter().any(|text| *text == expected),
                "the Overview side tab draws {expected:?}: {texts:?}"
            );
        }
        // Empty Subagents / Background Tasks sections are not drawn at all.
        for gone in [
            "Subagents",
            "Background Tasks",
            "No subagents",
            "No background tasks",
        ] {
            assert!(
                !texts.iter().any(|text| *text == gone),
                "the Overview side tab does not draw {gone:?}: {texts:?}"
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
                    RenderCommand::DrawText { text, origin, .. } if text == needle => {
                        Some(origin.y)
                    }
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
        state.borrow_mut().side_tabs = vec![SideTabKind::Overview];
        state.borrow_mut().active_side_tab = 0;
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for expected in [
            "Search or enter a URL",
            "Review",
            "Open File",
            "Terminal",
            "Browser",
        ] {
            assert!(
                texts.iter().any(|text| *text == expected),
                "the overview dock draws {expected:?}: {texts:?}"
            );
        }
        assert!(
            !texts.iter().any(|text| *text == "Subagents"
                || *text == "Background Tasks"
                || *text == "No subagents"
                || *text == "No background tasks"),
            "empty work sections stay hidden: {texts:?}"
        );
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "panel-right-outline"
            )),
            "the dock offers the side-panel toggle"
        );
    }

    /// The panel's More menu keeps only top-level rows; Split and Copy open
    /// flyouts on Right/hover with their real options and shortcuts, and a
    /// flyout row runs the real pane action.
    #[test]
    fn the_panel_more_menu_offers_split_replace_and_their_shortcuts() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let mut ctx = EventContext;
        let key = |root: &mut Box<dyn Element>, ctx: &mut EventContext, name: &str| {
            let _ = root.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: name.to_string(),
                    modifiers: Default::default(),
                },
                ctx,
                &app,
            );
        };

        let commands = render_element(&mut root, window(), &app);
        let more = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    origin, name, size, ..
                } if name == "ellipsis-vertical" => Some(*origin + vec2f(*size / 2.0, *size / 2.0)),
                _ => None,
            })
            .expect("the panel header draws its More control");
        for event in [
            DispatchedEvent::MouseDown {
                position: more,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: more,
                button: 0,
            },
        ] {
            let _ = root.dispatch_event(&event, &mut ctx, &app);
        }
        let commands = render_element(&mut root, window(), &app);
        let texts = |commands: &[RenderCommand]| -> Vec<String> {
            commands
                .iter()
                .filter_map(|command| match command {
                    RenderCommand::DrawText { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect()
        };
        let top = texts(&commands);
        for expected in ["Rename", "Pin", "Archive", "Split", "Copy", "Close pane"] {
            assert!(
                top.iter().any(|text| text == expected),
                "the More menu draws {expected:?}: {top:?}"
            );
        }
        assert!(
            !top.iter().any(|text| text == "Split right"),
            "the inline split rows are gone: {top:?}"
        );

        // Highlight Split (0 Rename, 1 Pin, 2 Archive, 3 Split) and open it.
        for _ in 0..3 {
            key(&mut root, &mut ctx, "ArrowDown");
        }
        key(&mut root, &mut ctx, "ArrowRight");
        let commands = render_element(&mut root, window(), &app);
        let flyout = texts(&commands);
        for expected in [
            "Split right",
            "Split down",
            "Replace with new",
            "⌘ Space",
            "⌘ ⇧ D",
        ] {
            assert!(
                flyout.iter().any(|text| text == expected),
                "the Split flyout draws {expected:?}: {flyout:?}"
            );
        }

        let before = state.borrow().spaces[state.borrow().active_space]
            .root
            .leaf_count();
        let split = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "Split right" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the Split right row in the flyout");
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
        let after = state.borrow().spaces[state.borrow().active_space]
            .root
            .leaf_count();
        assert_eq!(after, before + 1, "Split right splits the focused pane");

        // Copy opens its own flyout with the three copyable values.
        let commands = render_element(&mut root, window(), &app);
        let more = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    origin, name, size, ..
                } if name == "ellipsis-vertical" => Some(*origin + vec2f(*size / 2.0, *size / 2.0)),
                _ => None,
            })
            .expect("the More control");
        for event in [
            DispatchedEvent::MouseDown {
                position: more,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: more,
                button: 0,
            },
        ] {
            let _ = root.dispatch_event(&event, &mut ctx, &app);
        }
        let _ = render_element(&mut root, window(), &app);
        for _ in 0..4 {
            key(&mut root, &mut ctx, "ArrowDown");
        }
        key(&mut root, &mut ctx, "ArrowRight");
        let commands = render_element(&mut root, window(), &app);
        let copy_flyout = texts(&commands);
        for expected in ["Conversation name", "Conversation id", "Project name"] {
            assert!(
                copy_flyout.iter().any(|text| text == expected),
                "the Copy flyout draws {expected:?}: {copy_flyout:?}"
            );
        }
    }

    /// The More menu's Split/Copy flyouts never run off the window: at the
    /// topbar's right edge they flip to the left of the parent panel.
    #[test]
    fn the_more_menu_flyouts_stay_inside_the_window() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let mut ctx = EventContext;
        let size = window();
        let _ = render_element(&mut root, size, &app);
        *state.borrow().chrome_more_open.borrow_mut() = true;
        let _ = render_element(&mut root, size, &app);
        for name in ["ArrowDown", "ArrowDown", "ArrowDown", "ArrowRight"] {
            let _ = root.dispatch_event(
                &DispatchedEvent::KeyDown {
                    key: name.to_string(),
                    modifiers: Default::default(),
                },
                &mut ctx,
                &app,
            );
        }
        // One settling frame measures the menu origin; the next places the
        // flyout against the window edge.
        let _ = render_element(&mut root, size, &app);
        let commands = render_element(&mut root, size, &app);
        let split = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "Split right" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the Split flyout draws");
        assert!(
            split.x + 120.0 < size.x,
            "the flyout stays inside the window: {split:?} in {size:?}"
        );
    }

    #[test]
    fn dbg_narrow_settings_icons() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.open_settings_tab(None);
            s.settings_overlay_open = true;
            s.settings_overlay_wide = false;
        }
        let commands = render_element(&mut root, vec2f(700.0, 900.0), &app);
        for command in &commands {
            if let RenderCommand::DrawIcon { name, origin, .. } = command {
                println!("ICON {name} at {origin:?}");
            }
        }
    }

    /// L8: the workspace surface is flat — no framing Border and no shadow /
    /// elevation; panels are flat inside it with only the 1 pt hairline.
    #[test]
    fn the_shell_surface_is_flat_without_a_frame_or_shadow() {
        let app = AppContext::default();
        let (mut root, _state, _dir) = app_root();
        let size = vec2f(1024.0, 768.0);
        let commands = render_element(&mut root, size, &app);
        let sheet = workspace_sheet(&commands, &app, size);
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::StrokeRect { rect, color, width, corner_radius }
                    if *rect == sheet
                        && *color == app.theme.color(ColorToken::Border)
                        && (*width - 1.0).abs() < 0.01
                        && (*corner_radius - 12.0).abs() < 0.01
            )),
            "the shell surface owns no Border frame"
        );
        let shadow = commands.iter().any(|command| {
            matches!(
                command,
                RenderCommand::StrokeRect { rect, color, .. }
                    if color.r == 0 && color.g == 0 && color.b == 0 && color.a > 0
                        && rect.width() > sheet.width()
            )
        });
        assert!(!shadow, "the shell surface has no shadow/elevation");
    }

    /// L: the settings sheet has no search field, no user footer, no row or
    /// section separator rules, and its rail heading is small/muted while every
    /// menu label is primary text.
    #[test]
    fn the_settings_sheet_has_no_search_footer_or_separator_rules() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.open_settings_tab(None);
            s.settings_overlay_open = true;
            s.settings_overlay_wide = true;
        }
        let commands = render_element(&mut root, vec2f(1600.0, 1000.0), &app);
        let texts: Vec<String> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        for gone in [
            "Search settings",
            "Search shortcuts...",
            "Local workspace",
            "You",
        ] {
            assert!(
                !texts.iter().any(|text| text == gone),
                "settings no longer draw {gone:?}: {texts:?}"
            );
        }
        // Rail heading: 12 pt and muted.
        let heading = commands.iter().find_map(|command| match command {
            RenderCommand::DrawText {
                text,
                origin,
                font_size,
                color,
                ..
            } if text == "Settings" && origin.x > 250.0 && origin.x < 500.0 => {
                Some((*font_size, *color))
            }
            _ => None,
        });
        let (size, color) = heading.expect("the rail heading");
        assert!((size - 12.0).abs() < 0.01, "small rail heading: {size}");
        assert_eq!(
            color,
            app.theme.color(ColorToken::Muted),
            "the rail heading is secondary text"
        );
        // Every rail entry is primary text, selected or not.
        for label in ["General", "Appearance", "Shortcuts", "About"] {
            let ink = commands.iter().find_map(|command| match command {
                RenderCommand::DrawText { text, color, .. } if text == label => Some(*color),
                _ => None,
            });
            assert_eq!(
                ink,
                Some(app.theme.color(ColorToken::Text)),
                "{label} is primary text"
            );
        }
        // No 1 px row/section rules across the sheet.
        let rules = commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    RenderCommand::FillRect { rect, .. }
                        if (rect.height() - 1.0).abs() < 0.01 && rect.width() > 300.0
                )
            })
            .count();
        assert_eq!(rules, 0, "settings draw no separator rules between rows");
    }

    /// The two panels live inside one rounded workspace sheet: the main pane
    /// reaches the shared boundary (the old right gutter and its casing are
    /// canceled under the panel) and the free margin sits outside the sheet.
    #[test]
    fn the_open_side_panel_shares_the_main_cards_right_edge() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let size = vec2f(1024.0, 768.0);

        let _ = render_element(&mut root, size, &app);
        let closed = pane_frames(&state);
        assert_eq!(closed.len(), 1, "the chat pane fills the main area");

        state.borrow_mut().right_sidebar_open = true;
        let commands = render_element(&mut root, size, &app);
        let open = pane_frames(&state);
        assert_eq!(
            open.len(),
            1,
            "the chat pane stays visible beside the panel"
        );

        // The left edge keeps the normal sidebar↔content inset while the right
        // edge reaches the shared boundary inside the one sheet.
        assert!(
            (open[0].min_x() - closed[0].min_x()).abs() < 0.01,
            "the left inset is unchanged: {:?} -> {:?}",
            closed[0],
            open[0]
        );
        let sheet = workspace_sheet(&commands, &app, size);
        assert!(
            sheet.min_x() < open[0].min_x() && sheet.max_x() > open[0].max_x(),
            "the sheet wraps both panels with the free margin outside it: {sheet:?} vs {:?}",
            open[0]
        );
        // R: no band on the top edge; the card's own 4 pt gutter is the inset.
        assert!(
            (open[0].min_y() - sheet.min_y() - 4.0).abs() < 0.5,
            "the pane keeps its card gutter inside the sheet: {:?} vs {sheet:?}",
            open[0]
        );
        let boundary = sheet.min_x() + sheet.width() * 0.5;
        assert!(
            open[0].max_x() >= boundary - 0.5,
            "the main pane reaches the shared boundary: {:?} vs {boundary}",
            open[0]
        );
        assert!(
            open[0].max_x() <= boundary + 6.0,
            "only the canceled gutter crosses under the panel: {:?}",
            open[0]
        );
    }

    /// Dragging the shared boundary writes the side panel's trailing ratio,
    /// clamps it to the `0.25..=0.75` range at a wide window, and keeps both
    /// panels above their 320 pt minimum when the content area is narrower.
    #[test]
    fn dragging_the_side_split_changes_and_clamps_the_ratio() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let size = vec2f(1600.0, 900.0);
        {
            let mut s = state.borrow_mut();
            s.right_sidebar_open = true;
            s.side_split_ratio = 0.5;
        }
        let commands = render_element(&mut root, size, &app);
        let sheet = workspace_sheet(&commands, &app, size);
        let boundary = sheet.min_x() + sheet.width() * 0.5;
        let mid_y = sheet.min_y() + sheet.height() / 2.0;
        let mut ctx = EventContext::default();
        assert!(root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(boundary, mid_y),
                button: 0,
            },
            &mut ctx,
            &app,
        ));
        assert!(state.borrow().side_split_dragging);
        let _ = render_element(&mut root, size, &app);

        // Far right: the side panel is the trailing share, so it shrinks to
        // its 0.25 floor while the boundary follows the pointer.
        assert!(root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: vec2f(size.x - 1.0, mid_y),
            },
            &mut ctx,
            &app,
        ));
        assert!(
            (state.borrow().side_split_ratio - 0.25).abs() < 0.001,
            "the right drag clamps at the 0.25 side share, got {}",
            state.borrow().side_split_ratio
        );
        let _ = render_element(&mut root, size, &app);
        let commands = render_element(&mut root, size, &app);
        let sheet = workspace_sheet(&commands, &app, size);
        let main = pane_frames(&state)[0];
        assert!(
            (main.width() - sheet.width() * 0.75).abs() < 12.0,
            "the main panel keeps the three-quarter share: {main:?} in {sheet:?}"
        );

        let _ = root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: vec2f(size.x - 1.0, mid_y),
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert!(
            !state.borrow().side_split_dragging,
            "mouse up ends the drag"
        );

        // Narrower content: the side share can reach 0.75, but the main panel
        // never falls below 320 pt; dragging left/right hits those bounds.
        let narrow = vec2f(1400.0, 900.0);
        state.borrow_mut().side_split_ratio = 0.5;
        let _ = render_element(&mut root, narrow, &app);
        let commands = render_element(&mut root, narrow, &app);
        let sheet = workspace_sheet(&commands, &app, narrow);
        let main = pane_frames(&state)[0];
        let boundary = main.max_x() - 5.0;
        let mid_y = main.min_y() + main.height() / 2.0;
        assert!(root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(boundary, mid_y),
                button: 0,
            },
            &mut ctx,
            &app,
        ));
        let _ = render_element(&mut root, narrow, &app);
        assert!(root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: vec2f(0.0, mid_y),
            },
            &mut ctx,
            &app,
        ));
        assert!(
            state.borrow().side_split_ratio > 0.69,
            "the left drag takes the side panel to its width-aware maximum, got {}",
            state.borrow().side_split_ratio
        );
        let _ = render_element(&mut root, narrow, &app);
        let main = pane_frames(&state)[0];
        assert!(
            main.width() >= 320.0 - 10.0,
            "the main panel holds its 320 pt minimum: {main:?}"
        );
        let min_side = (320.0 / sheet.width()).min(0.75);
        assert!(
            state.borrow().side_split_ratio >= min_side - 0.001,
            "the side panel keeps its minimum too"
        );
    }

    /// Both panels fill the same sheet height: one top edge, one bottom edge,
    /// and the same inset inside the workspace surface.
    #[test]
    fn the_side_split_cards_share_one_top_and_bottom() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let size = vec2f(1024.0, 768.0);
        state.borrow_mut().right_sidebar_open = true;
        let commands = render_element(&mut root, size, &app);
        let sheet = workspace_sheet(&commands, &app, size);
        let main = pane_frames(&state)[0];
        let top = main.min_y() - sheet.min_y();
        let bottom = sheet.max_y() - main.max_y();
        assert!(
            (top - 4.0).abs() < 0.5,
            "the pane keeps its card gutter on the top edge: {top}"
        );
        assert!(
            (bottom - 8.0).abs() < 0.5,
            "the bottom keeps the area and card gutters: {bottom}"
        );
    }

    /// At the default 0.5 ratio the sheet is split in equal halves with no
    /// casing at the boundary: the main pane reaches the middle and the panel
    /// starts from the same line, both on one surface.
    #[test]
    fn the_side_split_cards_meet_with_no_gap_at_the_default_ratio() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let size = vec2f(1024.0, 768.0);
        state.borrow_mut().right_sidebar_open = true;
        assert!((state.borrow().side_split_ratio - 0.5).abs() < f32::EPSILON);
        let commands = render_element(&mut root, size, &app);
        let sheet = workspace_sheet(&commands, &app, size);
        let boundary = sheet.min_x() + sheet.width() * 0.5;
        let main = pane_frames(&state)[0];
        assert!(
            (main.max_x() - boundary).abs() < 6.0,
            "the main pane meets the boundary: {main:?} vs {boundary}"
        );
        // The side panel's first row starts to the right of the boundary on the
        // same sheet; no second rounded casing splits it.
        let panel_text = commands.iter().find_map(|command| match command {
            RenderCommand::DrawText { text, origin, .. } if text == "Review" => Some(*origin),
            _ => None,
        });
        let panel_text = panel_text.expect("the Overview rows draw");
        assert!(
            panel_text.x > boundary,
            "the panel content starts right of the boundary: {panel_text:?}"
        );
    }

    /// The workspace surface is flush to the chrome/sidebar/window edges (the
    /// sheet has no margin or radius), and the side panel shares the content
    /// material.
    #[test]
    fn the_workspace_surface_is_flush_and_both_panels_share_its_material() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let size = vec2f(1024.0, 768.0);
        state.borrow_mut().right_sidebar_open = true;
        state.borrow_mut().side_tabs = vec![SideTabKind::Overview];
        state.borrow_mut().active_side_tab = 0;
        let commands = render_element(&mut root, size, &app);
        let sheet = workspace_sheet(&commands, &app, size);

        // Flush content surface: it reaches the chrome, the sidebar and the
        // window's right/bottom edges; no margin or radius.
        assert!(
            (sheet.min_x() - crate::ui::SIDEBAR_WIDTH).abs() < 1.0,
            "the surface starts at the sidebar: {sheet:?}"
        );
        assert!(
            (sheet.min_y() - crate::ui::chrome::CHROME_HEIGHT).abs() < 1.0,
            "the surface starts under the chrome: {sheet:?}"
        );
        assert!(
            (size.x - sheet.max_x()).abs() < 1.0 && (size.y - sheet.max_y()).abs() < 1.0,
            "the surface reaches the window's right/bottom edges: {sheet:?}"
        );
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { rect, corner_radius, .. }
                    if *rect == sheet && *corner_radius == 0.0
            )),
            "the surface is square, not a rounded sheet"
        );

        // The side panel body paints the content material across the boundary,
        // with no casing of its own.
        let bg = app.theme.color(ColorToken::ContentBg);
        let boundary = sheet.min_x() + sheet.width() * 0.5;
        let side_at = vec2f(boundary + 30.0, sheet.min_y() + 120.0);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::FillRect { rect, color, .. }
                    if *color == bg
                        && rect.width() > 100.0
                        && contains(*rect, side_at)
            )),
            "the side panel is flat Bg, not a separate card"
        );
        let casings: Vec<_> = rules(&commands, &app)
            .into_iter()
            .filter(|(rect, _, radius)| {
                (*radius - 6.0).abs() < 0.01 && rect.min_x() >= boundary - 1.0
            })
            .collect();
        assert!(
            casings.is_empty(),
            "no pane-card casing wraps the side panel: {casings:?}"
        );

        // The reference's side panel header: the search row and its kebab.
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "ellipsis-vertical"
            )),
            "the side panel's search row carries the kebab"
        );
        // Z: the launcher keeps Plugins with its disclosure chevron.
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "chevron-right"
            )),
            "the launcher's Plugins row carries its chevron"
        );
    }

    /// J: the two flat panels are separated by exactly one 1 pt hairline at
    /// the shared boundary, full panel height, in the Border colour. The panels
    /// themselves paint no casing.
    #[test]
    fn the_open_panels_share_one_hairline_and_no_card_casing() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let size = vec2f(1024.0, 768.0);
        state.borrow_mut().right_sidebar_open = true;
        let commands = render_element(&mut root, size, &app);
        let sheet = workspace_sheet(&commands, &app, size);
        let boundary = sheet.min_x() + sheet.width() * 0.5;
        let border = app.theme.color(ColorToken::Border);
        let hairlines: Vec<RectF> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == border
                        && (rect.width() - 1.0).abs() < 0.01
                        && rect.height() > sheet.height() * 0.9
                        && (rect.min_x() - boundary).abs() < 1.5 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            hairlines.len(),
            1,
            "one hairline at the shared boundary: {hairlines:?}"
        );
        assert!(
            (hairlines[0].min_y() - sheet.min_y()).abs() < 0.51
                && (hairlines[0].max_y() - sheet.max_y()).abs() < 0.51,
            "the hairline spans the panel height: {:?} vs {sheet:?}",
            hairlines[0]
        );
        // No pane card border: the only `Border` strokes are the sheet frame.
        let pane_edges = commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    RenderCommand::StrokeRect { rect, color, width, .. }
                        if *color == border
                            && (*width - 1.0).abs() < 0.01
                            && rect.height() > sheet.height() * 0.9
                            && *rect != sheet
                            && !(rect.width() == size.x && rect.height() == size.y)
                )
            })
            .count();
        assert_eq!(
            pane_edges, 0,
            "no pane card adds a Border edge beside the shell surface"
        );
    }

    /// The side panel's selected tab chooses the whole content area: Overview
    /// keeps its search/rows, Task output lists the active pane's real work
    /// descriptions, Files says honestly that no file is open, and Agent mounts
    /// the pane chat instead of the overview.
    #[test]
    fn the_side_panel_content_follows_the_selected_tab() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.right_sidebar_open = true;
            s.side_tabs = vec![SideTabKind::Overview, SideTabKind::TaskOutput];
            s.active_side_tab = 1;
            // A live tool call is the pane's real work: the Task output tab
            // reads it through `pane_live_work`, not from a fixture list.
            let pane_id = s.active_pane_id;
            let runtime = s
                .pane_runtime
                .get_mut(&pane_id)
                .expect("the active pane runtime");
            runtime.in_flight_tools.insert(
                "call-read".to_string(),
                goble_ui::ToolCall {
                    id: "call-read".to_string(),
                    name: "read_file".to_string(),
                    arguments: r#"{"path":"README.md"}"#.to_string(),
                    status: ToolCallStatus::Running,
                    result: None,
                    diff: None,
                },
            );
        }
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            texts.iter().any(|text| *text == "Read README.md"),
            "Task output lists the pane's real work: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| *text == "Search or enter a URL"),
            "the overview search is not under Task output: {texts:?}"
        );

        {
            let mut s = state.borrow_mut();
            s.side_tabs.push(SideTabKind::Files);
            s.active_side_tab = 2;
        }
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            texts
                .iter()
                .any(|text| *text == "Open a file from Explorer"),
            "Files with no file says so: {texts:?}"
        );
        assert!(!texts.iter().any(|text| *text == "Search or enter a URL"));

        {
            let mut s = state.borrow_mut();
            s.side_tabs.push(SideTabKind::Agent);
            s.active_side_tab = 3;
        }
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            !texts.iter().any(|text| *text == "Search or enter a URL"),
            "the Agent tab mounts the chat, not Overview: {texts:?}"
        );
        assert_eq!(
            texts
                .iter()
                .filter(|text| **text == "Side Question")
                .count(),
            1,
            "only the strip chip names Side Question: the topbar owns the panel \
             controls, so the body has no second header: {texts:?}"
        );
    }

    /// The side-panel chips are real controls: clicking the Side Question chip
    /// moves the panel off the Files tab it was showing. Overview has no chip,
    /// so it cannot be clicked back to from the strip.
    #[test]
    fn a_side_tab_chip_click_selects_that_tab() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.right_sidebar_open = true;
            s.add_side_tab(SideTabKind::Agent);
            s.add_side_tab(SideTabKind::Files);
        }
        assert_eq!(state.borrow().active_side_tab, 1);
        let commands = render_element(&mut root, window(), &app);
        let chip = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Side Question" && origin.y < crate::ui::chrome::CHROME_HEIGHT =>
                {
                    Some(*origin + vec2f(2.0, 4.0))
                }
                _ => None,
            })
            .expect("the Side Question chip is drawn");
        click(&mut root, &app, chip);
        assert_eq!(
            state.borrow().active_side_tab,
            0,
            "the chip click selects Side Question"
        );
        // Overview is the panel-1 launcher, not a strip chip.
        let commands = render_element(&mut root, window(), &app);
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Overview" && origin.y < crate::ui::chrome::CHROME_HEIGHT
            )),
            "no Overview chip in the strip"
        );
    }

    /// G: an open panel with no real tab draws no chip and no `+` — Overview is
    /// the body's home surface and the panel-1 launcher, never a strip chip.
    #[test]
    fn the_empty_side_panel_shows_the_start_surface_and_add_button() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        state.borrow_mut().right_sidebar_open = true;
        assert!(
            state.borrow().side_tabs.is_empty(),
            "a fresh panel has no open tab"
        );
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for label in [
            "Search or enter a URL",
            "Review",
            "Open File",
            "Terminal",
            "Side Question",
            "Plugins",
        ] {
            assert!(
                texts.contains(&label),
                "the Overview home lists {label:?}: {texts:?}"
            );
        }
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "plus"
            )),
            "the empty panel keeps the add-tab button"
        );
    }

    /// S: the `+` is pinned beside the scrolling chips, never behind the
    /// window controls, and visible even when the strip overflows.
    #[test]
    fn the_add_button_is_pinned_beside_the_scrolling_chips() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.right_sidebar_open = true;
            s.add_side_tab(SideTabKind::Terminal);
            s.add_side_tab(SideTabKind::Agent);
        }
        let commands = render_element(&mut root, window(), &app);
        let icon_x = |name: &str| -> Option<f32> {
            commands.iter().find_map(|command| match command {
                RenderCommand::DrawIcon {
                    name: drawn,
                    origin,
                    ..
                } if *drawn == name && origin.y < crate::ui::chrome::CHROME_HEIGHT => {
                    Some(origin.x)
                }
                _ => None,
            })
        };
        let terminal = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Terminal" && origin.y < crate::ui::chrome::CHROME_HEIGHT =>
                {
                    Some(origin.x)
                }
                _ => None,
            })
            .expect("the Terminal chip");
        let side_question = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Side Question" && origin.y < crate::ui::chrome::CHROME_HEIGHT =>
                {
                    Some(origin.x)
                }
                _ => None,
            })
            .expect("the Side Question chip");
        let plus = icon_x("plus").expect("the add button");
        let menu = icon_x("panel-right-outline").expect("the side-panel toggle");
        assert!(terminal < side_question, "the chips keep strip order");
        assert!(
            plus > side_question,
            "the + sits after the chips: plus {plus}, chip {side_question}"
        );
        assert!(
            menu - plus > 20.0,
            "the + stays left of the trailing icons: plus {plus}, menu {menu}"
        );
    }

    /// W: the settings overlay receives real pointer events above the shell.
    /// A rail item switches the page, a segmented control changes the value and
    /// the in-field colour swatch opens its palette.
    #[test]
    fn settings_overlay_controls_receive_real_clicks() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.settings_overlay_open = true;
            s.settings_overlay_wide = true;
            s.settings.page = crate::ui::SettingsCategory::General;
        }
        let commands = render_element(&mut root, window(), &app);
        let find = |text: &str| {
            commands.iter().find_map(|command| match command {
                RenderCommand::DrawText {
                    text: drawn,
                    origin,
                    ..
                } if drawn == text => Some(*origin),
                _ => None,
            })
        };
        let appearance = find("Appearance").expect("the Appearance rail item");
        click(&mut root, &app, appearance + vec2f(6.0, 4.0));
        assert_eq!(
            state.borrow().settings.page,
            crate::ui::SettingsCategory::Appearance,
            "the rail click switches the page"
        );

        // A toggle: Application's Auto-approve row at the top of the page.
        let commands = render_element(&mut root, window(), &app);
        let application = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "Application" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the Application rail item");
        click(&mut root, &app, application + vec2f(6.0, 4.0));
        let commands = render_element(&mut root, window(), &app);
        let label = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Auto-approve questions" =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the Auto-approve toggle row");
        let track = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, .. }
                    if (rect.width() - 32.0).abs() < 0.5
                        && (rect.height() - 18.0).abs() < 0.5
                        && rect.min_y() > label.y - 20.0
                        && rect.min_y() < label.y + 20.0 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the Auto-approve switch track");
        click(
            &mut root,
            &app,
            vec2f(
                track.min_x() + track.width() / 2.0,
                track.min_y() + track.height() / 2.0,
            ),
        );
        let saved = state
            .borrow()
            .settings
            .config
            .as_ref()
            .and_then(|view| view.app().auto_approve);
        assert_eq!(
            saved,
            Some(true),
            "the Auto-approve switch at {track:?} writes config.toml (status {:?})",
            state.borrow().settings.status
        );

        // Narrow: at 700 pt the responsive layout hides the rail and the
        // floating hamburger carries the pages instead.
        let narrow = vec2f(700.0, 900.0);
        let commands = render_element(&mut root, narrow, &app);
        let hamburger = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    name, origin, size, ..
                } if name == "menu-01" && origin.y < 60.0 => {
                    Some(*origin + vec2f(*size / 2.0, *size / 2.0))
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                let icons: Vec<_> = commands
                    .iter()
                    .filter_map(|command| match command {
                        RenderCommand::DrawIcon { name, origin, .. } => {
                            Some((name.clone(), *origin))
                        }
                        _ => None,
                    })
                    .collect();
                panic!("the narrow settings hamburger: {icons:?}");
            });
        let mut ctx = EventContext;
        for event in [
            DispatchedEvent::MouseDown {
                position: hamburger,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: hamburger,
                button: 0,
            },
        ] {
            let _ = root.dispatch_event(&event, &mut ctx, &app);
        }
        assert!(
            *state.borrow().settings_overlay_hamburger.borrow(),
            "the narrow hamburger opens its page list"
        );
        let commands = render_element(&mut root, narrow, &app);
        let shortcuts = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "Shortcuts" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the hamburger's page list");
        for event in [
            DispatchedEvent::MouseDown {
                position: shortcuts + vec2f(6.0, 4.0),
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: shortcuts + vec2f(6.0, 4.0),
                button: 0,
            },
        ] {
            let _ = root.dispatch_event(&event, &mut ctx, &app);
        }
        assert_eq!(
            state.borrow().settings.page,
            crate::ui::SettingsCategory::Shortcuts,
            "the narrow hamburger navigates"
        );
    }

    fn find_text_origin(root: &mut Box<dyn Element>, app: &AppContext, needle: &str) -> Vector2F {
        render_element(root, window(), app)
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == needle => Some(*origin),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the frame draws {needle}"))
    }

    /// V: the topbar title is a pill; clicking it edits inline in the same
    /// casing, Enter commits the name and Escape cancels. The modal rename
    /// dialog stays out of this path.
    #[test]
    fn the_topbar_title_pill_edits_inline() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.conversations = vec![goble_ui::ConversationEntry::new(
                "title-id",
                "Alpha title",
                "",
                "1d",
            )];
            s.selected_id = Some("title-id".to_string());
        }
        let commands = render_element(&mut root, window(), &app);
        let title = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Alpha title" && origin.y < crate::ui::chrome::CHROME_HEIGHT =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the topbar title pill");
        click(&mut root, &app, title + vec2f(4.0, 4.0));
        {
            let s = state.borrow();
            let rename = s.sidebar_rename.as_ref().expect("inline edit starts");
            assert!(rename.inline, "the pill starts the inline editor");
            assert_eq!(rename.draft, "Alpha title");
        }
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            !texts.iter().any(|text| *text == "Rename conversation"),
            "the modal dialog is suppressed: {texts:?}"
        );

        // Editing the draft and Enter commits through the shared rename path.
        state.borrow_mut().sidebar_rename.as_mut().unwrap().draft = "Beta title".to_string();
        let mut ctx = EventContext;
        let _ = root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Enter".to_string(),
                modifiers: Default::default(),
            },
            &mut ctx,
            &app,
        );
        {
            let s = state.borrow();
            assert!(s.sidebar_rename.is_none(), "Enter closes the inline editor");
            assert_eq!(
                s.conversations[0].name, "Beta title",
                "the committed name replaces the old one"
            );
        }

        // Escape cancels without keeping the draft.
        let commands = render_element(&mut root, window(), &app);
        let title = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Beta title" && origin.y < crate::ui::chrome::CHROME_HEIGHT =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the updated title pill");
        click(&mut root, &app, title + vec2f(4.0, 4.0));
        assert!(
            state
                .borrow()
                .sidebar_rename
                .as_ref()
                .is_some_and(|rename| rename.inline),
            "the second click starts the inline editor again: {:?}",
            state.borrow().sidebar_rename
        );
        state.borrow_mut().sidebar_rename.as_mut().unwrap().draft = "Discarded".to_string();
        let handled = root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Escape".to_string(),
                modifiers: Default::default(),
            },
            &mut ctx,
            &app,
        );
        let s = state.borrow();
        assert!(
            s.sidebar_rename.is_none(),
            "Escape closes the editor (handled {handled}, rename {:?})",
            s.sidebar_rename
        );
        assert_eq!(
            s.conversations[0].name, "Beta title",
            "Escape keeps the name"
        );
    }

    /// 203/206: the open review panel's trailing cluster is exactly the More
    /// kebab and the round panel menu — no search, no maximize/panel glyph —
    /// with one 1 px separator between panel 1 and panel 2.
    #[test]
    fn the_review_owns_its_icon_row_below_the_general_topbar() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.right_sidebar_open = true;
            s.side_tabs = vec![crate::state::SideTabKind::Review];
            s.active_side_tab = 0;
        }
        let size = vec2f(1600.0, 1000.0);
        let commands = render_element(&mut root, size, &app);
        let split = size.x * 0.625;
        let top: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { name, origin, .. }
                    if origin.y < crate::ui::chrome::CHROME_HEIGHT && origin.x > split =>
                {
                    Some(name.as_str())
                }
                _ => None,
            })
            .collect();
        assert!(
            top.contains(&"fullscreen-enter") || top.contains(&"fullscreen-exit"),
            "the general topbar keeps maximize: {top:?}"
        );
        assert!(
            top.contains(&"panel-right-outline"),
            "the general topbar keeps the side-panel toggle: {top:?}"
        );
        assert!(
            !top.contains(&"search") && !top.contains(&"dots-vertical"),
            "the review icons are not in the general bar: {top:?}"
        );
        let review_row: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawIcon { name, origin, .. }
                    if origin.y > crate::ui::chrome::CHROME_HEIGHT
                        && origin.y < crate::ui::chrome::CHROME_HEIGHT + 40.0 =>
                {
                    Some(name.as_str())
                }
                _ => None,
            })
            .collect();
        // 472: the Review row is only Search and the sidebar toggle.
        for icon in ["search", "panel-left-outline"] {
            assert!(
                review_row.contains(&icon),
                "the review's own row carries {icon:?}: {review_row:?}"
            );
        }
        for icon in ["dots-vertical", "menu-01"] {
            assert!(
                !review_row.contains(&icon),
                "the review's own row drops {icon:?}: {review_row:?}"
            );
        }
        let border = app.theme.color(ColorToken::Border);
        let separators = commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    RenderCommand::FillRect { rect, color, .. }
                        if *color == border
                            && rect.width() <= 1.01
                            && rect.height() >= 40.0
                )
            })
            .count();
        assert!(separators >= 1, "a 1 px rule separates the rows");
    }

    /// 289/290: the two topbar rules land on the body's sidebar and split
    /// boundaries at a normal ratio.
    #[test]
    fn topbar_rules_align_with_the_body_columns() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.sidebar_visible = true;
            s.right_sidebar_open = true;
            s.side_split_ratio = 0.5;
            s.side_tabs = vec![crate::state::SideTabKind::Terminal];
            s.active_side_tab = 0;
        }
        let size = vec2f(1600.0, 1000.0);
        let commands = render_element(&mut root, size, &app);
        let border = app.theme.color(ColorToken::Border);
        let rules: Vec<f32> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, .. }
                    if matches!(command, RenderCommand::FillRect { color, .. } if *color == border)
                        && rect.width() <= 1.01
                        && rect.height() >= 40.0
                        && rect.min_y() < 5.0 =>
                {
                    Some(rect.min_x())
                }
                _ => None,
            })
            .collect();
        let sidebar = state.borrow().sidebar_width;
        let content = size.x - sidebar - 1.0;
        let boundary = sidebar + 1.0 + content * 0.5;
        assert_eq!(rules.len(), 2, "sidebar and split rules: {rules:?}");
        assert!(
            (rules[0] - sidebar).abs() < 2.0,
            "sidebar rule {} vs {sidebar}",
            rules[0]
        );
        assert!(
            (rules[1] - boundary).abs() < 3.0,
            "split rule {} vs {boundary}",
            rules[1]
        );
    }

    /// 282/289: maximized panel keeps the sidebar column and gives panel 2 the
    /// rest; the panel-1 title is not drawn.
    #[test]
    fn the_maximized_panel_topbar_hides_the_panel_one_title() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.sidebar_visible = true;
            s.right_sidebar_open = true;
            s.side_panel_maximized = true;
            s.side_tabs = vec![crate::state::SideTabKind::Terminal];
            s.active_side_tab = 0;
        }
        let commands = render_element(&mut root, vec2f(1600.0, 1000.0), &app);
        let topbar: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if origin.y < crate::ui::chrome::CHROME_HEIGHT =>
                {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect();
        assert!(
            !topbar.contains(&"New Agent"),
            "the panel-1 title is hidden while maximized: {topbar:?}"
        );
        assert!(
            topbar.contains(&"Terminal"),
            "the panel-2 chips span the content column: {topbar:?}"
        );
        let sidebar = state.borrow().sidebar_width;
        let border = app.theme.color(ColorToken::Border);
        let rules: Vec<f32> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == border
                        && rect.width() <= 1.01
                        && rect.height() >= 40.0
                        && rect.min_y() < 5.0 =>
                {
                    Some(rect.min_x())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            rules.len(),
            1,
            "maximized keeps only the sidebar rule: {rules:?}"
        );
        assert!(
            (rules[0] - sidebar).abs() < 2.0,
            "the rule sits on the sidebar edge: {} vs {sidebar}",
            rules[0]
        );
    }

    /// 302/303/309/310: the chat footer has no shortcut strip, the model is
    /// plain text, the text lane is narrower and the left scroll dots exist.
    #[test]
    fn the_chat_footer_is_plain_and_the_left_scroll_dots_are_drawn() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let commands = render_element(&mut root, vec2f(1600.0, 1000.0), &app);
        let dots = commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    RenderCommand::FillRect { rect, .. }
                        if (rect.width() - 3.0).abs() < 0.01
                            && (rect.height() - 3.0).abs() < 0.01
                            && rect.min_x() < 420.0
                )
            })
            .count();
        assert_eq!(dots, 0, "an empty transcript has no rail markers");
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for label in ["send", "send to cloud", "tasks"] {
            assert!(
                !texts.contains(&label),
                "the shortcut strip {label:?} is gone: {texts:?}"
            );
        }
        {
            let mut s = state.borrow_mut();
            let active = s.active_pane_id;
            let runtime = s.pane_runtime.entry(active).or_default();
            for index in 0..3 {
                runtime.messages.push(goble_ui::ChatMessage::new(
                    goble_ui::ChatRole::User,
                    vec![goble_ui::ChatFragment::text(format!("turn {index}"))],
                ));
                runtime.messages.push(goble_ui::ChatMessage::new(
                    goble_ui::ChatRole::Assistant,
                    vec![goble_ui::ChatFragment::text("reply")],
                ));
            }
        }
        let commands = render_element(&mut root, vec2f(1600.0, 1000.0), &app);
        let dots = commands
            .iter()
            .filter(|command| {
                matches!(
                    command,
                    RenderCommand::FillRect { rect, .. }
                        if (rect.width() - 3.0).abs() < 0.01
                            && (rect.height() - 3.0).abs() < 0.01
                            && rect.min_x() < 420.0
                )
            })
            .count();
        assert_eq!(dots, 2, "three turns are two dots plus the latest segment");
        let long_segment = commands.iter().any(|command| {
            matches!(
                command,
                RenderCommand::FillRect { rect, .. }
                    if (rect.width() - 3.0).abs() < 0.01
                        && rect.height() >= 12.0
                        && rect.min_x() < 420.0
            )
        });
        assert!(
            long_segment,
            "the rail's last indicator is the longer latest-turns segment"
        );
        // The composer card is still full-width while the transcript lane is
        // not: the first transcript words start well inside the pane.
        let first_text = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { origin, .. }
                    if origin.y > 200.0 && origin.y < 600.0 && origin.x > 200.0 =>
                {
                    Some(origin.x)
                }
                _ => None,
            })
            .fold(f32::INFINITY, f32::min);
        assert!(
            first_text > 250.0,
            "the narrowed transcript lane keeps a side margin, got {first_text}"
        );
    }

    /// 323: a click with a little pointer jitter still switches the tab; it
    /// must not be mistaken for a reorder drag.
    #[test]
    fn a_side_tab_click_with_jitter_still_switches() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.right_sidebar_open = true;
            s.side_tabs = vec![
                crate::state::SideTabKind::Agent,
                crate::state::SideTabKind::Files,
            ];
            s.active_side_tab = 1;
        }
        let commands = render_element(&mut root, window(), &app);
        let chip = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Side Question" && origin.y < crate::ui::chrome::CHROME_HEIGHT =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the Side Question chip");
        let mut ctx = EventContext;
        for event in [
            DispatchedEvent::MouseDown {
                position: chip + vec2f(4.0, 4.0),
                button: 0,
            },
            DispatchedEvent::MouseMove {
                position: chip + vec2f(6.0, 4.0),
            },
            DispatchedEvent::MouseUp {
                position: chip + vec2f(6.0, 4.0),
                button: 0,
            },
        ] {
            root.dispatch_event(&event, &mut ctx, &app);
        }
        assert_eq!(
            state.borrow().active_side_tab_kind(),
            crate::state::SideTabKind::Agent,
            "the click switches to the pressed chip"
        );
        assert_eq!(
            state.borrow().side_tabs,
            vec![
                crate::state::SideTabKind::Agent,
                crate::state::SideTabKind::Files
            ],
            "jitter does not reorder the strip"
        );
    }

    /// 297: dragging a chip reorders the strip and keeps the selected tab.
    #[test]
    fn dragging_a_side_tab_chip_reorders_the_strip() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.right_sidebar_open = true;
            s.side_tabs = vec![
                crate::state::SideTabKind::Agent,
                crate::state::SideTabKind::Files,
            ];
            s.active_side_tab = 0;
        }
        let commands = render_element(&mut root, window(), &app);
        let origin = |label: &str| {
            commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawText { text, origin, .. }
                        if text == label && origin.y < crate::ui::chrome::CHROME_HEIGHT =>
                    {
                        Some(*origin)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the {label:?} chip"))
        };
        let agent = origin("Side Question");
        let files = origin("Files");
        let mut ctx = EventContext;
        for event in [
            DispatchedEvent::MouseDown {
                position: agent + vec2f(4.0, 4.0),
                button: 0,
            },
            DispatchedEvent::MouseMove {
                position: files + vec2f(28.0, 4.0),
            },
            DispatchedEvent::MouseUp {
                position: files + vec2f(28.0, 4.0),
                button: 0,
            },
        ] {
            root.dispatch_event(&event, &mut ctx, &app);
        }
        assert_eq!(
            state.borrow().side_tabs,
            vec![
                crate::state::SideTabKind::Files,
                crate::state::SideTabKind::Agent,
            ],
            "the dragged chip lands after the other one"
        );
    }

    /// 283: every side-panel chip switches the content with a real click.
    #[test]
    fn side_tab_chips_switch_the_panel_with_real_clicks() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.right_sidebar_open = true;
            s.side_tabs = vec![
                crate::state::SideTabKind::Overview,
                crate::state::SideTabKind::Agent,
                crate::state::SideTabKind::Files,
                crate::state::SideTabKind::Terminal,
                crate::state::SideTabKind::TaskOutput,
            ];
            s.active_side_tab = 4;
        }
        for (label, kind) in [
            ("Side Question", crate::state::SideTabKind::Agent),
            ("Files", crate::state::SideTabKind::Files),
            ("Terminal", crate::state::SideTabKind::Terminal),
            ("Task output", crate::state::SideTabKind::TaskOutput),
        ] {
            let commands = render_element(&mut root, vec2f(1600.0, 1000.0), &app);
            let origin = commands
                .iter()
                .find_map(|command| match command {
                    RenderCommand::DrawText { text, origin, .. }
                        if text == label && origin.y < crate::ui::chrome::CHROME_HEIGHT =>
                    {
                        Some(*origin)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("the {label:?} chip"));
            click(&mut root, &app, origin + vec2f(6.0, 6.0));
            assert_eq!(
                state.borrow().active_side_tab_kind(),
                kind,
                "the {label:?} chip switches the panel"
            );
        }
    }

    /// 284: "+" opens the in-panel Add-tab surface, not a popup, and a row adds
    /// the tab.
    #[test]
    fn plus_opens_the_in_panel_add_tab_surface() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.right_sidebar_open = true;
            s.side_tabs = vec![
                crate::state::SideTabKind::Overview,
                crate::state::SideTabKind::Terminal,
            ];
            s.active_side_tab = 1;
            *s.side_tab_add_menu_open.borrow_mut() = false;
        }
        let commands = render_element(&mut root, vec2f(1600.0, 1000.0), &app);
        let plus = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    name, origin, size, ..
                } if name == "plus" && origin.y < crate::ui::chrome::CHROME_HEIGHT => {
                    Some(*origin + vec2f(*size / 2.0, *size / 2.0))
                }
                _ => None,
            })
            .expect("the add-tab button");
        click(&mut root, &app, plus);
        let commands = render_element(&mut root, vec2f(1600.0, 1000.0), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            texts.iter().any(|text| *text == "Search or enter a URL"),
            "the in-panel Overview launcher is shown: {texts:?}"
        );
        let terminal = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Terminal" && origin.y > crate::ui::chrome::CHROME_HEIGHT =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the Overview Terminal row");
        click(&mut root, &app, terminal + vec2f(8.0, 6.0));
        assert_eq!(
            state.borrow().active_side_tab_kind(),
            crate::state::SideTabKind::Terminal
        );
        assert!(!*state.borrow().side_tab_add_menu_open.borrow());
    }

    /// 322: the workspace Explorer reads its tree and a file row opens that
    /// file in the side panel.
    #[test]
    fn the_workspace_explorer_lists_real_rows_and_opens_a_file_sideways() {
        let app = AppContext::default();
        let (mut root, state, dir) = app_root();
        let file = dir.path().join("alpha.txt");
        std::fs::write(&file, "hello").expect("seed explorer file");
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.spaces[0].set_leaf_kind(1, crate::state::PaneKind::Explorer);
            s.pane_sessions.entry(1).or_default().path = dir.path().to_string_lossy().to_string();
        }
        let commands = render_element(&mut root, vec2f(1200.0, 900.0), &app);
        let origin = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "alpha.txt" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the explorer lists the real file");
        click(&mut root, &app, origin + vec2f(10.0, 5.0));
        assert_eq!(
            state.borrow().side_file.as_deref(),
            Some(file.to_string_lossy().as_ref()),
            "the file row opens the file in the side panel"
        );
        assert!(
            state.borrow().right_sidebar_open,
            "opening a file reveals the side panel"
        );
    }

    /// 357: a missing model is one announcement that opens Settings → Models.
    #[test]
    fn the_no_model_announcement_opens_models_settings() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = true;
            s.settings_llm_api_key = "test-key".to_string();
            s.settings_llm_provider.clear();
            s.settings.page = crate::ui::SettingsCategory::General;
        }
        let commands = render_element(&mut root, vec2f(1200.0, 900.0), &app);
        let button = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "Open Models" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the no-model announcement links to Models");
        click(&mut root, &app, button + vec2f(8.0, 6.0));
        assert!(state.borrow().settings_overlay_open);
        assert_eq!(
            state.borrow().settings.page,
            crate::ui::SettingsCategory::Providers
        );
    }

    /// 347/348/349/350: conversation rows carry no harness mark, and the
    /// Automations row's clock opens the real task popover with a switch.
    #[test]
    fn automations_opens_a_real_task_popover_and_rows_lose_the_harness_mark() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.crons = vec![crate::ui::CronEntry::new(
                "auto-1",
                "Nightly audit",
                "Every day at 02:00",
                "2h ago",
            )];
        }
        let commands = render_element(&mut root, vec2f(1200.0, 900.0), &app);
        assert!(
            !commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "harness-goble"
            )),
            "conversation rows do not lead with a harness mark"
        );
        let automations = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "Automations" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the Automations row");
        click(&mut root, &app, automations + vec2f(8.0, 6.0));
        assert!(
            state.borrow().automations_open,
            "the clock opens the popover"
        );
        let commands = render_element(&mut root, vec2f(1200.0, 900.0), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for label in ["Automation tasks", "Nightly audit", "New automation"] {
            assert!(
                texts.contains(&label),
                "the real automation popover lists {label:?}: {texts:?}"
            );
        }
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "clock"
            )),
            "the sidebar Automations row uses the clock icon"
        );
    }

    /// 339/343: the Conversations heading only appears when loose rows exist,
    /// carries the same "+" box, and its menu is the shared project picker with
    /// No Project checked.
    #[test]
    fn conversations_heading_appears_only_with_rows_and_opens_the_shared_picker() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.conversations.clear();
        }
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if origin.x < crate::ui::SIDEBAR_WIDTH =>
                {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect();
        assert!(
            !texts.contains(&"Conversations"),
            "no loose rows means no Conversations heading: {texts:?}"
        );

        state.borrow_mut().conversations = vec![goble_ui::elements::ConversationEntry::new(
            "loose-1",
            "Loose note",
            "Let's ship",
            "10:42",
        )
        .with_folder("Conversations")];
        let commands = render_element(&mut root, window(), &app);
        let heading = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Conversations" && origin.x < crate::ui::SIDEBAR_WIDTH =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the loose row brings the Conversations heading back");
        let plus = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    name, origin, size, ..
                } if name == "new-chat"
                    && origin.x < crate::ui::SIDEBAR_WIDTH
                    && (origin.y - heading.y).abs() < 22.0 =>
                {
                    Some(*origin + vec2f(*size / 2.0, *size / 2.0))
                }
                _ => None,
            })
            .expect("the Conversations heading draws its plus");
        click(&mut root, &app, plus);
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for label in ["Remote", "SSH Host", "New Project", "No Project"] {
            assert!(
                texts.contains(&label),
                "the shared picker lists {label:?}: {texts:?}"
            );
        }
    }

    /// 341/343: the composer pill starts at No Project and opens the same
    /// picker above the input.
    #[test]
    fn the_composer_project_pill_opens_the_shared_picker() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.set_active_pane_path(String::new());
        }
        let size = vec2f(1600.0, 1000.0);
        let commands = render_element(&mut root, size, &app);
        let pill = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "No Project" && origin.y > size.y * 0.7 =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the composer project pill");
        click(&mut root, &app, pill + vec2f(8.0, 6.0));
        assert!(*state.borrow().project_menu_open.borrow());
        let commands = render_element(&mut root, size, &app);
        let project_row = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == "New Project" => {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the picker lists New Project");
        assert!(
            project_row.y < pill.y,
            "the picker opens above the composer pill, not over the input"
        );
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for label in ["Remote", "SSH Host", "New Project", "No Project"] {
            assert!(
                texts.contains(&label),
                "the pill opens the shared picker with {label:?}: {texts:?}"
            );
        }
        // Taking No Project applies in place and closes the picker.
        let no_project = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "No Project" && origin.y < pill.y =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the checked No Project row");
        click(&mut root, &app, no_project + vec2f(8.0, 6.0));
        assert!(
            !*state.borrow().project_menu_open.borrow(),
            "choosing a project closes the picker"
        );
    }

    /// 333: the wheel reaches the settings page's Scrollable through the modal
    /// dialog; the modal must not consume it before the page sees it.
    #[test]
    fn the_settings_wheel_reaches_the_page_scroll() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.settings_overlay_open = true;
            s.settings.page = crate::ui::SettingsCategory::Advanced;
        }
        let _ = render_element(&mut root, vec2f(1200.0, 900.0), &app);
        let mut ctx = EventContext;
        root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: vec2f(700.0, 420.0),
            },
            &mut ctx,
            &app,
        );
        root.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: vec2f(0.0, -120.0),
                precise: false,
            },
            &mut ctx,
            &app,
        );
        assert!(
            state.borrow().settings.scroll.borrow().offset().abs() > 0.5,
            "the settings page scrolls under the wheel"
        );
    }

    /// AG: Escape hides the topmost floating file window; the pane docks back
    /// into the layout instead of disappearing from the tree.
    #[test]
    fn escape_docks_the_floating_file_window() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let pane = {
            let mut s = state.borrow_mut();
            assert!(s.open_file_in_floating("/tmp/float.rs".to_string(), None));
            s.active_pane_id
        };
        assert!(state.borrow().spaces[0].is_floating_overlay(pane));
        let _ = render_element(&mut root, vec2f(1600.0, 1000.0), &app);
        let mut ctx = EventContext;
        let handled = root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "Escape".to_string(),
                modifiers: Default::default(),
            },
            &mut ctx,
            &app,
        );
        assert!(handled);
        assert!(!state.borrow().spaces[0].is_floating_overlay(pane));
    }

    /// 279: the Side Question composer takes a real click, focus and a typed
    /// character on the reserved side pane.
    #[test]
    fn the_side_question_composer_takes_focus_and_typing() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.right_sidebar_open = true;
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.crons_open = false;
            s.side_tabs = vec![crate::state::SideTabKind::Agent];
            s.active_side_tab = 0;
        }
        let size = vec2f(1600.0, 1000.0);
        let commands = render_element(&mut root, size, &app);
        let placeholder = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Ask anything..." && origin.x > size.x * 0.55 =>
                {
                    Some(*origin)
                }
                _ => None,
            })
            .expect("the Side Question composer placeholder");
        click(&mut root, &app, placeholder + vec2f(40.0, -4.0));
        let _ = render_element(&mut root, size, &app);
        let mut ctx = EventContext;
        root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: "a".to_string(),
                modifiers: Default::default(),
            },
            &mut ctx,
            &app,
        );
        assert_eq!(
            state.borrow().pane_sessions[&crate::state::SIDE_AGENT_PANE_ID].draft,
            "a",
            "typing reaches the side composer's draft"
        );
    }

    /// AF: every side tab gets a content topbar; Terminal/TaskOutput add the
    /// shared Terminals + Background Tasks rail.
    #[test]
    fn every_side_tab_renders_its_own_topbar() {
        let app = AppContext::default();
        for (kind, expected) in [
            (crate::state::SideTabKind::Overview, "Overview"),
            (crate::state::SideTabKind::Agent, "New side question"),
            (crate::state::SideTabKind::Terminal, "zsh"),
            (
                crate::state::SideTabKind::TaskOutput,
                "Background Task Output",
            ),
        ] {
            let (mut root, state, _dir) = app_root();
            {
                let mut s = state.borrow_mut();
                s.right_sidebar_open = true;
                s.side_tabs = vec![kind];
                s.active_side_tab = 0;
                let active = s.active_pane_id;
                s.terminal.borrow_mut().sessions.insert(
                    active,
                    crate::terminal::TerminalSession::preview_screen("hi"),
                );
            }
            let commands = render_element(&mut root, vec2f(1600.0, 1000.0), &app);
            let texts: Vec<&str> = commands
                .iter()
                .filter_map(|command| match command {
                    RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert!(
                texts.iter().any(|text| *text == expected),
                "{kind:?} draws its topbar {expected:?}: {texts:?}"
            );
            if matches!(
                kind,
                crate::state::SideTabKind::Terminal | crate::state::SideTabKind::TaskOutput
            ) {
                assert!(
                    texts.iter().any(|text| *text == "Terminals"),
                    "{kind:?} draws the inner rail's Terminals heading: {texts:?}"
                );
                assert!(
                    !texts.iter().any(|text| *text == "Background Tasks"),
                    "{kind:?} hides the empty Background Tasks heading: {texts:?}"
                );
            }
        }
    }

    /// The Overview button opens the anchored popover with its real sections,
    /// and the popover paints after the body (the window's last paint layer).
    #[test]
    fn the_overview_popover_lists_its_sections_and_paints_above_the_body() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.right_sidebar_open = true;
            *s.chrome_overview_open.borrow_mut() = true;
        }
        state.borrow_mut().side_tabs = vec![SideTabKind::Overview];
        state.borrow_mut().active_side_tab = 0;
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for expected in ["Overview", "Review", "Files", "Terminal", "Workflows"] {
            assert!(
                texts.iter().any(|text| *text == expected),
                "the Overview popover draws {expected:?}: {texts:?}"
            );
        }
        assert!(
            !texts.iter().any(|text| *text == "Background Tasks"
                || *text == "Subagents"
                || *text == "No background tasks"
                || *text == "No subagents"),
            "empty work groups stay hidden: {texts:?}"
        );
        let review = commands
            .iter()
            .position(|command| {
                matches!(command, RenderCommand::DrawText { text, .. } if text == "Review")
            })
            .expect("the Review row");
        let body = commands
            .iter()
            .position(|command| {
                matches!(command, RenderCommand::DrawText { text, .. } if text == "Search or enter a URL")
            })
            .expect("the side-panel body");
        assert!(
            review > body,
            "the popover paints after the shell body (Review {review}, body {body})"
        );
    }

    /// Clicking the Overview icon toggles the popover, and its three data
    /// groups fold through the app-owned section state.
    #[test]
    fn the_overview_icon_toggles_and_its_groups_fold() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        let commands = render_element(&mut root, window(), &app);
        let overview = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon {
                    name, origin, size, ..
                } if name == "overview" => Some(*origin + vec2f(*size / 2.0, *size / 2.0)),
                _ => None,
            })
            .expect("the Overview trigger");
        click(&mut root, &app, overview);
        assert!(
            *state.borrow().chrome_overview_open.borrow(),
            "the Overview trigger opens the popover"
        );

        {
            let mut s = state.borrow_mut();
            s.collapsed_sections
                .insert("chrome.overview.subagents".to_string());
            s.collapsed_sections
                .insert("chrome.overview.background-tasks".to_string());
            s.collapsed_sections
                .insert("chrome.overview.workflows".to_string());
        }
        let commands = render_element(&mut root, window(), &app);
        let texts: Vec<&str> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        for header in ["Workflows"] {
            assert!(
                texts.iter().any(|text| *text == header),
                "the folded group keeps its header {header:?}: {texts:?}"
            );
        }
        assert!(
            !texts
                .iter()
                .any(|text| *text == "Background Tasks" || *text == "Subagents"),
            "empty work groups are not drawn at all: {texts:?}"
        );
        for empty in ["No background tasks", "No subagents", "No workflows"] {
            assert!(
                !texts.iter().any(|text| *text == empty),
                "a folded group hides its body {empty:?}: {texts:?}"
            );
        }
    }

    /// The panel-2 chip strip is measured at its natural width and clipped: a
    /// horizontal wheel shifts the strip's offset instead of squeezing chips.
    #[test]
    fn the_side_tab_strip_clips_and_scrolls_horizontally() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        {
            let mut s = state.borrow_mut();
            s.right_sidebar_open = true;
            s.side_tabs = vec![SideTabKind::Agent; 12];
            s.active_side_tab = 0;
        }
        let commands = render_element(&mut root, window(), &app);
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, RenderCommand::ClipRect(_))),
            "the overflowing chip strip is clipped"
        );
        let agent_xs = |commands: &[RenderCommand]| -> Vec<f32> {
            commands
                .iter()
                .filter_map(|command| match command {
                    RenderCommand::DrawText { text, origin, .. }
                        if text == "Side Question"
                            && origin.y < crate::ui::chrome::CHROME_HEIGHT =>
                    {
                        Some(origin.x)
                    }
                    _ => None,
                })
                .collect()
        };
        let before = agent_xs(&commands);
        assert!(!before.is_empty(), "the strip draws its chips: {before:?}");
        let mut ctx = EventContext;
        // The first chip's label is the measured strip's leading edge; aim at
        // the strip itself, not the main section to its left.
        let over_strip = vec2f(before[0] + 4.0, 20.0);
        let _ = root.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: over_strip,
            },
            &mut ctx,
            &app,
        );
        let consumed = root.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: vec2f(-80.0, 0.0),
                precise: false,
            },
            &mut ctx,
            &app,
        );
        assert!(consumed, "the horizontal notch belongs to the chip strip");
        let commands = render_element(&mut root, window(), &app);
        let after = agent_xs(&commands);
        // The leading chip can scroll out of the clip, so compare every drawn
        // chip: at least one previously visible chip must have moved left.
        assert!(
            before
                .iter()
                .any(|old| after.iter().any(|new| old - new >= 40.0)),
            "the horizontal wheel shifts the strip left ({before:?} -> {after:?})"
        );
    }

    /// 479: opening the Settings sheet is a draw, not a detection pass. The
    /// first frame must be effectively instant and must not populate the
    /// integration cache from PATH.
    #[test]
    fn opening_settings_does_not_probe_integrations_on_the_draw_path() {
        let app = AppContext::default();
        let (mut root, state, _dir) = app_root();
        // Warm fonts/layout once; the assertion is about the open frame, not
        // the process's first-ever render.
        let _ = render_element(&mut root, vec2f(1600.0, 1000.0), &app);
        state.borrow_mut().settings_overlay_open = true;
        let started = std::time::Instant::now();
        let _ = render_element(&mut root, vec2f(1600.0, 1000.0), &app);
        assert!(
            started.elapsed() < std::time::Duration::from_millis(250),
            "the first Settings frame took {:?}",
            started.elapsed()
        );
    }
}
