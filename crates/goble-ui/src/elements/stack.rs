use crate::elements::{Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::scene::{Fill, Rect as SceneRect, RectF};

pub struct Stack {
    axis: Axis,
    spacing: f32,
    children: Vec<Box<dyn Element>>,
    background: Fill,
    padding: f32,
    size: Option<(f32, f32)>,
    justify_main: Align,
    justify_cross: Align,
}

pub enum Axis {
    Horizontal,
    Vertical,
    Depth,
}

impl Copy for Axis {}
impl Clone for Axis {
    fn clone(&self) -> Self {
        *self
    }
}

#[derive(Clone, Copy)]
pub enum Align {
    Start,
    Center,
    End,
    Stretch,
}

impl Stack {
    pub fn horizontal() -> Self {
        Self::new(Axis::Horizontal)
    }

    pub fn vertical() -> Self {
        Self::new(Axis::Vertical)
    }

    pub fn z() -> Self {
        Self::new(Axis::Depth)
    }

    fn new(axis: Axis) -> Self {
        Self {
            axis,
            spacing: 0.0,
            children: vec![],
            background: Fill::None,
            padding: 0.0,
            size: None,
            justify_main: Align::Start,
            justify_cross: Align::Stretch,
        }
    }

    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn with_padding(mut self, padding: f32) -> Self {
        self.padding = padding;
        self
    }

    pub fn with_background(mut self, fill: impl Into<Fill>) -> Self {
        self.background = fill.into();
        self
    }

    pub fn with_children(mut self, children: Vec<Box<dyn Element>>) -> Self {
        self.children = children;
        self
    }

    pub fn push(&mut self, child: Box<dyn Element>) {
        self.children.push(child);
    }
}

impl Element for Stack {
    fn layout(&mut self, constraint: SizeConstraint, ctx: &mut LayoutContext) -> (f32, f32) {
        let inner_width = (constraint.max_width - self.padding * 2.0).max(0.0);
        let inner_height = (constraint.max_height - self.padding * 2.0).max(0.0);
        let child_constraint = SizeConstraint::new(inner_width, inner_height);

        let (total_main, max_cross) = match self.axis {
            Axis::Horizontal | Axis::Vertical => {
                let mut total_main = 0.0;
                let mut max_cross: f32 = 0.0;
                let count = self.children.len();
                let gaps = if count > 0 {
                    count.saturating_sub(1) as f32 * self.spacing
                } else {
                    0.0
                };
                let available_main = match self.axis {
                    Axis::Horizontal => child_constraint.max_width - gaps,
                    Axis::Vertical => child_constraint.max_height - gaps,
                    _ => unreachable!(),
                };
                let flex_children: f32 =
                    self.children.iter().filter(|c| c.size().is_none()).count() as f32;
                let fixed_main_sum: f32 = self
                    .children
                    .iter_mut()
                    .filter_map(|c| {
                        if c.size().is_some() {
                            Some(c.layout(child_constraint, ctx))
                        } else {
                            None
                        }
                    })
                    .map(|s| match self.axis {
                        Axis::Horizontal => s.0,
                        Axis::Vertical => s.1,
                        _ => 0.0,
                    })
                    .sum();
                let flex_budget = (available_main - fixed_main_sum).max(0.0);
                let flex_share = if flex_children > 0.0 {
                    flex_budget / flex_children
                } else {
                    0.0
                };

                for child in self.children.iter_mut() {
                    if child.size().is_none() {
                        let cc = match self.axis {
                            Axis::Horizontal => SizeConstraint::new(flex_share, inner_height),
                            Axis::Vertical => SizeConstraint::new(inner_width, flex_share),
                            _ => child_constraint,
                        };
                        child.layout(cc, ctx);
                    }
                    let child_size = child.size().unwrap_or((0.0, 0.0));
                    let (main, cross) = match self.axis {
                        Axis::Horizontal => (child_size.0, child_size.1),
                        Axis::Vertical => (child_size.1, child_size.0),
                        _ => (0.0, 0.0),
                    };
                    total_main += main;
                    max_cross = max_cross.max(cross);
                }
                total_main += gaps;
                (total_main, max_cross)
            }
            Axis::Depth => {
                let mut max_w: f32 = 0.0;
                let mut max_h: f32 = 0.0;
                for child in self.children.iter_mut() {
                    let s = child.layout(child_constraint, ctx);
                    max_w = max_w.max(s.0);
                    max_h = max_h.max(s.1);
                }
                (max_w, max_h)
            }
        };

        let width = match self.axis {
            Axis::Horizontal => total_main,
            Axis::Vertical => max_cross,
            Axis::Depth => total_main,
        } + self.padding * 2.0;
        let height = match self.axis {
            Axis::Horizontal => max_cross,
            Axis::Vertical => total_main,
            Axis::Depth => max_cross,
        } + self.padding * 2.0;

        let final_width = width.min(constraint.max_width).max(constraint.min_width);
        let final_height = height.min(constraint.max_height).max(constraint.min_height);
        self.size = Some((final_width, final_height));
        (final_width, final_height)
    }

    fn paint(&mut self, origin: Point, size: (f32, f32), ctx: &mut PaintContext) {
        ctx.scene.push_rect(
            SceneRect::new(RectF::new(origin.x, origin.y, size.0, size.1))
                .with_background(self.background),
        );
        let inner_x = origin.x + self.padding;
        let inner_y = origin.y + self.padding;
        let inner_w = size.0 - self.padding * 2.0;
        let inner_h = size.1 - self.padding * 2.0;

        let total_main = match self.axis {
            Axis::Horizontal | Axis::Vertical => {
                let gaps = if self.children.len() > 1 {
                    (self.children.len() - 1) as f32 * self.spacing
                } else {
                    0.0
                };
                self.children
                    .iter()
                    .map(|c| match self.axis {
                        Axis::Horizontal => c.size().unwrap_or((0.0, 0.0)).0,
                        Axis::Vertical => c.size().unwrap_or((0.0, 0.0)).1,
                        _ => 0.0,
                    })
                    .sum::<f32>()
                    + gaps
            }
            Axis::Depth => 0.0,
        };

        let start_main = match (self.axis, self.justify_main) {
            (Axis::Horizontal | Axis::Vertical, Align::Start) => 0.0,
            (Axis::Horizontal | Axis::Vertical, Align::Center) => {
                let container_main = match self.axis {
                    Axis::Horizontal => inner_w,
                    Axis::Vertical => inner_h,
                    _ => 0.0,
                };
                (container_main - total_main) / 2.0
            }
            _ => 0.0,
        };

        let mut cursor = start_main;
        for child in self.children.iter_mut() {
            let child_size = child.size().unwrap_or((0.0, 0.0));
            let (child_w, child_h) = child_size;
            let cross_offset = match (self.axis, self.justify_cross) {
                (Axis::Horizontal, Align::Center) => (inner_h - child_h) / 2.0,
                (Axis::Horizontal, Align::End) => inner_h - child_h,
                (Axis::Vertical, Align::Center) => (inner_w - child_w) / 2.0,
                (Axis::Vertical, Align::End) => inner_w - child_w,
                _ => 0.0,
            };

            let child_origin = match self.axis {
                Axis::Horizontal => {
                    Point::new(inner_x + cursor, inner_y + cross_offset, origin.z_index + 1)
                }
                Axis::Vertical => {
                    Point::new(inner_x + cross_offset, inner_y + cursor, origin.z_index + 1)
                }
                Axis::Depth => Point::new(inner_x, inner_y, origin.z_index + 1),
            };
            child.paint(child_origin, child_size, ctx);
            cursor += match self.axis {
                Axis::Horizontal => child_w,
                Axis::Vertical => child_h,
                Axis::Depth => 0.0,
            } + self.spacing;
        }
    }

    fn dispatch_event(&mut self, event: &EventContext, origin: Point, _size: (f32, f32)) -> bool {
        for child in self.children.iter_mut().rev() {
            let child_size = child.size().unwrap_or((0.0, 0.0));
            if child.dispatch_event(event, origin, child_size) {
                return true;
            }
        }
        false
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
    use crate::elements::rect::RectElement;
    use crate::scene::{Color, Scene};
    use crate::theme::Theme;

    #[test]
    fn test_vertical_stack() {
        let mut stack = Stack::vertical()
            .with_spacing(10.0)
            .with_padding(8.0)
            .with_children(vec![
                Box::new(RectElement::new(Color::from_u8(255, 0, 0, 255)).with_size(100.0, 20.0))
                    as Box<dyn Element>,
                Box::new(RectElement::new(Color::from_u8(0, 255, 0, 255)).with_size(100.0, 30.0)),
            ]);
        let theme = Theme::dark();
        let mut ctx = LayoutContext::new(theme, 1.0);
        let size = stack.layout(SizeConstraint::new(800.0, 600.0), &mut ctx);
        assert_eq!(size, (116.0, 76.0));
    }

    #[test]
    fn test_horizontal_stack() {
        let mut stack = Stack::horizontal().with_spacing(5.0).with_children(vec![
            Box::new(RectElement::new(Color::from_u8(255, 0, 0, 255)).with_size(50.0, 20.0))
                as Box<dyn Element>,
            Box::new(RectElement::new(Color::from_u8(0, 255, 0, 255)).with_size(50.0, 20.0)),
        ]);
        let theme = Theme::dark();
        let mut ctx = LayoutContext::new(theme, 1.0);
        let size = stack.layout(SizeConstraint::new(800.0, 600.0), &mut ctx);
        assert_eq!(size, (105.0, 20.0));
    }
}
