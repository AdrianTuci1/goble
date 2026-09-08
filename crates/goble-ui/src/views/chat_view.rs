use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::elements::chat_content::{ChatAction, ChatMessage};
use crate::elements::{
    AppContext, AskUserCard, AskUserUi, Axis, Border, Button, ButtonVariant, ChatComposer,
    ChatMessageBubble, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Expanded, Fill,
    Flex, FrameView, LayoutContext, MainAxisAlignment, MainAxisSize, PaintContext, Point,
    PopupMenuItem, QuickActionButton, Scrollable, SizeConstraint, Switch, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;
use crate::theme::{ColorToken, SpacingToken};

/// A live remote-desktop frame rendered inline at the end of the transcript.
#[derive(Clone)]
struct InlineScreen {
    source: String,
    frame_seq: u64,
    width: u32,
    height: u32,
    data: Arc<[u8]>,
}

pub struct ChatView {
    header: Option<Box<dyn Element>>,
    messages: Vec<ChatMessage>,
    quick_actions: Vec<(String, Rc<RefCell<dyn FnMut() + 'static>>)>,
    on_send: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_cmd_enter: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_action: Option<Rc<RefCell<dyn FnMut(ChatAction) + 'static>>>,
    empty_title: Option<String>,
    empty_subtitle: Option<String>,
    composer_value: Rc<RefCell<String>>,
    composer_focused: bool,
    composer_model_label: Option<String>,
    composer_path: Option<String>,
    composer_stop_visible: bool,
    composer_harness_label: Option<String>,
    composer_branch_label: Option<String>,
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
    composer_harness_items: Vec<PopupMenuItem>,
    composer_harness_menu_open: Rc<RefCell<bool>>,
    on_select_harness_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    composer_dir_items: Vec<PopupMenuItem>,
    composer_dir_menu_open: Rc<RefCell<bool>>,
    on_select_dir_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    composer_branch_items: Vec<PopupMenuItem>,
    composer_branch_menu_open: Rc<RefCell<bool>>,
    on_select_branch_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    on_composer_slash: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    pending_ask: Option<AskUserUi>,
    on_answer_ask: Option<Rc<RefCell<dyn FnMut(String, Option<(String, String)>) + 'static>>>,
    on_skip_ask: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    auto_approve: bool,
    on_toggle_auto_approve: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    /// A prompt the user sent while the agent was running, queued (not
    /// interrupted) and shown inline as a pending block (warp-new model).
    queued_prompt: Option<String>,
    on_send_queued: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_dismiss_queued: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    /// A live remote desktop stream shown inline at the end of the transcript
    /// when the harness hands off control (the "harness takes over" moment).
    inline_screen: Option<InlineScreen>,
    on_close_inline_screen: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    /// A detected BYOH handoff URI rendered as a "open remote desktop" action.
    screen_link: Option<String>,
    on_open_screen_link: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
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
            on_cmd_enter: None,
            on_action: None,
            empty_title: None,
            empty_subtitle: None,
            composer_value: Rc::new(RefCell::new(String::new())),
            composer_focused: false,
            composer_model_label: None,
            composer_path: None,
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
            composer_harness_label: None,
            composer_branch_label: None,
            composer_harness_items: Vec::new(),
            composer_harness_menu_open: Rc::new(RefCell::new(false)),
            on_select_harness_item: None,
            composer_dir_items: Vec::new(),
            composer_dir_menu_open: Rc::new(RefCell::new(false)),
            on_select_dir_item: None,
            composer_branch_items: Vec::new(),
            composer_branch_menu_open: Rc::new(RefCell::new(false)),
            on_select_branch_item: None,
            on_composer_slash: None,
            pending_ask: None,
            on_answer_ask: None,
            on_skip_ask: None,
            auto_approve: false,
            on_toggle_auto_approve: None,
            queued_prompt: None,
            on_send_queued: None,
            on_dismiss_queued: None,
            inline_screen: None,
            on_close_inline_screen: None,
            screen_link: None,
            on_open_screen_link: None,
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

    /// Cmd/Ctrl+Enter in the composer starts a NEW agent conversation (warp-new).
    pub fn with_composer_on_cmd_enter<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_cmd_enter = Some(Rc::new(RefCell::new(callback)));
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

    pub fn with_composer_path(mut self, path: impl Into<String>) -> Self {
        self.composer_path = Some(path.into());
        self
    }

    pub fn with_composer_harness_label(mut self, label: impl Into<String>) -> Self {
        self.composer_harness_label = Some(label.into());
        self
    }

    pub fn with_composer_branch_label(mut self, label: impl Into<String>) -> Self {
        self.composer_branch_label = Some(label.into());
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

    /// Set the composer's harness dropdown (items, open flag, select callback).
    pub fn with_composer_harness_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.composer_harness_items = items;
        self.composer_harness_menu_open = open;
        self.on_select_harness_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the composer's working-directory dropdown.
    pub fn with_composer_dir_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.composer_dir_items = items;
        self.composer_dir_menu_open = open;
        self.on_select_dir_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the composer's git-branch dropdown.
    pub fn with_composer_branch_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.composer_branch_items = items;
        self.composer_branch_menu_open = open;
        self.on_select_branch_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired when the composer draft begins with `/` (slash command).
    pub fn with_composer_on_slash<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_composer_slash = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the pending ask rendered inline at the end of the transcript. When
    /// `Some`, the chat renders an `AskUserCard` (warp-new model) in the
    /// message stream instead of pinning a question to the composer.
    pub fn with_pending_ask(mut self, ask: Option<AskUserUi>) -> Self {
        self.pending_ask = ask;
        self
    }

    /// Fired with the composed response when the user submits the inline ask.
    pub fn with_on_answer_ask<F: FnMut(String, Option<(String, String)>) + 'static>(
        mut self,
        callback: F,
    ) -> Self {
        self.on_answer_ask = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired when the user skips the inline ask.
    pub fn with_on_skip_ask<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_skip_ask = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Whether the agent auto-approves `ask_user` questions (skip the ask and
    /// continue). Shown as a toggle in the controls strip above the composer.
    pub fn with_auto_approve(mut self, enabled: bool) -> Self {
        self.auto_approve = enabled;
        self
    }

    /// Fired when the user toggles the auto-approve switch.
    pub fn with_on_toggle_auto_approve<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_toggle_auto_approve = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// A prompt queued while the agent was busy, shown inline as a pending block.
    pub fn with_queued_prompt(mut self, prompt: Option<String>) -> Self {
        self.queued_prompt = prompt;
        self
    }

    /// Fired when the user clicks "Send now" on a queued prompt (sends it
    /// immediately and interrupts the in-flight turn).
    pub fn with_on_send_queued<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_send_queued = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired when the user dismisses a queued prompt.
    pub fn with_on_dismiss_queued<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_dismiss_queued = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set a live remote-desktop frame to render inline at the end of the
    /// transcript, marking the harness's handoff of control (computer use).
    pub fn with_inline_screen(
        mut self,
        source: impl Into<String>,
        frame_seq: u64,
        width: u32,
        height: u32,
        data: Arc<[u8]>,
    ) -> Self {
        self.inline_screen = Some(InlineScreen {
            source: source.into(),
            frame_seq,
            width,
            height,
            data,
        });
        self
    }

    /// Fired when the user closes the inline remote-desktop block.
    pub fn with_on_close_inline_screen<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_close_inline_screen = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// A detected BYOH desktop-handoff URI, rendered as an open-roremote action.
    pub fn with_screen_link(mut self, link: Option<String>) -> Self {
        self.screen_link = link;
        self
    }

    /// Fired when the user clicks the detected handoff link to open the desktop.
    pub fn with_on_open_screen_link<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_open_screen_link = Some(Rc::new(RefCell::new(callback)));
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

        let message_area: Box<dyn Element> = if self.messages.is_empty()
            && self.pending_ask.is_none()
            && self.inline_screen.is_none()
            && self.screen_link.is_none()
        {
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
                    .with_tool_calls(message.tool_calls.clone())
                    .with_on_action(move |action| {
                        if let Some(cb) = on_action.as_ref() {
                            (cb.borrow_mut())(action);
                        }
                    })
                    .finish();
                message_column = message_column.with_child(bubble);
            }
            // A pending ask sits inline at the end of the transcript (warp-new
            // model) so it scrolls with the conversation rather than pinning to
            // the bottom of the window.
            if let Some(ask) = &self.pending_ask {
                let mut card = AskUserCard::new(ask.question.clone(), ask.quick_replies.clone());
                if let Some(cb) = self.on_answer_ask.clone() {
                    card = card.with_on_answer(move |resp, cred| (cb.borrow_mut())(resp, cred));
                }
                if let Some(cb) = self.on_skip_ask.clone() {
                    card = card.with_on_skip(move || (cb.borrow_mut())());
                }
                message_column = message_column.with_child(card.finish());
            }
            // A prompt queued while the agent was running renders as a pending
            // block in the transcript (warp-new model) with "Send now" + dismiss.
            if let Some(prompt) = &self.queued_prompt {
                let sm = app.theme.spacing_px(SpacingToken::Sm);
                let md = app.theme.spacing_px(SpacingToken::Md);
                let mut header = Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new("Pending message")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    );
                if let Some(cb) = self.on_dismiss_queued.clone() {
                    let dismiss = Button::new(
                        Text::new("✕")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_variant(ButtonVariant::Ghost)
                    .with_on_click(move || (cb.borrow_mut())())
                    .finish();
                    header = header.with_child(dismiss);
                }
                let mut footer = Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::End);
                if let Some(cb) = self.on_send_queued.clone() {
                    let send_now = Button::new(
                        Text::new("Send now")
                            .with_theme_color(ColorToken::Bg, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_variant(ButtonVariant::Primary)
                    .with_on_click(move || (cb.borrow_mut())())
                    .finish();
                    footer = footer.with_child(send_now);
                }
                let queued_card = Container::new(
                    Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .with_spacing(sm)
                        .with_child(header.finish())
                        .with_child(
                            Text::new(prompt.clone())
                                .with_theme_color(ColorToken::Text, app)
                                .with_font_size(12.0)
                                .finish(),
                        )
                        .with_child(footer.finish())
                        .finish(),
                )
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .with_border(
                    Border::all(1.0)
                        .with_border_fill(Fill::Solid(app.theme.color(ColorToken::Border))),
                )
                .with_padding(EdgeInsets::uniform(md))
                .with_corner_radius(8.0)
                .finish();
                message_column = message_column.with_child(queued_card);
            }
            // A live remote-desktop handoff renders inline at the end of the
            // transcript (the harness takes over control). It scrolls with the
            // conversation and carries a close button to clear it.
            if let Some(screen) = &self.inline_screen {
                let md = app.theme.spacing_px(SpacingToken::Md);
                let sm = app.theme.spacing_px(SpacingToken::Sm);
                let mut header = Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Text::new("Component desktop · harness in control")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    );
                if let Some(cb) = self.on_close_inline_screen.clone() {
                    let close = Button::new(
                        Text::new("✕")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_variant(ButtonVariant::Ghost)
                    .with_on_click(move || (cb.borrow_mut())())
                    .finish();
                    header = header.with_child(close);
                }
                let frame = FrameView::new(
                    screen.source.clone(),
                    screen.frame_seq,
                    screen.width,
                    screen.height,
                    Arc::clone(&screen.data),
                );
                let block = Container::new(
                    Flex::column()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .with_spacing(sm)
                        .with_child(header.finish())
                        .with_child(frame.finish())
                        .finish(),
                )
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .with_border(Border::all(1.0).with_border_fill(
                    Fill::Solid(app.theme.color(ColorToken::Border)),
                ))
                .with_padding(EdgeInsets::uniform(md))
                .with_corner_radius(8.0)
                .finish();
                message_column = message_column.with_child(block);
            }
            // A detected BYOH handoff hyperlink (no live frame yet): show a
            // clickable affordance so the user can take over the harness's
            // desktop.
            if self.inline_screen.is_none() {
                if let Some(link) = &self.screen_link {
                    let md = app.theme.spacing_px(SpacingToken::Md);
                    let sm = app.theme.spacing_px(SpacingToken::Sm);
                    let mut row = Flex::row()
                        .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_child(
                            Text::new("Harness handed off a desktop")
                                .with_theme_color(ColorToken::Muted, app)
                                .with_font_size(12.0)
                                .finish(),
                        );
                    if let Some(cb) = self.on_open_screen_link.clone() {
                        let link_for_cb = link.clone();
                        let open = Button::new(
                            Text::new("Open remote desktop")
                                .with_theme_color(ColorToken::Bg, app)
                                .with_font_size(12.0)
                                .finish(),
                        )
                        .with_variant(ButtonVariant::Primary)
                        .with_on_click(move || (cb.borrow_mut())(link_for_cb.clone()))
                        .finish();
                        row = row.with_child(open);
                    }
                    let card = Container::new(
                        Flex::column()
                            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                            .with_spacing(sm)
                            .with_child(row.finish())
                            .finish(),
                    )
                    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                    .with_border(Border::all(1.0).with_border_fill(
                        Fill::Solid(app.theme.color(ColorToken::Border)),
                    ))
                    .with_padding(EdgeInsets::uniform(md))
                    .with_corner_radius(8.0)
                    .finish();
                    message_column = message_column.with_child(card);
                }
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

        // Control strip above the rich input: the auto-approve toggle. Stop
        // lives in the composer footer (visible while the agent is streaming).
        if let Some(cb) = self.on_toggle_auto_approve.clone() {
            let sm = app.theme.spacing_px(SpacingToken::Sm);
            let md = app.theme.spacing_px(SpacingToken::Md);
            let switch = Switch::new()
                .with_checked(self.auto_approve)
                .with_size(Vector2F::new(36.0, 20.0))
                .with_on_change(move |checked| (cb.borrow_mut())(checked))
                .finish();
            let controls = Container::new(
                Flex::row()
                    .with_main_axis_alignment(MainAxisAlignment::Start)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(sm)
                    .with_child(
                        Text::new("Auto-approve")
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_child(switch)
                    .finish(),
            )
            .with_padding(EdgeInsets::new(0.0, md, 0.0, 0.0))
            .finish();
            column = column.with_child(controls);
        }

        let current_value = self.composer_value.borrow().clone();
        let composer_value_for_change = self.composer_value.clone();
        let composer_value = self.composer_value.clone();
        let composer_value_cmd = self.composer_value.clone();
        let on_send = self.on_send.clone();
        let on_cmd_enter = self.on_cmd_enter.clone();
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
            })
            .with_on_cmd_enter(move |text| {
                *composer_value_cmd.borrow_mut() = String::new();
                if let Some(cb) = on_cmd_enter.as_ref() {
                    (cb.borrow_mut())(text);
                }
            });
        if let Some(label) = self.composer_model_label.clone() {
            composer = composer.with_model_label(label);
        }
        if let Some(path) = self.composer_path.clone() {
            composer = composer.with_path_label(path);
        }
        if let Some(label) = self.composer_harness_label.clone() {
            composer = composer.with_harness_label(label);
        }
        if let Some(label) = self.composer_branch_label.clone() {
            composer = composer.with_branch_label(label);
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
        if let Some(cb) = self.on_select_harness_item.clone() {
            composer = composer.with_harness_menu(
                self.composer_harness_items.clone(),
                self.composer_harness_menu_open.clone(),
                move |idx| (cb.borrow_mut())(idx),
            );
        }
        if let Some(cb) = self.on_select_dir_item.clone() {
            composer = composer.with_dir_menu(
                self.composer_dir_items.clone(),
                self.composer_dir_menu_open.clone(),
                move |idx| (cb.borrow_mut())(idx),
            );
        }
        if let Some(cb) = self.on_select_branch_item.clone() {
            composer = composer.with_branch_menu(
                self.composer_branch_items.clone(),
                self.composer_branch_menu_open.clone(),
                move |idx| (cb.borrow_mut())(idx),
            );
        }
        if let Some(cb) = self.on_composer_slash.clone() {
            composer = composer.with_on_slash(move || (cb.borrow_mut())());
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

    #[test]
    fn chat_view_renders_inline_screen_as_image() {
        use crate::test_util::{command_counts, render_element};

        let app = AppContext::default();
        let pixels = vec![0u8; 200 * 120 * 4];
        let mut view = ChatView::new()
            .with_inline_screen("inline-1", 1, 200, 120, Arc::from(pixels))
            .finish();
        let commands = render_element(&mut view, vec2f(600.0, 800.0), &app);
        let counts = command_counts(&commands);
        assert!(
            counts.draw_image > 0,
            "a live inline screen should paint the frame image"
        );
        assert!(
            counts.draw_text > 0,
            "the harness-in-control header should render text"
        );
    }

    #[test]
    fn chat_view_renders_screen_link_card_without_frame() {
        use crate::test_util::{command_counts, render_element};

        let app = AppContext::default();
        let mut view = ChatView::new()
            .with_screen_link(Some("rdp://host:3389".to_string()))
            .finish();
        let commands = render_element(&mut view, vec2f(600.0, 800.0), &app);
        let counts = command_counts(&commands);
        assert_eq!(
            counts.draw_image, 0,
            "no live frame is drawn when only a URI link is present"
        );
        assert!(
            counts.draw_text > 0,
            "the handoff card should render a label and action"
        );
    }
}
