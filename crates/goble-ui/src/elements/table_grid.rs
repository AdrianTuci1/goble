use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Border, Container, EdgeInsets, Element, EventContext, LayoutContext, PaintContext,
    Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::theme::ColorToken;

/// Hairline of a table's rules, the same weight as
/// [`Divider`](crate::elements::Divider) so a grid reads as the transcript's
/// own rule rather than a widget.
const RULE: f32 = 1.0;

/// One weight per table column, from the longest cell in each.
///
/// The whole table is weighed at once: a column whose cells are longer gets a
/// wider share, and every row is laid out with the same shares, so a cell
/// stands over its header instead of each row splitting its own width. The
/// square root keeps one long cell (a path, a URL) from squeezing the other
/// columns to a sliver.
pub(crate) fn column_weights(header: &[String], rows: &[Vec<String>]) -> Vec<f32> {
    let columns = header.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
    (0..columns)
        .map(|index| {
            let longest = std::iter::once(header.get(index))
                .chain(rows.iter().map(|row| row.get(index)))
                .flatten()
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0);
            (longest as f32).sqrt().max(1.0)
        })
        .collect()
}

/// Draws a table as a full grid: an outline around the whole table, a rule
/// between every pair of columns, and a rule between every pair of rows.
///
/// A cell keeps the room [`TableGrid::new`] gives it, so its text never touches
/// a rule; the rules are painted from the cells' own bounds, so a row whose
/// cell wraps to several lines gets a box of its real height.
pub(crate) struct TableGrid {
    padding: EdgeInsets,
}

impl TableGrid {
    /// `gap` is the room between neighbouring cells' text. A rule stands in
    /// the middle, so a cell keeps half of it on each side and the table's edge
    /// is as tight as its middle. A row is tighter than a column because a line
    /// of text already carries its own leading.
    pub(crate) fn new(gap: f32) -> Self {
        Self {
            padding: EdgeInsets::new(gap / 2.0, gap / 4.0, gap / 2.0, gap / 4.0),
        }
    }

    /// A cell holding `weight` of its row's width.
    pub(crate) fn cell(&self, content: Box<dyn Element>, weight: f32) -> TableCell {
        TableCell {
            child: Container::new(content).with_padding(self.padding).finish(),
            weight,
            edges: Rc::new(RefCell::new(Vec::new())),
            size: None,
            origin: None,
        }
    }

    /// One row of the grid. `bottom` is false for the last row, whose bottom
    /// edge is the outline.
    pub(crate) fn row(&self, mut cells: Vec<TableCell>, bottom: bool) -> Box<dyn Element> {
        let edges = Rc::new(RefCell::new(Vec::new()));
        for cell in &mut cells {
            cell.edges = Rc::clone(&edges);
        }
        Box::new(TableRow {
            cells,
            edges,
            bottom,
            size: None,
            origin: None,
        })
    }

    /// The table's outline, drawn around the column of rows.
    pub(crate) fn outline(&self, content: Box<dyn Element>, app: &AppContext) -> Box<dyn Element> {
        let table = TableGridBody {
            child: content,
            size: None,
            origin: None,
        };
        Container::new(table.finish())
            .with_border(Border::all(RULE).with_border_color(app.theme.color(ColorToken::Border)))
            .finish()
    }
}

/// A table cell: the row's width slice, recorded so the row can stand its
/// vertical rules on the column edges.
pub(crate) struct TableCell {
    child: Box<dyn Element>,
    weight: f32,
    /// The x of the cell's right edge, shared with the cell's row. Filled in
    /// paint order, which is left to right.
    edges: Rc<RefCell<Vec<f32>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Element for TableCell {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let mut size = self.child.layout(constraint, ctx, app);
        // The row hands this cell an exact slice of its width. The slice is the
        // cell's even when its text stops short, so the rule that closes it
        // stands on the column edge in every row.
        if constraint.max.x.is_finite() && constraint.min.x == constraint.max.x {
            size.x = constraint.max.x;
        }
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.child.paint(origin, ctx, app);
        if let Some(size) = self.size {
            self.edges.borrow_mut().push(origin.x + size.x);
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.child.dispatch_event(event, ctx, app)
    }
}

/// A table row: shares the row's width between its cells by the grid's weights
/// and paints the rules the row owns — one between each pair of its cells, and
/// one under it unless the outline closes the table there.
struct TableRow {
    cells: Vec<TableCell>,
    edges: Rc<RefCell<Vec<f32>>>,
    bottom: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Element for TableRow {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let mut size = Vector2F::zero();
        if self.cells.is_empty() {
            self.size = Some(size);
            return size;
        }
        if constraint.max.x.is_finite() {
            let total: f32 = self.cells.iter().map(|cell| cell.weight).sum();
            for cell in &mut self.cells {
                let width = constraint.max.x * cell.weight / total;
                let cell_size = cell.layout(
                    SizeConstraint::new(vec2f(width, 0.0), vec2f(width, constraint.max.y)),
                    ctx,
                    app,
                );
                size.x += cell_size.x;
                size.y = size.y.max(cell_size.y);
            }
        } else {
            // No width to share: measure each cell at its intrinsic width. The
            // grid lays the rows out again once it knows the table's width.
            for cell in &mut self.cells {
                let cell_size = cell.layout(
                    SizeConstraint::new(Vector2F::zero(), vec2f(f32::INFINITY, constraint.max.y)),
                    ctx,
                    app,
                );
                size.x += cell_size.x;
                size.y = size.y.max(cell_size.y);
            }
        }
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        self.edges.borrow_mut().clear();
        let mut x = origin.x;
        for cell in &mut self.cells {
            let cell_size = cell.size.unwrap_or_default();
            cell.paint(vec2f(x, origin.y), ctx, app);
            x += cell_size.x;
        }

        let color = app.theme.color(ColorToken::Border);
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let right = origin.x + size.x;
        for edge in self.edges.borrow().iter().copied() {
            // The rightmost edge is the table's outline, already drawn.
            if edge < right - 0.5 {
                renderer.fill_rect(rectf(edge, origin.y, RULE, size.y), color);
            }
        }
        if self.bottom && size.y >= RULE {
            renderer.fill_rect(
                rectf(origin.x, origin.y + size.y - RULE, size.x, RULE),
                color,
            );
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        for cell in &mut self.cells {
            if cell.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        false
    }
}

/// The column of rows inside the outline.
struct TableGridBody {
    child: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Element for TableGridBody {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let mut size = self.child.layout(constraint, ctx, app);
        // A table whose parent is unbounded (a thread row sized to its content)
        // has no width to split between its columns, so each row would fall
        // back to its own intrinsic width and the columns would not line up.
        // The widest row is the table's width: lay the rows out again at it so
        // every row shares the same columns.
        if !constraint.max.x.is_finite() && size.x > 0.0 {
            size = self.child.layout(
                SizeConstraint::new(vec2f(size.x, 0.0), vec2f(size.x, constraint.max.y)),
                ctx,
                app,
            );
        }
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.child.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.child.dispatch_event(event, ctx, app)
    }
}
