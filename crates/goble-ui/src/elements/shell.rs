use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, ConstrainedBox, Container, CrossAxisAlignment, Element, Empty, EventContext, Fill,
    Flex, LayoutContext, PaintContext, Point, SizeConstraint, Topbar,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::ColorToken;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShellState {
    pub sidebar_collapsed: bool,
}

impl ShellState {
    pub fn new() -> Self {
        Self::default()
    }
}

pub struct ShellView {
    state: Rc<RefCell<ShellState>>,
    dirty: Rc<RefCell<bool>>,
    content_resolver:
        Box<dyn Fn(Rc<RefCell<ShellState>>, Rc<RefCell<bool>>) -> Box<dyn Element> + 'static>,
    conversation_sidebar_builder:
        Option<Box<dyn Fn(&AppContext, Rc<RefCell<bool>>) -> Box<dyn Element> + 'static>>,
    event_checker: Option<Rc<RefCell<dyn FnMut() -> bool>>>,
    root: Box<dyn Element>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ShellView {
    pub fn new(state: ShellState, app: &AppContext) -> Self {
        let bg = app.theme.color(ColorToken::Bg);
        let resolver: Box<
            dyn Fn(Rc<RefCell<ShellState>>, Rc<RefCell<bool>>) -> Box<dyn Element> + 'static,
        > = Box::new(
            move |_state: Rc<RefCell<ShellState>>, _dirty: Rc<RefCell<bool>>| -> Box<dyn Element> {
                Container::new(Empty::new().finish())
                    .with_background(Fill::Solid(bg))
                    .finish()
            },
        );
        Self::with_content(state, app, resolver)
    }

    pub fn with_content<F>(state: ShellState, app: &AppContext, content_resolver: F) -> Self
    where
        F: Fn(Rc<RefCell<ShellState>>, Rc<RefCell<bool>>) -> Box<dyn Element> + 'static,
    {
        Self::with_content_and_event_checker(state, app, content_resolver, None)
    }

    pub fn with_content_and_event_checker<F>(
        state: ShellState,
        app: &AppContext,
        content_resolver: F,
        event_checker: Option<Rc<RefCell<dyn FnMut() -> bool>>>,
    ) -> Self
    where
        F: Fn(Rc<RefCell<ShellState>>, Rc<RefCell<bool>>) -> Box<dyn Element> + 'static,
    {
        let state = Rc::new(RefCell::new(state));
        let dirty = Rc::new(RefCell::new(false));
        let content_resolver = Box::new(content_resolver);
        let mut view = Self {
            state,
            dirty,
            content_resolver,
            conversation_sidebar_builder: None,
            event_checker,
            root: Container::new(Empty::new().finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
                .finish(),
            size: None,
            origin: None,
        };
        view.rebuild(app);
        view
    }

    pub fn with_conversation_sidebar<F>(mut self, builder: F) -> Self
    where
        F: Fn(&AppContext, Rc<RefCell<bool>>) -> Box<dyn Element> + 'static,
    {
        self.conversation_sidebar_builder = Some(Box::new(builder));
        self.request_rebuild();
        self
    }

    pub fn state(&self) -> Rc<RefCell<ShellState>> {
        Rc::clone(&self.state)
    }

    pub fn request_rebuild(&self) {
        *self.dirty.borrow_mut() = true;
    }

    fn check_events(&self) {
        if let Some(checker) = self.event_checker.as_ref() {
            if (checker.borrow_mut())() {
                self.request_rebuild();
            }
        }
    }

    fn rebuild(&mut self, app: &AppContext) {
        self.root = self.build_root(app);
        *self.dirty.borrow_mut() = false;
    }

    fn build_root(&self, app: &AppContext) -> Box<dyn Element> {
        let topbar = Self::topbar(Rc::clone(&self.state), Rc::clone(&self.dirty), app);
        let body = self.body(
            Rc::clone(&self.state),
            Rc::clone(&self.dirty),
            &self.content_resolver,
            app,
        );

        let column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(topbar)
            .with_child(body)
            .finish();

        Container::new(column)
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .finish()
    }

    fn topbar(
        state: Rc<RefCell<ShellState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let menu_state = Rc::clone(&state);
        let menu_dirty = Rc::clone(&dirty);
        let on_menu = move || {
            let collapsed = menu_state.borrow().sidebar_collapsed;
            menu_state.borrow_mut().sidebar_collapsed = !collapsed;
            *menu_dirty.borrow_mut() = true;
        };

        Topbar::new(false, false, false, on_menu, || {}, || {}, || {}, app).finish()
    }

    fn body(
        &self,
        state: Rc<RefCell<ShellState>>,
        dirty: Rc<RefCell<bool>>,
        content_resolver: &dyn Fn(Rc<RefCell<ShellState>>, Rc<RefCell<bool>>) -> Box<dyn Element>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let content = content_resolver(Rc::clone(&state), Rc::clone(&dirty));

        if self.conversation_sidebar_builder.is_none() {
            return content;
        }

        let collapsed = state.borrow().sidebar_collapsed;
        let sidebar: Box<dyn Element> = if collapsed {
            Empty::new().finish()
        } else {
            (self.conversation_sidebar_builder.as_ref().unwrap())(app, Rc::clone(&self.dirty))
        };

        let width = if collapsed { 56.0 } else { 260.0 };
        let sidebar = ConstrainedBox::new(sidebar)
            .with_width(width)
            .with_min_width(width)
            .with_max_width(width)
            .finish();

        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(sidebar)
            .with_child(content)
            .finish()
    }
}

impl Element for ShellView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.check_events();
        if *self.dirty.borrow() {
            self.rebuild(app);
        }
        let size = self.root.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.check_events();
        if *self.dirty.borrow() {
            self.rebuild(app);
        }
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
        app: &AppContext,
    ) -> bool {
        self.root.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::vec2f;

    #[test]
    fn shell_view_layouts_non_zero() {
        let app = AppContext::default();
        let mut shell = ShellView::new(ShellState::default(), &app);
        let size = shell.layout(
            SizeConstraint::loose(vec2f(1024.0, 768.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }
}
