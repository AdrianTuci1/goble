use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};
use crate::theme::SpacingToken;

pub struct Chip {
    child: Box<dyn Element>,
    /// A second content the chip paints instead of `child` while the pointer is
    /// over it. Both are built with the same constraint and are expected to be
    /// the same size; the pair is what gives a click target a hover ink (a
    /// label that brightens under the pointer), which a child element cannot do
    /// because its colour is chosen when the tree is built, before paint knows
    /// where the pointer is.
    hover_child: Option<Box<dyn Element>>,
    state: InteractiveState,
    disabled: bool,
    selected: bool,
    /// Whether the chip adds the padding that makes it a stand-alone control.
    /// A chip that is only a hit target around a row of text turns this off, so
    /// the row stays where the text around it starts.
    padded: bool,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Chip {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            hover_child: None,
            state: InteractiveState::default(),
            disabled: false,
            selected: false,
            padded: true,
            on_click: None,
            size: None,
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

    /// Paint `hover_child` instead of the normal content while the pointer is
    /// over the chip. The two must be laid out under the same constraint and be
    /// the same size — the usual pair is one run of text in its resting ink and
    /// the same run in its hover ink.
    pub fn with_hover_variant(mut self, hover_child: Box<dyn Element>) -> Self {
        self.hover_child = Some(hover_child);
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
        let inner = SizeConstraint::new(inner_min, inner_max);
        let child_size = self.child.layout(inner, ctx, app);
        if let Some(hover_child) = self.hover_child.as_mut() {
            // Laid out under the same constraint so the hover content lands
            // exactly where the resting one does.
            hover_child.layout(inner, ctx, app);
        }
        let size = vec2f(child_size.x + h_pad * 2.0, child_size.y + v_pad * 2.0);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let hovered = self.bounds().is_some_and(|bounds| ctx.hovered(bounds));
        let h_pad = self.horizontal_padding(app);
        let v_pad = self.vertical_padding(app);
        // Hover ink is chosen here, while the pointer is known: the child runs
        // were built before paint and cannot recolour themselves.
        let child: &mut dyn Element = match self.hover_child.as_mut() {
            Some(hover_child) if hovered => hover_child.as_mut(),
            _ => self.child.as_mut(),
        };
        let child_size = child.size().unwrap_or(Vector2F::zero());
        let child_origin = vec2f(
            origin.x + h_pad + (size.x - child_size.x - h_pad * 2.0).max(0.0) / 2.0,
            origin.y + v_pad + (size.y - child_size.y - v_pad * 2.0).max(0.0) / 2.0,
        );
        child.paint(child_origin, ctx, app);
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
