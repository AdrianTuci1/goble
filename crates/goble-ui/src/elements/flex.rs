use crate::elements::{
    AppContext, Axis, AxisOrientation, CrossAxisAlignment, Element, LayoutContext,
    MainAxisAlignment, MainAxisSize, PaintContext, Point, SizeConstraint, Vector2FExt,
};
use crate::geometry::{vec2f, Vector2F};

pub struct Flex {
    axis: Axis,
    orientation: AxisOrientation,
    children: Vec<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    main_axis_size: MainAxisSize,
    main_axis_alignment: MainAxisAlignment,
    cross_axis_alignment: CrossAxisAlignment,
    spacing: f32,
}

impl Flex {
    pub fn new(axis: Axis) -> Self {
        Self {
            axis,
            orientation: AxisOrientation::Normal,
            children: Vec::new(),
            size: None,
            origin: None,
            main_axis_size: MainAxisSize::Min,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Start,
            spacing: 0.0,
        }
    }

    pub fn row() -> Self {
        Self::new(Axis::Horizontal)
    }

    pub fn column() -> Self {
        Self::new(Axis::Vertical)
    }

    pub fn with_reverse_orientation(mut self) -> Self {
        self.orientation = AxisOrientation::Reverse;
        self
    }

    pub fn with_main_axis_size(mut self, size: MainAxisSize) -> Self {
        self.main_axis_size = size;
        self
    }

    pub fn with_main_axis_alignment(mut self, alignment: MainAxisAlignment) -> Self {
        self.main_axis_alignment = alignment;
        self
    }

    pub fn with_cross_axis_alignment(mut self, alignment: CrossAxisAlignment) -> Self {
        self.cross_axis_alignment = alignment;
        self
    }

    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn with_child(mut self, child: Box<dyn Element>) -> Self {
        self.children.push(child);
        self
    }

    pub fn with_children(mut self, children: impl IntoIterator<Item = Box<dyn Element>>) -> Self {
        self.children.extend(children);
        self
    }

    fn child_constraint(&self, constraint: SizeConstraint) -> SizeConstraint {
        let cross = self.axis.invert();
        let main_max = match self.main_axis_size {
            MainAxisSize::Max => constraint.max.along(self.axis),
            MainAxisSize::Min => f32::INFINITY,
        };
        let cross_max = if self.cross_axis_alignment == CrossAxisAlignment::Stretch {
            constraint.max.along(cross)
        } else {
            f32::INFINITY
        };
        let min = vec2f(0.0, 0.0);
        let max = self.axis.to_point(main_max, cross_max);
        SizeConstraint::new(min, max)
    }
}

impl Extend<Box<dyn Element>> for Flex {
    fn extend<T: IntoIterator<Item = Box<dyn Element>>>(&mut self, iter: T) {
        self.children.extend(iter);
    }
}

impl Element for Flex {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let cross = self.axis.invert();
        let child_constraint = self.child_constraint(constraint);

        let mut total_main = 0.0_f32;
        let mut cross_max = 0.0_f32;
        for child in &mut self.children {
            let child_size = child.layout(child_constraint, ctx, app);
            total_main += child_size.along(self.axis);
            cross_max = cross_max.max(child_size.along(cross));
        }

        let total_spacing = self.spacing * (self.children.len().saturating_sub(1)) as f32;
        let main_size = if self.main_axis_size == MainAxisSize::Max {
            constraint.max.along(self.axis)
        } else {
            total_main + total_spacing
        };
        let cross_size = if self.cross_axis_alignment == CrossAxisAlignment::Stretch {
            constraint.max.along(cross)
        } else {
            cross_max
        };

        let size = self.axis.to_point(main_size, cross_size);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if self.children.is_empty() {
            return;
        }

        let cross = self.axis.invert();
        let total_main: f32 = self
            .children
            .iter()
            .map(|c| c.size().map(|s| s.along(self.axis)).unwrap_or(0.0))
            .sum::<f32>()
            + self.spacing * (self.children.len() - 1) as f32;
        let available_main = self.size.unwrap_or(Vector2F::zero()).along(self.axis);
        let extra = (available_main - total_main).max(0.0);

        let (between, leading) = match self.main_axis_alignment {
            MainAxisAlignment::Start => (0.0, 0.0),
            MainAxisAlignment::Center => (0.0, extra / 2.0),
            MainAxisAlignment::End => (0.0, extra),
            MainAxisAlignment::SpaceBetween => {
                if self.children.len() <= 1 {
                    (0.0, 0.0)
                } else {
                    (extra / (self.children.len() - 1) as f32, 0.0)
                }
            }
            MainAxisAlignment::SpaceEvenly => {
                let n = (self.children.len() + 1) as f32;
                (extra / n, extra / n)
            }
        };

        let mut cursor = leading;
        for child in &mut self.children {
            let child_size = child.size().unwrap_or(Vector2F::zero());
            let cross_pos = match self.cross_axis_alignment {
                CrossAxisAlignment::Start => 0.0,
                CrossAxisAlignment::Center => {
                    let cross_size = self.size.unwrap_or(Vector2F::zero()).along(cross);
                    (cross_size - child_size.along(cross)) / 2.0
                }
                CrossAxisAlignment::End => {
                    let cross_size = self.size.unwrap_or(Vector2F::zero()).along(cross);
                    cross_size - child_size.along(cross)
                }
                CrossAxisAlignment::Stretch => 0.0,
            };

            let main_pos = cursor;
            let offset = self.axis.to_point(main_pos, cross_pos);
            child.paint(origin + offset, ctx, app);
            cursor += child_size.along(self.axis) + self.spacing + between;
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}
