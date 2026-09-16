use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    file_icon_name, AppContext, Button, ConstrainedBox, Container, CrossAxisAlignment, Divider,
    EdgeInsets, Element, Empty, EventContext, Fill, Flex, Icon, LayoutContext, MainAxisAlignment,
    MainAxisSize, PaintContext, Point, PopupMenu, PopupMenuItem, RunningIndicator, SizeConstraint,
    Text, TextInput, Tooltip, TooltipPosition, TopbarButton,
};
use goble_ui::event::{DispatchedEvent, BUTTON_PRIMARY, BUTTON_SECONDARY};
use goble_ui::elements::interactive::contains;
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::theme::{ColorToken, SpacingToken, TabColor, TabTint};

use crate::state::PaneDrag;

use super::super::tab_menu::{build_tab_menu, TabMenuTab, TabMenuAction};
use super::super::{file_view, MediaActions, MediaSnapshot, UiActions, UiSnapshot};

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

/// The width a tab of the bar is never drawn narrower than, while the bar's
/// room can afford it: warp-new's own minimum for a tab slot
/// (`app/src/workspace/view.rs:19061`, `.with_min_width(80.)`).
const MIN_TAB_WIDTH: f32 = 80.0;

/// The width a tab never grows past, however wide the window is: warp-new caps
/// a tab at 200 (`app/src/tab.rs:1841`, `.with_max_width(200.)`).
const MAX_TAB_WIDTH: f32 = 200.0;

/// The thickness of the rule between two neighbouring tabs. `Divider`'s own
/// default, named here because the strip takes the rules out of its room before
/// it divides that room between the tabs.
const TAB_RULE_WIDTH: f32 = 1.0;

/// The width one tab of the bar is drawn at, given the room the bar has for
/// tabs (`room`: the window's width less everything the bar draws itself) and
/// how many tabs are on it — an equal share of that room, capped at
/// [`MAX_TAB_WIDTH`] and, while the share can afford it, floored at
/// [`MIN_TAB_WIDTH`].
///
/// A bar too crowded to give every tab the minimum divides the room it has
/// instead: warp-new's tabs share the bar the same way (each one a `Shrinkable`
/// of the row, capped at 200), and that is what keeps a rounded-up tab from
/// being pushed past the bar's own controls.
fn tab_slot(room: f32, tabs: usize) -> f32 {
    let share = room.max(0.0) / tabs.max(1) as f32;
    if share >= MIN_TAB_WIDTH {
        share.min(MAX_TAB_WIDTH)
    } else {
        share
    }
}

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

    // One tab per workspace, active one first-class (double-click renames it),
    // and — past the strip's own rule — one per file the active space shows.
    // Both frames are laid out by the strip as one row of tabs, so a file's tab
    // is exactly as wide as a workspace's (see [`TabStrip`]).
    let mut frames = vec![build_workspace_strip(app, state, actions)];
    if let Some(files) = build_file_tabs(app, state, actions) {
        frames.push(files);
    }
    let workspaces = TabStrip::new(frames).finish();

    // "+ ▾": open a new workspace, or pick the environment it runs in.
    let add_workspace = build_add_workspace_control(app, state, media, media_actions, actions);

    // The live cue: a spinner plus the count of work in flight (C1's state),
    // between the workspace controls and the settings icon. It renders nothing
    // at zero, so the bar keeps its current look when nothing is running. It is
    // information only — no click opens anything from it.
    let live_indicator = build_live_indicator(app, state.live_work_count, state.live_work_phase);

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

    // The strip's tabs share the room the bar has left for them, so the row is
    // laid out by the bar itself rather than by a plain flex row: the strip is
    // given what the toggle, the "+ ▾" control, the live cue and Settings leave
    // behind, and the controls keep the positions they had (see [`TopbarRow`]).
    let row = TopbarRow::new(
        vec![sidebar_button],
        workspaces,
        vec![add_workspace],
        vec![live_indicator, settings_button],
    )
    .with_left_spacing(sm * 0.5)
    .with_group_spacing(sm)
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
    // zero-sized child of a stack rather than part of the row. The tab menu is
    // the third: hung from the pointer it was opened at, it floats over the bar
    // and over the pane below it, and it is the first child dispatched, so the
    // press that lands on it never reaches the tab under it.
    let menu = build_space_menu(state, actions);
    goble_ui::elements::Stack::new()
        .with_children(vec![
            bar,
            build_pane_drag_ghost(app, state.pane_drag.as_ref()),
            menu,
        ])
        .finish()
}

/// The menu of the tab a right click opened, or nothing at all while none is
/// open. The panel hangs from the point the click landed on, over the window —
/// see [`crate::ui::tab_menu`].
fn build_space_menu(state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let Some(menu) = *state.space_menu.borrow() else {
        return Empty::new().with_size(vec2f(0.0, 0.0)).finish();
    };
    let Some(space) = state.spaces.get(menu.index) else {
        return Empty::new().with_size(vec2f(0.0, 0.0)).finish();
    };
    let tab = TabMenuTab {
        index: menu.index,
        tabs: state.spaces.len(),
        color: space.color,
        named: space.named,
    };
    let on_action = actions.on_space_menu_action.clone();
    let on_close = actions.on_space_menu_close.clone();
    let index = menu.index;
    build_tab_menu(
        tab,
        menu.at,
        Rc::new(RefCell::new(move |action: TabMenuAction| {
            (on_action.borrow_mut())(index, action)
        })),
        Rc::new(RefCell::new(move || (on_close.borrow_mut())())),
    )
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

/// The font size every tab of the bar reads at.
const TAB_FONT_SIZE: f32 = 12.0;

/// What a name cut down to its tab ends with.
const ELLIPSIS: &str = "…";

/// One tab's name, drawn on a single line: the name whole while the tab is wide
/// enough to draw it, and cut from its end with an ellipsis when it is not — the
/// way warp-new truncates a tab title — so a bar that has to share its room
/// shortens the names instead of wrapping them out of the bar.
struct TabLabel {
    name: String,
    color: ColorToken,
    /// The run drawn this frame: the name whole, or as much of it as fits.
    run: Option<Text>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TabLabel {
    fn new(name: impl Into<String>, color: ColorToken) -> Self {
        Self {
            name: name.into(),
            color,
            run: None,
            size: None,
            origin: None,
        }
    }

    /// The one-line run that draws `name` at a tab's font size.
    fn run(&self, name: &str, app: &AppContext) -> Text {
        Text::new(name)
            .with_theme_color(self.color, app)
            .with_font_size(TAB_FONT_SIZE)
            .with_max_lines(1)
    }

    /// How wide `name` is drawn on one line.
    fn width_of(&self, name: &str, app: &AppContext) -> f32 {
        let mut run = self.run(name, app);
        run.layout(
            SizeConstraint::new(Vector2F::zero(), vec2f(f32::INFINITY, f32::INFINITY)),
            &mut LayoutContext::default(),
            app,
        )
        .x
    }

    /// The name as the run that fits `room` draws it: whole when it fits, and
    /// otherwise as much of it as leaves room for the ellipsis that replaces the
    /// rest. Nothing at all when there is no room.
    fn fit(&self, room: f32, app: &AppContext) -> String {
        let width = self.width_of(&self.name, app);
        if width <= room {
            return self.name.clone();
        }
        if room <= 0.0 {
            return String::new();
        }
        let chars: Vec<char> = self.name.chars().collect();
        // The first guess comes from the run's own width per character; the walk
        // back from there is what pays for the ellipsis' own room.
        let per_char = (width / chars.len().max(1) as f32).max(1.0);
        let mut keep = ((room / per_char) as usize).min(chars.len().saturating_sub(1));
        while keep > 1 {
            let candidate = format!("{}{ELLIPSIS}", chars[..keep].iter().collect::<String>());
            if self.width_of(&candidate, app) <= room {
                return candidate;
            }
            keep -= 1;
        }
        ELLIPSIS.to_string()
    }
}

impl Element for TabLabel {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let name = self.fit(constraint.max.x, app);
        let mut run = self.run(&name, app);
        let size = run.layout(constraint, ctx, app);
        self.run = Some(run);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(run) = self.run.as_mut() {
            run.paint(origin, ctx, app);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

/// The bar's tab area: the frame of workspace tabs and, when the active space
/// shows files, the frame of their tabs, drawn side by side.
///
/// It exists to hand the frames the room the bar has for tabs — the window's
/// width less everything the bar draws itself, which [`TopbarRow`] is what
/// measures. The room is sliced *by tab*, not by frame, so a file's tab is
/// exactly as wide as a workspace's; each frame is then given the slice its own
/// tabs and rules need, and a frame draws no more than the tabs it was given
/// room for (see [`TabFrame`]).
struct TabStrip {
    frames: Vec<StripFrame>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

/// One frame of the tab strip: the frame's own element, and the counts the
/// strip slices its room with — how many tabs the frame draws and how many
/// rules stand between them. The frame reads neither of them from its element:
/// the element keeps them to itself.
struct StripFrame {
    element: Box<dyn Element>,
    tabs: usize,
    rules: usize,
}

impl StripFrame {
    fn new(element: Box<dyn Element>, tabs: usize, rules: usize) -> Self {
        Self {
            element,
            tabs,
            rules,
        }
    }
}

/// The rule between two neighbouring tabs, as thick as [`TAB_RULE_WIDTH`] says.
fn tab_rule() -> Box<dyn Element> {
    Divider::vertical().with_thickness(TAB_RULE_WIDTH).finish()
}

impl TabStrip {
    fn new(frames: Vec<StripFrame>) -> Self {
        Self {
            frames,
            size: None,
            origin: None,
        }
    }
}

impl Element for TabStrip {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let tabs: usize = self.frames.iter().map(|frame| frame.tabs).sum();
        let rules: usize = self.frames.iter().map(|frame| frame.rules).sum();
        let slot = tab_slot(
            constraint.max.x - rules as f32 * TAB_RULE_WIDTH,
            tabs,
        );
        let mut total = Vector2F::zero();
        for frame in &mut self.frames {
            let room = slot * frame.tabs as f32 + frame.rules as f32 * TAB_RULE_WIDTH;
            let size = frame.element.layout(
                SizeConstraint::loose(vec2f(room, constraint.max.y)),
                ctx,
                app,
            );
            total.x += size.x;
            total.y = total.y.max(size.y);
        }
        self.size = Some(total);
        total
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let mut x = origin.x;
        for frame in &mut self.frames {
            frame.element.paint(vec2f(x, origin.y), ctx, app);
            x += frame.element.size().unwrap_or(Vector2F::zero()).x;
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
        self.frames
            .iter_mut()
            .any(|frame| frame.element.dispatch_event(event, ctx, app))
    }
}

/// One frame of tabs: its tabs laid out in slots of equal width — every tab of
/// the bar an equal share of the room the bar has for tabs, floored at
/// [`MIN_TAB_WIDTH`] while the room can afford it and capped at
/// [`MAX_TAB_WIDTH`] — with [`TAB_RULE_WIDTH`] of rule between neighbours, and
/// as tall as the bar.
struct TabFrame {
    children: Vec<FrameChild>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

/// A child of a [`TabFrame`]: one of its tabs, or the rule that stands between
/// two of them.
enum FrameChild {
    Tab(Box<dyn Element>),
    Rule(Box<dyn Element>),
}

impl TabFrame {
    fn new(children: Vec<FrameChild>) -> Self {
        Self {
            children,
            size: None,
            origin: None,
        }
    }

    fn tab_count(&self) -> usize {
        self.children
            .iter()
            .filter(|child| matches!(child, FrameChild::Tab(_)))
            .count()
    }

    fn rule_count(&self) -> usize {
        self.children
            .iter()
            .filter(|child| matches!(child, FrameChild::Rule(_)))
            .count()
    }
}

impl Element for TabFrame {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // The slot every tab of the frame is drawn in: an equal share of the
        // room the frame was handed, once the rules between the tabs are out of
        // it. The floor yields to the share on a bar that cannot afford the
        // minimum for every tab, so the tabs stay inside the room they were
        // given.
        let slot = tab_slot(
            constraint.max.x - self.rule_count() as f32 * TAB_RULE_WIDTH,
            self.tab_count(),
        );
        let floor = MIN_TAB_WIDTH.min(slot);
        let mut total = 0.0;
        for child in &mut self.children {
            let size = match child {
                // A tab takes what its name needs between the readable minimum
                // and its slot, and never more than its slot.
                FrameChild::Tab(tab) => tab.layout(
                    SizeConstraint::new(vec2f(floor, 0.0), vec2f(slot, WorkspaceChip::height())),
                    ctx,
                    app,
                ),
                FrameChild::Rule(rule) => rule.layout(
                    SizeConstraint::loose(vec2f(TAB_RULE_WIDTH, WorkspaceChip::height())),
                    ctx,
                    app,
                ),
            };
            total += size.x;
        }
        let size = vec2f(total, WorkspaceChip::height());
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let mut x = origin.x;
        for child in &mut self.children {
            let element = match child {
                FrameChild::Tab(tab) => tab,
                FrameChild::Rule(rule) => rule,
            };
            element.paint(vec2f(x, origin.y), ctx, app);
            x += element.size().unwrap_or(Vector2F::zero()).x;
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
        for child in &mut self.children {
            let element = match child {
                FrameChild::Tab(tab) => tab,
                FrameChild::Rule(rule) => rule,
            };
            if element.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        false
    }
}

/// The bar's one row: the sidebar toggle, the tab strip, the "+ ▾" control, and
/// then the live cue and Settings spread to the right.
///
/// The engine's `Flex` gives a child either no bound along its row
/// (`MainAxisSize::Min`) or the whole row's width to grow into, so the strip
/// could not tell a wide window from a crowded bar — and a strip that was not
/// told took the room it wanted and pushed the "+ ▾" control and Settings off
/// the window. So the bar lays its own row out: the controls first, at their own
/// widths, and the strip into the room that is left for it. Everything else is
/// the flex row it replaces: the left group flush with `left_spacing`, the cue
/// and Settings spread with `group_spacing` between them, every child centered
/// on the bar's height (`SidebarLayout` solves the same problem for the shell's
/// split).
struct TopbarRow {
    /// The controls before the strip, in draw order.
    leading: Vec<Box<dyn Element>>,
    /// The tab strip: the one child that takes the room the rest leaves.
    strip: Box<dyn Element>,
    /// The controls after the strip, before the cue and Settings.
    trailing: Vec<Box<dyn Element>>,
    /// The live cue and Settings, spread to the right of the row.
    right: Vec<Box<dyn Element>>,
    left_spacing: f32,
    group_spacing: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TopbarRow {
    fn new(
        leading: Vec<Box<dyn Element>>,
        strip: Box<dyn Element>,
        trailing: Vec<Box<dyn Element>>,
        right: Vec<Box<dyn Element>>,
    ) -> Self {
        Self {
            leading,
            strip,
            trailing,
            right,
            left_spacing: 0.0,
            group_spacing: 0.0,
            size: None,
            origin: None,
        }
    }

    fn with_left_spacing(mut self, spacing: f32) -> Self {
        self.left_spacing = spacing;
        self
    }

    fn with_group_spacing(mut self, spacing: f32) -> Self {
        self.group_spacing = spacing;
        self
    }

    fn width_of(child: &Box<dyn Element>) -> f32 {
        child.size().unwrap_or(Vector2F::zero()).x
    }

    /// Where a child sits vertically: centered on the bar, as the flex row the
    /// tab strip used to sit in centered it.
    fn center_y(child: &Box<dyn Element>, height: f32) -> f32 {
        (height - child.size().unwrap_or(Vector2F::zero()).y).max(0.0) / 2.0
    }

    /// The widths of the row's fixed controls: everything but the strip, plus
    /// the gaps between them at their narrowest — `left_spacing` inside each
    /// group, and `group_spacing` before and between the row's right end, whose
    /// last control sits on the row's own right edge.
    fn fixed_width(&self) -> f32 {
        let sum = |children: &Vec<Box<dyn Element>>| -> f32 {
            children.iter().map(Self::width_of).sum()
        };
        let left_gaps = self.left_spacing * (self.leading.len() + self.trailing.len()) as f32;
        let group_gaps = self.group_spacing * self.right.len() as f32;
        sum(&self.leading) + sum(&self.trailing) + sum(&self.right) + left_gaps + group_gaps
    }
}

impl Element for TopbarRow {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let room = constraint.max.x;
        // The controls take their own sizes, exactly as they did inside the flex
        // row this replaces — none of them reads its constraint.
        let inner = SizeConstraint::loose(vec2f(f32::INFINITY, f32::INFINITY));
        let mut cross_max: f32 = 0.0;
        for child in &mut self.leading {
            cross_max = cross_max.max(child.layout(inner, ctx, app).y);
        }
        for child in &mut self.trailing {
            cross_max = cross_max.max(child.layout(inner, ctx, app).y);
        }
        for child in &mut self.right {
            cross_max = cross_max.max(child.layout(inner, ctx, app).y);
        }
        // The strip gets what the controls leave, and that room is what sizes
        // its tabs: an unbounded row leaves it to the strip's own cap.
        let strip_room = if room.is_finite() {
            (room - self.fixed_width()).max(0.0)
        } else {
            room
        };
        let strip = self
            .strip
            .layout(SizeConstraint::loose(vec2f(strip_room, constraint.max.y)), ctx, app);
        cross_max = cross_max.max(strip.y);

        let size = vec2f(
            if room.is_finite() {
                room
            } else {
                self.fixed_width() + strip.x
            },
            cross_max,
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let height = self.size.unwrap_or(Vector2F::zero()).y;
        let width = self.size.unwrap_or(Vector2F::zero()).x;

        // The left group: the controls and the strip, flush with `left_spacing`
        // between neighbours.
        let mut x = 0.0;
        let leading = self.leading.len();
        for (index, child) in self.leading.iter_mut().enumerate() {
            let y = Self::center_y(child, height);
            child.paint(origin + vec2f(x, y), ctx, app);
            x += Self::width_of(child);
            if index + 1 < leading {
                x += self.left_spacing;
            }
        }
        let y = Self::center_y(&self.strip, height);
        self.strip.paint(origin + vec2f(x, y), ctx, app);
        x += Self::width_of(&self.strip);
        x += self.left_spacing;
        let trailing = self.trailing.len();
        for (index, child) in self.trailing.iter_mut().enumerate() {
            let y = Self::center_y(child, height);
            child.paint(origin + vec2f(x, y), ctx, app);
            x += Self::width_of(child);
            if index + 1 < trailing {
                x += self.left_spacing;
            }
        }

        // The cue and Settings are spread over what is left of the row, with
        // the one gap on either side of them that a `SpaceBetween` row drew.
        let right_widths: Vec<f32> = self.right.iter().map(Self::width_of).collect();
        let right_total: f32 = right_widths.iter().sum();
        let gaps = self.right.len().max(1) as f32;
        let gap = ((width - x - right_total) / gaps).max(self.group_spacing);
        let mut x = x + gap;
        for (child, child_width) in self.right.iter_mut().zip(right_widths) {
            let y = Self::center_y(child, height);
            child.paint(origin + vec2f(x, y), ctx, app);
            x += child_width + gap;
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
        for child in &mut self.leading {
            if child.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        if self.strip.dispatch_event(event, ctx, app) {
            return true;
        }
        for child in &mut self.trailing {
            if child.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        self.right
            .iter_mut()
            .any(|child| child.dispatch_event(event, ctx, app))
    }
}

/// The contents of one tab of the bar: the icon that names its kind, and its
/// name in the room the icon leaves behind.
///
/// It lays the row out itself for the reason [`TopbarRow`] exists: a `Flex` row
/// hands every child the row's whole width, so a tab the strip narrowed would
/// draw its whole name past its own end — over its close control and into the
/// tab beside it — instead of shortening it.
struct IconLabelRow {
    icon: Box<dyn Element>,
    label: Box<dyn Element>,
    spacing: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl IconLabelRow {
    fn new(icon: Box<dyn Element>, label: Box<dyn Element>, spacing: f32) -> Self {
        Self {
            icon,
            label,
            spacing,
            size: None,
            origin: None,
        }
    }
}

impl Element for IconLabelRow {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let icon = self.icon.layout(
            SizeConstraint::loose(vec2f(f32::INFINITY, constraint.max.y)),
            ctx,
            app,
        );
        let room = (constraint.max.x - icon.x - self.spacing).max(0.0);
        let label = self.label.layout(
            SizeConstraint::loose(vec2f(room, constraint.max.y)),
            ctx,
            app,
        );
        let size = vec2f(icon.x + self.spacing + label.x, icon.y.max(label.y));
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let icon = self.icon.size().unwrap_or(Vector2F::zero());
        let label = self.label.size().unwrap_or(Vector2F::zero());
        self.icon
            .paint(origin + vec2f(0.0, (size.y - icon.y).max(0.0) / 2.0), ctx, app);
        self.label.paint(
            origin + vec2f(icon.x + self.spacing, (size.y - label.y).max(0.0) / 2.0),
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
        if self.icon.dispatch_event(event, ctx, app) {
            return true;
        }
        self.label.dispatch_event(event, ctx, app)
    }
}

/// The tabs of the files the active space shows, past the workspace tabs, or
/// `None` when it shows none.
///
/// A file open in a pane is not a workspace, so the strip's own slots stay what
/// they were — the drop index a lifted pane resolves is an index among the
/// workspace tabs and nothing else — and the file's tab is drawn after them,
/// behind their rule, with the same geometry every tab of the bar has.
fn build_file_tabs(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Option<StripFrame> {
    let space = state.spaces.get(state.active_space)?;
    let mut leaves = Vec::new();
    space.root.file_leaves(&mut leaves);
    if leaves.is_empty() {
        return None;
    }
    // One rule per boundary, belonging to the tab that follows it, exactly as
    // between the workspace tabs: the files' tabs read as their own frame
    // rather than as more workspaces.
    let tabs = leaves.len();
    let mut children = vec![FrameChild::Rule(tab_rule())];
    for (pane_id, path) in leaves {
        children.push(FrameChild::Tab(build_file_tab(
            app,
            actions,
            pane_id,
            &path,
            pane_id == state.active_pane_id,
        )));
    }
    // The frame is laid out by the strip with the workspace frame, so a file's
    // tab is exactly as wide as a workspace's; it carries the counts the strip
    // slices its room with.
    Some(StripFrame::new(TabFrame::new(children).finish(), tabs, 1))
}

/// One file pane as the bar draws it: the file's type icon and its name, the
/// full path on the name's hover, and the control that closes that pane.
fn build_file_tab(
    app: &AppContext,
    actions: &UiActions,
    pane_id: u64,
    path: &str,
    active: bool,
) -> Box<dyn Element> {
    let color = if active {
        ColorToken::Text
    } else {
        ColorToken::Muted
    };
    let name = file_view::file_name(path);
    let label = IconLabelRow::new(
        Icon::new(file_icon_name(&name))
            .with_size(14.0)
            .with_theme_color(color, app)
            .finish(),
        // The name is the bar's own tab label, so a tab the strip squeezed
        // shortens it the way every other tab does; the tooltip still reads
        // the whole path.
        Tooltip::new(TabLabel::new(name, color).finish(), path.to_string())
            .with_position(TooltipPosition::Below)
            .finish(),
        6.0,
    )
    .finish();
    // Closing a pane is the app's own rule and it names the pane the tab stands
    // for by making it the active one first: the tab and the pane it closes are
    // then the same pane on both counts.
    let on_activate = actions.on_pane_activate.clone();
    let on_close = actions.on_close_pane.clone();
    let close = TopbarButton::new(
        Icon::new("x")
            .with_size(10.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(16.0)
    .with_on_click(move || {
        (on_activate.borrow_mut())(pane_id);
        (on_close.borrow_mut())();
    })
    .finish();
    Box::new(FileTab::new(
        label,
        close,
        pane_id,
        active,
        actions.on_pane_activate.clone(),
    ))
}

/// A file's tab in the topbar: the same full-height, square-cornered surface a
/// workspace tab is — the geometry is [`WorkspaceChip`]'s own, so the two kinds
/// of tab in the bar cannot drift apart — carrying the file's own icon and name,
/// and, unlike a workspace tab, whose close appears under the pointer, the close
/// control always drawn, because a file's tab is the one place the pane can be
/// closed from the bar.
struct FileTab {
    child: Box<dyn Element>,
    close: Box<dyn Element>,
    pane_id: u64,
    active: bool,
    on_activate: Rc<RefCell<dyn FnMut(u64)>>,
    state: goble_ui::elements::interactive::InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl FileTab {
    fn new(
        child: Box<dyn Element>,
        close: Box<dyn Element>,
        pane_id: u64,
        active: bool,
        on_activate: Rc<RefCell<dyn FnMut(u64)>>,
    ) -> Self {
        Self {
            child,
            close,
            pane_id,
            active,
            on_activate,
            state: Default::default(),
            size: None,
            origin: None,
        }
    }

    fn bounds(&self) -> Option<RectF> {
        Some(rectf(
            self.origin?.x(),
            self.origin?.y(),
            self.size?.x,
            self.size?.y,
        ))
    }
}

impl Element for FileTab {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // The close control's slot is reserved before the name is measured, so
        // the name gets what is left of the tab and never runs under it.
        let close_size = self.close.layout(
            SizeConstraint::loose(vec2f(WorkspaceChip::height(), WorkspaceChip::height())),
            ctx,
            app,
        );
        let child_size = self
            .child
            .layout(WorkspaceChip::inner(constraint, close_size.x), ctx, app);
        let intrinsic =
            child_size.x + WorkspaceChip::close_gap() + close_size.x + WorkspaceChip::h_pad() * 2.0;
        let size = vec2f(
            WorkspaceChip::width(intrinsic, constraint),
            WorkspaceChip::height(),
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
            // Square corners and no stroke, as every tab of the bar: the rule
            // between neighbours is the strip's own.
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
                origin.x + WorkspaceChip::h_pad(),
                origin.y + (size.y - child_size.y).max(0.0) / 2.0,
            ),
            ctx,
            app,
        );
        self.close.paint(
            WorkspaceChip::close_origin(origin, size, close_size),
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
        // The close control gets the event first and consumes only what lands
        // in its own bounds, so a click on the name still activates the pane.
        // It is drawn always, so it is offered the pointer always too.
        if self.close.dispatch_event(event, ctx, app) {
            return true;
        }
        let bounds = match self.bounds() {
            Some(bounds) => bounds,
            None => return false,
        };
        let activate = self.on_activate.clone();
        let pane_id = self.pane_id;
        let mut on_click = move || (activate.borrow_mut())(pane_id);
        goble_ui::elements::interactive::handle_mouse_event(
            &mut self.state,
            event,
            bounds,
            ctx,
            &mut on_click,
        )
    }
}

/// The workspace row: one browser-style tab per space, in order, laid out flush
/// with a single vertical rule between neighbours. The active tab is filled and,
/// while renaming, swaps for the inline name field.
///
/// The frame itself pins every tab to [`TOPBAR_HEIGHT`] so it runs the full
/// height of the bar, and gives each one its slot (see [`TabFrame`]); each tab
/// is square-cornered (see [`WorkspaceChip`]).
fn build_workspace_strip(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> StripFrame {
    // One slot per tab, filled with the rectangle that tab is drawn at while it
    // paints: the drop an insertion index resolves against (see
    // [`SpaceDropStrip`]) is read off exactly what the user can see.
    let tab_rects: Rc<RefCell<Vec<Option<RectF>>>> =
        Rc::new(RefCell::new(vec![None; state.spaces.len()]));
    let mut children = Vec::with_capacity(state.spaces.len() * 2);
    for (index, space) in state.spaces.iter().enumerate() {
        // One rule per boundary: it belongs to the tab that follows it, so N
        // tabs draw N-1 lines and no boundary carries two.
        if index > 0 {
            children.push(FrameChild::Rule(tab_rule()));
        }
        let active = index == state.active_space;
        let tab: Box<dyn Element> = if active && state.space_rename_editing {
            build_rename_tab(app, state, actions)
        } else {
            build_workspace_chip(app, actions, index, &space.name, active, space.color)
        };
        children.push(FrameChild::Tab(Box::new(TabRect::new(
            tab,
            index,
            Rc::clone(&tab_rects),
        ))));
    }
    // The drop target wraps the workspace frame alone, never the files' tabs:
    // the insertion index a lifted pane resolves is an index among the workspace
    // tabs and nothing else.
    let frame = TabFrame::new(children).finish();
    StripFrame::new(
        Box::new(SpaceDropStrip::new(
            frame,
            state.pane_drag.clone(),
            tab_rects,
            actions.on_pane_lift_move.clone(),
            actions.on_pane_drop.clone(),
        )),
        state.spaces.len(),
        state.spaces.len().saturating_sub(1),
    )
}

/// One clickable workspace tab: the name plus an "x" that closes that
/// workspace. A single click selects the space; a second click within the
/// double-click window starts the inline rename — counted on the press, which
/// is what [`WorkspaceChip`] reports here. A right click opens the tab's menu
/// at the pointer.
fn build_workspace_chip(
    app: &AppContext,
    actions: &UiActions,
    index: usize,
    name: &str,
    active: bool,
    color: Option<TabColor>,
) -> Box<dyn Element> {
    let label_color = if active {
        ColorToken::Text
    } else {
        ColorToken::Muted
    };
    let label = TabLabel::new(name, label_color).finish();
    let on_click = actions.on_workspace_click.clone();
    let on_close = actions.on_close_space.clone();
    let on_press = actions.on_space_press.clone();
    let on_right_click = actions.on_space_menu.clone();
    Box::new(WorkspaceChip::new(
        label,
        index,
        active,
        color,
        Rc::new(RefCell::new(move |i: usize| (on_click.borrow_mut())(i))),
        Rc::new(RefCell::new(move |i: usize| (on_press.borrow_mut())(i))),
        Rc::new(RefCell::new(move |i: usize, at: Vector2F| {
            (on_right_click.borrow_mut())(i, at)
        })),
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
///
/// A tab carrying a colour fills its surface with that colour, at the share the
/// tab's own state gives it — 60 % while it is the active tab, 40 % while the
/// pointer is over it, 20 % otherwise, over the fill that state would have had
/// anyway (see [`TabColor::tint_over`]). A tab with no colour paints exactly the
/// three fills it painted before colours existed.
pub(super) struct WorkspaceChip {
    child: Box<dyn Element>,
    close: Box<dyn Element>,
    index: usize,
    active: bool,
    color: Option<TabColor>,
    on_click: Rc<RefCell<dyn FnMut(usize)>>,
    /// The press, reported before the click so the multi-click is counted from
    /// it (see [`UiState::space_press_run`](crate::state::SpacePressRun)).
    on_press: Rc<RefCell<dyn FnMut(usize)>>,
    /// A right click on the tab, with the point the pointer was at: the tab's
    /// menu hangs from the pointer itself.
    on_right_click: Rc<RefCell<dyn FnMut(usize, Vector2F)>>,
    state: goble_ui::elements::interactive::InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl WorkspaceChip {
    pub(super) fn new(
        child: Box<dyn Element>,
        index: usize,
        active: bool,
        color: Option<TabColor>,
        on_click: Rc<RefCell<dyn FnMut(usize)>>,
        on_press: Rc<RefCell<dyn FnMut(usize)>>,
        on_right_click: Rc<RefCell<dyn FnMut(usize, Vector2F)>>,
        close: Box<dyn Element>,
    ) -> Self {
        Self {
            child,
            close,
            index,
            active,
            color,
            on_click,
            on_press,
            on_right_click,
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

    /// What of a tab's slot is left for the name once the padding and the close
    /// control's own slot are out of it. Both kinds of tab measure their name
    /// against this, so a long name never runs under the control.
    fn inner(constraint: SizeConstraint, close_width: f32) -> SizeConstraint {
        let room = (constraint.max.x - Self::h_pad() * 2.0 - Self::close_gap() - close_width)
            .max(0.0);
        SizeConstraint::new(Vector2F::zero(), vec2f(room, Self::height()))
    }

    /// The tab's own width: what its name and its close control need, clamped
    /// into the slot the frame gave it — never narrower than the slot's minimum
    /// and never wider than the slot itself.
    fn width(intrinsic: f32, constraint: SizeConstraint) -> f32 {
        intrinsic.max(constraint.min.x).min(constraint.max.x)
    }

    /// The close control's origin inside a tab of `size`: pinned to the tab's
    /// own right edge, so a tab the strip squeezed still draws — and hits — the
    /// control inside its own bounds.
    fn close_origin(origin: Vector2F, size: Vector2F, close_size: Vector2F) -> Vector2F {
        vec2f(
            origin.x + (size.x - Self::h_pad() - close_size.x).max(Self::h_pad()),
            origin.y + (size.y - close_size.y).max(0.0) / 2.0,
        )
    }

    /// The fill a tab of this state is drawn with: the theme's own surface for
    /// that state, carrying the tab's colour at the state's share of it.
    fn tinted(&self, app: &AppContext, base: ColorToken, tint: TabTint) -> goble_ui::ColorU {
        let base = app.theme.color(base);
        match self.color {
            Some(color) => color.tint_over(base, tint),
            None => base,
        }
    }
}

impl Element for WorkspaceChip {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // The close "x"'s slot is reserved in the tab whether it is drawn or
        // not, and it is reserved *before* the name is measured: the name gets
        // what is left of the tab, so it never runs under the control.
        let close_size = self.close.layout(
            SizeConstraint::loose(vec2f(Self::height(), Self::height())),
            ctx,
            app,
        );
        let child_size = self
            .child
            .layout(Self::inner(constraint, close_size.x), ctx, app);
        let intrinsic =
            child_size.x + Self::close_gap() + close_size.x + Self::h_pad() * 2.0;
        let size = vec2f(Self::width(intrinsic, constraint), Self::height());
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
            // and no stroke joins it to its neighbour's rule. A coloured tab
            // tints the fill its own state gives it, so the tab still reads as
            // active (or hovered) and the colour reads as a colour.
            if self.active {
                renderer.fill_rect(rect, self.tinted(app, ColorToken::SurfaceRaised, TabTint::Active));
            } else if hovered {
                renderer.fill_rect(rect, self.tinted(app, ColorToken::Hover, TabTint::Hovered));
            } else if self.color.is_some() {
                renderer.fill_rect(rect, self.tinted(app, ColorToken::Surface, TabTint::Resting));
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
            self.close
                .paint(Self::close_origin(origin, size, close_size), ctx, app);
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
        // A right press is the tab's menu, hung from the point it landed on
        // rather than from the tab's own corner (warp-new's
        // `TabContextMenuAnchor::Pointer`). Nothing else sees the press: it
        // opens the menu, it is not a click on the tab.
        if let DispatchedEvent::MouseDown { position, button } = event {
            if *button == BUTTON_SECONDARY && over_tab {
                let cb = Rc::clone(&self.on_right_click);
                let index = self.index;
                let at = *position;
                (cb.borrow_mut())(index, at);
                return true;
            }
        }
        // A primary press is what the tab's multi-click is counted from, so it
        // is reported before the release that follows it reads the count.
        if let DispatchedEvent::MouseDown { button, .. } = event {
            if *button == BUTTON_PRIMARY && over_tab {
                let cb = Rc::clone(&self.on_press);
                let index = self.index;
                (cb.borrow_mut())(index);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root_view::RootView;
    use crate::state::UiState;
    use crate::ui::{Pane, PaneKind, Space, SplitDir};
    use goble_ui::event::DispatchedEvent;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;

    fn window() -> Vector2F {
        vec2f(1024.0, 768.0)
    }

    /// The whole app over a real store whose bar carries one tab per name in
    /// `names`, the first-run overlays out of the way.
    fn app_with_workspaces(
        names: Vec<String>,
    ) -> (Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir) {
        use goble_core::store::Store;
        use goble_desktop_service::{DesktopState, ThreadStore};
        use std::sync::Arc;

        assert!(!names.is_empty(), "a bar needs at least one tab");
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
            // The space the app opens on takes the first name; the rest are new
            // spaces of their own, as "+" opens them.
            s.spaces[0].name = names[0].clone();
            s.spaces[0].named = true;
            for name in &names[1..] {
                let id = s.next_pane_id;
                s.next_pane_id += 1;
                s.spaces.push(Space::new(
                    name.clone(),
                    Pane::Leaf {
                        id,
                        kind: PaneKind::Terminal,
                    },
                ));
            }
        }
        (Box::new(view), state, dir)
    }

    /// Every text run drawn in the topbar band, which is where the tab labels
    /// live.
    fn topbar_texts(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if origin.y < TOPBAR_HEIGHT => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// The fill of the active workspace tab: the one raised, full-height surface
    /// in the toolbar band, which is the tab's own rectangle.
    fn active_tab(commands: &[RenderCommand], app: &AppContext) -> RectF {
        let raised = app.theme.color(ColorToken::SurfaceRaised);
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == raised
                        && rect.min_y() == 0.0
                        && (rect.height() - TOPBAR_HEIGHT).abs() < 0.01 =>
                {
                    Some(*rect)
                }
                _ => None,
            })
            .expect("the active workspace tab is the filled tab of the strip")
    }

    /// The x of every rule the strip draws between two tabs, in draw order.
    fn tab_rules(commands: &[RenderCommand], app: &AppContext) -> Vec<f32> {
        let border = app.theme.color(ColorToken::Border);
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, color, .. }
                    if *color == border
                        && rect.width() == TAB_RULE_WIDTH
                        && rect.min_y() == 0.0 =>
                {
                    Some(rect.min_x())
                }
                _ => None,
            })
            .collect()
    }

    /// The rightmost x anything is drawn at inside the topbar band, so a test
    /// can tell whether the bar's own controls were pushed past the window.
    fn topbar_max_x(commands: &[RenderCommand]) -> f32 {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, .. } if rect.min_y() < TOPBAR_HEIGHT => {
                    Some(rect.max_x())
                }
                RenderCommand::DrawIcon { origin, size, .. } if origin.y < TOPBAR_HEIGHT => {
                    Some(origin.x + size)
                }
                // A run's own width is not in the command; where it starts is
                // what says whether it begins inside the bar.
                RenderCommand::DrawText { origin, .. } if origin.y < TOPBAR_HEIGHT => {
                    Some(origin.x)
                }
                _ => None,
            })
            .fold(0.0_f32, f32::max)
    }

    /// The whole app over a real store with a file open in a pane of its own —
    /// the explorer's own shape — and the first-run overlays out of the way.
    fn app_with_file(path: &str) -> (Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir) {
        use goble_core::store::Store;
        use goble_desktop_service::{DesktopState, ThreadStore};
        use std::sync::Arc;

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
            let (space, pane) = (s.active_space, s.active_pane_id);
            let mut next = s.next_pane_id;
            let new_id = s.spaces[space]
                .split_with_kind(
                    pane,
                    SplitDir::Horizontal,
                    &mut next,
                    PaneKind::File {
                        path: path.to_string(),
                    },
                )
                .expect("a file pane opens beside the pane that asked for it");
            s.next_pane_id = next;
            s.active_pane_id = new_id;
            s.sync_active_view();
        }
        (Box::new(view), state, dir)
    }

    /// The origin of the topbar run that reads exactly `text`.
    fn topbar_text(commands: &[RenderCommand], text: &str) -> Option<Vector2F> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawText { text: drawn, origin, .. }
                if drawn == text && origin.y < TOPBAR_HEIGHT =>
            {
                Some(*origin)
            }
            _ => None,
        })
    }

    /// The centre of the topbar icon drawn from `name`, which is the control
    /// that icon sits in.
    fn topbar_icon(commands: &[RenderCommand], name: &str) -> Option<Vector2F> {
        commands.iter().find_map(|command| match command {
            RenderCommand::DrawIcon {
                name: drawn, origin, size, ..
            } if drawn == name && origin.y < TOPBAR_HEIGHT => {
                Some(vec2f(origin.x + size / 2.0, origin.y + size / 2.0))
            }
            _ => None,
        })
    }

    /// A full press and release at `pos`, with a frame between them, the way the
    /// running app rebuilds the tree between the two events.
    fn click(root: &mut Box<dyn Element>, app: &AppContext, pos: Vector2F) {
        let mut ctx = EventContext::default();
        let _ = root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: pos,
                button: 0,
            },
            &mut ctx,
            app,
        );
        let _ = render_element(root, window(), app);
        let _ = root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: pos,
                button: 0,
            },
            &mut ctx,
            app,
        );
    }

    /// A file pane is a tab of the bar in its own frame: the file's type icon,
    /// the file's name, and the control that closes that pane. The close is
    /// drawn on every frame rather than appearing under the pointer, because the
    /// bar is where a file pane is closed from.
    #[test]
    fn a_file_pane_draws_as_a_tab_that_names_it_and_closes_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("notes.rs");
        std::fs::write(&path, "fn main() {}\n").expect("write");
        let path = path.to_string_lossy().to_string();

        let app = AppContext::default();
        let (mut root, state, _threads) = app_with_file(&path);
        let pane_id = state.borrow().active_pane_id;
        let commands = render_element(&mut root, window(), &app);

        assert!(
            topbar_icon(&commands, "file-rust").is_some(),
            "the tab carries the file's own type icon: {commands:?}"
        );
        assert!(
            topbar_text(&commands, "notes.rs").is_some(),
            "and reads the file's name: {commands:?}"
        );

        // The close is the pane's own: a press on it leaves the space without
        // the pane the tab stands for, and the tab goes with it.
        // `Icon::new("x")` draws the atlas' own `x-close`, the control every
        // tab of this bar closes with.
        let close = topbar_icon(&commands, "x-close").unwrap_or_else(|| {
            panic!("the tab draws its close control on every frame: {commands:?}")
        });
        click(&mut root, &app, close);

        assert!(
            !state.borrow().spaces.iter().any(|space| space.root.contains_leaf(pane_id)),
            "the close removes the pane the tab stands for"
        );
        let after = render_element(&mut root, window(), &app);
        assert!(
            topbar_text(&after, "notes.rs").is_none(),
            "and the tab goes with its pane: {after:?}"
        );
    }

    /// A press on the name rather than the close leaves the pane where it is:
    /// the close control consumes only what lands in its own bounds.
    #[test]
    fn a_press_on_the_tab_name_does_not_close_the_pane() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("notes.rs");
        std::fs::write(&path, "fn main() {}\n").expect("write");
        let path = path.to_string_lossy().to_string();

        let app = AppContext::default();
        let (mut root, state, _threads) = app_with_file(&path);
        let pane_id = state.borrow().active_pane_id;
        let commands = render_element(&mut root, window(), &app);
        let name = topbar_text(&commands, "notes.rs").expect("the tab reads the file's name");
        let close = topbar_icon(&commands, "x-close").expect("the tab draws its close control");
        assert!(
            name.x + 20.0 < close.x,
            "the name is drawn to the left of the close: {name:?} against {close:?}"
        );

        click(&mut root, &app, vec2f(name.x + 2.0, name.y + 4.0));

        assert!(
            state.borrow().spaces.iter().any(|space| space.root.contains_leaf(pane_id)),
            "the pane stays: only the close control closes it"
        );
    }

    /// The bar's tabs never fall below the readable minimum ([`MIN_TAB_WIDTH`])
    /// while the bar can afford it: a short name no longer sets the tab's width
    /// on its own — the tab keeps the width a tab is read at, and the room left
    /// over stays with the bar.
    #[test]
    fn a_tab_is_at_least_the_minimum_width() {
        let app = AppContext::default();
        let (mut root, _state, _threads) = app_with_workspaces(vec!["One".to_string()]);
        let commands = render_element(&mut root, window(), &app);
        let tab = active_tab(&commands, &app);

        assert!(
            tab.width() >= MIN_TAB_WIDTH,
            "one short tab is drawn at the minimum, not at its name's width: {tab:?}"
        );
        assert!(
            tab.width() <= MAX_TAB_WIDTH,
            "and never past the maximum: {tab:?}"
        );
    }

    /// A tab stops at the maximum ([`MAX_TAB_WIDTH`]) however wide the window is
    /// and however long its name: the room past the maximum stays with the bar,
    /// and the name is cut to the tab's own width instead of being drawn past it.
    #[test]
    fn a_long_name_is_cut_to_the_maximum_width() {
        let app = AppContext::default();
        let name = "A workspace named far longer than any tab of the bar is wide";
        let (mut root, _state, _threads) = app_with_workspaces(vec![name.to_string()]);
        let commands = render_element(&mut root, window(), &app);
        let tab = active_tab(&commands, &app);

        // The name is far wider than a tab ever gets, so the tab grows past the
        // minimum — up to the cap, which the run it draws is cut to fit.
        assert!(
            tab.width() <= MAX_TAB_WIDTH,
            "the tab stops at the maximum in a wide window: {tab:?}"
        );
        assert!(
            tab.width() > MIN_TAB_WIDTH,
            "and a name longer than the cap fills the tab past the minimum: {tab:?}"
        );
        assert!(
            topbar_text(&commands, name).is_none(),
            "the whole name does not fit the tab: {:?}",
            topbar_texts(&commands)
        );
        assert!(
            topbar_texts(&commands)
                .iter()
                .any(|text| text.starts_with("A workspace") && text.ends_with(ELLIPSIS)),
            "the tab reads the name cut at its own width: {:?}",
            topbar_texts(&commands)
        );
    }

    /// More tabs than the bar can give the minimum: they share the room the bar
    /// has left, every one exactly as wide as its neighbours and none of them
    /// drawn past the window, so the "+ ▾" control and Settings keep the place
    /// inside the window they hold with a single tab.
    #[test]
    fn many_tabs_share_the_bar_and_keep_the_controls_inside_it() {
        const COUNT: usize = 14;
        let app = AppContext::default();
        let names: Vec<String> = (0..COUNT).map(|index| format!("Workspace {index}")).collect();
        let (mut root, _state, _threads) = app_with_workspaces(names);
        let commands = render_element(&mut root, window(), &app);

        let rules = tab_rules(&commands, &app);
        assert_eq!(
            rules.len(),
            COUNT - 1,
            "one rule per boundary, one per pair of tabs: {rules:?}"
        );

        let widths: Vec<f32> = rules.windows(2).map(|pair| pair[1] - pair[0]).collect();
        let share = widths[0];
        assert!(share > 0.0, "the tabs have room between them: {widths:?}");
        assert!(
            share < MIN_TAB_WIDTH,
            "the room is shared rather than every tab kept at the minimum: {widths:?}"
        );
        for width in &widths {
            assert!(
                (width - share).abs() < 0.01,
                "every tab of the strip is the same width: {widths:?}"
            );
        }

        assert!(
            topbar_max_x(&commands) <= window().x + 0.5,
            "nothing the bar draws is pushed past the window's right edge: {commands:?}"
        );
        let chevron = topbar_icon(&commands, "chevron-down").expect("the environment menu");
        let settings = topbar_icon(&commands, "settings").expect("the settings control");
        assert!(
            chevron.x < settings.x,
            "the bar's own controls keep their order: {chevron:?} against {settings:?}"
        );
        assert!(
            settings.x < window().x,
            "and Settings stays inside the window: {settings:?}"
        );
    }

    /// The same rule in a window too narrow for the tabs' minimum: the tabs come
    /// down to an equal share of what is left and cut their names, and the bar's
    /// own controls still land inside the window.
    #[test]
    fn a_narrow_window_still_holds_every_control() {
        const WIDTH: f32 = 420.0;
        let app = AppContext::default();
        let names: Vec<String> = (0..3).map(|index| format!("Workspace {index}")).collect();
        let (mut root, _state, _threads) = app_with_workspaces(names);
        let commands = render_element(&mut root, vec2f(WIDTH, 600.0), &app);

        let rules = tab_rules(&commands, &app);
        assert_eq!(rules.len(), 2, "one rule per boundary: {rules:?}");
        let share = rules[1] - rules[0];
        assert!(
            share < MIN_TAB_WIDTH,
            "a narrow bar shares its room instead of holding every tab at the minimum: {rules:?}"
        );
        assert!(
            topbar_texts(&commands)
                .iter()
                .any(|text| text.ends_with(ELLIPSIS)),
            "the squeezed tabs read their names cut: {:?}",
            topbar_texts(&commands)
        );
        assert!(
            topbar_max_x(&commands) <= WIDTH + 0.5,
            "nothing the bar draws is pushed past the window's right edge: {commands:?}"
        );
        let settings = topbar_icon(&commands, "settings").expect("the settings control");
        assert!(settings.x < WIDTH, "Settings stays inside the window: {settings:?}");
    }

    /// A file's tab is squeezed by the same rule, and it cuts its name rather
    /// than drawing it over its close control and into the tab beside it.
    #[test]
    fn a_squeezed_file_tab_cuts_its_name() {
        const WIDTH: f32 = 420.0;
        let dir = tempfile::tempdir().expect("temp dir");
        let name = "a_very_long_file_name_for_a_bars_tab.rs";
        let path = dir.path().join(name);
        std::fs::write(&path, "fn main() {}\n").expect("write");
        let path = path.to_string_lossy().to_string();

        let app = AppContext::default();
        let (mut root, _state, _threads) = app_with_file(&path);
        let commands = render_element(&mut root, vec2f(WIDTH, 600.0), &app);

        assert!(
            topbar_text(&commands, name).is_none(),
            "the tab draws the name it has room for, not the whole one: {:?}",
            topbar_texts(&commands)
        );
        assert!(
            topbar_texts(&commands)
                .iter()
                .any(|text| text.starts_with("a_very") && text.ends_with(ELLIPSIS)),
            "the file's tab reads its name cut: {:?}",
            topbar_texts(&commands)
        );
        let close = topbar_icon(&commands, "x-close").expect("the tab draws its close control");
        assert!(
            close.x < WIDTH,
            "the close control stays inside the window: {close:?}"
        );
    }
}
