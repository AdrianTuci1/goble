/// A 2D scene description produced by the UI element tree and consumed by the WGPU renderer.
#[derive(Clone, Default, Debug)]
pub struct Scene {
    pub layers: Vec<Layer>,
    pub scale_factor: f32,
}

impl Scene {
    pub fn new(scale_factor: f32) -> Self {
        Self {
            layers: vec![Layer::default()],
            scale_factor,
        }
    }

    pub fn current_layer(&mut self) -> &mut Layer {
        self.layers.last_mut().unwrap()
    }

    pub fn push_layer(&mut self) -> &mut Layer {
        let parent_clip = self.layers.last().and_then(|l| l.clip_bounds);
        self.layers.push(Layer {
            clip_bounds: parent_clip,
            ..Default::default()
        });
        self.current_layer()
    }

    pub fn pop_layer(&mut self) {
        if self.layers.len() > 1 {
            let layer = self.layers.pop().unwrap();
            self.current_layer().children.push(layer);
        }
    }

    pub fn push_rect(&mut self, rect: Rect) {
        self.current_layer().rects.push(rect);
    }

    pub fn push_glyph(&mut self, glyph: Glyph) {
        self.current_layer().glyphs.push(glyph);
    }

    pub fn set_clip(&mut self, bounds: Option<RectF>) {
        if let Some(layer) = self.layers.last_mut() {
            layer.clip_bounds = bounds;
        }
    }

    pub fn iter_layers(&self) -> impl Iterator<Item = &Layer> {
        self.layers.iter()
    }
}

#[derive(Clone, Default, Debug)]
pub struct Layer {
    pub clip_bounds: Option<RectF>,
    pub rects: Vec<Rect>,
    pub glyphs: Vec<Glyph>,
    pub children: Vec<Layer>,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl RectF {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.width
            && point.y >= self.y
            && point.y <= self.y + self.height
    }

    pub fn intersection(&self, other: RectF) -> Option<RectF> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);
        let width = right - x;
        let height = bottom - y;
        if width > 0.0 && height > 0.0 {
            Some(RectF::new(x, y, width, height))
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub z_index: i32,
}

impl Point {
    pub fn new(x: f32, y: f32, z_index: i32) -> Self {
        Self { x, y, z_index }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fill {
    None,
    Solid(Color),
}

impl Default for Fill {
    fn default() -> Self {
        Fill::None
    }
}

impl From<Color> for Fill {
    fn from(color: Color) -> Self {
        Fill::Solid(color)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_u8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Border {
    pub width: f32,
    pub color: Color,
    pub top: bool,
    pub left: bool,
    pub bottom: bool,
    pub right: bool,
}

impl Border {
    pub const fn new(width: f32) -> Self {
        Self {
            width,
            color: Color::new(0.0, 0.0, 0.0, 0.0),
            top: false,
            left: false,
            bottom: false,
            right: false,
        }
    }

    pub fn all(width: f32, color: Color) -> Self {
        Self {
            width,
            color,
            top: true,
            left: true,
            bottom: true,
            right: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CornerRadius {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_left: f32,
    pub bottom_right: f32,
}

impl CornerRadius {
    pub const fn uniform(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_left: radius,
            bottom_right: radius,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub font: FontId,
    pub glyph_index: u16,
    pub position: Point,
    pub size: f32,
    pub color: Color,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct FontId(pub usize);

#[derive(Clone, Debug)]
pub struct Rect {
    pub bounds: RectF,
    pub background: Fill,
    pub border: Border,
    pub corner_radius: CornerRadius,
}

impl Rect {
    pub fn new(bounds: RectF) -> Self {
        Self {
            bounds,
            background: Fill::None,
            border: Border::default(),
            corner_radius: CornerRadius::default(),
        }
    }

    pub fn with_background(mut self, fill: impl Into<Fill>) -> Self {
        self.background = fill.into();
        self
    }

    pub fn with_border(mut self, border: Border) -> Self {
        self.border = border;
        self
    }

    pub fn with_corner_radius(mut self, radius: CornerRadius) -> Self {
        self.corner_radius = radius;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rectf_contains() {
        let r = RectF::new(0.0, 0.0, 100.0, 100.0);
        assert!(r.contains(Point::new(50.0, 50.0, 0)));
        assert!(!r.contains(Point::new(150.0, 50.0, 0)));
    }

    #[test]
    fn test_scene_push_rect() {
        let mut scene = Scene::new(1.0);
        scene.push_rect(Rect::new(RectF::new(0.0, 0.0, 10.0, 10.0)));
        assert_eq!(scene.current_layer().rects.len(), 1);
    }
}
