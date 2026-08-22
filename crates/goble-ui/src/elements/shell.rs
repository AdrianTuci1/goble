use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Container, ConversationEntry, ConversationSidebar, CrossAxisAlignment, Element,
    Empty, EventContext, Fill, Flex, LayoutContext, PaintContext, Point, SizeConstraint, Stack,
    Topbar,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::ColorToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarMode {
    Agent,
    Threads,
    Drive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveView {
    Chat,
    AgentManagement,
    Threads,
    Drive,
    Settings(SettingsTab),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Appearance,
    Account,
    Cluster,
    Workers,
    Keys,
}

impl Default for SettingsTab {
    fn default() -> Self {
        SettingsTab::General
    }
}

pub struct ShellState {
    pub sidebar_collapsed: bool,
    pub sidebar_mode: SidebarMode,
    pub active_view: ActiveView,
    pub chat_sidebar_visible: bool,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            sidebar_collapsed: false,
            sidebar_mode: SidebarMode::Agent,
            active_view: ActiveView::Chat,
            chat_sidebar_visible: false,
        }
    }
}

pub struct ShellView {
    state: Rc<RefCell<ShellState>>,
    dirty: Rc<RefCell<bool>>,
    content_resolver: Box<dyn Fn(Rc<RefCell<ShellState>>, Rc<RefCell<bool>>) -> Box<dyn Element> + 'static>,
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
        > = Box::new(move |_state: Rc<RefCell<ShellState>>, _dirty: Rc<RefCell<bool>>| -> Box<dyn Element> {
            Container::new(Empty::new().finish())
                .with_background(Fill::Solid(bg))
                .finish()
        });
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
        let root = Self::build_root(
            Rc::clone(&state),
            Rc::clone(&dirty),
            &content_resolver,
            app,
        );
        Self {
            state,
            dirty,
            content_resolver,
            event_checker,
            root,
            size: None,
            origin: None,
        }
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
        self.root = Self::build_root(
            Rc::clone(&self.state),
            Rc::clone(&self.dirty),
            &self.content_resolver,
            app,
        );
        *self.dirty.borrow_mut() = false;
    }

    fn build_root(
        state: Rc<RefCell<ShellState>>,
        dirty: Rc<RefCell<bool>>,
        content_resolver: &dyn Fn(Rc<RefCell<ShellState>>, Rc<RefCell<bool>>) -> Box<dyn Element>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let topbar = Self::titlebar(Rc::clone(&state), Rc::clone(&dirty), app);
        let body = Self::body(Rc::clone(&state), Rc::clone(&dirty), content_resolver, app);

        let column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(topbar)
            .with_child(body)
            .finish();

        Container::new(column)
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .finish()
    }

    fn titlebar(
        state: Rc<RefCell<ShellState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let active_view = state.borrow().active_view;
        let threads_active = active_view == ActiveView::Threads;
        let inbox_active = active_view == ActiveView::AgentManagement;
        let settings_active = matches!(active_view, ActiveView::Settings(_));

        let menu_state = Rc::clone(&state);
        let menu_dirty = Rc::clone(&dirty);
        let on_menu = move || {
            let mut s = menu_state.borrow_mut();
            match s.active_view {
                ActiveView::Chat => s.sidebar_collapsed = !s.sidebar_collapsed,
                _ => s.active_view = ActiveView::Chat,
            }
            *menu_dirty.borrow_mut() = true;
        };

        let threads_state = Rc::clone(&state);
        let threads_dirty = Rc::clone(&dirty);
        let on_threads = move || {
            let mut s = threads_state.borrow_mut();
            s.active_view = match s.active_view {
                ActiveView::Threads => ActiveView::Chat,
                _ => ActiveView::Threads,
            };
            *threads_dirty.borrow_mut() = true;
        };

        let inbox_state = Rc::clone(&state);
        let inbox_dirty = Rc::clone(&dirty);
        let on_inbox = move || {
            inbox_state.borrow_mut().active_view = ActiveView::AgentManagement;
            *inbox_dirty.borrow_mut() = true;
        };

        let settings_state = Rc::clone(&state);
        let settings_dirty = Rc::clone(&dirty);
        let on_settings = move || {
            settings_state.borrow_mut().active_view = ActiveView::Settings(SettingsTab::default());
            *settings_dirty.borrow_mut() = true;
        };

        Topbar::new(
            threads_active,
            inbox_active,
            settings_active,
            on_menu,
            on_threads,
            on_inbox,
            on_settings,
            app,
        )
        .finish()
    }

    fn body(
        state: Rc<RefCell<ShellState>>,
        dirty: Rc<RefCell<bool>>,
        content_resolver: &dyn Fn(Rc<RefCell<ShellState>>, Rc<RefCell<bool>>) -> Box<dyn Element>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let sidebar = Self::left_panel(Rc::clone(&state), Rc::clone(&dirty), app);
        let content = content_resolver(Rc::clone(&state), Rc::clone(&dirty));

        // The sidebar is an overlay so the content sees the full window width.
        Stack::new()
            .with_children(vec![content, sidebar])
            .finish()
    }

    fn left_panel(
        state: Rc<RefCell<ShellState>>,
        _dirty: Rc<RefCell<bool>>,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let s = state.borrow();
        if s.sidebar_collapsed {
            return Empty::new().finish();
        }
        match s.active_view {
            ActiveView::Chat => {
                drop(s);
                let conversations = sample_conversations();
                ConversationSidebar::new(conversations)
                    .with_selected("c1")
                    .with_on_create(|| log::info!("new conversation clicked"))
                    .with_on_select(|id| log::info!("selected conversation: {}", id))
                    .with_on_delete(|id| log::info!("delete conversation: {}", id))
                    .finish()
            }
            _ => Empty::new().finish(),
        }
    }

}

fn sample_conversations() -> Vec<ConversationEntry> {
    use crate::elements::ConversationStatus;
    vec![
        ConversationEntry::new("c1", "Ada", "I finished the review.", "10:42")
            .with_status(ConversationStatus::Success),
        ConversationEntry::new("c2", "Coder", "Build failed on step 3.", "09:15")
            .with_status(ConversationStatus::Error),
        ConversationEntry::new("c3", "Planner", "Stopped by user.", "Yesterday")
            .with_status(ConversationStatus::Stopped),
        ConversationEntry::new("c4", "Research", "Here are the sources you asked for.", "Mon")
            .with_status(ConversationStatus::Default),
    ]
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
