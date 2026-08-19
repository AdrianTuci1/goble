use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{vec2f, Vector2F};

pub struct Stack {
    children: Vec<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Stack {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            size: None,
            origin: None,
        }
    }

    pub fn with_children(mut self, children: impl IntoIterator<Item = Box<dyn Element>>) -> Self {
        self.children.extend(children);
        self
    }
}

impl Default for Stack {
    fn default() -> Self {
        Self::new()
    }
}

impl Extend<Box<dyn Element>> for Stack {
    fn extend<T: IntoIterator<Item = Box<dyn Element>>>(&mut self, iter: T) {
        self.children.extend(iter);
    }
}

impl Element for Stack {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let mut size = Vector2F::zero();
        for child in &mut self.children {
            let child_size = child.layout(constraint, ctx, app);
            size = vec2f(size.x.max(child_size.x), size.y.max(child_size.y));
        }
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        for child in &mut self.children {
            child.paint(origin, ctx, app);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}
