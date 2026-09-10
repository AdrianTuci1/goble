use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};

pub struct IconButton {
    child: Box<dyn Element>,
    state: InteractiveState,
    disabled: bool,
    size: Vector2F,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    layout_size: Option<Vector2F>,
    origin: Option<Point>,
}

impl IconButton {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            state: InteractiveState::default(),
            disabled: false,
            size: vec2f(40.0, 40.0),
            on_click: None,
            layout_size: None,
            origin: None,
        }
    }

    pub fn with_size(mut self, size: Vector2F) -> Self {
        self.size = size;
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }
}

impl Element for IconButton {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let _child_size = self
            .child
            .layout(SizeConstraint::tight(self.size), ctx, app);
        self.layout_size = Some(self.size);
        self.size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let child_size = self.child.size().unwrap_or(Vector2F::zero());
        let offset = vec2f(
            (self.size.x - child_size.x).max(0.0) / 2.0,
            (self.size.y - child_size.y).max(0.0) / 2.0,
        );
        self.child.paint(origin + offset, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.layout_size
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
        if self.disabled {
            return false;
        }
        let bounds = match self.bounds() {
            Some(b) => b,
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
    use crate::elements::Empty;
    use crate::event::DispatchedEvent;

    #[test]
    fn icon_button_click_fires_callback() {
        let clicked = Rc::new(RefCell::new(false));
        let clicked_clone = clicked.clone();
        let mut button = IconButton::new(Empty::new().finish())
            .with_on_click(move || *clicked_clone.borrow_mut() = true);

        let app = AppContext::default();
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
