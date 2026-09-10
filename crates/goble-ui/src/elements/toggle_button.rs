use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};
use crate::theme::SpacingToken;

pub struct ToggleButton {
    child: Box<dyn Element>,
    state: InteractiveState,
    active: bool,
    disabled: bool,
    on_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ToggleButton {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            state: InteractiveState::default(),
            active: false,
            disabled: false,
            on_change: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_on_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn active(&self) -> bool {
        self.active
    }

    fn horizontal_padding(&self, app: &AppContext) -> f32 {
        app.theme.spacing_px(SpacingToken::Md)
    }

    fn vertical_padding(&self, app: &AppContext) -> f32 {
        app.theme.spacing_px(SpacingToken::Sm)
    }
}

impl Element for ToggleButton {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let h_pad = self.horizontal_padding(app);
        let v_pad = self.vertical_padding(app);
        let inner_max = vec2f(
            (constraint.max.x - h_pad * 2.0).max(0.0),
            (constraint.max.y - v_pad * 2.0).max(0.0),
        );
        let inner_min = vec2f(
            (constraint.min.x - h_pad * 2.0).max(0.0),
            (constraint.min.y - v_pad * 2.0).max(0.0),
        );
        let child_size = self
            .child
            .layout(SizeConstraint::new(inner_min, inner_max), ctx, app);
        let size = vec2f(child_size.x + h_pad * 2.0, child_size.y + v_pad * 2.0);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let child_size = self.child.size().unwrap_or(Vector2F::zero());
        let h_pad = self.horizontal_padding(app);
        let v_pad = self.vertical_padding(app);
        let child_origin = vec2f(
            origin.x + h_pad + (size.x - child_size.x - h_pad * 2.0).max(0.0) / 2.0,
            origin.y + v_pad + (size.y - child_size.y - v_pad * 2.0).max(0.0) / 2.0,
        );
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
        _app: &AppContext,
    ) -> bool {
        if self.disabled {
            return false;
        }
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };

        let change = self.on_change.clone();
        let mut toggle = || {
            self.active = !self.active;
            if let Some(cb) = change.as_ref() {
                (cb.borrow_mut())(self.active);
            }
        };

        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut toggle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::Empty;
    use crate::event::DispatchedEvent;

    #[test]
    fn toggle_button_toggles_on_click() {
        let active = Rc::new(RefCell::new(false));
        let active_clone = active.clone();
        let mut button = ToggleButton::new(Empty::new().with_size(vec2f(80.0, 32.0)).finish())
            .with_on_change(move |v| *active_clone.borrow_mut() = v);

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
        assert!(*active.borrow());
        assert!(button.active());
    }
}
