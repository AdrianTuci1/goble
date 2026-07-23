use crate::elements::{Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::scene::{Fill, Rect as SceneRect, RectF};

pub struct RectElement {
    size: Option<(f32, f32)>,
    fill: Fill,
}

impl RectElement {
    pub fn new(fill: impl Into<Fill>) -> Self {
        Self {
            size: None,
            fill: fill.into(),
        }
    }

    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.size = Some((width, height));
        self
    }
}

impl Element for RectElement {
    fn layout(&mut self, constraint: SizeConstraint, _ctx: &mut LayoutContext) -> (f32, f32) {
        let size = self
            .size
            .unwrap_or((constraint.max_width, constraint.max_height));
        let width = size.0.min(constraint.max_width).max(constraint.min_width);
        let height = size.1.min(constraint.max_height).max(constraint.min_height);
        (width, height)
    }

    fn paint(&mut self, origin: Point, size: (f32, f32), ctx: &mut PaintContext) {
        ctx.scene.push_rect(
            SceneRect::new(RectF::new(origin.x, origin.y, size.0, size.1))
                .with_background(self.fill),
        );
    }

    fn size(&self) -> Option<(f32, f32)> {
        self.size
    }

    fn set_size(&mut self, size: (f32, f32)) {
        self.size = Some(size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Color, Scene};
    use crate::theme::Theme;

    #[test]
    fn test_rect_layout_paint() {
        let mut el = RectElement::new(Color::from_u8(255, 0, 0, 255)).with_size(100.0, 50.0);
        let theme = Theme::dark();
        let mut ctx = LayoutContext::new(theme, 1.0);
        let size = el.layout(SizeConstraint::new(800.0, 600.0), &mut ctx);
        assert_eq!(size, (100.0, 50.0));

        let mut scene = Scene::new(1.0);
        {
            let mut paint_ctx = PaintContext {
                scene: &mut scene,
                theme: &Theme::dark(),
            };
            el.paint(Point::new(10.0, 20.0, 0), size, &mut paint_ctx);
        }
        assert_eq!(scene.current_layer().rects.len(), 1);
    }
}
