use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{vec2f, Vector2F};

/// A child of [`Stack`] that is positioned relative to the stack's origin
/// (instead of pinned to it), used for floating tooltips / popovers.
struct Positioned {
    child: Box<dyn Element>,
    offset: Vector2F,
}

pub struct Stack {
    children: Vec<Box<dyn Element>>,
    overlays: Vec<Positioned>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Stack {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            overlays: Vec::new(),
            size: None,
            origin: None,
        }
    }

    pub fn with_children(mut self, children: impl IntoIterator<Item = Box<dyn Element>>) -> Self {
        self.children.extend(children);
        self
    }

    /// Add an overlay child drawn on top of the ordered children, offset by
    /// `offset` from the stack's origin. Overlays are laid out but do not
    /// contribute to the stack's own size, and they get the first chance to
    /// handle pointer events (they are topmost).
    pub fn with_overlay(mut self, child: Box<dyn Element>, offset: Vector2F) -> Self {
        self.overlays.push(Positioned { child, offset });
        self
    }
}

impl Default for Stack {
    fn default() -> Self {
        Self::new()
    }
}

impl Extend<Box<dyn Element>> for Stack {
    fn extend<T: IntoIterator<Item = Box<dyn Element>>>(&mut self, iter: T) {
        self.children.extend(iter);
    }
}

impl Element for Stack {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let mut size = Vector2F::zero();
        for child in &mut self.children {
            let child_size = child.layout(constraint, ctx, app);
            size = vec2f(size.x.max(child_size.x), size.y.max(child_size.y));
        }
        // Overlays are laid out so they have a size for painting/hit-testing,
        // but they do not grow the stack itself.
        for overlay in &mut self.overlays {
            let _ = overlay.child.layout(constraint, ctx, app);
        }
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        for child in &mut self.children {
            child.paint(origin, ctx, app);
        }
        for overlay in &mut self.overlays {
            overlay.child.paint(origin + overlay.offset, ctx, app);
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
        app: &AppContext,
    ) -> bool {
        // Topmost overlay first, then topmost ordered child.
        for overlay in self.overlays.iter_mut().rev() {
            if overlay.child.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        for child in self.children.iter_mut().rev() {
            if child.dispatch_event(event, ctx, app) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::geometry::vec2f;

    struct Recorder {
        name: &'static str,
        hits: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Element for Recorder {
        fn layout(
            &mut self,
            _constraint: SizeConstraint,
            _ctx: &mut LayoutContext,
            _app: &AppContext,
        ) -> Vector2F {
            vec2f(10.0, 10.0)
        }

        fn paint(&mut self, _origin: Vector2F, _ctx: &mut PaintContext, _app: &AppContext) {}

        fn size(&self) -> Option<Vector2F> {
            Some(vec2f(10.0, 10.0))
        }

        fn origin(&self) -> Option<Point> {
            None
        }

        fn dispatch_event(
            &mut self,
            _event: &DispatchedEvent,
            _ctx: &mut EventContext,
            _app: &AppContext,
        ) -> bool {
            self.hits.borrow_mut().push(self.name);
            false
        }
    }

    #[test]
    fn dispatch_tries_topmost_first() {
        let hits = Rc::new(RefCell::new(Vec::new()));
        let top = Recorder {
            name: "top",
            hits: hits.clone(),
        };
        let bottom = Recorder {
            name: "bottom",
            hits: hits.clone(),
        };
        let mut stack = Stack::new().with_children(vec![bottom.finish(), top.finish()]);
        let app = AppContext::default();
        let event = DispatchedEvent::MouseMove {
            position: vec2f(5.0, 5.0),
        };
        let mut ctx = EventContext::default();
        let handled = stack.dispatch_event(&event, &mut ctx, &app);
        assert!(!handled);
        assert_eq!(*hits.borrow(), vec!["top", "bottom"]);
    }

    #[test]
    fn overlay_dispatches_before_children() {
        let hits = Rc::new(RefCell::new(Vec::new()));
        let child = Recorder {
            name: "child",
            hits: hits.clone(),
        };
        let overlay = Recorder {
            name: "overlay",
            hits: hits.clone(),
        };
        let mut stack = Stack::new()
            .with_children(vec![child.finish()])
            .with_overlay(overlay.finish(), vec2f(0.0, 0.0));
        let app = AppContext::default();
        let event = DispatchedEvent::MouseMove {
            position: vec2f(5.0, 5.0),
        };
        let mut ctx = EventContext::default();
        let _ = stack.dispatch_event(&event, &mut ctx, &app);
        assert_eq!(*hits.borrow(), vec!["overlay", "child"]);
    }
}
