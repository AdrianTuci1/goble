use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, ConstrainedBox, Container, CrossAxisAlignment, Divider, EdgeInsets, Element,
    Empty, EventContext, Fill, Flex, Icon, LayoutContext, MainAxisAlignment, MainAxisSize,
    PaintContext,
    Point, PopupMenu, PopupMenuItem, RunningIndicator, SizeConstraint, Text, TextInput,
    TopbarButton,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, Vector2F};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::super::{MediaActions, MediaSnapshot, UiActions, UiSnapshot};

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
    Container::new(row)
        .with_padding(EdgeInsets::new(TOPBAR_TRAFFIC_INSET, 0.0, 0.0, 0.0))
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
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
            Text::new(format!("◆ {work_count}"))
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
    for (index, space) in state.spaces.iter().enumerate() {
        // One rule per boundary: it belongs to the tab that follows it, so N
        // tabs draw N-1 lines and no boundary carries two.
        if index > 0 {
            row = row.with_child(Divider::vertical().finish());
        }
        let active = index == state.active_space;
        if active && state.space_rename_editing {
            row = row.with_child(build_rename_tab(app, state, actions));
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
    ConstrainedBox::new(row.finish())
        .with_height(TOPBAR_HEIGHT)
        .finish()
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
        self.close.paint(
            vec2f(
                origin.x + Self::h_pad() + child_size.x + Self::close_gap(),
                origin.y + (size.y - close_size.y).max(0.0) / 2.0,
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
