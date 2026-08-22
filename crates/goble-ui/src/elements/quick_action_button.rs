use crate::elements::{
    AppContext, Button, ButtonVariant, Element, EventContext, LayoutContext, PaintContext, Point,
    SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::ColorToken;

/// A small, low-emphasis button for suggested follow-up actions in chat.
pub struct QuickActionButton {
    root: Button,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl QuickActionButton {
    pub fn new(label: impl Into<String>, on_click: impl FnMut() + 'static) -> Self {
        let label = Text::new(label.into())
            .with_theme_color(ColorToken::Muted, &AppContext::default())
            .finish();
        let root = Button::new(label)
            .with_variant(ButtonVariant::Ghost)
            .with_on_click(on_click);
        Self {
            root,
            size: None,
            origin: None,
        }
    }
}

impl Element for QuickActionButton {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.root.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.paint(origin, ctx, app);
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
        self.root.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::geometry::vec2f;

    #[test]
    fn quick_action_click_fires_callback() {
        let app = AppContext::default();
        let clicked = Rc::new(RefCell::new(false));
        let clicked_clone = clicked.clone();
        let mut button =
            QuickActionButton::new("Explain", move || *clicked_clone.borrow_mut() = true);

        button.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        button.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        };

        assert!(button.dispatch_event(&down, &mut event_ctx, &app));
        assert!(button.dispatch_event(&up, &mut event_ctx, &app));
        assert!(*clicked.borrow());
    }
}
