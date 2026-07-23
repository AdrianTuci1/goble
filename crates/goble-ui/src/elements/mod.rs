pub mod rect;
pub mod stack;
pub mod text;

use crate::scene::{Point, Scene};
use crate::theme::Theme;

pub struct LayoutContext {
    pub theme: Theme,
    pub scale_factor: f32,
}

impl LayoutContext {
    pub fn new(theme: Theme, scale_factor: f32) -> Self {
        Self {
            theme,
            scale_factor,
        }
    }
}

pub struct PaintContext<'a> {
    pub scene: &'a mut Scene,
    pub theme: &'a Theme,
}

pub struct EventContext {
    pub mouse_position: Option<Point>,
    pub clicked: bool,
}

#[derive(Clone, Copy)]
pub struct SizeConstraint {
    pub max_width: f32,
    pub max_height: f32,
    pub min_width: f32,
    pub min_height: f32,
}

impl SizeConstraint {
    pub fn new(max_width: f32, max_height: f32) -> Self {
        Self {
            max_width,
            max_height,
            min_width: 0.0,
            min_height: 0.0,
        }
    }
}

pub trait Element {
    fn layout(&mut self, constraint: SizeConstraint, ctx: &mut LayoutContext) -> (f32, f32);
    fn paint(&mut self, origin: Point, size: (f32, f32), ctx: &mut PaintContext);
    fn dispatch_event(&mut self, _event: &EventContext, _origin: Point, _size: (f32, f32)) -> bool {
        false
    }
    fn size(&self) -> Option<(f32, f32)>;
    fn set_size(&mut self, size: (f32, f32));
}

pub type BoxedElement = Box<dyn Element>;
