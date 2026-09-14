//! The low-emphasis pill button used by the rich input.
//!
//! A card at rest: its own box outlined in the theme's border colour, filled
//! with the hover colour under the pointer. The outline is all it draws at
//! rest, so the rich input's footer controls read as controls on the flat
//! panel around them rather than as bare text and icons.
//!
//! A row that already sits inside a card of its own — the command proposal's
//! candidates — turns the outline off ([`ComposerButton::with_outline`]): it is
//! a list row, not a footer control, and a stack of outlined rows inside the
//! proposal's card would read as boxes rather than as the one list.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint, Vector2F,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f};
use crate::theme::ColorToken;

/// The rich input's own corner radius, smaller than the theme's `radius_px`:
/// these controls sit inside the input's box at 28 pt tall, where the theme's
/// radius reads as a pill rather than as a card.
pub const COMPOSER_CONTROL_RADIUS: f32 = 4.0;

/// A refined, low-emphasis pill button used in the composer footer.
///
/// A card with [`COMPOSER_CONTROL_RADIUS`] and a 1px border at rest, and the
/// theme's hover fill under the pointer. Fixed height with auto width for
/// icon+label content.
pub struct ComposerButton {
    child: Box<dyn Element>,
    state: InteractiveState,
    height: f32,
    /// Whether the control outlines its own box at rest. On for the rich
    /// input's footer controls; off for a full-width row that belongs to a card
    /// it is drawn inside.
    outlined: bool,
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
            outlined: true,
            on_click: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Whether the control draws its resting outline (see the module docs).
    pub fn with_outline(mut self, outlined: bool) -> Self {
        self.outlined = outlined;
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
        // The outline and the hover fill share the input's own corner radius,
        // so the fill sits exactly inside the card's border. At rest the
        // control draws the outline only — no fill of its own.
        let radius = COMPOSER_CONTROL_RADIUS;
        if let Some(renderer) = ctx.renderer.as_mut() {
            if hovered {
                renderer.fill_rounded_rect(rect, app.theme.color(ColorToken::Hover), radius);
            }
            if self.outlined {
                renderer.stroke_rect(rect, app.theme.color(ColorToken::Border), 1.0, radius);
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
