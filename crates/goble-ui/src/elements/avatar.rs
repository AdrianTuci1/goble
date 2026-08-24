use crate::color::ColorU;
use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::theme::ColorToken;

const DEFAULT_AVATAR_SIZE: f32 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AvatarShape {
    #[default]
    Circle,
    Squircle,
}

pub struct Avatar {
    label: String,
    size: f32,
    shape: AvatarShape,
    background: ColorU,
    foreground: ColorU,
    layout_size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Avatar {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            size: DEFAULT_AVATAR_SIZE,
            shape: AvatarShape::default(),
            background: ColorU::new(0, 0, 0, 0),
            foreground: ColorU::new(255, 255, 255, 255),
            layout_size: None,
            origin: None,
        }
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn with_shape(mut self, shape: AvatarShape) -> Self {
        self.shape = shape;
        self
    }

    pub fn with_background(mut self, color: impl Into<ColorU>) -> Self {
        self.background = color.into();
        self
    }

    pub fn with_foreground(mut self, color: impl Into<ColorU>) -> Self {
        self.foreground = color.into();
        self
    }

    pub fn with_theme_background(mut self, token: ColorToken, app: &AppContext) -> Self {
        self.background = app.theme.color(token);
        self
    }

    pub fn with_theme_foreground(mut self, token: ColorToken, app: &AppContext) -> Self {
        self.foreground = app.theme.color(token);
        self
    }

    pub fn initials(&self) -> String {
        self.label
            .split_whitespace()
            .filter_map(|word| word.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase()
    }
}

impl Element for Avatar {
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

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));

        let bg = if self.background.a == 0 {
            app.theme.color(ColorToken::Accent)
        } else {
            self.background
        };
        let fg = if self.foreground.a == 0 {
            app.theme.color(ColorToken::Text)
        } else {
            self.foreground
        };

        let radius = match self.shape {
            AvatarShape::Circle => self.size * 0.5,
            AvatarShape::Squircle => self.size * 0.22,
        };

        ctx.renderer.as_mut().unwrap().fill_rounded_rect(
            rectf(origin.x, origin.y, self.size, self.size),
            bg,
            radius,
        );

        let initials = self.initials();
        if !initials.is_empty() {
            let font_size = self.size * 0.45;
            let approx_char_width = font_size * 0.55;
            let text_width = initials.len() as f32 * approx_char_width;
            let text_x = origin.x + (self.size - text_width) * 0.5;
            let text_y = origin.y + (self.size - font_size) * 0.5;
            ctx.renderer.as_mut().unwrap().draw_text(
                vec2f(text_x, text_y),
                initials,
                font_size,
                fg,
                self.size,
            );
        }
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
    fn avatar_initials_from_two_words() {
        let avatar = Avatar::new("Ada Lovelace");
        assert_eq!(avatar.initials(), "AL");
    }

    #[test]
    fn avatar_initials_from_one_word() {
        let avatar = Avatar::new("goble");
        assert_eq!(avatar.initials(), "G");
    }

    #[test]
    fn avatar_layout_is_square() {
        let app = AppContext::default();
        let mut avatar = Avatar::new("User").with_size(48.0);
        let size = avatar.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size, vec2f(48.0, 48.0));
    }
}
