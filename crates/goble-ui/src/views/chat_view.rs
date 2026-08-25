use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::chat_content::{ChatAction, ChatMessage};
use crate::elements::{
    AppContext, Axis, ChatComposer, ChatMessageBubble, Container, CrossAxisAlignment, Divider,
    EdgeInsets, Element, Expanded, Fill, Flex, LayoutContext, MainAxisAlignment, MainAxisSize,
    PaintContext, Point, PopupMenuItem, QuickActionButton, Scrollable, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

pub struct ChatView {
    header: Option<Box<dyn Element>>,
    messages: Vec<ChatMessage>,
    quick_actions: Vec<(String, Rc<RefCell<dyn FnMut() + 'static>>)>,
    on_send: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_action: Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    empty_title: Option<String>,
    empty_subtitle: Option<String>,
    composer_value: Rc<RefCell<String>>,
    composer_focused: bool,
    composer_model_label: Option<String>,
    composer_stop_visible: bool,
    on_composer_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_composer_focus_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    on_attach: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_voice: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_model: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_stop: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_profile: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    composer_model_items: Vec<PopupMenuItem>,
    composer_model_menu_open: Rc<RefCell<bool>>,
    on_select_model_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    composer_profile_items: Vec<PopupMenuItem>,
    composer_profile_menu_open: Rc<RefCell<bool>>,
    on_select_profile_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
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
            empty_title: None,
            empty_subtitle: None,
            composer_value: Rc::new(RefCell::new(String::new())),
            composer_focused: false,
            composer_model_label: None,
            composer_stop_visible: false,
            on_composer_change: None,
            on_composer_focus_change: None,
            on_attach: None,
            on_voice: None,
            on_select_model: None,
            on_stop: None,
            on_profile: None,
            composer_model_items: Vec::new(),
            composer_model_menu_open: Rc::new(RefCell::new(false)),
            on_select_model_item: None,
            composer_profile_items: Vec::new(),
            composer_profile_menu_open: Rc::new(RefCell::new(false)),
            on_select_profile_item: None,
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

    pub fn with_empty_state(
        mut self,
        title: impl Into<String>,
        subtitle: impl Into<String>,
    ) -> Self {
        self.empty_title = Some(title.into());
        self.empty_subtitle = Some(subtitle.into());
        self
    }

    pub fn composer_value(&self) -> String {
        self.composer_value.borrow().clone()
    }

    /// Sets the composer draft from external state (the snapshot). The draft is
    /// re-applied on every layout, so typing is driven by the app-owned value.
    pub fn with_composer_value(self, value: impl Into<String>) -> Self {
        *self.composer_value.borrow_mut() = value.into();
        self
    }

    pub fn with_composer_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_composer_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_composer_focused(mut self, focused: bool) -> Self {
        self.composer_focused = focused;
        self
    }

    pub fn with_composer_model_label(mut self, label: impl Into<String>) -> Self {
        self.composer_model_label = Some(label.into());
        self
    }

    pub fn with_composer_stop_visible(mut self, visible: bool) -> Self {
        self.composer_stop_visible = visible;
        self
    }

    pub fn with_composer_on_focus_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_composer_focus_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_composer_on_attach<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_attach = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_composer_on_voice<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_voice = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_composer_on_select_model<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_model = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_composer_on_stop<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_stop = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_composer_on_profile<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_profile = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the composer's model dropdown (items, app-owned open flag, select callback).
    pub fn with_composer_model_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.composer_model_items = items;
        self.composer_model_menu_open = open;
        self.on_select_model_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the composer's account/profile dropdown.
    pub fn with_composer_profile_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.composer_profile_items = items;
        self.composer_profile_menu_open = open;
        self.on_select_profile_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn build_empty_state(&self, app: &AppContext) -> Box<dyn Element> {
        let spacing = app.theme.spacing_px(SpacingToken::Sm);
        let xl = app.theme.spacing_px(SpacingToken::Xl);
        let title = self
            .empty_title
            .clone()
            .unwrap_or_else(|| "New conversation".to_string());
        let subtitle = self
            .empty_subtitle
            .clone()
            .unwrap_or_else(|| "Ask anything to get started.".to_string());

        let column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing)
            .with_child(
                Text::new(title)
                    .with_theme_color(ColorToken::Text, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .with_child(
                Text::new(subtitle)
                    .with_theme_color(ColorToken::Muted, app)
                    .with_font_size(12.0)
                    .finish(),
            )
            .finish();

        Container::new(column)
            .with_padding(EdgeInsets::new(0.0, xl, 0.0, 0.0))
            .finish()
    }

    fn rebuild(&mut self, app: &AppContext) {
        let spacing = app.theme.spacing_px(SpacingToken::Md);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_main_axis_size(MainAxisSize::Max)
            .with_spacing(spacing);

        if let Some(header) = self.header.take() {
            column = column.with_child(header);
        }

        let message_area: Box<dyn Element> = if self.messages.is_empty() {
            // Wrap in a scrollable so it fills the remaining height and pins
            // the composer to the bottom of the window (a bare container would
            // size to its content and leave a gap under the composer).
            Scrollable::new(self.build_empty_state(app), Axis::Vertical).finish()
        } else {
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
            Scrollable::new(message_column.finish(), Axis::Vertical).finish()
        };
        // The transcript consumes the remaining space so the composer pins to
        // the bottom of the chat view.
        column = column.with_child(Expanded::new(message_area).finish());

        if !self.messages.is_empty() && !self.quick_actions.is_empty() {
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
        let composer_value_for_change = self.composer_value.clone();
        let composer_value = self.composer_value.clone();
        let on_send = self.on_send.clone();
        let on_composer_change = self.on_composer_change.clone();
        let mut composer = ChatComposer::new()
            .with_value(current_value)
            .with_focused(self.composer_focused)
            .with_stop_visible(self.composer_stop_visible)
            .with_on_change(move |text| {
                *composer_value_for_change.borrow_mut() = text.clone();
                if let Some(cb) = on_composer_change.as_ref() {
                    (cb.borrow_mut())(text);
                }
            })
            .with_on_send(move |text| {
                *composer_value.borrow_mut() = String::new();
                if let Some(cb) = on_send.as_ref() {
                    (cb.borrow_mut())(text);
                }
            });
        if let Some(label) = self.composer_model_label.clone() {
            composer = composer.with_model_label(label);
        }
        if let Some(cb) = self.on_composer_focus_change.clone() {
            composer = composer.with_on_focus_change(move |focused| (cb.borrow_mut())(focused));
        }
        if let Some(cb) = self.on_attach.clone() {
            composer = composer.with_on_attach(move || (cb.borrow_mut())());
        }
        if let Some(cb) = self.on_voice.clone() {
            composer = composer.with_on_voice(move || (cb.borrow_mut())());
        }
        if let Some(cb) = self.on_select_model.clone() {
            composer = composer.with_on_select_model(move || (cb.borrow_mut())());
        }
        if let Some(cb) = self.on_stop.clone() {
            composer = composer.with_on_stop(move || (cb.borrow_mut())());
        }
        if let Some(cb) = self.on_profile.clone() {
            composer = composer.with_on_profile(move || (cb.borrow_mut())());
        }
        if let Some(cb) = self.on_select_model_item.clone() {
            composer = composer.with_model_menu(
                self.composer_model_items.clone(),
                self.composer_model_menu_open.clone(),
                move |idx| (cb.borrow_mut())(idx),
            );
        }
        if let Some(cb) = self.on_select_profile_item.clone() {
            composer = composer.with_profile_menu(
                self.composer_profile_items.clone(),
                self.composer_profile_menu_open.clone(),
                move |idx| (cb.borrow_mut())(idx),
            );
        }
        let composer = composer.finish();
        // A separator line above the rich input separates it from the
        // transcript. The composer still pins flush to the bottom of the view.
        column = column.with_child(Divider::horizontal().finish());
        column = column.with_child(composer);

        // No outer padding: the header and composer span the full width and
        // the composer pins flush to the bottom of the window.
        self.root = Some(
            Container::new(column.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
                .finish(),
        );
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

    #[test]
    fn chat_view_empty_state_layouts() {
        let app = AppContext::default();
        let mut view = ChatView::new().with_empty_state("Start chatting", "Type below");
        let size = view.layout(
            SizeConstraint::loose(vec2f(600.0, 800.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }
}
