//! The low-emphasis pill button used by the rich input.
//!
//! Transparent by default with a subtle rounded hover overlay, so a pill reads
//! as part of its surrounding bar (the chat composer's footer or a terminal
//! pane's topbar) instead of an inset box.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint, Vector2F,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f};
use crate::theme::ColorToken;

/// A refined, low-emphasis pill button used in the composer footer.
///
/// Mirrors the warp-new button style: transparent by default, a subtle rounded
/// hover overlay, and a fixed height with auto width for icon+label content.
pub struct ComposerButton {
    child: Box<dyn Element>,
    state: InteractiveState,
    height: f32,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ComposerButton {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            state: InteractiveState::default(),
            height: 28.0,
            on_click: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }
}

impl Element for ComposerButton {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let h_pad = 10.0;
        let inner_max = vec2f(
            (constraint.max.x - h_pad * 2.0).max(0.0),
            (self.height - 4.0).max(0.0),
        );
        let child_size = self
            .child
            .layout(SizeConstraint::new(vec2f(0.0, 0.0), inner_max), ctx, app);
        let size = vec2f(child_size.x + h_pad * 2.0, self.height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let rect = rectf(origin.x, origin.y, size.x, size.y);
        let hovered = ctx.hovered(rect);
        if let Some(renderer) = ctx.renderer.as_mut() {
            // Rich-input pills are flat: no filled box or 1px border by default,
            // so they do not look inset inside the composer. Only a rounded
            // hover overlay is drawn so the trigger still gives feedback.
            if hovered {
                renderer.fill_rounded_rect(rect, app.theme.color(ColorToken::Hover), 6.0);
            }
        }
        let child_size = self.child.size().unwrap_or(Vector2F::zero());
        let offset = vec2f(
            (size.x - child_size.x).max(0.0) / 2.0,
            (size.y - child_size.y).max(0.0) / 2.0,
        );
        self.child.paint(origin + offset, ctx, app);
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
        ctx: &mut crate::elements::EventContext,
        _app: &AppContext,
    ) -> bool {
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
