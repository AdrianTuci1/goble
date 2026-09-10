use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, PointF, RectF, Size2F, Vector2F};

/// Default width of the panel when expanded.
pub const SHEET_DEFAULT_WIDTH: f32 = 360.0;

/// Dimmed backdrop color used while the sheet is open.
const BACKDROP_COLOR: ColorU = ColorU::new(0, 0, 0, 110);

/// An overlay panel anchored to the right edge.
///
/// When `expanded` is false the sheet occupies zero space and ignores events,
/// so the underlying UI is fully interactive. When expanded it paints a dimmed
/// backdrop over the whole area plus the panel on the right; clicking the
/// backdrop fires `on_close`.
pub struct Sheet {
    child: Box<dyn Element>,
    expanded: bool,
    width: f32,
    on_close: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    /// Offset of the panel relative to the element origin.
    panel_origin: Vector2F,
    panel_size: Vector2F,
}

impl Sheet {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            expanded: false,
            width: SHEET_DEFAULT_WIDTH,
            on_close: None,
            size: None,
            origin: None,
            panel_origin: Vector2F::zero(),
            panel_size: Vector2F::zero(),
        }
    }

    pub fn with_expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width.max(0.0);
        self
    }

    pub fn with_on_close<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_close = Some(Rc::new(RefCell::new(callback)));
        self
    }
}

impl Element for Sheet {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        if !self.expanded {
            self.size = Some(Vector2F::zero());
            self.panel_origin = Vector2F::zero();
            self.panel_size = Vector2F::zero();
            return Vector2F::zero();
        }
        let panel_width = self.width.min(constraint.max.x).max(0.0);
        let panel_constraint =
            SizeConstraint::loose(vec2f(panel_width, constraint.max.y.max(0.0)));
        let child_size = self.child.layout(panel_constraint, ctx, app);
        self.panel_origin = vec2f((constraint.max.x - panel_width).max(0.0), 0.0);
        self.panel_size = vec2f(panel_width, constraint.max.y.max(0.0));
        let size = vec2f(constraint.max.x, constraint.max.y);
        self.size = Some(size);
        let _ = child_size;
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if !self.expanded {
            return;
        }
        let size = self.size.unwrap_or(Vector2F::zero());
        if let Some(renderer) = ctx.renderer.as_mut() {
            let backdrop = RectF::new(
                PointF::new(origin.x, origin.y),
                Size2F::new(size.x, size.y),
            );
            renderer.fill_rounded_rect(backdrop, BACKDROP_COLOR, 0.0);
        }
        let child_origin = vec2f(origin.x + self.panel_origin.x, origin.y + self.panel_origin.y);
        self.child.paint(child_origin, ctx, app);
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
        if !self.expanded {
            return false;
        }
        let origin = match self.origin {
            Some(p) => p.xy(),
            None => return false,
        };
        let full_bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };
        let panel_bounds = RectF::new(
            PointF::new(origin.x + self.panel_origin.x, origin.y + self.panel_origin.y),
            Size2F::new(self.panel_size.x, self.panel_size.y),
        );
        let cb = self.on_close.clone();

        match event {
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. } => {
                if panel_bounds.contains(PointF::new(position.x, position.y)) {
                    return self.child.dispatch_event(event, ctx, app);
                }
                if full_bounds.contains(PointF::new(position.x, position.y)) {
                    // Click on the backdrop closes the sheet.
                    if matches!(event, DispatchedEvent::MouseDown { .. }) {
                        if let Some(cb) = cb.as_ref() {
                            (cb.borrow_mut())();
                        }
                    }
                    return true;
                }
                false
            }
            _ => self.child.dispatch_event(event, ctx, app),
        }
    }
}
