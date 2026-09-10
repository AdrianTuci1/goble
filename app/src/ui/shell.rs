//! Shell layout: topbar, main content switcher, and the fixed-width
//! sidebar+main split.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Button, ButtonVariant, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets,
    Element, EventContext, Fill, Flex, Icon, LayoutContext, MainAxisAlignment, PaintContext, Point,
    PopupMenu, PopupMenuItem, SizeConstraint, Text, TextInput, TopbarButton,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::connectors::build_connectors_page;
use super::harness;
use super::panes;
use super::projects::build_projects_view;
use super::{
    AiActions, AiSnapshot, AppTab, MediaActions, MediaSnapshot, ProjectsActions,
    ProjectsSnapshot, UiActions, UiSnapshot,
};

// The general toolbar doubles as the OS titlebar on macOS, so it is a touch
// shorter and leaves room for the traffic lights on the left. Shared with the
// pane headers (terminal/agent) so the general toolbar and the per-pane topbar
// render at the same height.
#[cfg(target_os = "macos")]
pub const TOPBAR_HEIGHT: f32 = 36.0;
#[cfg(not(target_os = "macos"))]
pub const TOPBAR_HEIGHT: f32 = 40.0;

/// Height of the toolbar's tallest control (`TopbarButton`'s default size). The
/// toolbar pads itself out to exactly [`TOPBAR_HEIGHT`] around it, so it lines
/// up with the per-pane headers instead of hugging its controls.
const TOPBAR_CONTROL_HEIGHT: f32 = 32.0;
#[cfg(target_os = "macos")]
const TOPBAR_TRAFFIC_INSET: f32 = 76.0;
#[cfg(not(target_os = "macos"))]
const TOPBAR_TRAFFIC_INSET: f32 = 0.0;

/// Single general toolbar, reduced to the essentials: a sidebar toggle, the
/// workspace chips (one per space, the active one editable inline), a "+ ▾"
/// control that opens a new workspace / picks the environment it runs in, and
/// the Settings icon pinned to the far right.
///
/// The chips sit flush next to the toggle with only a couple of points of space
/// between them (the Warp workspace row), so a new workspace shows up *next to*
/// the current one instead of replacing it.
///
/// The "+" opens a new workspace in the active environment; the "▾" lists the
/// environments (the VMs the user added appear by name) and a trailing
/// "New environment…" entry, so the pick decides where new workspaces run.
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

    // One chip per workspace, active one first-class (double-click renames it).
    let workspaces = build_workspace_strip(app, state, actions);

    // "+ ▾": open a new workspace, or pick the environment it runs in.
    let add_workspace = build_add_workspace_control(app, state, media, media_actions, actions);

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
        .with_child(settings_button)
        .finish();

    // Pad the bar out to exactly `TOPBAR_HEIGHT` around its tallest control so
    // it is as tall as a pane header (the body reserves the same height below
    // it), and keep the macOS traffic lights vertically centered inside it.
    let vertical_padding = ((TOPBAR_HEIGHT - TOPBAR_CONTROL_HEIGHT) / 2.0).max(0.0);

    Container::new(row)
        .with_padding(EdgeInsets::new(
            TOPBAR_TRAFFIC_INSET,
            vertical_padding,
            0.0,
            vertical_padding,
        ))
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .finish()
}

/// The workspace row: one chip per space, in order, laid out flush with no
/// leading indent (Warp-style). The active chip is highlighted and, while
/// renaming, swaps for the inline name field.
fn build_workspace_strip(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(2.0);
    for (index, space) in state.spaces.iter().enumerate() {
        let active = index == state.active_space;
        if active && state.space_rename_editing {
            row = row.with_child(build_rename_field(app, state, actions));
        } else {
            row = row.with_child(build_workspace_chip(
                app,
                actions,
                index,
                &space.name,
                active,
            ));
        }
    }
    row.finish()
}

/// One clickable workspace chip: the name plus an "x" that closes that
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

/// The inline rename field shown in place of the active workspace chip.
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

/// A flat workspace chip: no idle background, a hover fill, and (when active) a
/// raised fill with a border and the themed text color. Tight padding keeps the
/// row flush, like Warp's workspace row. The trailing "x" closes the workspace;
/// it is hit-tested before the chip body so it never doubles as a select click.
struct WorkspaceChip {
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
    fn new(
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

    fn h_pad() -> f32 {
        7.0
    }

    fn v_pad() -> f32 {
        3.0
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
                (constraint.max.y - Self::v_pad() * 2.0).max(0.0),
            ),
        );
        let child_size = self.child.layout(inner, ctx, app);
        let close_size = self.close.layout(inner, ctx, app);
        let size = vec2f(
            child_size.x + Self::close_gap() + close_size.x + Self::h_pad() * 2.0,
            child_size.y.max(close_size.y) + Self::v_pad() * 2.0,
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let rect = rectf(origin.x, origin.y, size.x, size.y);
        let radius = app.theme.radius_px();
        let hovered = ctx.hovered(rect);
        if let Some(renderer) = ctx.renderer.as_mut() {
            if self.active {
                renderer.fill_rounded_rect(rect, app.theme.color(ColorToken::SurfaceRaised), radius);
                renderer.stroke_rect(rect, app.theme.color(ColorToken::Border), 1.0, radius);
            } else if hovered {
                renderer.fill_rounded_rect(rect, app.theme.color(ColorToken::Hover), radius);
            }
        }
        let child_size = self.child.size().unwrap_or(Vector2F::zero());
        let close_size = self.close.size().unwrap_or(Vector2F::zero());
        let inner_height = size.y - Self::v_pad() * 2.0;
        self.child.paint(
            vec2f(
                origin.x + Self::h_pad(),
                origin.y + Self::v_pad() + (inner_height - child_size.y).max(0.0) / 2.0,
            ),
            ctx,
            app,
        );
        self.close.paint(
            vec2f(
                origin.x + Self::h_pad() + child_size.x + Self::close_gap(),
                origin.y + Self::v_pad() + (inner_height - close_size.y).max(0.0) / 2.0,
            ),
            ctx,
            app,
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
        // The close "x" gets the event first: it only consumes clicks inside its
        // own bounds, so clicking the name still selects the workspace.
        if self.close.dispatch_event(event, ctx, app) {
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
/// "▾" opens the environment menu (the environments, plus "New environment…").
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

    let mut items = Vec::new();
    let mut ids = Vec::new();
    for m in &media.mediums {
        let mut item = PopupMenuItem::new(m.label.clone()).with_icon("computer");
        if m.id == media.selected_medium {
            item = item.selected();
        }
        items.push(item);
        ids.push(m.id.clone());
    }
    // The trailing "New environment…" entry; its index is `ids.len()`.
    items.push(PopupMenuItem::new("New environment…").with_icon("plus"));

    let on_select_medium = media_actions.on_select_medium.clone();
    let on_open_add_medium = actions.on_open_add_medium.clone();
    let menu = PopupMenu::new(trigger, items)
        .with_open(state.env_selector_open.clone())
        .with_on_select(move |index| {
            if index < ids.len() {
                (on_select_medium.borrow_mut())(ids[index].clone());
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

/// Main content area: threads placeholder, terminal/chat, settings, or the
/// per-project observability panel. Navigation is driven by the topbar buttons.
pub fn build_main(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    projects: &ProjectsSnapshot,
    projects_actions: &ProjectsActions,
    ai: &AiSnapshot,
    ai_actions: &AiActions,
    _media: &MediaSnapshot,
    _media_actions: &MediaActions,
) -> Box<dyn Element> {
    match state.current_tab {
        // The chat/terminal area is the active space's pane tree; the medium
        // selector lives in the topbar, so the whole main column is panes.
        AppTab::Chat => panes::build_active_space(app, state, actions),
        // The Settings surface is disabled in this build: reachable only if a
        // stale tab flag survives, so we render a dead-end instead of the view.
        AppTab::Settings => build_settings_disabled(app, actions),
        AppTab::Projects => build_projects_view(app, projects, projects_actions),
        // Harness observability pages, driven by real daemon/store data.
        AppTab::Workflows => harness::build_workflows_page(app, state, actions),
        AppTab::Executions => harness::build_executions_page(app, state, actions),
        AppTab::Timeline => harness::build_timeline_page(app, state, actions),
        AppTab::Costs => harness::build_costs_page(app, state, actions),
        AppTab::Mcps => build_connectors_page(app, ai, ai_actions, &actions.on_settings_back),
    }
}

/// A dead-end panel shown if the (disabled) Settings tab is ever reached. The
/// Settings surface is inactive in this build, so this only backs out to Chat.
fn build_settings_disabled(app: &AppContext, actions: &UiActions) -> Box<dyn Element> {
    let on_back = actions.on_settings_back.clone();
    let back = Button::new(Text::new("← Back").with_theme_color(ColorToken::Text, app).finish())
        .with_variant(ButtonVariant::Ghost)
        .with_on_click(move || (on_back.borrow_mut())())
        .finish();
    let md = app.theme.spacing_px(SpacingToken::Md);
    let body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(md)
        .with_child(back)
        .with_child(
            Text::new("Settings")
                .with_theme_color(ColorToken::Text, app)
                .with_font_size(16.0)
                .finish(),
        )
        .with_child(
            Text::new("Settings is disabled in this version.")
                .with_theme_color(ColorToken::Muted, app)
                .with_font_size(12.0)
                .finish(),
        )
        .finish();
    Container::new(body)
        .with_padding(goble_ui::elements::EdgeInsets::uniform(md))
        .finish()
}

/// Splits horizontal space: fixed-width sidebar + main area filling the rest.
/// The engine's `Flex` cannot yet distribute remaining space, so this custom
/// element does the split at layout time.
///
/// A draggable divider sits on the sidebar's right edge so the user can resize
/// it. The width lives in app state (it must survive the per-frame rebuild),
/// and the drag flags/callbacks are passed in from the snapshot.
const RESIZE_HANDLE_HALF: f32 = 4.0;

pub struct SidebarLayout {
    sidebar: Box<dyn Element>,
    main: Box<dyn Element>,
    width: f32,
    dragging: bool,
    on_drag_start: Option<Rc<RefCell<dyn FnMut(f32)>>>,
    on_drag_move: Option<Rc<RefCell<dyn FnMut(f32)>>>,
    on_drag_end: Option<Rc<RefCell<dyn FnMut()>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SidebarLayout {
    pub fn new(sidebar: Box<dyn Element>, main: Box<dyn Element>, width: f32) -> Self {
        Self {
            sidebar,
            main,
            width,
            dragging: false,
            on_drag_start: None,
            on_drag_move: None,
            on_drag_end: None,
            size: None,
            origin: None,
        }
    }

    /// Whether a drag is in progress (carried from app state so it survives
    /// the per-frame rebuild).
    pub fn with_dragging(mut self, dragging: bool) -> Self {
        self.dragging = dragging;
        self
    }

    pub fn with_on_drag_start<F: FnMut(f32) + 'static>(mut self, callback: F) -> Self {
        self.on_drag_start = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_drag_move<F: FnMut(f32) + 'static>(mut self, callback: F) -> Self {
        self.on_drag_move = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_drag_end<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_drag_end = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Bounds of the resizable divider, in absolute window coordinates.
    fn handle_bounds(&self) -> Option<RectF> {
        let origin = self.origin?;
        let size = self.size?;
        Some(rectf(
            origin.x() + self.width - RESIZE_HANDLE_HALF,
            origin.y(),
            RESIZE_HANDLE_HALF * 2.0,
            size.y,
        ))
    }
}

impl Element for SidebarLayout {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let sidebar_constraint =
            SizeConstraint::new(vec2f(0.0, 0.0), vec2f(self.width, constraint.max.y));
        let _ = self.sidebar.layout(sidebar_constraint, ctx, app);

        let main_width = (constraint.max.x - self.width).max(0.0);
        let main_constraint =
            SizeConstraint::new(vec2f(0.0, 0.0), vec2f(main_width, constraint.max.y));
        let _ = self.main.layout(main_constraint, ctx, app);

        let size = vec2f(constraint.max.x, constraint.max.y);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.sidebar.paint(origin, ctx, app);
        self.main.paint(origin + vec2f(self.width, 0.0), ctx, app);

        // Resizable divider along the sidebar's right edge; highlights while
        // dragging so the user can see where the grab point is.
        let height = self.size.map(|s| s.y).unwrap_or(0.0);
        let color = if self.dragging {
            app.theme.color(ColorToken::Accent)
        } else {
            app.theme.color(ColorToken::Border)
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rounded_rect(
                rectf(origin.x + self.width - 0.5, origin.y, 1.0, height),
                color,
                0.0,
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
        // While dragging, consume move/up events so the split updates even when
        // the pointer is over the main content.
        if self.dragging {
            match event {
                DispatchedEvent::MouseMove { position } => {
                    if let Some(cb) = &self.on_drag_move {
                        (cb.borrow_mut())(position.x);
                    }
                    return true;
                }
                DispatchedEvent::MouseUp { .. } => {
                    if let Some(cb) = &self.on_drag_end {
                        (cb.borrow_mut())();
                    }
                    return true;
                }
                _ => {}
            }
        }
        // Start a drag when the pointer grabs the divider.
        if let DispatchedEvent::MouseDown { position, .. } = event {
            if let Some(bounds) = self.handle_bounds() {
                if contains(bounds, *position) {
                    if let Some(cb) = &self.on_drag_start {
                        (cb.borrow_mut())(position.x);
                    }
                    return true;
                }
            }
        }
        if self.sidebar.dispatch_event(event, ctx, app) {
            return true;
        }
        self.main.dispatch_event(event, ctx, app)
    }
}
