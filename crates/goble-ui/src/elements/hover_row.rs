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
pub struct HoverRow {
    content: Box<dyn Element>,
    selected: bool,
    padding: EdgeInsets,
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

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
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
        if let Some(bounds) = self.bounds() {
            let background = if self.selected {
                Some(ColorToken::Selected)
            } else if ctx.hovered(bounds) {
                Some(ColorToken::Hover)
            } else {
                None
            };
            if let Some(token) = background {
                if let Some(renderer) = ctx.renderer.as_mut() {
                    renderer.fill_rect(bounds, app.theme.color(token));
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
