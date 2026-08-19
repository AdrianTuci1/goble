use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{vec2f, Vector2F};

/// A status indicator that signals an ongoing operation.
///
/// For now this renders a single colored dot; future iterations can add
/// animation frames or pulsing effects.
pub struct RunningIndicator {
    size: f32,
    size_cache: Option<Vector2F>,
    origin: Option<Point>,
}

impl RunningIndicator {
    pub fn new() -> Self {
        Self {
            size: 8.0,
            size_cache: None,
            origin: None,
        }
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl Default for RunningIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for RunningIndicator {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(self.size, self.size);
        self.size_cache = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, _ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
    }

    fn size(&self) -> Option<Vector2F> {
        self.size_cache
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::AppContext;

    #[test]
    fn indicator_has_requested_size() {
        let app = AppContext::default();
        let mut indicator = RunningIndicator::new().with_size(12.0);
        let size = indicator.layout(
            SizeConstraint::loose(vec2f(100.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size, vec2f(12.0, 12.0));
    }
}
