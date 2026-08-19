use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::Vector2F;

/// A flexible spacer that expands to fill available space along the main axis.
///
/// Currently it reports zero size; the flex layout will later distribute
/// leftover space to spacers with a `flex` factor.
pub struct Spacer {
    flex: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Spacer {
    pub fn new() -> Self {
        Self {
            flex: 1.0,
            size: None,
            origin: None,
        }
    }

    pub fn with_flex(mut self, flex: f32) -> Self {
        self.flex = flex.max(0.0);
        self
    }

    pub fn flex(&self) -> f32 {
        self.flex
    }
}

impl Default for Spacer {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for Spacer {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = Vector2F::zero();
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacer_reports_zero_size_by_default() {
        let app = AppContext::default();
        let mut spacer = Spacer::new();
        let size = spacer.layout(
            SizeConstraint::loose(Vector2F::new(100.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size, Vector2F::zero());
    }
}
