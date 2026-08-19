use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{vec2f, Vector2F};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

pub struct Align {
    child: Option<Box<dyn Element>>,
    alignment: Alignment,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Align {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child: Some(child),
            alignment: Alignment::Center,
            size: None,
            origin: None,
        }
    }

    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }
}

impl Extend<Box<dyn Element>> for Align {
    fn extend<T: IntoIterator<Item = Box<dyn Element>>>(&mut self, iter: T) {
        for child in iter {
            self.child = Some(child);
        }
    }
}

impl Element for Align {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = if let Some(child) = self.child.as_mut() {
            child.layout(constraint, ctx, app)
        } else {
            Vector2F::zero()
        };
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(child) = self.child.as_mut() {
            let child_size = child.size().unwrap_or(Vector2F::zero());
            let parent_size = self.size.unwrap_or(child_size);
            let x = match self.alignment {
                Alignment::TopLeft | Alignment::CenterLeft | Alignment::BottomLeft => 0.0,
                Alignment::TopCenter | Alignment::Center | Alignment::BottomCenter => {
                    (parent_size.x - child_size.x) / 2.0
                }
                Alignment::TopRight | Alignment::CenterRight | Alignment::BottomRight => {
                    parent_size.x - child_size.x
                }
            };
            let y = match self.alignment {
                Alignment::TopLeft | Alignment::TopCenter | Alignment::TopRight => 0.0,
                Alignment::CenterLeft | Alignment::Center | Alignment::CenterRight => {
                    (parent_size.y - child_size.y) / 2.0
                }
                Alignment::BottomLeft | Alignment::BottomCenter | Alignment::BottomRight => {
                    parent_size.y - child_size.y
                }
            };
            child.paint(origin + vec2f(x, y), ctx, app);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}
