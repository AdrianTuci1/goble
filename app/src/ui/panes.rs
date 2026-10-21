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
    file_icon_name, AppContext, Axis, Border, ConstrainedBox, Container, CrossAxisAlignment,
    Divider, EdgeInsets, Element, Empty, EventContext, Expanded, Fill, Flex, HoverButton, Icon,
    LayoutContext, MainAxisSize, Padding, PaintContext, Point, SizeConstraint, Spacer, SplitNode,
    Stack, Text, TopbarButton,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::chat;
use super::file_view;
use super::pane::{carousel_regions, default_floating_rect, drop_ghost_rect, master_stack_regions};
use super::settings;
use super::shell::TOPBAR_HEIGHT;
use super::terminal;
use super::worker;
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
pub fn build_active_space(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let content = match state.spaces.get(state.active_space) {
        Some(space) => {
            // One pane at a time is drawn over the whole panes space: the leaf
            // the header's expand control names, instead of the tree. The tree
            // itself is left alone, so retracting puts the pane back in the
            // place it had. An id that names no leaf of *this* space (it was
            // closed, or it belongs to another space) falls through to the tree.
            if let Some(id) = state.maximized_pane {
                if let Some(kind) = leaf_kind_of(&space.root, id) {
                    let leaf = build_leaf(app, state, actions, id, kind);
                    if is_lifted(state, id) {
                        Box::new(HiddenInPlace::new(leaf))
                    } else {
                        leaf
                    }
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
        // A lifted pane leaves the tree while it is in flight (see
        // [`build_pane`]); a space whose only pane is in flight has nothing
        // left to draw until the drop lands.
        LayoutKind::Bsp => match build_pane(app, state, actions, &space.root) {
            Some(content) => content,
            None => Empty::new().finish(),
        },
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
                true,
                actions,
                state.floating_drag.clone(),
                lifted_pane(state),
            ))
        }
        LayoutKind::Floating => {
            // The stored order is the z-order; a leaf the space has not given a
            // rect yet falls back to the cascade, so nothing is drawn twice or
            // left invisible. A minimized window is not drawn over the pane
            // area: its chip in the dock below stands for it.
            let mut children = Vec::new();
            let mut placed = Vec::new();
            for pane in &space.floating {
                let Some(kind) = leaf_kind_of(&space.root, pane.pane_id) else {
                    continue;
                };
                // A minimized window is accounted for even though it is not
                // drawn: its chip in the dock stands for it, and the fallback
                // below must not put it back on the pane area.
                placed.push(pane.pane_id);
                if pane.minimized {
                    continue;
                }
                children.push(RegionChild::new(
                    pane.pane_id,
                    pane.rect,
                    build_window(app, state, actions, pane.pane_id, kind, true),
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
                    build_window(app, state, actions, id, kind, true),
                ));
            }
            let layer = RegionLayer::new(
                children,
                true,
                true,
                actions,
                state.floating_drag.clone(),
                lifted_pane(state),
            );
            let docked = space.minimized_panes();
            if docked.is_empty() {
                return Box::new(layer);
            }
            // The dock is a strip below the pane area: one chip per minimized
            // window, each one click away from coming back. It only exists
            // while something is docked, so the windows keep the whole area
            // otherwise.
            let dock = build_window_dock(app, actions, &space.root, &docked);
            Flex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(Expanded::new(Box::new(layer)).finish())
                .with_child(dock)
                .finish()
        }
        LayoutKind::Carousel => {
            // The row is centered on the app's focused pane; clicking any
            // visible sliver of a neighbour focuses it and the next frame puts
            // it in the middle. Every window carries the same chrome a floating
            // window has, including maximize and close.
            let mut leaves = Vec::new();
            space.root.leaves(&mut leaves);
            let focused = if leaves.iter().any(|(id, _)| *id == state.active_pane_id) {
                state.active_pane_id
            } else {
                leaves.first().map(|(id, _)| *id).unwrap_or_default()
            };
            let children = carousel_regions(&leaves, focused)
                .into_iter()
                .filter_map(|(id, rect)| {
                    let kind = leaf_kind_of(&space.root, id)?;
                    Some(RegionChild::new(
                        id,
                        PaneRect::new(rect.min_x(), rect.min_y(), rect.width(), rect.height()),
                        build_window(app, state, actions, id, kind, false),
                    ))
                })
                .collect();
            Box::new(RegionLayer::new(
                children,
                false,
                true,
                actions,
                state.floating_drag.clone(),
                lifted_pane(state),
            ))
        }
    }
}

/// The strip of minimized windows under the floating pane area. Each chip names
/// the window it stands for and puts it back on click; the dock itself is the
/// surface of the strip, so the chips read as windows waiting below the desk.
fn build_window_dock(
    app: &AppContext,
    actions: &UiActions,
    root: &Pane,
    docked: &[(u64, PaneRect)],
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Sm);
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(spacing * 0.5);
    row = row.with_child(
        Text::new("Minimized")
            .with_font_size(10.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    );
    for (id, _) in docked {
        let Some(kind) = leaf_kind_of(root, *id) else {
            continue;
        };
        let on_restore = actions.on_float_restore.clone();
        let pane_id = *id;
        let label = window_title(&kind);
        let chip = HoverButton::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(4.0)
                .with_child(
                    Icon::new("terminal")
                        .with_size(12.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_child(
                    Text::new(label)
                        .with_font_size(11.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .finish(),
            Rc::new(RefCell::new(false)),
        )
        .with_padding(EdgeInsets::new(8.0, 2.0, 8.0, 2.0))
        .with_corner_radius(4.0)
        .with_on_click(move || (on_restore.borrow_mut())(pane_id))
        .finish();
        row = row.with_child(chip);
    }
    Container::new(row.finish())
        .with_padding(EdgeInsets::new(spacing, 2.0, spacing, 2.0))
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .with_border(Border::all(1.0).with_border_color(app.theme.color(ColorToken::Border)))
        .finish()
}

fn build_pane(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    pane: &Pane,
) -> Option<Box<dyn Element>> {
    match pane {
        Pane::Leaf { id, kind } => {
            // The lifted pane is not drawn and holds no slot: the panes that
            // stay take the room it leaves, so the area under the pointer is
            // exactly the panes a drop can land on.
            if is_lifted(state, *id) {
                return None;
            }
            let leaf = build_leaf(app, state, actions, *id, kind.clone());
            // The frame sits a few pixels inside its split slot, so the
            // outline stays visible where two panes meet instead of being
            // clipped by the divider or the window edge.
            Some(
                Padding::new(
                    Box::new(PaneShadow::new(leaf)),
                    EdgeInsets::uniform(PANE_INSET),
                )
                .finish(),
            )
        }
        Pane::Split {
            id,
            dir,
            ratio,
            first,
            second,
        } => {
            let first = build_pane(app, state, actions, first);
            let second = build_pane(app, state, actions, second);
            // A split with one side lifted collapses onto the side that stays:
            // the survivor fills the slot both used to share.
            let (first, second) = match (first, second) {
                (Some(first), Some(second)) => (first, second),
                (Some(only), None) | (None, Some(only)) => return Some(only),
                (None, None) => return None,
            };
            let axis = match dir {
                SplitDir::Horizontal => Axis::Horizontal,
                SplitDir::Vertical => Axis::Vertical,
            };
            let dragging = state.dragging_pane_id == Some(*id);
            let on_start = actions.on_pane_drag_start.clone();
            let on_move = actions.on_pane_drag_move.clone();
            let on_end = actions.on_pane_drag_end.clone();
            let split_id = *id;
            let node = SplitNode::new(axis, *ratio, first, second)
                .with_dragging(dragging)
                .with_on_drag_start(move || (on_start.borrow_mut())(split_id))
                .with_on_drag_move(move |r| (on_move.borrow_mut())(split_id, r))
                .with_on_drag_end(move || (on_end.borrow_mut())());
            Some(Box::new(node))
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

/// A lifted pane drawn as nothing: until the drop lands, the drag card under
/// the pointer is the pane's only visible part. Layout still runs, so the place
/// the pane came from is the place it returns to when the drag is cancelled.
///
/// The tiled (BSP) tree does not use this: it leaves the lifted leaf out and
/// lets the panes that stay take its room (see [`build_pane`]). This wrapper
/// covers the slots that cannot be shared — the maximized pane that owns the
/// whole area, and a floating window whose own rectangle is its place.
struct HiddenInPlace {
    content: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl HiddenInPlace {
    fn new(content: Box<dyn Element>) -> Self {
        Self {
            content,
            size: None,
            origin: None,
        }
    }
}

impl Element for HiddenInPlace {
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

    fn paint(&mut self, origin: Vector2F, _ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
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

/// The pane currently lifted far enough to read as a drag, if any.
fn lifted_pane(state: &UiSnapshot) -> Option<u64> {
    state
        .pane_drag
        .as_ref()
        .filter(|drag| drag.moved())
        .map(|drag| drag.pane_id)
}

/// Whether `id` is the pane currently lifted far enough to read as a drag.
fn is_lifted(state: &UiSnapshot, id: u64) -> bool {
    lifted_pane(state) == Some(id)
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

/// The gap between a pane's frame and the slot it occupies, in pixels. It
/// keeps the outline visible at split boundaries and against the window edge.
pub(crate) const PANE_INSET: f32 = 4.0;

/// `rect` shrunk by `amount` on every side, never past half its width or
/// height. Pane frames are painted and hit-tested against this rectangle.
fn inset_rect(rect: RectF, amount: f32) -> RectF {
    let amount = amount.min(rect.width() * 0.5).min(rect.height() * 0.5);
    rectf(
        rect.min_x() + amount,
        rect.min_y() + amount,
        (rect.width() - amount * 2.0).max(1.0),
        (rect.height() - amount * 2.0).max(1.0),
    )
}

/// The corner grip a floating pane resizes from, in pixels.
const FLOAT_RESIZE_GRIP: f32 = 16.0;

/// The soft drop shadow every framed pane casts: a translucent rectangle offset
/// down and right of the pane, painted before it so only the edges show. Shared
/// by the region layouts and the BSP tree, so a tiled pane and a floating window
/// read as the same kind of surface.
const WINDOW_SHADOW_X: f32 = 4.0;
const WINDOW_SHADOW_Y: f32 = 6.0;
const WINDOW_SHADOW_COLOR: goble_ui::ColorU = goble_ui::ColorU::new(0, 0, 0, 72);

fn paint_window_shadow(ctx: &mut PaintContext, rect: RectF) {
    let Some(renderer) = ctx.renderer.as_mut() else {
        return;
    };
    renderer.fill_rect(
        rectf(
            rect.min_x() + WINDOW_SHADOW_X,
            rect.min_y() + WINDOW_SHADOW_Y,
            rect.width(),
            rect.height(),
        ),
        WINDOW_SHADOW_COLOR,
    );
}

/// A tiled pane drawn over the same soft shadow a floating window casts. The
/// BSP tree places its leaves with a `SplitNode`, where there is no
/// [`RegionLayer`] to paint the shadow, so this wrapper supplies it. It is
/// paint-only: layout and every event belong to the pane it wraps, and the
/// shadow is never a hit target.
struct PaneShadow {
    content: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl PaneShadow {
    fn new(content: Box<dyn Element>) -> Self {
        Self {
            content,
            size: None,
            origin: None,
        }
    }
}

impl Element for PaneShadow {
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
        if let Some(size) = self.size {
            paint_window_shadow(ctx, rectf(origin.x, origin.y, size.x, size.y));
        }
        self.content.paint(origin, ctx, app);
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
        self.content.dispatch_event(event, ctx, app)
    }
}

struct RegionLayer {
    children: Vec<RegionChild>,
    /// Whether presses on a window's titlebar move it and the corner grip
    /// resizes it. Only the floating layout owns stored rects to change.
    floating: bool,
    /// Whether the children are windows: shadow under each one, and a press
    /// anywhere on a window focuses it.
    windows: bool,
    floating_drag: Rc<RefCell<Option<FloatingDrag>>>,
    /// The pane lifted out of this layer, if any: it keeps its slot and layout
    /// but paints no frame and no shadow, and takes no pointer.
    lifted: Option<u64>,
    on_float_start: FloatStartHandler,
    on_float_rect: Rc<RefCell<dyn FnMut(u64, PaneRect)>>,
    on_float_rect_end: Rc<RefCell<dyn FnMut()>>,
    on_activate: Rc<RefCell<dyn FnMut(u64)>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl RegionLayer {
    fn new(
        children: Vec<RegionChild>,
        floating: bool,
        windows: bool,
        actions: &UiActions,
        floating_drag: Rc<RefCell<Option<FloatingDrag>>>,
        lifted: Option<u64>,
    ) -> Self {
        Self {
            children,
            floating,
            windows,
            floating_drag,
            lifted,
            on_float_start: actions.on_float_start.clone(),
            on_float_rect: actions.on_float_rect.clone(),
            on_float_rect_end: actions.on_float_rect_end.clone(),
            on_activate: actions.on_pane_activate.clone(),
            size: None,
            origin: None,
        }
    }

    /// The pixels a child occupies: its stored rect, inset by [`PANE_INSET`].
    /// Layout, painting, shadow and hit testing all read this one rectangle.
    fn child_pixels(child: &RegionChild, area: Vector2F) -> RectF {
        inset_rect(child.rect.to_pixels(area), PANE_INSET)
    }

    fn child_bounds(&self, child: &RegionChild) -> Option<RectF> {
        if self.lifted == Some(child.pane_id) {
            return None;
        }
        let origin = self.origin?;
        let px = Self::child_pixels(child, self.size?);
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
            let px = Self::child_pixels(child, area);
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
            if self.lifted == Some(child.pane_id) {
                continue;
            }
            let px = Self::child_pixels(child, area);
            let child_origin = origin + vec2f(px.min_x(), px.min_y());
            if self.windows {
                // A soft drop shadow under every window, so overlapping panes
                // read as separate surfaces the way OS windows do. It is drawn
                // before the child, so the window covers all but its edges.
                paint_window_shadow(
                    ctx,
                    rectf(child_origin.x, child_origin.y, px.width(), px.height()),
                );
            }
            child.content.paint(child_origin, ctx, app);
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
                        // The controls own their presses: a press on close must
                        // not also start dragging the window behind it.
                        let on_controls = position.x >= bounds.max_x() - WINDOW_CONTROLS_WIDTH
                            && position.y - bounds.min_y() <= WINDOW_TITLEBAR_HEIGHT;
                        let on_header = position.y - bounds.min_y() <= WINDOW_TITLEBAR_HEIGHT;
                        if on_grip {
                            (self.on_float_start.borrow_mut())(
                                pane.pane_id,
                                FloatingDragMode::Resize,
                                position,
                                area,
                            );
                        } else if on_header && !on_controls {
                            (self.on_float_start.borrow_mut())(
                                pane.pane_id,
                                FloatingDragMode::Move,
                                position,
                                area,
                            );
                        }
                    }
                    let _ = self.children[index].content.dispatch_event(event, ctx, app);
                    return true;
                }
                if self.windows {
                    if let DispatchedEvent::MouseDown { .. } = event {
                        (self.on_activate.borrow_mut())(self.children[index].pane_id);
                    }
                }
                return self.children[index].content.dispatch_event(event, ctx, app);
            }
            return false;
        }
        // Keys, focus changes and scrolling go to the topmost pane first, the
        // same order a stack dispatches to its overlays. The lifted pane is in
        // flight and takes nothing while its card is under the pointer.
        for child in self.children.iter_mut().rev() {
            if self.lifted == Some(child.pane_id) {
                continue;
            }
            if child.content.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        false
    }
}

/// The titlebar a floating or carousel window wears: a title on the left and
/// the three window controls on the right. The band is the drag handle the
/// region layer reads; the controls are wide enough that a press on them never
/// starts a move (see [`WINDOW_CONTROLS_WIDTH`]).
pub(crate) const WINDOW_TITLEBAR_HEIGHT: f32 = 26.0;
/// The room the window controls take at the titlebar's right end: three 20 px
/// buttons, the 6 px gaps between them and the bar's right padding. A press in
/// this band belongs to the control under it and never starts a window move.
pub(crate) const WINDOW_CONTROLS_WIDTH: f32 = 80.0;
const WINDOW_CONTROL_SIZE: f32 = 20.0;
/// The corner radius of a pane's frame: the same on a tiled pane and a window.
const WINDOW_CORNER_RADIUS: f32 = 6.0;

/// A pane as an operating-system window: its titlebar and controls above its
/// content, a border that turns accent when it is focused, and the shadow the
/// region layer paints under it. The content is the same leaf every layout
/// draws, so moving between layouts never remounts a shell or a conversation.
///
/// `minimize` is offered in the floating layout, whose dock can hold the
/// window; the carousel has no dock, so its windows carry maximize and close
/// only.
fn build_window(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
    id: u64,
    kind: PaneKind,
    minimize: bool,
) -> Box<dyn Element> {
    let active = state.active_pane_id == id;
    let titlebar = build_window_titlebar(app, actions, id, &kind, active, minimize);
    let body = build_leaf_column(app, state, actions, id, kind, false);
    let column = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            ConstrainedBox::new(titlebar)
                .with_height(WINDOW_TITLEBAR_HEIGHT)
                .finish(),
        )
        .with_child(Divider::horizontal().with_thickness(1.0).finish())
        .with_child(Expanded::new(body).finish());
    let border = if active {
        ColorToken::Accent
    } else {
        ColorToken::Border
    };
    let window: Box<dyn Element> = Container::new(column.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(Border::all(1.0).with_border_color(app.theme.color(border)))
        .with_corner_radius(WINDOW_CORNER_RADIUS)
        .finish();
    // A floating window keeps its own rectangle while lifted — there is no
    // sibling to take the room — so only the drag card stays visible.
    if is_lifted(state, id) {
        Box::new(HiddenInPlace::new(window))
    } else {
        window
    }
}

/// The titlebar row of one window. Presses on the band outside the controls are
/// the region layer's move gesture; the controls themselves are ordinary
/// buttons, which is why the move never starts on them.
fn build_window_titlebar(
    app: &AppContext,
    actions: &UiActions,
    id: u64,
    kind: &PaneKind,
    active: bool,
    minimize: bool,
) -> Box<dyn Element> {
    let text_color = if active {
        ColorToken::Text
    } else {
        ColorToken::Muted
    };
    let mut row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.0)
        .with_child(
            Icon::new(window_icon(kind))
                .with_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_child(
            Text::new(window_title(kind))
                .with_font_size(11.0)
                .with_theme_color(text_color, app)
                .finish(),
        )
        .with_child(Spacer::new().finish());

    if minimize {
        let on_minimize = actions.on_float_minimize.clone();
        let pane_id = id;
        row = row.with_child(
            TopbarButton::new(
                Icon::new("minimize")
                    .with_size(12.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_size(WINDOW_CONTROL_SIZE)
            .with_on_click(move || (on_minimize.borrow_mut())(pane_id))
            .finish(),
        );
    }

    let on_toggle_maximized = actions.on_toggle_pane_maximized.clone();
    let pane_id = id;
    row = row.with_child(
        TopbarButton::new(
            Icon::new("maximize")
                .with_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_size(WINDOW_CONTROL_SIZE)
        .with_on_click(move || (on_toggle_maximized.borrow_mut())(pane_id))
        .finish(),
    );

    // Close is the app's close rule, which names a pane by making it active
    // first: the window clicked and the pane closed are then the same one.
    let on_activate = actions.on_pane_activate.clone();
    let on_close = actions.on_close_pane.clone();
    let pane_id = id;
    row = row.with_child(
        TopbarButton::new(
            Icon::new("x")
                .with_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_size(WINDOW_CONTROL_SIZE)
        .with_on_click(move || {
            (on_activate.borrow_mut())(pane_id);
            (on_close.borrow_mut())();
        })
        .finish(),
    );

    Container::new(row.finish())
        .with_padding(EdgeInsets::new(8.0, 0.0, 4.0, 0.0))
        .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
        .finish()
}

/// The icon a window's titlebar carries for the pane's kind.
fn window_icon(kind: &PaneKind) -> &'static str {
    match kind {
        PaneKind::Chat | PaneKind::Worker => "agent",
        PaneKind::Terminal => "terminal",
        PaneKind::File { path } => file_icon_name(&file_view::file_name(path)),
        PaneKind::Settings { .. } => "settings",
    }
}

/// The title a window's titlebar draws.
fn window_title(kind: &PaneKind) -> String {
    match kind {
        PaneKind::Chat | PaneKind::Worker => "Agent".to_string(),
        PaneKind::Terminal => "Terminal".to_string(),
        PaneKind::File { path } => file_view::file_name(path),
        PaneKind::Settings { .. } => "Settings".to_string(),
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
    let column = build_leaf_column(app, state, actions, id, kind, lift);
    let border_color = if active {
        ColorToken::Accent
    } else {
        ColorToken::Border
    };
    let pane = Container::new(column)
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(app.theme.color(border_color).into())
        .with_corner_radius(WINDOW_CORNER_RADIUS)
        .finish();
    let framed: Box<dyn Element> = if active {
        // The focused pane's mark: the accent triangle sits in the pane's own
        // top-left corner, which is where the pane the keyboard is in is read
        // from. The pane's bar starts past it (see `chat::build_agent_header`).
        let marker = Icon::new("upper-left-triangle")
            .with_size(FOCUS_MARKER_SIZE)
            .with_theme_color(ColorToken::Accent, app)
            .finish();
        Stack::new()
            .with_children(vec![pane])
            .with_overlay(marker, vec2f(0.0, 0.0))
            .finish()
    } else {
        pane
    };
    // A lifted pane paints nothing where it stood. The tiled tree has already
    // left it out (see `build_pane`); a maximized pane or a region layout keeps
    // its slot, so cancelling the drag puts it back exactly there.
    if is_lifted(state, id) {
        Box::new(HiddenInPlace::new(framed))
    } else {
        framed
    }
}

/// The pane's content as a column: the click-focus wrapper around the surface,
/// with no border or focus mark. A tiled leaf and a window frame both build on
/// this, so the two share exactly one definition of what a pane holds.
fn build_leaf_column(
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
    Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(Expanded::new(content).finish())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_frames_are_inset_from_their_slots() {
        let slot = rectf(0.0, 0.0, 100.0, 50.0);
        assert_eq!(PANE_INSET, 4.0);
        assert_eq!(
            inset_rect(slot, PANE_INSET),
            rectf(4.0, 4.0, 92.0, 42.0),
            "the frame leaves room for the outline on every side"
        );
    }

    #[test]
    fn the_inset_clamps_on_a_slot_smaller_than_the_frame() {
        let frame = inset_rect(rectf(10.0, 20.0, 6.0, 4.0), PANE_INSET);
        assert_eq!(frame, rectf(12.0, 22.0, 2.0, 1.0));
    }
}
