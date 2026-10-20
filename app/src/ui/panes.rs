//! Pane-tree builder for a "space" (warp-new style).
//!
//! Recursively renders a [`Space`]'s [`Pane`] tree: leaves host content (the
//! agent chat or a PTY terminal), splits are resizable [`SplitNode`]s with a
//! draggable divider. The active pane is outlined; clicking anywhere in a pane
//! (its header or body) focuses it, and its header has a close button.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Axis, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext, Expanded,
    Fill, Flex, HoverButton, Icon, LayoutContext, MainAxisSize, PaintContext, Point,
    SizeConstraint, SplitNode, Stack, Text,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::chat;
use super::file_view;
use super::settings;
use super::shell::TOPBAR_HEIGHT;
use super::terminal;
use super::worker;
use super::pane::{default_floating_rect, drop_ghost_rect, master_stack_regions};
use super::{
    FloatStartHandler, FloatingDrag, FloatingDragMode, LayoutKind, Pane, PaneDropTarget, PaneKind,
    PaneRect, Space, SplitDir, UiActions, UiSnapshot,
};

/// The corner mark the focused pane carries, and the room its own topbar leaves
/// for it. warp-new draws the same mark in the same corner
/// (`add_active_pane_indicator_to_stack`): an accent right triangle filling the
/// pane's top-left, which is how the pane the keyboard is in reads at a glance
/// even where nothing else in the bar changes.
pub(crate) const FOCUS_MARKER_SIZE: f32 = 16.0;

/// Build the active space's pane tree as the main chat content.
///
/// Guards against an empty space list or an out-of-range active index (which
/// could arise from a corrupt persisted pane layout): the caller falls back to
/// an empty chat pane rather than panicking on user-controlled state.
pub fn build_active_space(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let content = match state.spaces.get(state.active_space) {
        Some(space) => {
            // One pane at a time is drawn over the whole panes space: the leaf
            // the header's expand control names, instead of the tree. The tree
            // itself is left alone, so retracting puts the pane back in the
            // place it had. An id that names no leaf of *this* space (it was
            // closed, or it belongs to another space) falls through to the tree.
            if let Some(id) = state.maximized_pane {
                if let Some(kind) = leaf_kind_of(&space.root, id) {
                    build_leaf(app, state, actions, id, kind)
                } else {
                    build_space(app, state, actions, space)
                }
            } else {
                build_space(app, state, actions, space)
            }
        }
        None => {
            let id = 0u64;
            let chat = chat::build_agent_chat(app, state, actions, id, true, None);
            // The chat's own agent header is the single topbar; do not stack the
            // generic pane header above it.
            let column = Flex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(Expanded::new(chat).finish());
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .with_border(app.theme.color(ColorToken::Border).into())
                .finish()
        }
    };
    // The drop ghost is drawn with the pane area but after its panes, so it
    // covers the pane the pointer is over and stays under the bar's floating
    // drag card (the bar paints above the body).
    let target = state
        .pane_drag
        .as_ref()
        .filter(|drag| drag.moved())
        .and_then(|drag| drag.drop_target);
    Box::new(PaneDropGhost::new(
        content,
        target,
        state.pane_bounds.clone(),
    ))
}

/// The kind of the leaf pane `target` holds in this subtree, or `None` when the
/// subtree has no such leaf. What the expand control needs: the id names a
/// pane, and drawing it needs its kind.
fn leaf_kind_of(pane: &Pane, target: u64) -> Option<PaneKind> {
    match pane {
        Pane::Leaf { id, kind } => (*id == target).then(|| kind.clone()),
        Pane::Split { first, second, .. } => {
            leaf_kind_of(first, target).or_else(|| leaf_kind_of(second, target))
        }
    }
}

/// Build a space's panes in its selected layout. BSP keeps the draggable split
/// tree; the other two place each leaf at a region of the pane area.
fn build_space(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    space: &Space,
) -> Box<dyn Element> {
    match space.layout {
        LayoutKind::Bsp => build_pane(app, state, actions, &space.root),
        LayoutKind::MasterStack => {
            let mut leaves = Vec::new();
            space.root.leaves(&mut leaves);
            let regions: Vec<(u64, PaneRect)> = master_stack_regions(&leaves)
                .into_iter()
                .map(|(id, rect)| {
                    (
                        id,
                        PaneRect::new(rect.min_x(), rect.min_y(), rect.width(), rect.height()),
                    )
                })
                .collect();
            let children = regions
                .into_iter()
                .filter_map(|(id, rect)| {
                    let kind = leaf_kind_of(&space.root, id)?;
                    Some(RegionChild::new(
                        id,
                        rect,
                        build_leaf_with_lift(app, state, actions, id, kind, false),
                    ))
                })
                .collect();
            Box::new(RegionLayer::new(
                children,
                false,
                actions,
                state.floating_drag.clone(),
            ))
        }
        LayoutKind::Floating => {
            // The stored order is the z-order; a leaf the space has not given a
            // rect yet falls back to the cascade, so nothing is drawn twice or
            // left invisible.
            let mut children = Vec::new();
            let mut placed = Vec::new();
            for pane in &space.floating {
                let Some(kind) = leaf_kind_of(&space.root, pane.pane_id) else {
                    continue;
                };
                placed.push(pane.pane_id);
                children.push(RegionChild::new(
                    pane.pane_id,
                    pane.rect,
                    build_leaf_with_lift(app, state, actions, pane.pane_id, kind, false),
                ));
            }
            let mut leaves = Vec::new();
            space.root.leaves(&mut leaves);
            for (index, (id, kind)) in leaves.into_iter().enumerate() {
                if placed.contains(&id) {
                    continue;
                }
                children.push(RegionChild::new(
                    id,
                    default_floating_rect(index),
                    build_leaf_with_lift(app, state, actions, id, kind, false),
                ));
            }
            Box::new(RegionLayer::new(
                children,
                true,
                actions,
                state.floating_drag.clone(),
            ))
        }
    }
}

fn build_pane(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane: &Pane,
) -> Box<dyn Element> {
    match pane {
        Pane::Leaf { id, kind } => build_leaf(app, state, actions, *id, kind.clone()),
        Pane::Split {
            id,
            dir,
            ratio,
            first,
            second,
        } => {
            let axis = match dir {
                SplitDir::Horizontal => Axis::Horizontal,
                SplitDir::Vertical => Axis::Vertical,
            };
            let dragging = state.dragging_pane_id == Some(*id);
            let on_start = actions.on_pane_drag_start.clone();
            let on_move = actions.on_pane_drag_move.clone();
            let on_end = actions.on_pane_drag_end.clone();
            let split_id = *id;
            let node = SplitNode::new(
                axis,
                *ratio,
                build_pane(app, state, actions, first),
                build_pane(app, state, actions, second),
            )
            .with_dragging(dragging)
            .with_on_drag_start(move || (on_start.borrow_mut())(split_id))
            .with_on_drag_move(move |r| (on_move.borrow_mut())(split_id, r))
            .with_on_drag_end(move || (on_end.borrow_mut())());
            Box::new(node)
        }
    }
}

/// Wraps a pane leaf's content so a mouse-down anywhere inside the pane focuses
/// it, while still forwarding the event to the content (so a composer, button,
/// or terminal keystroke keeps working). This gives mouse-driven pane
/// navigation: clicking a pane makes it the active split target/highlight.
///
/// A press on the pane's header — its own topbar, the top [`TOPBAR_HEIGHT`]
/// band — also lifts the pane into a drag toward the tab strip. Only the header
/// is a handle: a press in the body belongs to the editor or the terminal, and
/// text selection there must not become a pane drag.
struct PaneBody {
    pane_id: u64,
    content: Box<dyn Element>,
    on_activate: Rc<RefCell<dyn FnMut(u64)>>,
    on_lift: Rc<RefCell<dyn FnMut(u64, Vector2F)>>,
    /// Whether a header press lifts the pane toward the tab strip. A floating
    /// pane's header moves the pane instead, so the gesture is owned by the
    /// region layer above it.
    lift: bool,
    /// Where this pane was drawn, shared with app state so a lifted pane's
    /// drop target can be resolved against it (see
    /// [`UiSnapshot::pane_bounds`]).
    bounds: Rc<RefCell<std::collections::HashMap<u64, RectF>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl PaneBody {
    fn new(
        pane_id: u64,
        content: Box<dyn Element>,
        on_activate: Rc<RefCell<dyn FnMut(u64)>>,
        on_lift: Rc<RefCell<dyn FnMut(u64, Vector2F)>>,
        lift: bool,
        bounds: Rc<RefCell<std::collections::HashMap<u64, RectF>>>,
    ) -> Self {
        Self {
            pane_id,
            content,
            on_activate,
            on_lift,
            lift,
            bounds,
            size: None,
            origin: None,
        }
    }

    fn bounds(&self) -> Option<goble_ui::geometry::RectF> {
        let origin = self.origin?;
        let size = self.size?;
        Some(rectf(origin.x(), origin.y(), size.x, size.y))
    }
}

impl Element for PaneBody {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.content.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.content.paint(origin, ctx, app);
        // Recorded at paint time, so the strip resolves a drop against the
        // rectangles the user can actually see, exactly as the tab strip does.
        if let Some(bounds) = self.bounds() {
            self.bounds.borrow_mut().insert(self.pane_id, bounds);
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
        let handled = self.content.dispatch_event(event, ctx, app);
        // A mouse-down anywhere in the pane focuses it (pane navigation by
        // click). The event is still forwarded to the content above, so child
        // interactions are unaffected.
        let activated = matches!(event, DispatchedEvent::MouseDown { position, .. }
            if self.bounds().map(|b| contains(b, *position)).unwrap_or(false));
        if activated {
            (self.on_activate.borrow_mut())(self.pane_id);
        }
        // A press on the pane's own header starts a lift, so the pane can be
        // dragged onto the tab strip and become a tab. A header control that
        // took the press keeps it (its own click wins), and the body below the
        // header is never a handle.
        if self.lift {
            if let DispatchedEvent::MouseDown { position, .. } = event {
                let on_header = !handled
                    && self
                        .bounds()
                        .map(|b| contains(b, *position) && position.y - b.min_y() <= TOPBAR_HEIGHT)
                        .unwrap_or(false);
                if on_header {
                    (self.on_lift.borrow_mut())(self.pane_id, *position);
                }
            }
        }
        handled || activated
    }
}

/// The pane area with the drop ghost drawn over it: while a pane is lifted over
/// another pane, the half the lifted pane would fill is filled with a
/// translucent accent and bounded by an accent line, so the drop reads as a
/// place rather than as a release the user has to guess at.
///
/// It wraps the pane tree instead of living in the tree, because the ghost is
/// not a pane: it is one flat overlay whose rectangle comes from the bounds the
/// panes recorded while painting (see [`PaneBody`]). The bar's floating drag
/// card is drawn after the whole body, so it stays above this ghost.
struct PaneDropGhost {
    inner: Box<dyn Element>,
    target: Option<PaneDropTarget>,
    bounds: Rc<RefCell<std::collections::HashMap<u64, RectF>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

/// How opaque the drop ghost's fill is: enough to read against a pane's surface,
/// light enough that the pane under it stays legible.
const DROP_GHOST_ALPHA: u8 = 48;

impl PaneDropGhost {
    fn new(
        inner: Box<dyn Element>,
        target: Option<PaneDropTarget>,
        bounds: Rc<RefCell<std::collections::HashMap<u64, RectF>>>,
    ) -> Self {
        Self {
            inner,
            target,
            bounds,
            size: None,
            origin: None,
        }
    }
}

impl Element for PaneDropGhost {
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
        let Some(target) = self.target else {
            return;
        };
        let Some(pane) = self.bounds.borrow().get(&target.pane_id).copied() else {
            return;
        };
        let ghost = drop_ghost_rect(pane, target.side);
        let accent = app.theme.color(ColorToken::Accent);
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        renderer.fill_rect(
            ghost,
            goble_ui::ColorU::new(accent.r, accent.g, accent.b, DROP_GHOST_ALPHA),
        );
        renderer.stroke_rect(ghost, accent, 2.0, 0.0);
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

/// One pane placed at a normalized region of the pane area. The master-stack
/// and floating layouts are the same layer: the first positions panes without
/// overlap, the second draws them in z-order over one another.
struct RegionChild {
    pane_id: u64,
    rect: PaneRect,
    content: Box<dyn Element>,
}

impl RegionChild {
    fn new(pane_id: u64, rect: PaneRect, content: Box<dyn Element>) -> Self {
        Self {
            pane_id,
            rect,
            content,
        }
    }
}

/// The corner grip a floating pane resizes from, in pixels.
const FLOAT_RESIZE_GRIP: f32 = 16.0;

struct RegionLayer {
    children: Vec<RegionChild>,
    floating: bool,
    floating_drag: Rc<RefCell<Option<FloatingDrag>>>,
    on_float_start: FloatStartHandler,
    on_float_rect: Rc<RefCell<dyn FnMut(u64, PaneRect)>>,
    on_float_rect_end: Rc<RefCell<dyn FnMut()>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl RegionLayer {
    fn new(
        children: Vec<RegionChild>,
        floating: bool,
        actions: &UiActions,
        floating_drag: Rc<RefCell<Option<FloatingDrag>>>,
    ) -> Self {
        Self {
            children,
            floating,
            floating_drag,
            on_float_start: actions.on_float_start.clone(),
            on_float_rect: actions.on_float_rect.clone(),
            on_float_rect_end: actions.on_float_rect_end.clone(),
            size: None,
            origin: None,
        }
    }

    fn child_bounds(&self, child: &RegionChild) -> Option<RectF> {
        let origin = self.origin?;
        let area = self.size?;
        let px = child.rect.to_pixels(area);
        Some(rectf(
            origin.x() + px.min_x(),
            origin.y() + px.min_y(),
            px.width(),
            px.height(),
        ))
    }
}

impl Element for RegionLayer {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let area = vec2f(constraint.max.x.max(1.0), constraint.max.y.max(1.0));
        for child in &mut self.children {
            let px = child.rect.to_pixels(area);
            let _ = child.content.layout(
                SizeConstraint::tight(vec2f(px.width().max(1.0), px.height().max(1.0))),
                ctx,
                app,
            );
        }
        self.size = Some(area);
        area
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(area) = self.size else {
            return;
        };
        for child in &mut self.children {
            let px = child.rect.to_pixels(area);
            child
                .content
                .paint(origin + vec2f(px.min_x(), px.min_y()), ctx, app);
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
        // A drag owns every pointer move until it ends, wherever the pointer
        // has travelled: the pane under it may have changed, the gesture has
        // not.
        if self.floating {
            let drag = self.floating_drag.borrow().clone();
            if let Some(drag) = drag {
                match event {
                    DispatchedEvent::MouseMove { position } => {
                        (self.on_float_rect.borrow_mut())(drag.pane_id, drag.rect_at(*position));
                        return true;
                    }
                    DispatchedEvent::MouseUp { .. } => {
                        (self.on_float_rect_end.borrow_mut())();
                        return true;
                    }
                    _ => {}
                }
            }
        }
        let position = match event {
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. }
            | DispatchedEvent::MouseMove { position } => Some(*position),
            _ => None,
        };
        if let Some(position) = position {
            let area = self.size.unwrap_or_else(|| vec2f(1.0, 1.0));
            for index in (0..self.children.len()).rev() {
                let Some(bounds) = self.child_bounds(&self.children[index]) else {
                    continue;
                };
                if !contains(bounds, position) {
                    continue;
                }
                if self.floating {
                    if let DispatchedEvent::MouseDown { .. } = event {
                        let pane = &self.children[index];
                        let on_grip = position.x >= bounds.max_x() - FLOAT_RESIZE_GRIP
                            && position.y >= bounds.max_y() - FLOAT_RESIZE_GRIP;
                        let on_header = position.y - bounds.min_y() <= TOPBAR_HEIGHT;
                        if on_grip {
                            (self.on_float_start.borrow_mut())(
                                pane.pane_id,
                                FloatingDragMode::Resize,
                                position,
                                area,
                            );
                        } else if on_header {
                            (self.on_float_start.borrow_mut())(
                                pane.pane_id,
                                FloatingDragMode::Move,
                                position,
                                area,
                            );
                        }
                    }
                    let _ = self.children[index]
                        .content
                        .dispatch_event(event, ctx, app);
                    return true;
                }
                return self.children[index]
                    .content
                    .dispatch_event(event, ctx, app);
            }
            return false;
        }
        // Keys, focus changes and scrolling go to the topmost pane first, the
        // same order a stack dispatches to its overlays.
        for child in self.children.iter_mut().rev() {
            if child.content.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        false
    }
}

fn build_leaf(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    id: u64,
    kind: PaneKind,
) -> Box<dyn Element> {
    build_leaf_with_lift(app, state, actions, id, kind, true)
}

fn build_leaf_with_lift(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    id: u64,
    kind: PaneKind,
    lift: bool,
) -> Box<dyn Element> {
    let active = state.active_pane_id == id;
    let content = match kind {
        PaneKind::Chat => chat::build_agent_chat(app, state, actions, id, active, None),
        // A viewer session: the conversation runs on a worker, so the pane is
        // the conversation's own surface with no shell behind it. It is never
        // given the shell builder below, which is the only thing that mounts a
        // pty — there is nothing local for a viewer pane to fall back to.
        PaneKind::Worker => worker::build_worker_pane(app, state, actions, id, active),
        // The settings tab's surface: the rail, the page and the footer, with
        // the page the leaf carries. It composes with the tree like any other
        // pane — it can be split, walked to and closed.
        PaneKind::Settings { page } => settings::build_settings_pane(app, state, actions, id, page),
        // A file view is read-only: no composer, no shell, just the file the
        // pane was opened on.
        PaneKind::File { path } => file_view::build_file_view(app, state, actions, id, &path),
        PaneKind::Terminal => {
            // The terminal spawns in this pane's own cwd (the same per-session
            // path used by the composer for chat panes), so a terminal and a
            // chat pane never share a working directory.
            let cwd = state
                .pane_chat
                .get(&id)
                .map(|c| c.composer_path.clone())
                .unwrap_or_default();
            // The pane's own builder picks the surface from the pane's mode:
            // the shell's grid with its rich input bar, or — while Cmd+Enter's
            // switch has put this pane in agent mode — the very agent view a
            // chat pane mounts.
            terminal::build_terminal(app, state, actions, id, cwd, active)
        }
    };
    // Clicking anywhere in the pane body focuses the pane (mouse-driven pane
    // navigation), while child interactions (composer, buttons, terminal keys)
    // still get the event forwarded to them.
    let on_activate = actions.on_pane_activate.clone();
    let on_lift = actions.on_pane_lift.clone();
    let content = Box::new(PaneBody::new(
        id,
        content,
        on_activate,
        on_lift,
        lift,
        state.pane_bounds.clone(),
    ));
    // Every surface draws the one topbar a pane has, so nothing is stacked
    // above the content here: the agent header is the pane's topbar, and the
    // terminal pane draws it for its shell view too (see `build_terminal`).
    let column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(Expanded::new(content).finish());

    let active = state.active_pane_id == id;
    let border_color = if active {
        ColorToken::Accent
    } else {
        ColorToken::Border
    };
    let pane = Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(app.theme.color(border_color).into())
        .finish();
    if !active {
        return pane;
    }
    // The focused pane's mark: the accent triangle sits in the pane's own
    // top-left corner, which is where the pane the keyboard is in is read from.
    // The pane's bar starts past it (see `chat::build_agent_header`).
    let marker = Icon::new("upper-left-triangle")
        .with_size(FOCUS_MARKER_SIZE)
        .with_theme_color(ColorToken::Accent, app)
        .finish();
    Stack::new()
        .with_children(vec![pane])
        .with_overlay(marker, vec2f(0.0, 0.0))
        .finish()
}

/// While the harness is active on a pane, its topbar shows `esc for terminal`:
/// the way back to the plain pty, spelled out and clickable, so the surface can
/// be switched with the pointer as well as with the key.
pub(crate) fn build_esc_hint<F: FnMut() + 'static>(
    app: &AppContext,
    hover: Rc<RefCell<bool>>,
    on_click: F,
) -> Box<dyn Element> {
    let xs = app.theme.spacing_px(SpacingToken::Xs);
    let keycap = Container::new(
        Text::new("esc")
            .with_font_size(10.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_padding(EdgeInsets::new(xs, 1.0, xs, 1.0))
    .with_border(app.theme.color(ColorToken::Border).into())
    .with_corner_radius(4.0)
    .finish();
    let row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(4.0)
        .with_child(keycap)
        .with_child(
            Text::new("for terminal")
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .finish();
    HoverButton::new(row, hover)
        .with_padding(EdgeInsets::new(xs, 2.0, xs, 2.0))
        .with_on_click(on_click)
        .finish()
}
