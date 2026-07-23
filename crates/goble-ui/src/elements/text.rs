use crate::elements::{Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::scene::{Color, Glyph, Rect as SceneRect, RectF};

pub struct TextElement {
    text: String,
    size: f32,
    color: Color,
    line_height: f32,
    measured_size: Option<(f32, f32)>,
}

impl TextElement {
    pub fn new(text: impl Into<String>, size: f32) -> Self {
        Self {
            text: text.into(),
            size,
            color: Color::new(1.0, 1.0, 1.0, 1.0),
            line_height: 1.4,
            measured_size: None,
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.measured_size = None;
    }
}

fn default_font() -> &'static fontdue::Font {
    static FONT: std::sync::OnceLock<fontdue::Font> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        let bytes = include_bytes!("../../assets/DejaVuSans.ttf");
        fontdue::Font::from_bytes(bytes as &[u8], fontdue::FontSettings::default())
            .expect("failed to load font")
    })
}

fn measure_text(text: &str, size: f32, max_width: f32, line_height: f32) -> (f32, f32) {
    let font = default_font();
    let mut y: f32 = 0.0;
    let mut max_line_width: f32 = 0.0;
    for line in text.lines() {
        let mut line_width: f32 = 0.0;
        let mut line_height_px: f32 = size;
        for word in line.split_whitespace() {
            let word_width: f32 =
                font.metrics(word.chars().next().unwrap_or(' '), size).width as f32;
            if line_width > 0.0 && line_width + word_width > max_width {
                max_line_width = max_line_width.max(line_width);
                y += line_height_px * line_height;
                line_width = word_width + size * 0.25;
                line_height_px = size;
            } else {
                line_width += word_width + size * 0.25;
            }
        }
        max_line_width = max_line_width.max(line_width);
        y += size * line_height;
    }
    (max_line_width, y.max(size * line_height))
}

impl Element for TextElement {
    fn layout(&mut self, constraint: SizeConstraint, _ctx: &mut LayoutContext) -> (f32, f32) {
        let size = measure_text(
            &self.text,
            self.size,
            constraint.max_width,
            self.line_height,
        );
        self.measured_size = Some(size);
        size
    }

    fn paint(&mut self, origin: Point, size: (f32, f32), ctx: &mut PaintContext) {
        ctx.scene.push_rect(
            SceneRect::new(RectF::new(origin.x, origin.y, size.0, size.1))
                .with_background(Color::new(0.0, 0.0, 0.0, 0.0)),
        );
        let font = default_font();
        let mut y = origin.y;
        let mut x = origin.x;
        for c in self.text.chars() {
            if c == '\n' {
                y += self.size * self.line_height;
                x = origin.x;
                continue;
            }
            let (metrics, _bitmap) = font.rasterize(c, self.size);
            ctx.scene.push_glyph(Glyph {
                font: crate::scene::FontId(0),
                glyph_index: 0,
                position: Point::new(
                    x + metrics.xmin as f32,
                    y + self.size - metrics.ymin as f32,
                    origin.z_index + 1,
                ),
                size: self.size,
                color: self.color,
            });
            x += metrics.advance_width;
        }
    }

    fn size(&self) -> Option<(f32, f32)> {
        self.measured_size
    }

    fn set_size(&mut self, size: (f32, f32)) {
        self.measured_size = Some(size);
    }

    fn dispatch_event(&mut self, _event: &EventContext, _origin: Point, _size: (f32, f32)) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_measure() {
        let (w, h) = measure_text("hello", 16.0, 800.0, 1.4);
        assert!(w > 0.0);
        assert!(h > 0.0);
    }
}
