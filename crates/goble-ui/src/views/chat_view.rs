use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::chat_content::{ChatAction, ChatMessage};
use crate::elements::{
    AppContext, Axis, ChatComposer, ChatMessageBubble, Container, CrossAxisAlignment, EdgeInsets,
    Element, Fill, Flex, LayoutContext, MainAxisAlignment, PaintContext, Point, QuickActionButton,
    Scrollable, SizeConstraint,
};
use crate::elements::{ChatSidebar, ChatSidebarTab};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub struct ChatView {
    header: Option<Box<dyn Element>>,
    messages: Vec<ChatMessage>,
    quick_actions: Vec<(String, Rc<RefCell<dyn FnMut() + 'static>>)>,
    on_send: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_action: Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    composer_value: Rc<RefCell<String>>,
    model_options: Vec<String>,
    selected_model: Option<String>,
    on_model_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    runtime_options: Vec<String>,
    selected_runtime: Option<String>,
    on_runtime_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    variant_options: Vec<String>,
    selected_variant: Option<String>,
    on_variant_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    right_sidebar: Option<Box<dyn Element>>,
    sidebar_tab: ChatSidebarTab,
    on_sidebar_tab_change: Option<Rc<RefCell<dyn FnMut(ChatSidebarTab) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatView {
    pub fn new() -> Self {
        Self {
            header: None,
            messages: Vec::new(),
            quick_actions: Vec::new(),
            on_send: None,
            on_action: None,
            composer_value: Rc::new(RefCell::new(String::new())),
            model_options: Vec::new(),
            selected_model: None,
            on_model_change: None,
            runtime_options: Vec::new(),
            selected_runtime: None,
            on_runtime_change: None,
            variant_options: Vec::new(),
            selected_variant: None,
            on_variant_change: None,
            right_sidebar: None,
            sidebar_tab: ChatSidebarTab::Info,
            on_sidebar_tab_change: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_header(mut self, header: Box<dyn Element>) -> Self {
        self.header = Some(header);
        self
    }

    pub fn with_messages(mut self, messages: Vec<ChatMessage>) -> Self {
        self.messages = messages;
        self
    }

    pub fn with_quick_action<F: FnMut() + 'static>(
        mut self,
        label: impl Into<String>,
        callback: F,
    ) -> Self {
        self.quick_actions
            .push((label.into(), Rc::new(RefCell::new(callback))));
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

    pub fn with_model_options(mut self, options: Vec<String>, selected: Option<String>) -> Self {
        self.model_options = options;
        self.selected_model = selected;
        self
    }

    pub fn with_on_model_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_model_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_runtime_options(mut self, options: Vec<String>, selected: Option<String>) -> Self {
        self.runtime_options = options;
        self.selected_runtime = selected;
        self
    }

    pub fn with_on_runtime_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_runtime_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_variant_options(mut self, options: Vec<String>, selected: Option<String>) -> Self {
        self.variant_options = options;
        self.selected_variant = selected;
        self
    }

    pub fn with_on_variant_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_variant_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_right_sidebar(
        mut self,
        sidebar: Box<dyn Element>,
        tab: ChatSidebarTab,
        on_tab_change: impl FnMut(ChatSidebarTab) + 'static,
    ) -> Self {
        self.right_sidebar = Some(sidebar);
        self.sidebar_tab = tab;
        self.on_sidebar_tab_change = Some(Rc::new(RefCell::new(on_tab_change)));
        self
    }

    pub fn composer_value(&self) -> String {
        self.composer_value.borrow().clone()
    }

    fn rebuild(&mut self, app: &AppContext) {
        let padding = app.theme.spacing_px(SpacingToken::Md);
        let spacing = app.theme.spacing_px(SpacingToken::Md);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);

        if let Some(header) = self.header.take() {
            column = column.with_child(header);
        }

        let mut message_column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);
        for message in &self.messages {
            let on_action = self.on_action.clone();
            let fragments = message.fragments.clone();
            let bubble = ChatMessageBubble::new(message.role, fragments)
                .with_on_action(move |action| {
                    if let Some(cb) = on_action.as_ref() {
                        (cb.borrow_mut())(action);
                    }
                })
                .finish();
            message_column = message_column.with_child(bubble);
        }
        column =
            column.with_child(Scrollable::new(message_column.finish(), Axis::Vertical).finish());

        if !self.quick_actions.is_empty() {
            let mut row = Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::Start)
                .with_spacing(app.theme.spacing_px(SpacingToken::Sm));
            for (label, cb) in &self.quick_actions {
                let cb = cb.clone();
                row = row.with_child(
                    QuickActionButton::new(label.clone(), move || (cb.borrow_mut())()).finish(),
                );
            }
            column = column.with_child(row.finish());
        }

        let current_value = self.composer_value.borrow().clone();
        let composer_value = self.composer_value.clone();
        let on_send = self.on_send.clone();
        let on_model_change = self.on_model_change.clone();
        let on_runtime_change = self.on_runtime_change.clone();
        let on_variant_change = self.on_variant_change.clone();
        let mut composer = ChatComposer::new()
            .with_value(current_value)
            .with_model_options(self.model_options.clone(), self.selected_model.clone())
            .with_runtime_options(self.runtime_options.clone(), self.selected_runtime.clone())
            .with_variant_options(self.variant_options.clone(), self.selected_variant.clone())
            .with_on_send(move |text| {
                *composer_value.borrow_mut() = String::new();
                if let Some(cb) = on_send.as_ref() {
                    (cb.borrow_mut())(text);
                }
            });
        if let Some(cb) = on_model_change {
            composer = composer.with_on_model_change(move |value| {
                (cb.borrow_mut())(value);
            });
        }
        if let Some(cb) = on_runtime_change {
            composer = composer.with_on_runtime_change(move |value| {
                (cb.borrow_mut())(value);
            });
        }
        if let Some(cb) = on_variant_change {
            composer = composer.with_on_variant_change(move |value| {
                (cb.borrow_mut())(value);
            });
        }
        let composer = composer.finish();
        column = column.with_child(composer);

        let content = Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_padding(EdgeInsets::uniform(padding))
            .finish();

        self.root = Some(if let Some(sidebar) = self.right_sidebar.take() {
            let on_tab_change = self.on_sidebar_tab_change.clone();
            let sidebar_tab = self.sidebar_tab;
            let sidebar = ChatSidebar::new(sidebar_tab)
                .with_info_content(sidebar)
                .with_on_change_tab(move |tab| {
                    if let Some(cb) = on_tab_change.as_ref() {
                        (cb.borrow_mut())(tab);
                    }
                })
                .finish();
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(content)
                .with_child(sidebar)
                .finish()
        } else {
            content
        });
    }
}

impl Default for ChatView {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for ChatView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app);
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
    use crate::elements::LayoutContext;
    use crate::geometry::vec2f;

    #[test]
    fn chat_view_layouts_with_messages() {
        let app = AppContext::default();
        let messages = vec![ChatMessage::new(
            ChatRole::Assistant,
            vec![ChatFragment::text("Hi")],
        )];
        let mut view = ChatView::new().with_messages(messages);
        let size = view.layout(
            SizeConstraint::loose(vec2f(600.0, 800.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn chat_view_renders_markdown_message() {
        let app = AppContext::default();
        let messages = vec![ChatMessage::from_markdown(
            ChatRole::Assistant,
            "**bold** and `code`",
        )];
        let mut view = ChatView::new().with_messages(messages);
        let size = view.layout(
            SizeConstraint::loose(vec2f(600.0, 800.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }
}
