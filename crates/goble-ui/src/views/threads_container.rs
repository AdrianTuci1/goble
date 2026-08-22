use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::chat_content::{ChatAction, ChatMessage};
use crate::elements::{
    AppContext, ConstrainedBox, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Fill,
    Flex, LayoutContext, MainAxisAlignment, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};
use crate::views::thread_list_view::ThreadListEntry;
use crate::views::thread_sidebar::ThreadSidebar;
use crate::views::thread_view::ThreadView;

pub struct ThreadsContainer {
    threads: Vec<ThreadListEntry>,
    messages_by_thread: std::collections::HashMap<String, Vec<ChatMessage>>,
    selected_id: String,
    collapsed_sections: std::collections::HashSet<String>,
    header: Option<Box<dyn Element>>,
    on_select: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_send: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_action: Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    on_new: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_toggle_section: Option<Rc<RefCell<dyn FnMut(String, bool) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ThreadsContainer {
    pub fn new(selected_id: impl Into<String>) -> Self {
        Self {
            threads: Vec::new(),
            messages_by_thread: std::collections::HashMap::new(),
            selected_id: selected_id.into(),
            collapsed_sections: std::collections::HashSet::new(),
            header: None,
            on_select: None,
            on_send: None,
            on_action: None,
            on_new: None,
            on_toggle_section: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_threads(mut self, threads: Vec<ThreadListEntry>) -> Self {
        self.threads = threads;
        self
    }

    pub fn with_messages(
        mut self,
        thread_id: impl Into<String>,
        messages: Vec<ChatMessage>,
    ) -> Self {
        self.messages_by_thread.insert(thread_id.into(), messages);
        self
    }

    pub fn with_collapsed_sections(
        mut self,
        sections: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.collapsed_sections = sections.into_iter().map(|s| s.into()).collect();
        self
    }

    pub fn with_header(mut self, header: Box<dyn Element>) -> Self {
        self.header = Some(header);
        self
    }

    pub fn with_on_select<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_select = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_send<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_send = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_action<F: FnMut(ChatAction) + 'static>(mut self, callback: F) -> Self {
        self.on_action = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_new<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_new = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_toggle_section<F: FnMut(String, bool) + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_section = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn selected_id(&self) -> &str {
        &self.selected_id
    }

    fn rebuild(&mut self, app: &AppContext, width: f32) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);

        if self.threads.is_empty() {
            self.root = Some(
                Container::new(
                    Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_main_axis_alignment(MainAxisAlignment::Center)
                        .with_child(
                            Text::new("No threads available.")
                                .with_theme_color(ColorToken::Muted, app)
                                .finish(),
                        )
                        .finish(),
                )
                .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
                .with_padding(EdgeInsets::uniform(spacing))
                .finish(),
            );
            return;
        }

        let sidebar_width = (width * 0.35).max(200.0).min(320.0);

        let selected_id = self.selected_id.clone();
        let on_select = self.on_select.clone();
        let on_new = self.on_new.clone();
        let on_toggle_section = self.on_toggle_section.clone();
        let collapsed = self.collapsed_sections.clone();

        let mut sidebar = ThreadSidebar::new(self.threads.clone())
            .with_selected(selected_id.clone())
            .with_collapsed(collapsed);

        if on_select.is_some() {
            sidebar = sidebar.with_on_select(move |id| {
                if let Some(cb) = on_select.as_ref() {
                    (cb.borrow_mut())(id);
                }
            });
        }
        if on_new.is_some() {
            sidebar = sidebar.with_on_new(move || {
                if let Some(cb) = on_new.as_ref() {
                    (cb.borrow_mut())();
                }
            });
        }
        if on_toggle_section.is_some() {
            sidebar = sidebar.with_on_toggle_section(move |name, collapsed| {
                if let Some(cb) = on_toggle_section.as_ref() {
                    (cb.borrow_mut())(name, collapsed);
                }
            });
        }

        let title = self
            .threads
            .iter()
            .find(|t| t.id == selected_id)
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "Thread".to_string());
        let messages = self
            .messages_by_thread
            .get(&selected_id)
            .cloned()
            .unwrap_or_default();

        let on_send = self.on_send.clone();
        let on_action = self.on_action.clone();
        let thread_view = ThreadView::new(title, messages)
            .with_on_send(move |text| {
                if let Some(cb) = on_send.as_ref() {
                    (cb.borrow_mut())(text);
                }
            })
            .with_on_action(move |action| {
                if let Some(cb) = on_action.as_ref() {
                    (cb.borrow_mut())(action);
                }
            });

        let thread_width = (width - sidebar_width - 1.0).max(100.0);
        let mut thread_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(thread_view.finish());
        if let Some(header) = self.header.take() {
            thread_column = thread_column.with_child(header);
        }
        let thread_view = ConstrainedBox::new(thread_column.finish())
            .with_max_width(thread_width)
            .finish();

        let row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(
                ConstrainedBox::new(sidebar.finish())
                    .with_max_width(sidebar_width)
                    .finish(),
            )
            .with_child(Divider::vertical().finish())
            .with_child(thread_view);

        self.root = Some(
            Container::new(row.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
                .with_padding(EdgeInsets::uniform(spacing))
                .finish(),
        );
    }
}

impl Element for ThreadsContainer {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app, constraint.max.x);
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
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::chat_content::{ChatFragment, ChatRole};
    use crate::elements::{AppContext, LayoutContext};
    use crate::geometry::vec2f;
    use crate::views::thread_list_view::ThreadKind;

    #[test]
    fn threads_container_layouts() {
        let app = AppContext::default();
        let threads = vec![
            ThreadListEntry {
                id: "t1".to_string(),
                title: "General".to_string(),
                kind: ThreadKind::Channel,
                selected: true,
                unread_count: 0,
            },
            ThreadListEntry {
                id: "t2".to_string(),
                title: "Random".to_string(),
                kind: ThreadKind::Chat,
                selected: false,
                unread_count: 1,
            },
        ];
        let messages = vec![ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Hello")],
        )];
        let mut container = ThreadsContainer::new("t1")
            .with_threads(threads)
            .with_messages("t1", messages);
        let size = container.layout(
            SizeConstraint::loose(vec2f(800.0, 600.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

}
