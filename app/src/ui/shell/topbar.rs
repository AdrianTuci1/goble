use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Button, ConstrainedBox, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, Empty, EventContext, Fill, Flex, Icon, LayoutContext, MainAxisAlignment,
    MainAxisSize, PaintContext, Point, PopupMenu, PopupMenuItem, RunningIndicator, SizeConstraint,
    Text, TextInput, Tooltip, TopbarButton,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::elements::interactive::contains;
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::theme::{ColorToken, SpacingToken};

use crate::state::PaneDrag;

use super::super::{MediaActions, MediaSnapshot, UiActions, UiSnapshot};

/// The width of the environment menu's hover tray.
const ENV_TRAY_WIDTH: f32 = 220.0;

/// The widest the ghost card of a lifted pane grows before its title is
/// squeezed, and how far its corner sits below/right of the pointer so it never
/// covers the point the drop is resolved at.
const GHOST_MAX_WIDTH: f32 = 200.0;
const GHOST_OFFSET_X: f32 = 12.0;
const GHOST_OFFSET_Y: f32 = 6.0;

// The general toolbar doubles as the OS titlebar on macOS, so it is a touch
// shorter and leaves room for the traffic lights on the left. Shared with the
// pane headers (terminal/agent) so the general toolbar and the per-pane topbar
// render at the same height.
#[cfg(target_os = "macos")]
pub const TOPBAR_HEIGHT: f32 = 36.0;
#[cfg(not(target_os = "macos"))]
pub const TOPBAR_HEIGHT: f32 = 40.0;

#[cfg(target_os = "macos")]
const TOPBAR_TRAFFIC_INSET: f32 = 76.0;
#[cfg(not(target_os = "macos"))]
const TOPBAR_TRAFFIC_INSET: f32 = 0.0;

/// Single general toolbar, reduced to the essentials: a sidebar toggle, the
/// workspace tabs (one per space, the active one editable inline), a "+ ▾"
/// control that opens a new workspace / picks the environment it runs in, and
/// the Settings icon pinned to the far right.
///
/// The tabs sit flush next to the toggle and run the full height of the bar,
/// separated by a single vertical rule, so a new workspace shows up *next to*
/// the current one instead of replacing it.
///
/// The "+" opens a new workspace in the active environment; the "▾" lists the
/// environments (the VMs the user added appear by name) — choosing one opens a
/// new workspace that runs on it, and hovering one shows a tray whose
/// "Make default" makes it the environment new workspaces start in — plus a
/// trailing "New environment…" entry.
pub fn build_topbar(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    media: &MediaSnapshot,
    media_actions: &MediaActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    // Sidebar toggle (retract/expand the left conversation list).
    let on_toggle_sidebar = actions.on_toggle_sidebar.clone();
    let sidebar_button = TopbarButton::new(
        Icon::new("menu-01")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_on_click(move || (on_toggle_sidebar.borrow_mut())())
    .finish();

    // One tab per workspace, active one first-class (double-click renames it).
    let workspaces = build_workspace_strip(app, state, actions);

    // "+ ▾": open a new workspace, or pick the environment it runs in.
    let add_workspace = build_add_workspace_control(app, state, media, media_actions, actions);

    // The live cue: a spinner plus the count of work in flight (C1's state),
    // between the workspace controls and the settings icon. It renders nothing
    // at zero, so the bar keeps its current look when nothing is running. It is
    // information only — no click opens anything from it.
    let live_indicator = build_live_indicator(app, state.live_work_count, state.live_work_phase);

    let left = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm * 0.5)
        .with_child(sidebar_button)
        .with_child(workspaces)
        .with_child(add_workspace)
        .finish();

    // Settings stays reachable as an icon pinned to the far right.
    let on_settings = actions.on_settings.clone();
    let settings_button = TopbarButton::new(
        Icon::new("settings")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_on_click(move || (on_settings.borrow_mut())())
    .finish();

    let row = Flex::row()
        .with_main_axis_size(goble_ui::elements::MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(left)
        .with_child(live_indicator)
        .with_child(settings_button)
        .finish();

    // The workspace strip is already drawn at `TOPBAR_HEIGHT` (each tab runs the
    // full height of the bar), so the bar needs no vertical padding of its own:
    // the shorter controls center against the tabs and the bar is exactly
    // `TOPBAR_HEIGHT` tall — the height the body reserves below it — with the
    // macOS traffic lights vertically centered inside it.
    let bar: Box<dyn Element> = Container::new(row)
        .with_padding(EdgeInsets::new(TOPBAR_TRAFFIC_INSET, 0.0, 0.0, 0.0))
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .finish();
    // The card a pane being dragged onto the strip carries floats over the whole
    // bar — tabs, "+ ▾" and the settings icon alike — so it is a second,
    // zero-sized child of a stack rather than part of the row.
    goble_ui::elements::Stack::new()
        .with_children(vec![
            bar,
            build_pane_drag_ghost(app, state.pane_drag.as_ref()),
        ])
        .finish()
}

/// The topbar's live cue: a spinner and `◆ N`, where N is C1's count of work in
/// flight (`LiveWork::work_count`). At zero it renders nothing at all, so the
/// bar keeps its current look and there is no hit target to click.
///
/// It is information only: it carries no handler, so a click passes through.
pub(super) fn build_live_indicator(app: &AppContext, work_count: usize, phase: f32) -> Box<dyn Element> {
    if work_count == 0 {
        return Empty::new().with_size(vec2f(0.0, 0.0)).finish();
    }
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm * 0.5)
        .with_child(
            RunningIndicator::new()
                .with_size(12.0)
                .with_phase(phase)
                .finish(),
        )
        .with_child(
            Icon::new("diamond")
                .with_size(8.0)
                .with_theme_color(ColorToken::Accent, app)
                .finish(),
        )
        .with_child(
            Text::new(work_count.to_string())
                .with_theme_color(ColorToken::Accent, app)
                .with_font_size(12.0)
                .with_max_lines(1)
                .finish(),
        )
        .finish();
    // The same padding the indicator had as a hit target, so removing the click
    // leaves the bar's layout unchanged.
    Container::new(row)
        .with_padding(EdgeInsets::new(5.0, 4.0, 5.0, 4.0))
        .finish()
}

/// The workspace row: one browser-style tab per space, in order, laid out flush
/// with a single vertical rule between neighbours. The active tab is filled and,
/// while renaming, swaps for the inline name field.
///
/// The strip is pinned to [`TOPBAR_HEIGHT`] so every tab runs the full height of
/// the bar; each tab is square-cornered (see [`WorkspaceChip`]).
fn build_workspace_strip(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(0.0);
    // One slot per tab, filled with the rectangle that tab is drawn at while it
    // paints: the drop an insertion index resolves against (see
    // [`SpaceDropStrip`]) is read off exactly what the user can see.
    let tab_rects: Rc<RefCell<Vec<Option<RectF>>>> =
        Rc::new(RefCell::new(vec![None; state.spaces.len()]));
    for (index, space) in state.spaces.iter().enumerate() {
        // One rule per boundary: it belongs to the tab that follows it, so N
        // tabs draw N-1 lines and no boundary carries two.
        if index > 0 {
            row = row.with_child(Divider::vertical().finish());
        }
        let active = index == state.active_space;
        if active && state.space_rename_editing {
            let tab = build_rename_tab(app, state, actions);
            row = row.with_child(Box::new(TabRect::new(tab, index, Rc::clone(&tab_rects))));
        } else {
            let chip = build_workspace_chip(
                app,
                actions,
                index,
                &space.name,
                active,
            );
            row = row.with_child(Box::new(TabRect::new(chip, index, Rc::clone(&tab_rects))));
        }
    }
    let strip: Box<dyn Element> = ConstrainedBox::new(row.finish())
        .with_height(TOPBAR_HEIGHT)
        .finish();
    Box::new(SpaceDropStrip::new(
        strip,
        state.pane_drag.clone(),
        tab_rects,
        actions.on_pane_lift_move.clone(),
        actions.on_pane_drop.clone(),
    ))
}

/// One clickable workspace tab: the name plus an "x" that closes that
/// workspace. A single click selects the space; a second click within the
/// double-click window starts the inline rename.
fn build_workspace_chip(
    app: &AppContext,
    actions: &UiActions,
    index: usize,
    name: &str,
    active: bool,
) -> Box<dyn Element> {
    let color = if active {
        ColorToken::Text
    } else {
        ColorToken::Muted
    };
    let label = Text::new(name)
        .with_theme_color(color, app)
        .with_font_size(12.0)
        .finish();
    let on_click = actions.on_workspace_click.clone();
    let on_close = actions.on_close_space.clone();
    Box::new(WorkspaceChip::new(
        label,
        index,
        active,
        Rc::new(RefCell::new(move |i: usize| (on_click.borrow_mut())(i))),
        TopbarButton::new(
            Icon::new("x")
                .with_size(10.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_size(16.0)
        .with_on_click(move || (on_close.borrow_mut())(index))
        .finish(),
    ))
}

/// The inline rename field, centered in the active tab's full height so the
/// strip keeps `TOPBAR_HEIGHT` while the name is being edited.
fn build_rename_tab(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::Center)
        .with_child(build_rename_field(app, state, actions))
        .finish()
}

/// The inline rename field shown in place of the active workspace tab.
fn build_rename_field(_app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let on_change = actions.on_space_rename_change.clone();
    let on_focus = actions.on_space_rename_focus.clone();
    let on_submit = actions.on_space_rename_commit.clone();
    let input = TextInput::new()
        .with_value(state.space_rename_draft.clone())
        .with_placeholder("Workspace name")
        .with_focused(state.space_rename_focused)
        .with_on_change(move |v| (on_change.borrow_mut())(v))
        .with_on_focus_change(move |f: bool| (on_focus.borrow_mut())(f))
        .with_on_submit(move || (on_submit.borrow_mut())())
        .finish();
    ConstrainedBox::new(input)
        .with_width(160.0)
        .with_height(22.0)
        .finish()
}

/// A browser-style workspace tab: a full-height, square-cornered surface with
/// no idle background, a hover fill, and (when active) a raised fill plus the
/// themed text color. The strip draws the rule between neighbours, so the tab
/// itself never strokes a border — no boundary can read as a doubled line. The
/// trailing "x" closes the workspace; it is hit-tested before the tab body so
/// it never doubles as a select click.
pub(super) struct WorkspaceChip {
    child: Box<dyn Element>,
    close: Box<dyn Element>,
    index: usize,
    active: bool,
    on_click: Rc<RefCell<dyn FnMut(usize)>>,
    state: goble_ui::elements::interactive::InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl WorkspaceChip {
    pub(super) fn new(
        child: Box<dyn Element>,
        index: usize,
        active: bool,
        on_click: Rc<RefCell<dyn FnMut(usize)>>,
        close: Box<dyn Element>,
    ) -> Self {
        Self {
            child,
            close,
            index,
            active,
            on_click,
            state: Default::default(),
            size: None,
            origin: None,
        }
    }

    /// The tab runs the full height of the toolbar, like a browser tab.
    fn height() -> f32 {
        TOPBAR_HEIGHT
    }

    fn h_pad() -> f32 {
        7.0
    }

    /// Gap between the workspace name and its close "x".
    fn close_gap() -> f32 {
        4.0
    }
}

impl Element for WorkspaceChip {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let inner = SizeConstraint::new(
            Vector2F::zero(),
            vec2f(
                (constraint.max.x - Self::h_pad() * 2.0).max(0.0),
                Self::height(),
            ),
        );
        let child_size = self.child.layout(inner, ctx, app);
        let close_size = self.close.layout(inner, ctx, app);
        let size = vec2f(
            child_size.x + Self::close_gap() + close_size.x + Self::h_pad() * 2.0,
            Self::height(),
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let rect = rectf(origin.x, origin.y, size.x, size.y);
        let hovered = ctx.hovered(rect);
        if let Some(renderer) = ctx.renderer.as_mut() {
            // `fill_rect`, not `fill_rounded_rect`: a tab has square corners,
            // and no stroke joins it to its neighbour's rule.
            if self.active {
                renderer.fill_rect(rect, app.theme.color(ColorToken::SurfaceRaised));
            } else if hovered {
                renderer.fill_rect(rect, app.theme.color(ColorToken::Hover));
            }
        }
        let child_size = self.child.size().unwrap_or(Vector2F::zero());
        let close_size = self.close.size().unwrap_or(Vector2F::zero());
        self.child.paint(
            vec2f(
                origin.x + Self::h_pad(),
                origin.y + (size.y - child_size.y).max(0.0) / 2.0,
            ),
            ctx,
            app,
        );
        // The close "x" is drawn only while the pointer is over the tab. Its
        // slot stays reserved in `layout`, so the strip's tabs keep their width
        // (and their drop targets) instead of shifting under the pointer, and a
        // tab that is not hovered carries no invisible hit target.
        if hovered {
            self.close.paint(
                vec2f(
                    origin.x + Self::h_pad() + child_size.x + Self::close_gap(),
                    origin.y + (size.y - close_size.y).max(0.0) / 2.0,
                ),
                ctx,
                app,
            );
        }
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
        // The close "x" gets the event first: it only consumes clicks inside its
        // own bounds, so clicking the name still selects the workspace. It is
        // shown only while the pointer is over the tab (see `paint`), so it is
        // offered the pointer only there too — a tab that is not hovered has no
        // close target to hit.
        let over_tab = match event {
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. }
            | DispatchedEvent::MouseMove { position } => self
                .bounds()
                .map(|bounds| contains(bounds, *position))
                .unwrap_or(false),
            _ => false,
        };
        if over_tab && self.close.dispatch_event(event, ctx, app) {
            return true;
        }
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };
        let cb = self.on_click.clone();
        let index = self.index;
        let mut on_click = move || (cb.borrow_mut())(index);
        goble_ui::elements::interactive::handle_mouse_event(
            &mut self.state,
            event,
            bounds,
            ctx,
            &mut on_click,
        )
    }
}

/// The "+ ▾" control: "+" opens a new workspace in the active environment, and
/// "▾" opens the environment menu (a new terminal, the environments, plus
/// "New environment…").
fn build_add_workspace_control(
    app: &AppContext,
    state: &UiSnapshot,
    media: &MediaSnapshot,
    media_actions: &MediaActions,
    actions: &UiActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let on_add_space = actions.on_add_space.clone();
    let plus = TopbarButton::new(
        Icon::new("plus")
            .with_size(14.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(24.0)
    .with_on_click(move || (on_add_space.borrow_mut())())
    .finish();

    let trigger = TopbarButton::new(
        Icon::new("chevron-down")
            .with_size(12.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(20.0)
    .finish();

    // The menu opens with the terminal the workspace can launch — a new space
    // holding a plain PTY — and then the environments themselves: choosing one
    // opens a new space that runs on it (the check mark still marks the active
    // environment), and the trailing entry adds a brand-new one.
    let mut items = vec![PopupMenuItem::new("New terminal").with_icon("terminal")];
    // `(medium id, label)` per environment row, in item order.
    let mut envs: Vec<(String, String)> = Vec::new();
    for m in &media.mediums {
        let mut item = PopupMenuItem::new(m.label.clone()).with_icon("computer");
        if m.id == media.selected_medium {
            item = item.selected();
        }
        items.push(item);
        envs.push((m.id.clone(), m.label.clone()));
    }
    // The trailing "New environment…" entry; its index is `1 + envs.len()`.
    items.push(PopupMenuItem::new("New environment…").with_icon("plus"));

    // The tray warp-new shows beside the row under the pointer: the
    // environment's name and its "Make default", which makes that environment
    // the one new spaces start in — "+"'s pick — without opening anything.
    // Choosing the row itself opens a space on it instead.
    let tray_envs = envs.clone();
    let tray_default = media.selected_medium.clone();
    let on_select_medium = media_actions.on_select_medium.clone();
    let tray_open = state.env_selector_open.clone();

    let on_add_space = actions.on_add_space.clone();
    let on_add_space_with_medium = actions.on_add_space_with_medium.clone();
    let on_open_add_medium = actions.on_open_add_medium.clone();
    let menu = PopupMenu::new(trigger, items)
        .with_open(state.env_selector_open.clone())
        .with_hover_index(state.env_selector_hover.clone())
        .with_hover_tray(ENV_TRAY_WIDTH, move |index, app| {
            // Item 0 is the terminal and the trailing item adds an environment:
            // only the rows in between are environments, and only they carry a
            // tray.
            let (id, label) = tray_envs.get(index.checked_sub(1)?)?.clone();
            let is_default = id == tray_default;
            let select = on_select_medium.clone();
            let open = tray_open.clone();
            Some(build_environment_tray(app, &label, is_default, move || {
                (select.borrow_mut())(id.clone());
                // Setting the default closes the menu, as it does in warp-new.
                *open.borrow_mut() = false;
            }))
        })
        .with_on_select(move |index| {
            if index == 0 {
                (on_add_space.borrow_mut())();
            } else if index <= envs.len() {
                // An environment row opens a space running on that environment
                // (the tab lands at the end of the strip, as "+" does) instead
                // of only switching which environment is active.
                (on_add_space_with_medium.borrow_mut())(envs[index - 1].0.clone());
            } else {
                (on_open_add_medium.borrow_mut())();
            }
        })
        .finish();

    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm * 0.25)
        .with_child(plus)
        .with_child(menu)
        .finish()
}

/// The tray shown beside the environment row under the pointer: the
/// environment's name and a "Make default" button, disabled — with the reason,
/// as a tooltip — while it already is the environment new spaces start in.
fn build_environment_tray(
    app: &AppContext,
    label: &str,
    is_default: bool,
    on_make_default: impl FnMut() + 'static,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let title = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_child(
            Text::new(label.to_string())
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(12.0)
                .finish(),
        )
        .finish();
    let button = Button::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_child(
                Text::new("Make default")
                    .with_theme_color(
                        if is_default { ColorToken::Muted } else { ColorToken::Text },
                        app,
                    )
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish(),
    )
    .with_corner_radius(4.0)
    .with_disabled(is_default)
    .with_on_click(on_make_default)
    .finish();
    let button: Box<dyn Element> = if is_default {
        Tooltip::new(button, "Already the default").finish()
    } else {
        button
    };

    Container::new(
        Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm * 0.5)
            .with_child(title)
            .with_child(button)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(sm))
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_border(app.theme.color(ColorToken::Border).into())
    .with_corner_radius(6.0)
    .finish()
}

/// The tab index a pane released at `x` lands before: how many tab centers the
/// pointer is past, so `0` is left of every tab and `tabs.len()` right of them
/// all. This is the same count the tab reorder resolves a drag with, and it is
/// monotonic in `x`, so the marker never oscillates between two tabs around a
/// midpoint.
pub(super) fn insertion_index(tabs: &[RectF], x: f32) -> usize {
    tabs.iter().filter(|tab| x > tab.center().x).count()
}

/// Where insertion point `index` is marked: on the left edge of the tab it
/// precedes, and on the right edge of the last tab when the pane lands at the
/// end of the strip.
fn insertion_x(tabs: &[RectF], index: usize) -> f32 {
    match tabs.get(index) {
        Some(tab) => tab.min_x(),
        None => tabs.last().map(|tab| tab.max_x()).unwrap_or(0.0),
    }
}

/// Records the rectangle a tab was drawn at into the strip's registry while it
/// paints. Every tab of the strip is wrapped in one — the chip and the inline
/// rename field alike — so a drop's insertion index always names a position
/// among all the tabs, never one that is missing from the list.
struct TabRect {
    inner: Box<dyn Element>,
    index: usize,
    rects: Rc<RefCell<Vec<Option<RectF>>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TabRect {
    fn new(inner: Box<dyn Element>, index: usize, rects: Rc<RefCell<Vec<Option<RectF>>>>) -> Self {
        Self {
            inner,
            index,
            rects,
            size: None,
            origin: None,
        }
    }
}

impl Element for TabRect {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.inner.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.inner.paint(origin, ctx, app);
        if let Some(size) = self.size {
            if let Some(slot) = self.rects.borrow_mut().get_mut(self.index) {
                *slot = Some(rectf(origin.x, origin.y, size.x, size.y));
            }
        }
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
        self.inner.dispatch_event(event, ctx, app)
    }
}

/// The workspace tab strip as a drop target: a pane lifted out of its grid is
/// dropped between these tabs and becomes a tab of its own there.
///
/// The strip only resolves and reports the drop — the move itself is app
/// state's (`UiState::drop_pane`) and the ghost card is the bar's — which is
/// what lets the whole rule be tested without a pointer. It sees pointer events
/// before the shell body (the bar is layered above it), so while a pane is in
/// flight the strip holds the pointer; while nothing is in flight it forwards to
/// its tabs, whose own hit test is what makes a tab click win a press that lands
/// on a tab.
struct SpaceDropStrip {
    inner: Box<dyn Element>,
    /// The lifted pane, from app state; `None` when nothing is in flight.
    drag: Option<PaneDrag>,
    tab_rects: Rc<RefCell<Vec<Option<RectF>>>>,
    on_drag_move: Rc<RefCell<dyn FnMut(Vector2F, Option<usize>)>>,
    on_drop: Rc<RefCell<dyn FnMut(Option<usize>)>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SpaceDropStrip {
    fn new(
        inner: Box<dyn Element>,
        drag: Option<PaneDrag>,
        tab_rects: Rc<RefCell<Vec<Option<RectF>>>>,
        on_drag_move: Rc<RefCell<dyn FnMut(Vector2F, Option<usize>)>>,
        on_drop: Rc<RefCell<dyn FnMut(Option<usize>)>>,
    ) -> Self {
        Self {
            inner,
            drag,
            tab_rects,
            on_drag_move,
            on_drop,
            size: None,
            origin: None,
        }
    }

    /// The strip's bounds in window coordinates, known once it has been painted
    /// (which is also when the tabs' rectangles are written).
    fn bounds(&self) -> Option<RectF> {
        let origin = self.origin?;
        let size = self.size?;
        Some(rectf(origin.x(), origin.y(), size.x, size.y))
    }

    /// The tab rectangles as this frame's paint left them, in tab order.
    fn drawn_tabs(&self) -> Vec<RectF> {
        self.tab_rects.borrow().iter().flatten().copied().collect()
    }

    /// The insertion index a release at `position` names, or `None` when the
    /// pointer is not over the strip: a release there cancels the drag.
    fn drop_index_at(&self, position: Vector2F) -> Option<usize> {
        let bounds = self.bounds()?;
        if !contains(bounds, position) {
            return None;
        }
        Some(insertion_index(&self.drawn_tabs(), position.x))
    }
}

impl Element for SpaceDropStrip {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.inner.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.inner.paint(origin, ctx, app);
        // The tabs painted above, so the registry holds this frame's rectangles.
        let Some(drag) = self.drag.as_ref() else {
            return;
        };
        if !drag.moved() {
            return;
        }
        let Some(index) = drag.drop_index else {
            return;
        };
        let Some(bounds) = self.bounds() else {
            return;
        };
        let tabs = self.drawn_tabs();
        if tabs.is_empty() {
            return;
        }
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        // The strip lifts while a pane is over it, and the marker names the
        // boundary the pane lands on.
        renderer.fill_rect(bounds, app.theme.color(ColorToken::Selected));
        let x = insertion_x(&tabs, index).clamp(bounds.min_x(), bounds.max_x());
        renderer.fill_rect(
            rectf(x - 1.0, bounds.min_y(), 2.0, bounds.height()),
            app.theme.color(ColorToken::Accent),
        );
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
        if self.drag.is_some() {
            match event {
                DispatchedEvent::MouseMove { position } => {
                    let index = self.drop_index_at(*position);
                    (self.on_drag_move.borrow_mut())(*position, index);
                    return true;
                }
                DispatchedEvent::MouseUp { position, .. } => {
                    let index = self.drop_index_at(*position);
                    (self.on_drop.borrow_mut())(index);
                    return true;
                }
                // A second press while a pane is in flight ends the drag, the
                // way a release off the strip would, instead of starting a
                // second lift.
                DispatchedEvent::MouseDown { .. } => {
                    (self.on_drop.borrow_mut())(None);
                    return true;
                }
                _ => {}
            }
        }
        self.inner.dispatch_event(event, ctx, app)
    }
}

/// The card a lifted pane drags under the pointer: the pane's own title on the
/// raised surface, so the drag reads as carrying that pane. It is zero-sized —
/// the bar keeps its height — and only draws, at the pointer.
struct PaneDragGhost {
    card: Option<Box<dyn Element>>,
    position: Option<Vector2F>,
    size: Option<Vector2F>,
}

impl Element for PaneDragGhost {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        if let Some(card) = self.card.as_mut() {
            let _ = card.layout(constraint, ctx, app);
        }
        self.size = Some(Vector2F::zero());
        Vector2F::zero()
    }

    fn paint(&mut self, _origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        let (Some(card), Some(position)) = (self.card.as_mut(), self.position) else {
            return;
        };
        card.paint(
            position + vec2f(GHOST_OFFSET_X, GHOST_OFFSET_Y),
            ctx,
            app,
        );
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        None
    }

    fn dispatch_event(
        &mut self,
        _event: &DispatchedEvent,
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        false
    }
}

/// Build the ghost a lifted pane draws, or nothing at all when no pane is in
/// flight — or when the press has not travelled far enough to be a drag.
fn build_pane_drag_ghost(app: &AppContext, drag: Option<&PaneDrag>) -> Box<dyn Element> {
    let moved = drag.map(|drag| drag.moved()).unwrap_or(false);
    let Some(drag) = drag.filter(|_| moved) else {
        return Empty::new().with_size(vec2f(0.0, 0.0)).finish();
    };
    let pad = app.theme.spacing_px(SpacingToken::Sm);
    let card: Box<dyn Element> = Container::new(
        Text::new(drag.title.clone())
            .with_theme_color(ColorToken::Text, app)
            .with_font_size(12.0)
            .with_max_lines(1)
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(pad))
    .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
    .with_border(app.theme.color(ColorToken::Accent).into())
    .with_corner_radius(6.0)
    .finish();
    Box::new(PaneDragGhost {
        card: Some(
            ConstrainedBox::new(card)
                .with_max_width(GHOST_MAX_WIDTH)
                .finish(),
        ),
        position: Some(drag.position),
        size: None,
    })
}
