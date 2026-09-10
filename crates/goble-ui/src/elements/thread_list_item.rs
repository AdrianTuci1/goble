use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, CrossAxisAlignment, Element, Empty, EventContext, Fill, Flex, LayoutContext,
    MainAxisAlignment, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub struct ThreadListItem {
    root: Box<dyn Element>,
    state: InteractiveState,
    #[allow(dead_code)]
    selected: bool,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ThreadListItem {
    pub fn new(
        leading: Box<dyn Element>,
        title: Box<dyn Element>,
        badge: Option<Box<dyn Element>>,
        selected: bool,
        app: &AppContext,
    ) -> Self {
        let root = Self::build_root(leading, title, badge, selected, app);
        Self {
            root,
            state: InteractiveState::default(),
            selected,
            on_click: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn build_root(
        leading: Box<dyn Element>,
        title: Box<dyn Element>,
        badge: Option<Box<dyn Element>>,
        selected: bool,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let spacing = app.theme.spacing_px(SpacingToken::Md);
        let row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(leading)
            .with_child(title)
            .with_child(badge.unwrap_or_else(|| Empty::new().finish()))
            .finish();

        crate::elements::Container::new(row)
            .with_padding(crate::style::EdgeInsets::uniform(spacing))
            .with_background(if selected {
                Fill::Solid(app.theme.color(ColorToken::Selected))
            } else {
                Fill::None
            })
            .finish()
    }
}

impl Element for ThreadListItem {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.root.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.paint(origin, ctx, app);
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
    use crate::elements::{Avatar, Text};
    use crate::geometry::vec2f;

    #[test]
    fn thread_list_item_click_fires_callback() {
        let clicked = Rc::new(RefCell::new(false));
        let clicked_clone = clicked.clone();
        let app = AppContext::default();
        let mut item = ThreadListItem::new(
            Avatar::new("G").finish(),
            Text::new("General").finish(),
            None,
            false,
            &app,
        )
        .with_on_click(move || *clicked_clone.borrow_mut() = true);

        item.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        item.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        };

        assert!(item.dispatch_event(&down, &mut event_ctx, &app));
        assert!(item.dispatch_event(&up, &mut event_ctx, &app));
        assert!(*clicked.borrow());
    }
}
