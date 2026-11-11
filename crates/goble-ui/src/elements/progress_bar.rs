use crate::color::ColorU;
use crate::elements::{
    AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::geometry::{rectf, Vector2F};

/// A thin segmented progress bar: one rounded slice per step, in the order the
/// steps run. The caller resolves each step's colour from its own state.
///
/// The bar takes the width it is given and only the height it was asked for,
/// so it sits under a heading without pushing the rows below it.
pub struct ProgressBar {
    colors: Vec<ColorU>,
    height: f32,
    gap: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ProgressBar {
    pub fn new(colors: Vec<ColorU>) -> Self {
        Self {
            colors,
            height: 4.0,
            gap: 3.0,
            size: None,
            origin: None,
        }
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn with_gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }
}

impl Element for ProgressBar {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let width = if constraint.max.x.is_finite() {
            constraint.max.x
        } else {
            constraint.min.x
        }
        .max(0.0);
        let height = if self.colors.is_empty() { 0.0 } else { self.height };
        let size = Vector2F::new(width, height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        let count = self.colors.len();
        if count == 0 || size.x <= 0.0 {
            return;
        }
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let total_gap = self.gap * (count.saturating_sub(1)) as f32;
        let slice = ((size.x - total_gap) / count as f32).max(0.0);
        for (index, color) in self.colors.iter().enumerate() {
            let x = origin.x + index as f32 * (slice + self.gap);
            renderer.fill_rounded_rect(
                rectf(x, origin.y, slice, size.y),
                *color,
                size.y / 2.0,
            );
        }
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
    use crate::color::ColorU;
    use crate::elements::{LayoutContext, PaintContext};
    use crate::geometry::vec2f;
    use crate::render::{RenderCommand, Renderer};

    #[test]
    fn one_rounded_slice_per_step_in_order() {
        let app = AppContext::default();
        let colors = vec![
            ColorU::new(10, 20, 30, 255),
            ColorU::new(40, 50, 60, 255),
            ColorU::new(70, 80, 90, 255),
        ];
        let mut bar = ProgressBar::new(colors.clone()).with_height(4.0).with_gap(2.0);
        let size = bar.layout(
            SizeConstraint::loose(vec2f(100.0, 20.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size, vec2f(100.0, 4.0));
        let mut ctx = PaintContext::new(Renderer::new());
        bar.paint(vec2f(0.0, 0.0), &mut ctx, &app);
        let commands = ctx
            .renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default();
        let rects: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { rect, color, corner_radius } => {
                    Some((*rect, *color, *corner_radius))
                }
                _ => None,
            })
            .collect();
        assert_eq!(rects.len(), 3, "one slice per step");
        let slice = (100.0 - 4.0) / 3.0;
        for (index, (rect, color, radius)) in rects.iter().enumerate() {
            assert_eq!(*color, colors[index]);
            assert!((rect.size.width - slice).abs() < 0.01);
            assert!((rect.min_x() - index as f32 * (slice + 2.0)).abs() < 0.01);
            assert_eq!(*radius, 2.0);
        }
    }
}
