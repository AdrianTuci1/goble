use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{contains, handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Container, EdgeInsets, Element, EventContext, Fill, LayoutContext, PaintContext,
    Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::ColorToken;

/// A full-width button whose background highlights while hovered.
///
/// The element tree is rebuilt every frame, so element-local hover state is
/// reset before it is ever painted. Instead the hover flag lives in app-owned
/// state and is shared in via `Rc<RefCell<_>>`: this element reads it to pick
/// the background and writes it in response to mouse-move events.
pub struct HoverButton {
    child: Option<Box<dyn Element>>,
    hover: Rc<RefCell<bool>>,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    state: InteractiveState,
    padding: EdgeInsets,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl HoverButton {
    pub fn new(child: Box<dyn Element>, hover: Rc<RefCell<bool>>) -> Self {
        Self {
            child: Some(child),
            hover,
            on_click: None,
            state: InteractiveState::default(),
            padding: EdgeInsets::uniform(0.0),
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn ensure_root(&mut self, app: &AppContext) {
        if self.root.is_some() {
            return;
        }
        let bg = if *self.hover.borrow() {
            Fill::Solid(app.theme.color(ColorToken::Hover))
        } else {
            Fill::None
        };
        self.root = Some(
            Container::new(self.child.take().expect("child already consumed"))
                .with_padding(self.padding)
                .with_corner_radius(app.theme.radius_px())
                .with_background(bg)
                .finish(),
        );
    }
}

impl Element for HoverButton {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.ensure_root(app);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.as_mut().unwrap().paint(origin, ctx, app);
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
        self.ensure_root(app);
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };

        if let DispatchedEvent::MouseMove { position } = event {
            let inside = contains(bounds, *position);
            *self.hover.borrow_mut() = inside;
            return false;
        }

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
    use crate::elements::Text;
    use crate::geometry::vec2f;

    #[test]
    fn hover_updates_shared_flag() {
        let hover = Rc::new(RefCell::new(false));
        let mut button = HoverButton::new(
            Text::new("New agent").finish(),
            Rc::clone(&hover),
        );
        let app = AppContext::default();
        button.layout(
            SizeConstraint::loose(vec2f(200.0, 40.0)),
            &mut LayoutContext::default(),
            &app,
        );
        button.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let inside = DispatchedEvent::MouseMove {
            position: vec2f(10.0, 10.0),
        };
        button.dispatch_event(&inside, &mut event_ctx, &app);
        assert!(*hover.borrow());
    }

    #[test]
    fn click_fires_callback() {
        let clicked = Rc::new(RefCell::new(false));
        let clicked_clone = clicked.clone();
        let hover = Rc::new(RefCell::new(false));
        let mut button = HoverButton::new(
            Text::new("New agent").finish(),
            Rc::clone(&hover),
        )
        .with_on_click(move || *clicked_clone.borrow_mut() = true);
        let app = AppContext::default();
        button.layout(
            SizeConstraint::loose(vec2f(200.0, 40.0)),
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
