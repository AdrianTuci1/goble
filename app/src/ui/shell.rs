//! Shell layout: topbar, main content switcher, and the fixed-width
//! sidebar+main split.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Button, ButtonVariant, ConstrainedBox, Container, CrossAxisAlignment, EdgeInsets,
    Element, EventContext, Fill, Flex, Icon, LayoutContext, MainAxisAlignment, PaintContext, Point,
    SizeConstraint, Text, TopbarButton,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::panes;
use super::projects::build_projects_view;
use super::space_bar::SpaceBar;
use super::{
    AppTab, MediaActions, MediaSnapshot, ProjectsActions, ProjectsSnapshot, UiActions,
    UiSnapshot,
};

// The general toolbar doubles as the OS titlebar on macOS, so it is a touch
// shorter and leaves room for the traffic lights on the left.
#[cfg(target_os = "macos")]
const TOPBAR_HEIGHT: f32 = 36.0;
#[cfg(not(target_os = "macos"))]
const TOPBAR_HEIGHT: f32 = 40.0;
#[cfg(target_os = "macos")]
const TOPBAR_TRAFFIC_INSET: f32 = 76.0;
#[cfg(not(target_os = "macos"))]
const TOPBAR_TRAFFIC_INSET: f32 = 0.0;

/// Single general toolbar: menu + projects on the left, the space tab strip and
/// the environment controls in the middle, inbox + settings on the right.
///
/// The whole toolbar is one row so there is a single visible general toolbar
/// (the space strip used to be a separate row below it). The "+" in the
/// environment controls opens a warp-new style menu to add a space in a chosen
/// environment, or add a new environment by name.
pub fn build_topbar(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    media: &MediaSnapshot,
    media_actions: &MediaActions,
) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    // `md` is only used as the vertical toolbar padding on non-macOS platforms
    // (on macOS the toolbar doubles as the OS titlebar and drops the padding).
    #[cfg_attr(target_os = "macos", allow(unused_variables))]
    let md = app.theme.spacing_px(SpacingToken::Md);

    // The Settings surface is disabled in this build: the tab renders inactive
    // (flat, no click) and the app never navigates to it. See `build_main`.
    let projects_active = state.current_tab == AppTab::Projects;

    let on_menu = actions.on_menu.clone();
    let menu_icon = if projects_active { "arrow-left" } else { "menu-01" };
    let menu_button = TopbarButton::new(
        Icon::new(menu_icon)
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_on_click(move || (on_menu.borrow_mut())())
    .finish();

    let on_projects = actions.on_projects.clone();
    let projects_button = TopbarButton::new(
        Icon::new("layers-three-01")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_active(projects_active)
    .with_on_click(move || (on_projects.borrow_mut())())
    .finish();

    let left = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(menu_button)
        .with_child(projects_button)
        .finish();

    let on_inbox = actions.on_inbox.clone();
    let inbox_button = TopbarButton::new(
        Icon::new("inbox-01")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_on_click(move || (on_inbox.borrow_mut())())
    .finish();

    let on_settings = actions.on_settings.clone();
    let settings_button = TopbarButton::new(
        Icon::new("settings")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_disabled(true)
    .with_on_click(move || (on_settings.borrow_mut())())
    .finish();

    let right = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(inbox_button)
        .with_child(settings_button)
        .finish();

    // Middle of the toolbar: the space tab strip + the environment controls
    // (medium selector and the "+" add-space menu).
    let space_bar = build_space_tabs(state, actions);
    let medium_controls =
        super::media::build_medium_controls(app, state, media, media_actions, actions);
    let center = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(space_bar)
        .with_child(medium_controls)
        .finish();

    let row = Flex::row()
        .with_main_axis_size(goble_ui::elements::MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(left)
        .with_child(center)
        .with_child(right)
        .finish();

    // On macOS the toolbar doubles as the titlebar: drop the vertical padding so
    // the actions sit vertically centered next to the traffic lights. Elsewhere
    // keep the padded toolbar look.
    #[cfg(target_os = "macos")]
    let vertical_padding = 0.0;
    #[cfg(not(target_os = "macos"))]
    let vertical_padding = md;

    Container::new(ConstrainedBox::new(row).with_height(TOPBAR_HEIGHT).finish())
        .with_padding(EdgeInsets::new(
            TOPBAR_TRAFFIC_INSET,
            vertical_padding,
            0.0,
            vertical_padding,
        ))
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .finish()
}

/// The space tab strip: one tab per space, draggable to reorder, selectable to
/// switch. The trailing "+" lives in the environment controls (it opens the
/// add-space / add-medium menu) so the strip itself has no add button.
fn build_space_tabs(state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let on_select_space = actions.on_select_space.clone();
    let on_space_press = actions.on_space_press.clone();
    let on_space_hover = actions.on_space_hover.clone();
    let on_space_reorder = actions.on_space_reorder.clone();
    let on_space_release = actions.on_space_release.clone();
    let names = state.spaces.iter().map(|s| s.name.clone()).collect();
    Box::new(
        SpaceBar::new(names, state.active_space)
            .with_hover(state.space_hover)
            .with_press(state.space_press)
            .with_drag(state.space_drag)
            .with_on_select(move |index| (on_select_space.borrow_mut())(index))
            .with_on_space_press(move |index| (on_space_press.borrow_mut())(index))
            .with_on_space_hover(move |hover| (on_space_hover.borrow_mut())(hover))
            .with_on_space_reorder(move |from, to| (on_space_reorder.borrow_mut())(from, to))
            .with_on_space_release(move || (on_space_release.borrow_mut())()),
    )
}

/// Main content area: threads placeholder, terminal/chat, settings, or the
/// per-project observability panel. Navigation is driven by the topbar buttons.
pub fn build_main(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    projects: &ProjectsSnapshot,
    projects_actions: &ProjectsActions,
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
