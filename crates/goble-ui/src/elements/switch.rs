use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

pub struct Switch {
    state: InteractiveState,
    checked: bool,
    disabled: bool,
    size: Vector2F,
    on_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    layout_size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Switch {
    pub fn new() -> Self {
        Self {
            state: InteractiveState::default(),
            checked: false,
            disabled: false,
            size: vec2f(44.0, 24.0),
            on_change: None,
            layout_size: None,
            origin: None,
        }
    }

    pub fn with_checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_size(mut self, size: Vector2F) -> Self {
        self.size = size;
        self
    }

    pub fn with_on_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn checked(&self) -> bool {
        self.checked
    }
}

impl Default for Switch {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for Switch {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        self.layout_size = Some(self.size);
        self.size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));

        let track_rect = rectf(origin.x, origin.y, self.size.x, self.size.y);
        let radius = self.size.y * 0.5;
        let track_color = if self.disabled {
            app.theme.color(ColorToken::SurfaceRaised)
        } else if self.checked {
            app.theme.color(ColorToken::Accent)
        } else {
            app.theme.color(ColorToken::Border)
        };
        ctx.renderer
            .as_mut()
            .unwrap()
            .fill_rounded_rect(track_rect, track_color, radius);

        let padding = app.theme.spacing_px(SpacingToken::Xs);
        let thumb_size = self.size.y - padding * 2.0;
        let thumb_x = if self.checked {
            origin.x + self.size.x - thumb_size - padding
        } else {
            origin.x + padding
        };
        let thumb_y = origin.y + padding;
        let thumb_rect = rectf(thumb_x, thumb_y, thumb_size, thumb_size);
        let thumb_color = app.theme.color(ColorToken::Text);
        ctx.renderer
            .as_mut()
            .unwrap()
            .fill_rounded_rect(thumb_rect, thumb_color, thumb_size * 0.5);
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

        let change = self.on_change.clone();
        let mut toggle = || {
            self.checked = !self.checked;
            if let Some(cb) = change.as_ref() {
                (cb.borrow_mut())(self.checked);
            }
        };

        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut toggle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::DispatchedEvent;

    #[test]
    fn switch_toggles_on_click() {
        let checked = Rc::new(RefCell::new(false));
        let checked_clone = checked.clone();
        let mut switch = Switch::new().with_on_change(move |v| *checked_clone.borrow_mut() = v);

        let app = AppContext::default();
        switch.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        switch.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        };

        assert!(switch.dispatch_event(&down, &mut event_ctx, &app));
        assert!(switch.dispatch_event(&up, &mut event_ctx, &app));
        assert!(*checked.borrow());
        assert!(switch.checked());
    }
}
