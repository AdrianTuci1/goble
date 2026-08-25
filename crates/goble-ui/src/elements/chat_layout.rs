use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};

pub const CHAT_RIGHT_SIDEBAR_WIDTH: f32 = 280.0;

/// Lays out a main chat surface and an optional right sidebar.
///
/// The main content fills the remaining width; the right sidebar is fixed at
/// `CHAT_RIGHT_SIDEBAR_WIDTH` and is painted flush against the right edge.
pub struct ChatLayout {
    main: Box<dyn Element>,
    right: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatLayout {
    pub fn new(main: Box<dyn Element>) -> Self {
        Self {
            main,
            right: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_right_sidebar(mut self, right: Box<dyn Element>) -> Self {
        self.right = Some(right);
        self
    }
}

impl Element for ChatLayout {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let right_width = if self.right.is_some() {
            CHAT_RIGHT_SIDEBAR_WIDTH
        } else {
            0.0
        };
        let available_width = constraint.max.x;
        let main_max_width = (available_width - right_width).max(0.0);
        let main_constraint = SizeConstraint::new(
            vec2f(0.0, constraint.min.y),
            vec2f(main_max_width, constraint.max.y),
        );
        let main_size = self.main.layout(main_constraint, ctx, app);

        let height = if let Some(right) = self.right.as_mut() {
            let right_constraint =
                SizeConstraint::tight(vec2f(right_width, main_size.y.max(constraint.max.y)));
            let right_size = right.layout(right_constraint, ctx, app);
            main_size.y.max(right_size.y)
        } else {
            main_size.y
        };

        let total_width = if self.right.is_some() {
            available_width
        } else {
            main_size.x
        };

        self.size = Some(vec2f(total_width, height));
        self.size.unwrap()
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.main.paint(origin, ctx, app);
        if let Some(right) = self.right.as_mut() {
            let right_x =
                (self.size.unwrap_or(Vector2F::zero()).x - CHAT_RIGHT_SIDEBAR_WIDTH).max(0.0);
            right.paint(origin + vec2f(right_x, 0.0), ctx, app);
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
        // Route events to the right sidebar first because it overlays the main content.
        if let Some(right) = self.right.as_mut() {
            if right.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        self.main.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{AppContext, Empty, LayoutContext};
    use crate::geometry::vec2f;

    #[test]
    fn chat_layout_with_sidebar_sizes_correctly() {
        let app = AppContext::default();
        let mut layout = ChatLayout::new(Empty::new().with_size(vec2f(100.0, 100.0)).finish())
            .with_right_sidebar(
                Empty::new()
                    .with_size(vec2f(CHAT_RIGHT_SIDEBAR_WIDTH, 120.0))
                    .finish(),
            );
        let size = layout.layout(
            SizeConstraint::loose(vec2f(800.0, 600.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size.x, 800.0);
        assert!(size.y >= 120.0);
    }
}
