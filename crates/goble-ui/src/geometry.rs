pub use euclid::{rect, size2, vec2, Point2D, Rect, Size2D, Vector2D};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnknownUnit {}
pub type Vector2F = Vector2D<f32, UnknownUnit>;
pub type PointF = Point2D<f32, UnknownUnit>;
pub type Size2F = Size2D<f32, UnknownUnit>;
pub type RectF = Rect<f32, UnknownUnit>;

pub fn vec2f(x: f32, y: f32) -> Vector2F {
    vec2(x, y)
}

pub fn size2f(w: f32, h: f32) -> Size2F {
    size2(w, h)
}

pub fn rectf(x: f32, y: f32, w: f32, h: f32) -> RectF {
    rect(x, y, w, h)
}
