//! Shell layout: topbar, main content switcher, and the fixed-width
//! sidebar+main split.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, vec2f, RectF, Vector2F};
use goble_ui::theme::ColorToken;
use goble_ui::{ChatView, SettingsPage, SettingsView, Topbar};

use crate::chat;
use crate::{AppTab, UiActions, UiSnapshot};

/// Main topbar: threads on the left, inbox + user settings on the right.
pub fn build_topbar(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    let on_menu = actions.on_menu.clone();
    let on_threads = actions.on_threads.clone();
    let on_inbox = actions.on_inbox.clone();
    let on_settings = actions.on_settings.clone();
    Topbar::new(
        state.current_tab == AppTab::Threads,
        false,
        state.current_tab == AppTab::Settings,
        move || (on_menu.borrow_mut())(),
        move || (on_threads.borrow_mut())(),
        move || (on_inbox.borrow_mut())(),
        move || (on_settings.borrow_mut())(),
        app,
    )
    .finish()
}

/// Main content area: threads placeholder, terminal/chat, or settings.
/// Navigation is driven by the topbar buttons (threads left; settings right).
pub fn build_main(app: &AppContext, state: &UiSnapshot, actions: &UiActions) -> Box<dyn Element> {
    match state.current_tab {
        AppTab::Threads => ChatView::new()
            .with_messages(state.thread_messages.clone())
            .finish(),
        AppTab::Chat => chat::build_agent_chat(app, state, actions),
        AppTab::Settings => SettingsView::new(SettingsPage::Profile)
            .with_profile("Ada", "ada@example.com")
            .with_llm("openai", "gpt-4o", "", "", "")
            .finish(),
    }
}

/// Splits horizontal space: fixed-width sidebar + main area filling the rest.
/// The engine's `Flex` cannot yet distribute remaining space, so this custom
/// element does the split at layout time.
///
/// A draggable divider sits on the sidebar's right edge so the user can resize
/// it. The width lives in app state (it must survive the per-frame rebuild),
/// and the drag flags/callbacks are passed in from the snapshot.
const RESIZE_HANDLE_HALF: f32 = 4.0;

pub struct SidebarLayout {
    sidebar: Box<dyn Element>,
    main: Box<dyn Element>,
    width: f32,
    dragging: bool,
    on_drag_start: Option<Rc<RefCell<dyn FnMut(f32)>>>,
    on_drag_move: Option<Rc<RefCell<dyn FnMut(f32)>>>,
    on_drag_end: Option<Rc<RefCell<dyn FnMut()>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SidebarLayout {
    pub fn new(sidebar: Box<dyn Element>, main: Box<dyn Element>, width: f32) -> Self {
        Self {
            sidebar,
            main,
            width,
            dragging: false,
            on_drag_start: None,
            on_drag_move: None,
            on_drag_end: None,
            size: None,
            origin: None,
        }
    }

    /// Whether a drag is in progress (carried from app state so it survives
    /// the per-frame rebuild).
    pub fn with_dragging(mut self, dragging: bool) -> Self {
        self.dragging = dragging;
        self
    }

    pub fn with_on_drag_start<F: FnMut(f32) + 'static>(mut self, callback: F) -> Self {
        self.on_drag_start = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_drag_move<F: FnMut(f32) + 'static>(mut self, callback: F) -> Self {
        self.on_drag_move = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_drag_end<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_drag_end = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Bounds of the resizable divider, in absolute window coordinates.
    fn handle_bounds(&self) -> Option<RectF> {
        let origin = self.origin?;
        let size = self.size?;
        Some(rectf(
            origin.x() + self.width - RESIZE_HANDLE_HALF,
            origin.y(),
            RESIZE_HANDLE_HALF * 2.0,
            size.y,
        ))
    }
}

impl Element for SidebarLayout {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let sidebar_constraint =
            SizeConstraint::new(vec2f(0.0, 0.0), vec2f(self.width, constraint.max.y));
        let _ = self.sidebar.layout(sidebar_constraint, ctx, app);

        let main_width = (constraint.max.x - self.width).max(0.0);
        let main_constraint =
            SizeConstraint::new(vec2f(0.0, 0.0), vec2f(main_width, constraint.max.y));
        let _ = self.main.layout(main_constraint, ctx, app);

        let size = vec2f(constraint.max.x, constraint.max.y);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.sidebar.paint(origin, ctx, app);
        self.main.paint(origin + vec2f(self.width, 0.0), ctx, app);

        // Resizable divider along the sidebar's right edge; highlights while
        // dragging so the user can see where the grab point is.
        let height = self.size.map(|s| s.y).unwrap_or(0.0);
        let color = if self.dragging {
            app.theme.color(ColorToken::Accent)
        } else {
            app.theme.color(ColorToken::Border)
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rounded_rect(
                rectf(origin.x + self.width - 0.5, origin.y, 1.0, height),
                color,
                0.0,
            );
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
        // While dragging, consume move/up events so the split updates even when
        // the pointer is over the main content.
        if self.dragging {
            match event {
                DispatchedEvent::MouseMove { position } => {
                    if let Some(cb) = &self.on_drag_move {
                        (cb.borrow_mut())(position.x);
                    }
                    return true;
                }
                DispatchedEvent::MouseUp { .. } => {
                    if let Some(cb) = &self.on_drag_end {
                        (cb.borrow_mut())();
                    }
                    return true;
                }
                _ => {}
            }
        }
        // Start a drag when the pointer grabs the divider.
        if let DispatchedEvent::MouseDown { position, .. } = event {
            if let Some(bounds) = self.handle_bounds() {
                if contains(bounds, *position) {
                    if let Some(cb) = &self.on_drag_start {
                        (cb.borrow_mut())(position.x);
                    }
                    return true;
                }
            }
        }
        if self.sidebar.dispatch_event(event, ctx, app) {
            return true;
        }
        self.main.dispatch_event(event, ctx, app)
    }
}
