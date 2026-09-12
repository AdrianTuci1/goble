//! An element that lets its owner intercept key presses before the wrapped
//! subtree sees them.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::{DispatchedEvent, ModifiersState};
use crate::geometry::Vector2F;

/// Wraps a child and gives a callback first refusal on every key press.
///
/// The callback returns `true` to consume the key (the child never sees it) or
/// `false` to let it fall through. Everything else — layout, paint and mouse
/// events — is forwarded unchanged, so this is how a container implements
/// keyboard navigation for a subtree whose children are all mouse-driven.
pub struct KeyHandler {
    child: Box<dyn Element>,
    on_key: Rc<RefCell<dyn FnMut(&str, ModifiersState) -> bool>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl KeyHandler {
    pub fn new<F>(child: Box<dyn Element>, on_key: F) -> Self
    where
        F: FnMut(&str, ModifiersState) -> bool + 'static,
    {
        Self {
            child,
            on_key: Rc::new(RefCell::new(on_key)),
            size: None,
            origin: None,
        }
    }
}

impl Element for KeyHandler {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.child.paint(origin, ctx, app);
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
        if let DispatchedEvent::KeyDown { key, modifiers } = event {
            if (self.on_key.borrow_mut())(key, *modifiers) {
                return true;
            }
        }
        self.child.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{Container, Text};
    use crate::geometry::vec2f;
    use std::cell::Cell;

    fn key(name: &str) -> DispatchedEvent {
        DispatchedEvent::KeyDown {
            key: name.to_string(),
            modifiers: ModifiersState::none(),
        }
    }

    #[test]
    fn a_consumed_key_does_not_reach_the_child() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen_clone = seen.clone();
        let mut child = KeyHandler::new(
            Container::new(Text::new("hi").finish()).finish(),
            move |key: &str, _m| {
                seen_clone.borrow_mut().push(key.to_string());
                key == "ArrowDown"
            },
        )
        .finish();

        let app = AppContext::default();
        let mut ctx = EventContext::default();
        let consumed = child.dispatch_event(&key("ArrowDown"), &mut ctx, &app);
        assert!(consumed, "a handled key is consumed");
        assert_eq!(*seen.borrow(), vec!["ArrowDown"]);
    }

    #[test]
    fn other_keys_and_mouse_events_fall_through() {
        let hits = Rc::new(Cell::new(0));
        let hits_clone = hits.clone();
        let mut child = KeyHandler::new(
            Container::new(Text::new("hi").finish()).finish(),
            move |_key: &str, _m| {
                hits_clone.set(hits_clone.get() + 1);
                false
            },
        )
        .finish();

        let app = AppContext::default();
        child.layout(
            SizeConstraint::loose(vec2f(100.0, 100.0)),
            &mut LayoutContext::default(),
            &app,
        );
        child.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut ctx = EventContext::default();
        assert!(
            !child.dispatch_event(&key("a"), &mut ctx, &app),
            "an unhandled key is not consumed"
        );
        assert_eq!(hits.get(), 1, "the callback still saw the key");

        child.dispatch_event(
            &DispatchedEvent::MouseMove {
                position: vec2f(5.0, 5.0),
            },
            &mut ctx,
            &app,
        );
        assert_eq!(hits.get(), 1, "mouse events bypass the key callback");
    }
}
