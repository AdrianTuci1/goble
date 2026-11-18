use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::platform::window::CursorKind;

/// Splits horizontal space: fixed-width sidebar + main area filling the rest.
/// The engine's `Flex` cannot yet distribute remaining space, so this custom
/// element does the split at layout time.
///
/// An invisible draggable band sits on the sidebar's right edge so the user can
/// resize it. The width lives in app state (it must survive the per-frame
/// rebuild), and the drag flags/callbacks are passed in from the snapshot.
///
/// The edge paints no divider: like the agent content lane's handles, the band
/// shows only the resize cursor over it. The sidebar's own surface and the pane
/// area's inset separation do the separating.
const RESIZE_HANDLE_HALF: f32 = 4.0;

type DragHandler = Option<Rc<RefCell<dyn FnMut(f32)>>>;

pub struct SidebarLayout {
    sidebar: Box<dyn Element>,
    main: Box<dyn Element>,
    width: f32,
    dragging: bool,
    on_drag_start: DragHandler,
    on_drag_move: DragHandler,
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

    /// Bounds of the resizable band, in absolute window coordinates.
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

    /// Ask the platform for the cursor this frame should show: the east-west
    /// resize arrow over the sidebar's edge, and for the whole of a captured
    /// drag. A layout the pointer is outside the band leaves whatever another
    /// surface asked for alone.
    fn request_cursor(&self, ctx: &PaintContext, app: &AppContext) {
        let over_handle = ctx.cursor_inside
            && self
                .handle_bounds()
                .is_some_and(|bounds| contains(bounds, ctx.cursor_position));
        if self.dragging || over_handle {
            app.window_control.set_cursor(CursorKind::EwResize);
        }
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
        // The main column paints before the sidebar: the two regions do not
        // overlap, so the picture is the same, and the sidebar's runs come last
        // in the frame. The text atlas keeps the runs drawn last when a frame
        // is too large for it — the runs a window cannot afford to lose — so
        // the conversation list never blinks while a long pane streams.
        self.main.paint(origin + vec2f(self.width, 0.0), ctx, app);
        self.sidebar.paint(origin, ctx, app);

        // No rule is drawn on the sidebar's edge: the resize cursor over the
        // invisible grab band is the edge's whole affordance (the same
        // cursor-only treatment as the agent content lane's handles). It is
        // requested after both children paint, so the sidebar's own content
        // and the panes' lanes keep their cursor requests except over the band.
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
