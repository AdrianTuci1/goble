use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

pub struct Checkbox {
    label: Option<Box<dyn Element>>,
    state: InteractiveState,
    checked: bool,
    disabled: bool,
    box_size: Vector2F,
    on_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Checkbox {
    pub fn new() -> Self {
        Self {
            label: None,
            state: InteractiveState::default(),
            checked: false,
            disabled: false,
            box_size: vec2f(20.0, 20.0),
            on_change: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_label(mut self, label: Box<dyn Element>) -> Self {
        self.label = Some(label);
        self
    }

    pub fn with_checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_box_size(mut self, size: Vector2F) -> Self {
        self.box_size = size;
        self
    }

    pub fn with_on_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn checked(&self) -> bool {
        self.checked
    }

    fn gap(&self, app: &AppContext) -> f32 {
        app.theme.spacing_px(SpacingToken::Sm)
    }
}

impl Default for Checkbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for Checkbox {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let gap = self.gap(app);
        let label_size = match self.label.as_mut() {
            Some(label) => {
                let available = vec2f(
                    (constraint.max.x - self.box_size.x - gap).max(0.0),
                    constraint.max.y,
                );
                label.layout(SizeConstraint::new(constraint.min, available), ctx, app)
            }
            None => Vector2F::zero(),
        };

        let width = self.box_size.x + gap + label_size.x;
        let height = self.box_size.y.max(label_size.y);
        let size = vec2f(width.min(constraint.max.x), height.min(constraint.max.y));
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));

        let box_rect = rectf(origin.x, origin.y, self.box_size.x, self.box_size.y);
        let radius = app.theme.radius_px() * 0.5;
        let box_color = if self.checked {
            app.theme.color(ColorToken::Accent)
        } else {
            app.theme.color(ColorToken::SurfaceRaised)
        };
        ctx.renderer
            .as_mut()
            .unwrap()
            .fill_rounded_rect(box_rect, box_color, radius);
        ctx.renderer.as_mut().unwrap().stroke_rect(
            box_rect,
            app.theme.color(ColorToken::Border),
            1.0,
            radius,
        );

        if self.checked {
            let check_size = self.box_size.x.min(self.box_size.y) * 0.5;
            let check_x = origin.x + (self.box_size.x - check_size) * 0.5;
            let check_y = origin.y + (self.box_size.y - check_size) * 0.5;
            ctx.renderer.as_mut().unwrap().draw_text(
                vec2f(check_x, check_y),
                "✓",
                check_size,
                app.theme.color(ColorToken::Text),
                self.box_size.x,
                1.2,
            );
        }

        let gap = self.gap(app);
        if let Some(label) = self.label.as_mut() {
            let label_size = label.size().unwrap_or(Vector2F::zero());
            let y_offset = (self.box_size.y.max(label_size.y) - label_size.y).max(0.0) / 2.0;
            label.paint(
                vec2f(origin.x + self.box_size.x + gap, origin.y + y_offset),
                ctx,
                app,
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
    use crate::elements::Empty;
    use crate::event::DispatchedEvent;

    #[test]
    fn checkbox_toggles_on_click() {
        let checked = Rc::new(RefCell::new(false));
        let checked_clone = checked.clone();
        let mut checkbox = Checkbox::new()
            .with_label(Empty::new().with_size(vec2f(80.0, 20.0)).finish())
            .with_on_change(move |v| *checked_clone.borrow_mut() = v);

        let app = AppContext::default();
        checkbox.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        checkbox.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        };

        assert!(checkbox.dispatch_event(&down, &mut event_ctx, &app));
        assert!(checkbox.dispatch_event(&up, &mut event_ctx, &app));
        assert!(*checked.borrow());
        assert!(checkbox.checked());
    }
}
