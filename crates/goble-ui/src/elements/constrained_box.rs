use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;

pub struct ConstrainedBox {
    child: Option<Box<dyn Element>>,
    min_width: Option<f32>,
    max_width: Option<f32>,
    min_height: Option<f32>,
    max_height: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Default for ConstrainedBox {
    fn default() -> Self {
        Self {
            child: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            width: None,
            height: None,
            size: None,
            origin: None,
        }
    }
}

impl ConstrainedBox {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child: Some(child),
            ..Default::default()
        }
    }

    pub fn with_min_width(mut self, value: f32) -> Self {
        self.min_width = Some(value);
        self
    }

    pub fn with_max_width(mut self, value: f32) -> Self {
        self.max_width = Some(value);
        self
    }

    pub fn with_min_height(mut self, value: f32) -> Self {
        self.min_height = Some(value);
        self
    }

    pub fn with_max_height(mut self, value: f32) -> Self {
        self.max_height = Some(value);
        self
    }

    pub fn with_width(mut self, value: f32) -> Self {
        self.width = Some(value);
        self
    }

    pub fn with_height(mut self, value: f32) -> Self {
        self.height = Some(value);
        self
    }

    fn apply(&self, constraint: SizeConstraint) -> SizeConstraint {
        let min_x = self
            .min_width
            .unwrap_or(constraint.min.x)
            .max(constraint.min.x);
        let max_x = self
            .max_width
            .unwrap_or(constraint.max.x)
            .min(constraint.max.x);
        let min_y = self
            .min_height
            .unwrap_or(constraint.min.y)
            .max(constraint.min.y);
        let max_y = self
            .max_height
            .unwrap_or(constraint.max.y)
            .min(constraint.max.y);

        let min_x = self.width.unwrap_or(min_x).max(min_x).min(max_x);
        let max_x = self.width.unwrap_or(max_x).max(min_x).min(max_x);
        let min_y = self.height.unwrap_or(min_y).max(min_y).min(max_y);
        let max_y = self.height.unwrap_or(max_y).max(min_y).min(max_y);

        SizeConstraint::new(Vector2F::new(min_x, min_y), Vector2F::new(max_x, max_y))
    }
}

impl Extend<Box<dyn Element>> for ConstrainedBox {
    fn extend<T: IntoIterator<Item = Box<dyn Element>>>(&mut self, iter: T) {
        for child in iter {
            self.child = Some(child);
        }
    }
}

impl Element for ConstrainedBox {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let child_constraint = self.apply(constraint);
        let size = if let Some(child) = self.child.as_mut() {
            child.layout(child_constraint, ctx, app)
        } else {
            Vector2F::zero()
        };
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(child) = self.child.as_mut() {
            child.paint(origin, ctx, app);
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
