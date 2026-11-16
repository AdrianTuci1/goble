use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::focus::{centred_origin, hit_target, paint_focus_ring, TargetClass};
use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::theme::{FocusStyle, SpacingToken};

pub struct Chip {
    child: Box<dyn Element>,
    state: InteractiveState,
    disabled: bool,
    selected: bool,
    /// Whether the chip adds the padding that makes it a stand-alone control.
    /// A chip that is only a hit target around a row of text turns this off, so
    /// the row stays where the text around it starts.
    padded: bool,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    /// Whether the chip owns the keyboard focus and paints the ring.
    focused: bool,
    /// The minimum-target rule the chip follows.
    target_class: TargetClass,
    /// Whether the pointer is coarse, which grows the hit box around the paint.
    coarse_pointer: bool,
    size: Option<Vector2F>,
    /// The size the chip paints at inside its hit target.
    paint_size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Chip {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            state: InteractiveState::default(),
            disabled: false,
            selected: false,
            padded: true,
            on_click: None,
            focused: false,
            target_class: TargetClass::Icon,
            coarse_pointer: false,
            size: None,
            paint_size: None,
            origin: None,
        }
    }

    /// A chip with no padding: a click target around a row that is part of the
    /// surrounding text, not a control standing on its own.
    pub fn flat(child: Box<dyn Element>) -> Self {
        Self {
            padded: false,
            ..Self::new(child)
        }
    }

    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = selected;
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

    /// Paint the focus ring while the keyboard is on the chip.
    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Use a minimum-target rule other than the icon-button default.
    pub fn with_target_class(mut self, class: TargetClass) -> Self {
        self.target_class = class;
        self
    }

    /// Grow the hit box to 44×44 under a coarse pointer without growing the
    /// chip's paint.
    pub fn with_coarse_pointer(mut self, coarse_pointer: bool) -> Self {
        self.coarse_pointer = coarse_pointer;
        self
    }

    fn horizontal_padding(&self, app: &AppContext) -> f32 {
        if self.padded {
            app.theme.spacing_px(SpacingToken::Md)
        } else {
            0.0
        }
    }

    fn vertical_padding(&self, app: &AppContext) -> f32 {
        if self.padded {
            app.theme.spacing_px(SpacingToken::Xs)
        } else {
            0.0
        }
    }
}

impl Element for Chip {
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
        let visual = vec2f(child_size.x + h_pad * 2.0, child_size.y + v_pad * 2.0);
        let (size, paint_size) = hit_target(visual, self.target_class, self.coarse_pointer);
        self.size = Some(size);
        self.paint_size = Some(paint_size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let paint_size = self.paint_size.unwrap_or(size);
        let paint_origin = centred_origin(origin, size, paint_size);
        let child_size = self.child.size().unwrap_or(Vector2F::zero());
        let h_pad = self.horizontal_padding(app);
        let v_pad = self.vertical_padding(app);
        let child_origin = vec2f(
            paint_origin.x + h_pad + (paint_size.x - child_size.x - h_pad * 2.0).max(0.0) / 2.0,
            paint_origin.y + v_pad + (paint_size.y - child_size.y - v_pad * 2.0).max(0.0) / 2.0,
        );
        self.child.paint(child_origin, ctx, app);
        if let Some(renderer) = ctx.renderer.as_mut() {
            paint_focus_ring(
                renderer,
                rectf(paint_origin.x, paint_origin.y, paint_size.x, paint_size.y),
                self.focused,
                FocusStyle::Control,
                0.0,
                &app.theme,
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
    fn chip_click_fires_callback() {
        let clicked = Rc::new(RefCell::new(false));
        let clicked_clone = clicked.clone();
        let mut chip = Chip::new(Empty::new().with_size(vec2f(60.0, 20.0)).finish())
            .with_on_click(move || *clicked_clone.borrow_mut() = true);

        let app = AppContext::default();
        chip.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        chip.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        };

        assert!(chip.dispatch_event(&down, &mut event_ctx, &app));
        assert!(chip.dispatch_event(&up, &mut event_ctx, &app));
        assert!(*clicked.borrow());
    }
}
