use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Button, ButtonVariant, ConstrainedBox, Container, CrossAxisAlignment, Element,
    Empty, EventContext, Fill, Flex, Icon, LayoutContext, PaintContext, Point, SidebarItem,
    SizeConstraint, Text, Topbar,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::style::EdgeInsets as Insets;
use crate::theme::{ColorToken, SpacingToken};

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
    Executions,
    AgentTrace,
    Connectors,
    Workflows,
    Teams,
    Logs,
    Search,
    Settings(SettingsTab),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Appearance,
    Account,
    Cluster,
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
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            sidebar_collapsed: false,
            sidebar_mode: SidebarMode::Agent,
            active_view: ActiveView::Chat,
        }
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
        let active = state.borrow().active_view;
        let threads_active = active == ActiveView::Threads;
        let inbox_active = active == ActiveView::AgentManagement;
        let settings_active = matches!(active, ActiveView::Settings(_));

        let menu_state = Rc::clone(&state);
        let menu_dirty = Rc::clone(&dirty);
        let on_menu = move || {
            if active == ActiveView::Chat {
                let collapsed = menu_state.borrow().sidebar_collapsed;
                menu_state.borrow_mut().sidebar_collapsed = !collapsed;
            } else {
                menu_state.borrow_mut().active_view = ActiveView::Chat;
            }
            *menu_dirty.borrow_mut() = true;
        };

        let threads_state = Rc::clone(&state);
        let threads_dirty = Rc::clone(&dirty);
        let on_threads = move || {
            threads_state.borrow_mut().active_view = ActiveView::Threads;
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
            settings_state.borrow_mut().active_view = ActiveView::Settings(SettingsTab::General);
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
        &self,
        state: Rc<RefCell<ShellState>>,
        dirty: Rc<RefCell<bool>>,
        content_resolver: &dyn Fn(Rc<RefCell<ShellState>>, Rc<RefCell<bool>>) -> Box<dyn Element>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let sidebar = self.left_panel(app);
        let content = content_resolver(Rc::clone(&state), Rc::clone(&dirty));

        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(sidebar)
            .with_child(content)
            .finish()
    }

    fn left_panel(&self, app: &AppContext) -> Box<dyn Element> {
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let state = Rc::clone(&self.state);
        let dirty = Rc::clone(&self.dirty);
        let s = state.borrow();

        let use_conversation_sidebar =
            s.active_view == ActiveView::Chat && self.conversation_sidebar_builder.is_some();

        let mode_buttons = Flex::row()
            .with_spacing(spacing)
            .with_child(Self::mode_button(
                "Agent",
                SidebarMode::Agent,
                Rc::clone(&state),
                Rc::clone(&dirty),
                app,
            ))
            .with_child(Self::mode_button(
                "Threads",
                SidebarMode::Threads,
                Rc::clone(&state),
                Rc::clone(&dirty),
                app,
            ))
            .with_child(Self::mode_button(
                "Drive",
                SidebarMode::Drive,
                Rc::clone(&state),
                Rc::clone(&dirty),
                app,
            ))
            .finish();

        let items: Vec<Box<dyn Element>> = if use_conversation_sidebar {
            vec![(self.conversation_sidebar_builder.as_ref().unwrap())(
                app,
                Rc::clone(&self.dirty),
            )]
        } else {
            match s.sidebar_mode {
                SidebarMode::Agent => vec![
                    Self::sidebar_nav_item(
                        "New chat",
                        ActiveView::Chat,
                        Rc::clone(&state),
                        Rc::clone(&dirty),
                        app,
                    ),
                    Self::sidebar_nav_item(
                        "Agent runs",
                        ActiveView::AgentManagement,
                        Rc::clone(&state),
                        Rc::clone(&dirty),
                        app,
                    ),
                    Self::sidebar_nav_item(
                        "Executions",
                        ActiveView::Executions,
                        Rc::clone(&state),
                        Rc::clone(&dirty),
                        app,
                    ),
                    Self::sidebar_nav_item(
                        "Connectors",
                        ActiveView::Connectors,
                        Rc::clone(&state),
                        Rc::clone(&dirty),
                        app,
                    ),
                    Self::sidebar_nav_item(
                        "Workflows",
                        ActiveView::Workflows,
                        Rc::clone(&state),
                        Rc::clone(&dirty),
                        app,
                    ),
                    Self::sidebar_nav_item(
                        "Teams",
                        ActiveView::Teams,
                        Rc::clone(&state),
                        Rc::clone(&dirty),
                        app,
                    ),
                    Self::sidebar_nav_item(
                        "Logs",
                        ActiveView::Logs,
                        Rc::clone(&state),
                        Rc::clone(&dirty),
                        app,
                    ),
                    Self::sidebar_nav_item(
                        "Search",
                        ActiveView::Search,
                        Rc::clone(&state),
                        Rc::clone(&dirty),
                        app,
                    ),
                ],
                SidebarMode::Threads => vec![
                    Self::sidebar_nav_item(
                        "Recent",
                        ActiveView::Threads,
                        Rc::clone(&state),
                        Rc::clone(&dirty),
                        app,
                    ),
                    Self::sidebar_nav_item(
                        "Starred",
                        ActiveView::Threads,
                        Rc::clone(&state),
                        Rc::clone(&dirty),
                        app,
                    ),
                ],
                SidebarMode::Drive => vec![
                    Self::sidebar_nav_item(
                        "Workflows",
                        ActiveView::Drive,
                        Rc::clone(&state),
                        Rc::clone(&dirty),
                        app,
                    ),
                    Self::sidebar_nav_item(
                        "Agents",
                        ActiveView::AgentManagement,
                        Rc::clone(&state),
                        Rc::clone(&dirty),
                        app,
                    ),
                    Self::sidebar_nav_item(
                        "Teams",
                        ActiveView::Drive,
                        Rc::clone(&state),
                        Rc::clone(&dirty),
                        app,
                    ),
                ],
            }
        };

        let column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing)
            .with_child(mode_buttons)
            .with_children(items)
            .finish();

        let width = if s.sidebar_collapsed { 56.0 } else { 240.0 };
        ConstrainedBox::new(
            Container::new(column)
                .with_padding(Insets::uniform(spacing))
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .finish(),
        )
        .with_width(width)
        .with_min_width(width)
        .with_max_width(width)
        .finish()
    }

    fn mode_button(
        label: &str,
        mode: SidebarMode,
        state: Rc<RefCell<ShellState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let selected = state.borrow().sidebar_mode == mode;
        let state2 = Rc::clone(&state);
        let dirty2 = Rc::clone(&dirty);
        Button::new(
            Text::new(label)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_variant(if selected {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Ghost
        })
        .with_on_click(move || {
            state2.borrow_mut().sidebar_mode = mode;
            *dirty2.borrow_mut() = true;
        })
        .finish()
    }

    fn sidebar_nav_item(
        label: &str,
        view: ActiveView,
        state: Rc<RefCell<ShellState>>,
        dirty: Rc<RefCell<bool>>,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let state2 = Rc::clone(&state);
        let dirty2 = Rc::clone(&dirty);
        SidebarItem::new(
            Icon::new("circle")
                .with_size(16.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
            Text::new(label)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
            None,
            false,
            app,
        )
        .with_on_click(move || {
            state2.borrow_mut().active_view = view;
            *dirty2.borrow_mut() = true;
        })
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
