use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, PointF, RectF, Size2F, Vector2F};

/// Default width of a dialog panel.
pub const DIALOG_DEFAULT_WIDTH: f32 = 480.0;

/// Dimmed backdrop color used while a dialog is open.
const BACKDROP_COLOR: ColorU = ColorU::new(0, 0, 0, 110);

/// A centered modal panel with a dimmed backdrop.
///
/// When `open` is false the dialog occupies zero space and ignores events, so
/// the underlying UI is fully interactive. When open it paints a dimmed
/// backdrop over the whole area plus the panel centered in it; clicking the
/// backdrop fires `on_close`.
pub struct Dialog {
    child: Box<dyn Element>,
    open: bool,
    width: f32,
    on_close: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    panel_origin: Vector2F,
    panel_size: Vector2F,
}

impl Dialog {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            open: false,
            width: DIALOG_DEFAULT_WIDTH,
            on_close: None,
            size: None,
            origin: None,
            panel_origin: Vector2F::zero(),
            panel_size: Vector2F::zero(),
        }
    }

    pub fn with_open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width.max(0.0);
        self
    }

    pub fn with_on_close<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_close = Some(Rc::new(RefCell::new(callback)));
        self
    }
}

impl Element for Dialog {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        if !self.open {
            self.size = Some(Vector2F::zero());
            self.panel_origin = Vector2F::zero();
            self.panel_size = Vector2F::zero();
            return Vector2F::zero();
        }
        let panel_width = self.width.min(constraint.max.x).max(0.0);
        // Force the panel to the dialog width so the form fields fill it, and
        // cap the height so a tall panel stays centered on screen.
        let max_height = (constraint.max.y * 0.9).max(0.0);
        let panel_constraint =
            SizeConstraint::new(vec2f(panel_width, 0.0), vec2f(panel_width, max_height));
        let child_size = self.child.layout(panel_constraint, ctx, app);
        self.panel_origin = vec2f(
            ((constraint.max.x - panel_width) * 0.5).max(0.0),
            ((constraint.max.y - child_size.y) * 0.5).max(0.0),
        );
        self.panel_size = vec2f(panel_width, child_size.y);
        let size = vec2f(constraint.max.x, constraint.max.y);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if !self.open {
            return;
        }
        let size = self.size.unwrap_or(Vector2F::zero());
        if let Some(renderer) = ctx.renderer.as_mut() {
            let backdrop = RectF::new(
                PointF::new(origin.x, origin.y),
                Size2F::new(size.x, size.y),
            );
            renderer.fill_rounded_rect(backdrop, BACKDROP_COLOR, 0.0);
        }
        let child_origin = vec2f(origin.x + self.panel_origin.x, origin.y + self.panel_origin.y);
        self.child.paint(child_origin, ctx, app);
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
        if !self.open {
            return false;
        }
        let origin = match self.origin {
            Some(p) => p.xy(),
            None => return false,
        };
        let full_bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };
        let panel_bounds = RectF::new(
            PointF::new(origin.x + self.panel_origin.x, origin.y + self.panel_origin.y),
            Size2F::new(self.panel_size.x, self.panel_size.y),
        );
        let cb = self.on_close.clone();

        match event {
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. } => {
                if panel_bounds.contains(PointF::new(position.x, position.y)) {
                    return self.child.dispatch_event(event, ctx, app);
                }
                if full_bounds.contains(PointF::new(position.x, position.y)) {
                    // Click on the backdrop closes the dialog.
                    if matches!(event, DispatchedEvent::MouseDown { .. }) {
                        if let Some(cb) = cb.as_ref() {
                            (cb.borrow_mut())();
                        }
                    }
                    return true;
                }
                false
            }
            _ => self.child.dispatch_event(event, ctx, app),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{
        AppContext, Container, Fill, LayoutContext, PaintContext, Text,
    };
    use crate::event::DispatchedEvent;
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;
    use crate::test_util::{command_counts, render_element};

    fn app() -> AppContext {
        AppContext::default()
    }

    #[test]
    fn closed_dialog_renders_nothing() {
        let app = app();
        let mut element = Dialog::new(Text::new("hi").finish()).finish();
        let commands = render_element(&mut element, vec2f(400.0, 400.0), &app);
        let counts = command_counts(&commands);
        assert_eq!(counts.fill_rect, 0, "closed dialog should paint no backdrop");
    }

    #[test]
    fn open_dialog_fills_backdrop_and_centers_panel() {
        let app = app();
        let backdrop_color = crate::color::ColorU::new(0, 0, 0, 110);
        let mut element = Dialog::new(
            Container::new(Text::new("hi").finish())
                .with_background(Fill::Solid(app.theme.color(crate::theme::ColorToken::Surface)))
                .finish(),
        )
        .with_open(true)
        .with_width(200.0)
        .finish();
        let commands = render_element(&mut element, vec2f(400.0, 400.0), &app);

        let has_backdrop = commands.iter().any(|c| {
            matches!(c, RenderCommand::FillRect { color, .. } if *color == backdrop_color)
        });
        assert!(has_backdrop, "open dialog should paint a dimmed backdrop");

        let counts = command_counts(&commands);
        assert_eq!(counts.fill_rect, 2, "backdrop + panel background");
        assert!(counts.draw_text > 0, "panel content should render text");
    }

    #[test]
    fn click_outside_fires_on_close() {
        let app = app();
        let closed = Rc::new(RefCell::new(false));
        let closed_clone = closed.clone();
        let mut element = Dialog::new(Text::new("hi").finish())
            .with_open(true)
            .with_width(200.0)
            .with_on_close(move || *closed_clone.borrow_mut() = true)
            .finish();

        element.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        element.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = crate::elements::EventContext::default();
        let handled = element.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(5.0, 5.0),
                button: 0,
            },
            &mut event_ctx,
            &app,
        );
        assert!(handled, "backdrop click should be consumed");
        assert!(*closed.borrow(), "backdrop click should fire on_close");
    }

    #[test]
    fn click_inside_panel_dispatches_to_child() {
        let app = app();
        let closed = Rc::new(RefCell::new(false));
        let closed_clone = closed.clone();
        let mut element = Dialog::new(Text::new("hi").finish())
            .with_open(true)
            .with_width(200.0)
            .with_on_close(move || *closed_clone.borrow_mut() = true)
            .finish();

        element.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        element.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        // The panel is 200 wide centered in a 400 viewport, so the center
        // (200, 200) is inside the panel.
        let mut event_ctx = crate::elements::EventContext::default();
        let handled = element.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(200.0, 200.0),
                button: 0,
            },
            &mut event_ctx,
            &app,
        );
        assert!(!*closed.borrow(), "click inside panel should not close");
        let _ = handled;
    }
}
