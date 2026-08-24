use crate::elements::{
    AppContext, Border, EdgeInsets, Element, EventContext, Fill, LayoutContext, PaintContext,
    Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, RectF, Vector2F};

pub struct Container {
    child: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    background: Fill,
    border: Option<Border>,
    corner_radius: f32,
    padding: EdgeInsets,
}

impl Default for Container {
    fn default() -> Self {
        Self {
            child: None,
            size: None,
            origin: None,
            background: Fill::None,
            border: None,
            corner_radius: 0.0,
            padding: EdgeInsets::uniform(0.0),
        }
    }
}

impl Container {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child: Some(child),
            ..Default::default()
        }
    }

    pub fn with_background(mut self, fill: impl Into<Fill>) -> Self {
        self.background = fill.into();
        self
    }

    pub fn with_border(mut self, border: Border) -> Self {
        self.border = Some(border);
        self
    }

    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }

    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    pub fn with_padding_uniform(mut self, padding: f32) -> Self {
        self.padding = EdgeInsets::uniform(padding);
        self
    }

    pub fn with_padding_left(mut self, value: f32) -> Self {
        self.padding = self.padding.with_left(value);
        self
    }

    pub fn with_padding_top(mut self, value: f32) -> Self {
        self.padding = self.padding.with_top(value);
        self
    }

    pub fn with_padding_right(mut self, value: f32) -> Self {
        self.padding = self.padding.with_right(value);
        self
    }

    pub fn with_padding_bottom(mut self, value: f32) -> Self {
        self.padding = self.padding.with_bottom(value);
        self
    }
}

impl Extend<Box<dyn Element>> for Container {
    fn extend<T: IntoIterator<Item = Box<dyn Element>>>(&mut self, iter: T) {
        for child in iter {
            self.child = Some(child);
        }
    }
}

impl Element for Container {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let horizontal = self.padding.left + self.padding.right;
        let vertical = self.padding.top + self.padding.bottom;

        let inner_max = vec2f(
            (constraint.max.x - horizontal).max(0.0),
            (constraint.max.y - vertical).max(0.0),
        );
        let inner_min = vec2f(
            (constraint.min.x - horizontal).max(0.0),
            (constraint.min.y - vertical).max(0.0),
        );
        let inner_constraint = SizeConstraint::new(inner_min, inner_max);

        let child_size = if let Some(child) = self.child.as_mut() {
            child.layout(inner_constraint, ctx, app)
        } else {
            Vector2F::zero()
        };

        let size = vec2f(child_size.x + horizontal, child_size.y + vertical);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let rect = RectF::new(
            crate::geometry::PointF::new(origin.x, origin.y),
            crate::geometry::Size2F::new(size.x, size.y),
        );

        if let Fill::Solid(color) = self.background {
            if let Some(renderer) = ctx.renderer.as_mut() {
                renderer.fill_rounded_rect(rect, color, self.corner_radius);
            }
        }

        if let Some(border) = &self.border {
            if let Fill::Solid(color) = border.color {
                if let Some(renderer) = ctx.renderer.as_mut() {
                    renderer.stroke_rect(rect, color, border.width, self.corner_radius);
                }
            }
        }

        if let Some(child) = self.child.as_mut() {
            let child_origin = vec2f(origin.x + self.padding.left, origin.y + self.padding.top);
            child.paint(child_origin, ctx, app);
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
        if let Some(child) = self.child.as_mut() {
            child.dispatch_event(event, ctx, app)
        } else {
            false
        }
    }
}
