use crate::color::ColorU;
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{vec2f, Vector2F};
use crate::theme::ColorToken;

const DEFAULT_ICON_SIZE: f32 = 16.0;

pub type IconName = &'static str;

pub struct Icon {
    name: IconName,
    size: f32,
    color: ColorU,
    layout_size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Icon {
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            size: DEFAULT_ICON_SIZE,
            color: ColorU::default(),
            layout_size: None,
            origin: None,
        }
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn with_color(mut self, color: impl Into<ColorU>) -> Self {
        self.color = color.into();
        self
    }

    pub fn with_theme_color(mut self, token: ColorToken, app: &AppContext) -> Self {
        self.color = app.theme.color(token);
        self
    }

    pub fn name(&self) -> IconName {
        self.name
    }

    pub fn icon_size(&self) -> f32 {
        self.size
    }
}

impl Element for Icon {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(self.size, self.size);
        self.layout_size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, _ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
    }

    fn size(&self) -> Option<Vector2F> {
        self.layout_size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_size_defaults_to_square() {
        let app = AppContext::default();
        let mut icon = Icon::new("send");
        let size = icon.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size.x, size.y);
        assert_eq!(size.x, DEFAULT_ICON_SIZE);
    }
}
