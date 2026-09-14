use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, EdgeInsets, Element, EventContext, LayoutContext, PaintContext, Point,
    SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};
use crate::theme::ColorToken;

/// A row of a list or a tree: it highlights under the pointer, marks itself
/// selected, and reports a click.
///
/// The highlight is read from the frame's cursor while painting, so a row of a
/// tree the app rebuilds every frame needs no host-owned hover flag — any number
/// of rows highlight, however the tree changes between frames. Use
/// [`HoverButton`](crate::elements::HoverButton) instead when the *layout*
/// depends on hover (a control that exists only while hovered); this row is for
/// surfaces where only the paint does.
///
/// A row can also *report* its hover, through [`Self::with_hover_key`], for the
/// hosts whose row's own children are coloured by it: those are built before the
/// paint that decides hover, so they need the app to remember it between frames.
pub struct HoverRow {
    content: Box<dyn Element>,
    selected: bool,
    padding: EdgeInsets,
    corner_radius: f32,
    /// The app's own cell the row names itself in while the pointer is over it,
    /// and the key it writes there. See [`Self::with_hover_key`].
    hover_key: Option<(Rc<RefCell<Option<String>>>, String)>,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    state: InteractiveState,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl HoverRow {
    pub fn new(content: Box<dyn Element>) -> Self {
        Self {
            content,
            selected: false,
            padding: EdgeInsets::uniform(0.0),
            corner_radius: 0.0,
            hover_key: None,
            on_click: None,
            state: InteractiveState::default(),
            size: None,
            origin: None,
        }
    }

    /// Draw the selected background instead of the hover one.
    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    /// Round the highlight's corners. The default band is square, which is what
    /// a row of a list inside a flat surface draws; the project explorer passes
    /// the reference tool's 4pt so the highlight reads as the row's own shape
    /// rather than a slice of the panel.
    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Report the row under the pointer into the app's own cell, named `key`.
    ///
    /// For a host whose row is styled by hover (its chevron, icon and label in
    /// the row's own colour): the tree is rebuilt every frame, so those children
    /// are built in `layout` — before the `paint` that is where hover is known.
    /// The app passes one shared cell and a key per row, reads it while building
    /// the next frame's rows, and so styles the row the pointer was over. Pass
    /// the app's own cell, not an element-owned one, or the frame-to-frame
    /// memory is lost with the element.
    ///
    /// A row the pointer has left clears its own key, so the cell never keeps a
    /// row highlighted after the pointer has gone. The styling lags hover by one
    /// frame, which a pointer move covers: the move already asks for the frame
    /// that applies it.
    pub fn with_hover_key(
        mut self,
        cell: Rc<RefCell<Option<String>>>,
        key: impl Into<String>,
    ) -> Self {
        self.hover_key = Some((cell, key.into()));
        self
    }
}

impl Element for HoverRow {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let horizontal = self.padding.left + self.padding.right;
        let vertical = self.padding.top + self.padding.bottom;
        let inner = SizeConstraint::new(
            vec2f(
                (constraint.min.x - horizontal).max(0.0),
                (constraint.min.y - vertical).max(0.0),
            ),
            vec2f(
                (constraint.max.x - horizontal).max(0.0),
                (constraint.max.y - vertical).max(0.0),
            ),
        );
        let content = self.content.layout(inner, ctx, app);
        let size = vec2f(content.x + horizontal, content.y + vertical);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let bounds = self.bounds();
        let hovered = bounds.is_some_and(|bounds| ctx.hovered(bounds));
        if let Some((cell, key)) = &self.hover_key {
            let mut cell = cell.borrow_mut();
            if hovered {
                *cell = Some(key.clone());
            } else if cell.as_deref() == Some(key.as_str()) {
                *cell = None;
            }
        }
        if let Some(bounds) = bounds {
            let background = if self.selected {
                Some(ColorToken::Selected)
            } else if hovered {
                Some(ColorToken::Hover)
            } else {
                None
            };
            if let Some(token) = background {
                if let Some(renderer) = ctx.renderer.as_mut() {
                    let color = app.theme.color(token);
                    if self.corner_radius > 0.0 {
                        renderer.fill_rounded_rect(bounds, color, self.corner_radius);
                    } else {
                        renderer.fill_rect(bounds, color);
                    }
                }
            }
        }
        self.content.paint(
            vec2f(origin.x + self.padding.left, origin.y + self.padding.top),
            ctx,
            app,
        );
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
        _app: &AppContext,
    ) -> bool {
        let bounds = match self.bounds() {
            Some(bounds) => bounds,
            None => return false,
        };
        let cb = self.on_click.clone();
        let mut on_click = move || {
            if let Some(cb) = cb.as_ref() {
                (cb.borrow_mut())();
            }
        };
        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut on_click)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::Text;
    use crate::geometry::{rectf, RectF, vec2f};
    use crate::render::{RenderCommand, Renderer};

    fn row(child: Box<dyn Element>) -> HoverRow {
        HoverRow::new(child).with_padding(EdgeInsets::new(4.0, 4.0, 4.0, 4.0))
    }

    /// Lay the row out at the pointer's origin and paint it, returning the band
    /// it occupies.
    fn paint(row: &mut HoverRow, ctx: &mut PaintContext, app: &AppContext) -> RectF {
        let size = row.layout(
            SizeConstraint::loose(vec2f(200.0, 40.0)),
            &mut LayoutContext::default(),
            app,
        );
        row.paint(vec2f(0.0, 0.0), ctx, app);
        rectf(0.0, 0.0, size.x, size.y)
    }

    /// The highlight is decided while painting: the row draws a background band
    /// only while the pointer is over it, which is what lets a row of a
    /// per-frame-rebuilt tree highlight without any host-owned hover flag.
    #[test]
    fn the_hover_highlight_is_read_from_the_cursor_while_painting() {
        let app = AppContext::default();
        let bands = |ctx: &PaintContext| -> Vec<RectF> {
            ctx.renderer
                .as_ref()
                .expect("renderer")
                .commands()
                .iter()
                .filter_map(|command| match command {
                    RenderCommand::FillRect { rect, .. } => Some(*rect),
                    _ => None,
                })
                .collect()
        };

        let mut away = PaintContext::new(Renderer::new());
        away.cursor_inside = true;
        away.cursor_position = vec2f(0.0, 400.0);
        let band = paint(&mut row(Text::new("src").finish()), &mut away, &app);
        assert!(
            bands(&away).is_empty(),
            "a row the pointer is away from draws no highlight, band {band:?}"
        );

        let mut over = PaintContext::new(Renderer::new());
        over.cursor_inside = true;
        over.cursor_position = vec2f(band.max_x() / 2.0, band.max_y() / 2.0);
        let band = paint(&mut row(Text::new("src").finish()), &mut over, &app);
        assert!(
            bands(&over).contains(&band),
            "the hovered row draws its own band {band:?}: {:?}",
            bands(&over)
        );
    }

    /// A rounded row paints its highlight with the given corner radius, which is
    /// what the project explorer's tree asks for (the reference tree's 4pt): the
    /// band reads as the row's own shape rather than a slice of the panel. A row
    /// that asks for nothing keeps the square band every other list draws.
    #[test]
    fn a_row_with_a_corner_radius_paints_a_rounded_highlight() {
        let app = AppContext::default();
        let radius_of = |ctx: &PaintContext| -> Option<f32> {
            ctx.renderer
                .as_ref()
                .expect("renderer")
                .commands()
                .iter()
                .find_map(|command| match command {
                    RenderCommand::FillRect { corner_radius, .. } => Some(*corner_radius),
                    _ => None,
                })
        };
        let hovered = |row: &mut HoverRow, app: &AppContext| -> Option<f32> {
            let mut ctx = PaintContext::new(Renderer::new());
            ctx.cursor_inside = true;
            ctx.cursor_position = vec2f(10.0, 10.0);
            let _ = paint(row, &mut ctx, app);
            radius_of(&ctx)
        };

        assert_eq!(
            hovered(&mut row(Text::new("src").finish()), &app),
            Some(0.0),
            "a plain row's band is square"
        );
        assert_eq!(
            hovered(
                &mut row(Text::new("src").finish()).with_corner_radius(4.0),
                &app
            ),
            Some(4.0),
            "the explorer's row rounds its band"
        );
    }

    /// A row that is given the app's cell names itself there while the pointer
    /// is over it — and clears its own key when the pointer leaves, so the host
    /// that colours a row from that cell cannot keep one highlighted after the
    /// pointer has gone. The band it paints meanwhile is the rounded one.
    #[test]
    fn a_row_reports_its_hover_through_the_app_cell() {
        let app = AppContext::default();
        let cell = Rc::new(RefCell::new(None));
        let mut row = row(Text::new("src").finish())
            .with_corner_radius(4.0)
            .with_hover_key(Rc::clone(&cell), "src");

        let away = |row: &mut HoverRow, cell: &Rc<RefCell<Option<String>>>| {
            let mut ctx = PaintContext::new(Renderer::new());
            ctx.cursor_inside = true;
            ctx.cursor_position = vec2f(0.0, 400.0);
            let _ = paint(row, &mut ctx, &app);
            assert_eq!(
                *cell.borrow(),
                None,
                "a row the pointer is away from names nothing"
            );
        };
        away(&mut row, &cell);

        let mut over = PaintContext::new(Renderer::new());
        over.cursor_inside = true;
        over.cursor_position = vec2f(10.0, 10.0);
        let _ = paint(&mut row, &mut over, &app);
        assert_eq!(
            cell.borrow().as_deref(),
            Some("src"),
            "the row under the pointer names itself in the app's cell"
        );
        let band = over
            .renderer
            .as_ref()
            .expect("renderer")
            .commands()
            .iter()
            .find_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } if *color == app.theme.color(ColorToken::Hover) => Some((*rect, *corner_radius)),
                _ => None,
            });
        assert_eq!(
            band.map(|(_, radius)| radius),
            Some(4.0),
            "the row that reports its hover paints the rounded band"
        );

        away(&mut row, &cell);
    }

    /// A click anywhere in the row — the indent, the chevron, the label — runs
    /// the row's action, not just a click on the label.
    #[test]
    fn a_click_anywhere_in_the_row_runs_its_action() {
        let app = AppContext::default();
        let clicked = Rc::new(RefCell::new(false));
        let flag = Rc::clone(&clicked);
        let mut row = row(Text::new("src").finish())
            .with_on_click(move || *flag.borrow_mut() = true);
        let band = paint(&mut row, &mut PaintContext::default(), &app);

        let mut ctx = EventContext::default();
        let click = vec2f(band.max_x() - 1.0, band.max_y() - 1.0);
        row.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: click,
                button: 0,
            },
            &mut ctx,
            &app,
        );
        row.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: click,
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert!(*clicked.borrow());
    }
}
