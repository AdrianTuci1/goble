//! The grid geometry, and the probe that records where the grid landed.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint, TerminalGrid,
};
use goble_ui::geometry::Vector2F;

use super::{FONT_SIZE, LINE_HEIGHT};

/// The painted grid's box, in window coordinates.
#[derive(Debug, Clone, Copy)]
pub(super) struct GridGeometry {
    pub(super) origin: Vector2F,
    pub(super) columns: usize,
    pub(super) rows: usize,
}

impl GridGeometry {
    /// The cell a point falls in, or `None` when it is outside the grid.
    pub(super) fn cell_at(&self, position: Vector2F) -> Option<(usize, usize)> {
        let column_pitch = TerminalGrid::cell_width(FONT_SIZE);
        let row_pitch = TerminalGrid::row_pitch(FONT_SIZE, LINE_HEIGHT);
        let x = position.x - self.origin.x;
        let y = position.y - self.origin.y;
        if x < 0.0 || y < 0.0 || column_pitch <= 0.0 || row_pitch <= 0.0 {
            return None;
        }
        let column = (x / column_pitch) as usize;
        let row = (y / row_pitch) as usize;
        (column < self.columns && row < self.rows).then_some((row, column))
    }
}

/// Paints a [`TerminalGrid`] and records where it landed.
pub(super) struct GridProbe {
    pub(super) inner: TerminalGrid,
    pub(super) geometry: Rc<RefCell<Option<GridGeometry>>>,
}

impl Element for GridProbe {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.inner.layout(constraint, ctx, app)
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        *self.geometry.borrow_mut() = Some(GridGeometry {
            origin,
            columns: self.inner.columns(),
            rows: self.inner.rows(),
        });
        self.inner.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.inner.size()
    }

    fn origin(&self) -> Option<Point> {
        self.inner.origin()
    }
}

