use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Chip, Clipped, Container, CrossAxisAlignment, EdgeInsets, Element,
    Expanded, Flex, Icon, LayoutContext, MainAxisAlignment, MainAxisSize, PaintContext,
    Padding, Point, PopupMenu, PopupMenuItem, PopupMenuPosition, SizeConstraint, Text, TextArea,
    Tooltip, TooltipPosition,
};
use crate::event::{DispatchedEvent, ModifiersState};
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

pub struct ChatComposer {
    value: Rc<RefCell<String>>,
    placeholder: String,
    attachments: Vec<String>,
    model_label: Option<String>,
    path_label: Option<String>,
    focused: bool,
    stop_visible: bool,
    /// warp-new style context pills: the agent harness, the working directory
    /// and the git branch, each with its own dropdown selector. The working
    /// directory reuses `path_label` (set via `with_path_label`) as its label.
    harness_label: Option<String>,
    harness_menu_items: Vec<PopupMenuItem>,
    harness_menu_open: Rc<RefCell<bool>>,
    on_select_harness_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    dir_menu_items: Vec<PopupMenuItem>,
    dir_menu_open: Rc<RefCell<bool>>,
    on_select_dir_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    branch_label: Option<String>,
    branch_menu_items: Vec<PopupMenuItem>,
    branch_menu_open: Rc<RefCell<bool>>,
    on_select_branch_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    on_slash: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_change: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_send: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    /// Cmd/Ctrl+Enter submit: start a NEW agent conversation (warp-new). The
    /// host decides what "new conversation" means; the composer only reports
    /// the keybinding.
    on_cmd_enter: Option<Rc<RefCell<dyn FnMut(String) + 'static>>>,
    on_attach: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_model: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_key: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_select_variant: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_voice: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_image: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_code: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_link: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_stop: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    on_focus_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    on_profile: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    model_menu_items: Vec<PopupMenuItem>,
    model_menu_open: Rc<RefCell<bool>>,
    on_select_model_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    profile_menu_items: Vec<PopupMenuItem>,
    profile_menu_open: Rc<RefCell<bool>>,
    on_select_profile_item: Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ChatComposer {
    pub fn new() -> Self {
        Self {
            value: Rc::new(RefCell::new(String::new())),
            placeholder: String::from("Ask anything..."),
            attachments: Vec::new(),
            model_label: None,
            path_label: None,
            focused: false,
            stop_visible: false,
            on_change: None,
            on_send: None,
            on_cmd_enter: None,
            on_attach: None,
            on_select_model: None,
            on_select_key: None,
            on_select_variant: None,
            on_voice: None,
            on_image: None,
            on_code: None,
            on_link: None,
            on_stop: None,
            on_focus_change: None,
            on_profile: None,
            model_menu_items: Vec::new(),
            model_menu_open: Rc::new(RefCell::new(false)),
            on_select_model_item: None,
            profile_menu_items: Vec::new(),
            profile_menu_open: Rc::new(RefCell::new(false)),
            on_select_profile_item: None,
            harness_label: None,
            harness_menu_items: Vec::new(),
            harness_menu_open: Rc::new(RefCell::new(false)),
            on_select_harness_item: None,
            dir_menu_items: Vec::new(),
            dir_menu_open: Rc::new(RefCell::new(false)),
            on_select_dir_item: None,
            branch_label: None,
            branch_menu_items: Vec::new(),
            branch_menu_open: Rc::new(RefCell::new(false)),
            on_select_branch_item: None,
            on_slash: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_value(self, value: impl Into<String>) -> Self {
        *self.value.borrow_mut() = value.into();
        self
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn with_attachments(mut self, attachments: Vec<String>) -> Self {
        self.attachments = attachments;
        self
    }

    pub fn with_model_label(mut self, label: impl Into<String>) -> Self {
        self.model_label = Some(label.into());
        self
    }

    pub fn with_path_label(mut self, path: impl Into<String>) -> Self {
        self.path_label = Some(path.into());
        self
    }

    /// Label for the warp-new "harness" context pill (the agent/harness the
    /// current turn runs on). Shown as the left-most pill in the footer.
    pub fn with_harness_label(mut self, label: impl Into<String>) -> Self {
        self.harness_label = Some(label.into());
        self
    }

    /// Set the harness dropdown: items, the app-owned `open` flag, and a
    /// select callback (same contract as `with_model_menu`).
    pub fn with_harness_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.harness_menu_items = items;
        self.harness_menu_open = open;
        self.on_select_harness_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the working-directory dropdown (items, open flag, select callback).
    pub fn with_dir_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.dir_menu_items = items;
        self.dir_menu_open = open;
        self.on_select_dir_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Label for the git-branch context pill.
    pub fn with_branch_label(mut self, label: impl Into<String>) -> Self {
        self.branch_label = Some(label.into());
        self
    }

    /// Set the git-branch dropdown (items, open flag, select callback).
    pub fn with_branch_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.branch_menu_items = items;
        self.branch_menu_open = open;
        self.on_select_branch_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Fired when the user begins typing a slash command (the draft starts
    /// with `/`), so the host can open the command palette (grok-build style).
    pub fn with_on_slash<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_slash = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_stop_visible(mut self, visible: bool) -> Self {
        self.stop_visible = visible;
        self
    }

    pub fn with_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_send<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_send = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Cmd/Ctrl+Enter submits the draft as a NEW agent conversation (warp-new).
    pub fn with_on_cmd_enter<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_cmd_enter = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_attach<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_attach = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_model<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_model = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_key<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_key = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_select_variant<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_select_variant = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_voice<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_voice = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_image<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_image = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_code<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_code = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_link<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_link = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_stop<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_stop = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_focus_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_focus_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_profile<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_profile = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the model dropdown: `items` to show, the app-owned `open` flag (so
    /// open state survives the per-frame rebuild), and a select callback.
    pub fn with_model_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.model_menu_items = items;
        self.model_menu_open = open;
        self.on_select_model_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    /// Set the account/profile dropdown (same contract as `with_model_menu`).
    pub fn with_profile_menu<F: FnMut(usize) + 'static>(
        mut self,
        items: Vec<PopupMenuItem>,
        open: Rc<RefCell<bool>>,
        callback: F,
    ) -> Self {
        self.profile_menu_items = items;
        self.profile_menu_open = open;
        self.on_select_profile_item = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn value(&self) -> String {
        self.value.borrow().clone()
    }

    pub fn placeholder(&self) -> &str {
        &self.placeholder
    }

    pub fn attachments(&self) -> &[String] {
        &self.attachments
    }

    /// Build a warp-new context pill (icon + label + chevron) for the harness
    /// and branch selectors. Content-sized; when `items` is non-empty it is
    /// wrapped in the shared [`PopupMenu`] so the app-owned `open` flag survives
    /// the per-frame rebuild, otherwise it renders as a plain pill.
    fn context_pill(
        &self,
        app: &AppContext,
        icon: &'static str,
        label: &str,
        items: &[PopupMenuItem],
        open: Rc<RefCell<bool>>,
        on_select: &Option<Rc<RefCell<dyn FnMut(usize) + 'static>>>,
        tooltip: &str,
    ) -> Box<dyn Element> {
        let child = || {
            Flex::row()
                .with_spacing(6.0)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Icon::new(icon)
                        .with_size(14.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_child(
                    Text::new(label.to_string())
                        .with_theme_color(ColorToken::Muted, app)
                        .with_font_size(12.0)
                        .with_max_lines(1)
                        .finish(),
                )
                .with_child(
                    Icon::new("chevron-down")
                        .with_size(14.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .finish()
        };
        let trigger = Tooltip::new(ComposerButton::new(child()).with_height(28.0).finish(), tooltip)
            .with_position(TooltipPosition::Above)
            .finish();
        if !items.is_empty() {
            let mut menu = PopupMenu::new(trigger, items.to_vec())
                .with_open(open)
                .with_position(PopupMenuPosition::Above);
            if let Some(cb) = on_select.as_ref() {
                let cb = cb.clone();
                menu = menu.with_on_select(move |idx| (cb.borrow_mut())(idx));
            }
            menu.finish()
        } else {
            trigger
        }
    }

    fn rebuild(&mut self, app: &AppContext) {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let md = app.theme.spacing_px(SpacingToken::Md);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm);

        if !self.attachments.is_empty() {
            let mut attachment_row = Flex::row().with_spacing(sm);
            for attachment in &self.attachments {
                let chip = Chip::new(
                    Text::new(attachment.clone())
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .finish();
                attachment_row = attachment_row.with_child(chip);
            }
            column = column.with_child(attachment_row.finish());
        }

        // Send closure shared between Enter-to-submit and the (removed) send
        // button path; keeps Enter-to-send working. The submit carries the
        // Enter key's modifiers so the host can route a plain Enter (terminal
        // command) vs Cmd/Ctrl+Enter (a NEW agent conversation, warp-new).
        let value_for_send = self.value.clone();
        let on_send = self.on_send.clone();
        let on_cmd_enter = self.on_cmd_enter.clone();
        let send = Rc::new(RefCell::new(move |modifiers: ModifiersState| {
            let text = value_for_send.borrow().clone();
            if !text.is_empty() {
                let agent_submit = (modifiers.command || modifiers.ctrl) && !modifiers.shift;
                if agent_submit {
                    if let Some(cb) = on_cmd_enter.as_ref() {
                        (cb.borrow_mut())(text.clone());
                    }
                } else if let Some(cb) = on_send.as_ref() {
                    (cb.borrow_mut())(text.clone());
                }
                *value_for_send.borrow_mut() = String::new();
            }
        }));

        // The textarea fills the whole composer width and is visually part of
        // the rich-input bar (no separate box).
        let value = self.value.clone();
        let on_change = self.on_change.clone();
        let on_focus_change = self.on_focus_change.clone();
        let on_slash = self.on_slash.clone();
        let send_for_submit = send.clone();
        let textarea = TextArea::new()
            .with_value(self.value.borrow().clone())
            .with_placeholder(self.placeholder.clone())
            .with_min_height(80.0)
            .with_focused(self.focused)
            .with_on_change(move |text| {
                *value.borrow_mut() = text.clone();
                if text.trim_start().starts_with('/') {
                    if let Some(cb) = on_slash.as_ref() {
                        (cb.borrow_mut())();
                    }
                }
                if let Some(cb) = on_change.as_ref() {
                    (cb.borrow_mut())(text);
                }
            })
            .with_on_focus_change(move |focused| {
                if let Some(cb) = on_focus_change.as_ref() {
                    (cb.borrow_mut())(focused);
                }
            })
            .with_on_submit(move |mods| (send_for_submit.borrow_mut())(mods))
            .finish();
        column = column.with_child(textarea);

        // Footer: attach (+) on the left; model, profile (and stop while
        // streaming) on the right. No send button. The footer is bounded to the
        // composer width so the left group can flex and the path label clips.
        let mut footer = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);

        let mut left_group = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm);
        // Harness context pill (the agent/harness the turn runs on).
        if let Some(label) = self.harness_label.clone() {
            left_group = left_group.with_child(self.context_pill(
                app,
                "computer",
                &label,
                &self.harness_menu_items,
                self.harness_menu_open.clone(),
                &self.on_select_harness_item,
                "Select environment",
            ));
        }
        // Working-directory pill: fills the remaining left-group width so a
        // long path ellipsizes instead of overflowing the composer.
        if let Some(path) = self.path_label.clone() {
            let dir_child = || {
                Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_spacing(6.0)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Icon::new("folder")
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        Expanded::new(
                            Clipped::new(
                                Text::new(path.clone())
                                    .with_theme_color(ColorToken::Muted, app)
                                    .with_font_size(11.0)
                                    .with_max_lines(1)
                                    .finish(),
                            )
                            .finish(),
                        )
                        .finish(),
                    )
                    .with_child(
                        Icon::new("chevron-down")
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .finish()
            };
            let trigger = Tooltip::new(
                ComposerButton::new(dir_child()).with_height(28.0).finish(),
                "Select working directory",
            )
            .with_position(TooltipPosition::Above)
            .finish();
            let dir = if !self.dir_menu_items.is_empty() {
                let mut menu = PopupMenu::new(trigger, self.dir_menu_items.clone())
                    .with_open(self.dir_menu_open.clone())
                    .with_position(PopupMenuPosition::Above);
                if let Some(cb) = self.on_select_dir_item.clone() {
                    menu = menu.with_on_select(move |idx| (cb.borrow_mut())(idx));
                }
                menu.finish()
            } else {
                trigger
            };
            left_group = left_group.with_child(Expanded::new(dir).finish());
        }
        // Git-branch context pill.
        if let Some(label) = self.branch_label.clone() {
            left_group = left_group.with_child(self.context_pill(
                app,
                "git-branch",
                &label,
                &self.branch_menu_items,
                self.branch_menu_open.clone(),
                &self.on_select_branch_item,
                "Select branch",
            ));
        }
        if let Some(cb) = self.on_attach.clone() {
            let attach = ComposerButton::new(
                Icon::new("plus")
                    .with_size(16.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .with_height(28.0)
            .with_on_click(move || (cb.borrow_mut())())
            .finish();
            left_group = left_group.with_child(
                Tooltip::new(attach, "Attach")
                    .with_position(TooltipPosition::Above)
                    .finish(),
            );
        }
        footer = footer.with_child(Expanded::new(left_group.finish()).finish());

        let mut right_group = Flex::row().with_spacing(sm);
        if let Some(label) = self.model_label.clone() {
            let model_child = || {
                Flex::row()
                    .with_spacing(6.0)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_child(
                        Icon::new("sparkle")
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .with_child(
                        Text::new(label.clone())
                            .with_theme_color(ColorToken::Muted, app)
                            .with_font_size(12.0)
                            .finish(),
                    )
                    .with_child(
                        Icon::new("chevron-down")
                            .with_size(14.0)
                            .with_theme_color(ColorToken::Muted, app)
                            .finish(),
                    )
                    .finish()
            };
            if !self.model_menu_items.is_empty() {
                let trigger = Tooltip::new(
                    ComposerButton::new(model_child()).with_height(28.0).finish(),
                    "Select model",
                )
                .with_position(TooltipPosition::Above)
                .finish();
                let mut menu = PopupMenu::new(trigger, self.model_menu_items.clone())
                    .with_open(self.model_menu_open.clone())
                    .with_position(PopupMenuPosition::Above);
                if let Some(cb) = self.on_select_model_item.clone() {
                    menu = menu.with_on_select(move |idx| (cb.borrow_mut())(idx));
                }
                right_group = right_group.with_child(menu.finish());
            } else if let Some(cb) = self.on_select_model.clone() {
                let model = ComposerButton::new(model_child())
                    .with_height(28.0)
                    .with_on_click(move || (cb.borrow_mut())())
                    .finish();
                right_group = right_group.with_child(
                    Tooltip::new(model, "Select model")
                        .with_position(TooltipPosition::Above)
                        .finish(),
                );
            }
        }
        let profile_child = || {
            Icon::new("user")
                .with_size(16.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish()
        };
        if !self.profile_menu_items.is_empty() {
            let trigger = Tooltip::new(
                ComposerButton::new(profile_child()).with_height(28.0).finish(),
                "Account",
            )
            .with_position(TooltipPosition::Above)
            .finish();
            let mut menu = PopupMenu::new(trigger, self.profile_menu_items.clone())
                .with_open(self.profile_menu_open.clone())
                .with_position(PopupMenuPosition::Above);
            if let Some(cb) = self.on_select_profile_item.clone() {
                menu = menu.with_on_select(move |idx| (cb.borrow_mut())(idx));
            }
            right_group = right_group.with_child(menu.finish());
        } else if let Some(cb) = self.on_profile.clone() {
            let profile = ComposerButton::new(profile_child())
                .with_height(28.0)
                .with_on_click(move || (cb.borrow_mut())())
                .finish();
            right_group = right_group.with_child(
                Tooltip::new(profile, "Account")
                    .with_position(TooltipPosition::Above)
                    .finish(),
            );
        }
        if self.stop_visible {
            if let Some(cb) = self.on_stop.clone() {
                let stop = ComposerButton::new(
                    Icon::new("stop")
                        .with_size(16.0)
                        .with_theme_color(ColorToken::Error, app)
                        .finish(),
                )
                .with_height(28.0)
                .with_on_click(move || (cb.borrow_mut())())
                .finish();
                right_group = right_group.with_child(
                    Tooltip::new(stop, "Stop")
                        .with_position(TooltipPosition::Above)
                        .finish(),
                );
            }
        }
        footer = footer.with_child(right_group.finish());
        column = column.with_child(footer.finish());

        // The composer card keeps only its gutters/padding; the raised
        // background and 1px border are dropped so the rich input has no gray
        // inset box behind the textarea and pills.
        let card = Container::new(column.finish())
            .with_padding(EdgeInsets::new(md, md, md, sm))
            .with_corner_radius(8.0)
            .finish();
        self.root = Some(Padding::new(card, EdgeInsets::new(md, sm, md, md)).finish());
    }
}

impl Default for ChatComposer {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for ChatComposer {
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

/// A refined, low-emphasis pill button used in the composer footer.
///
/// Mirrors the warp-new button style: transparent by default, a subtle rounded
/// hover overlay, and a fixed height with auto width for icon+label content.
struct ComposerButton {
    child: Box<dyn Element>,
    state: InteractiveState,
    height: f32,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ComposerButton {
    fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            state: InteractiveState::default(),
            height: 28.0,
            on_click: None,
            size: None,
            origin: None,
        }
    }

    fn with_height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }
}

impl Element for ComposerButton {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let h_pad = 10.0;
        let inner_max = vec2f(
            (constraint.max.x - h_pad * 2.0).max(0.0),
            (self.height - 4.0).max(0.0),
        );
        let child_size = self
            .child
            .layout(SizeConstraint::new(vec2f(0.0, 0.0), inner_max), ctx, app);
        let size = vec2f(child_size.x + h_pad * 2.0, self.height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let rect = rectf(origin.x, origin.y, size.x, size.y);
        let hovered = ctx.hovered(rect);
        if let Some(renderer) = ctx.renderer.as_mut() {
            // Rich-input pills are flat: no filled box or 1px border by default,
            // so they do not look inset inside the composer. Only a rounded
            // hover overlay is drawn so the trigger still gives feedback.
            if hovered {
                renderer.fill_rounded_rect(rect, app.theme.color(ColorToken::Hover), 6.0);
            }
        }
        let child_size = self.child.size().unwrap_or(Vector2F::zero());
        let offset = vec2f(
            (size.x - child_size.x).max(0.0) / 2.0,
            (size.y - child_size.y).max(0.0) / 2.0,
        );
        self.child.paint(origin + offset, ctx, app);
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
    use crate::elements::LayoutContext;
    use crate::geometry::vec2f;

    #[test]
    fn composer_layouts_non_zero() {
        let app = AppContext::default();
        let mut composer = ChatComposer::new();
        let size = composer.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
    }

    #[test]
    fn composer_keeps_value_and_attachments() {
        let app = AppContext::default();
        let mut composer = ChatComposer::new()
            .with_value("hello")
            .with_attachments(vec!["doc.md".to_string()]);

        let size = composer.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );

        assert!(size.x > 0.0);
        assert!(size.y > 0.0);
        assert_eq!(composer.value(), "hello");
        assert_eq!(composer.attachments(), &["doc.md".to_string()]);
    }

    #[test]
    fn composer_renders_model_profile_attach_and_stop_pills() {
        use crate::elements::PaintContext;
        use crate::render::{RenderCommand, Renderer};

        let app = AppContext::default();
        let mut composer = ChatComposer::new()
            .with_model_label("gpt-4o")
            .with_on_attach(|| {})
            .with_on_select_model(|| {})
            .with_on_profile(|| {})
            .with_on_stop(|| {})
            .with_stop_visible(true);

        let size = composer.layout(
            SizeConstraint::loose(vec2f(400.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);

        let mut paint_ctx = PaintContext::new(Renderer::new());
        composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();

        let icons: Vec<String> = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        for expected in ["sparkle", "user", "plus", "stop"] {
            assert!(icons.iter().any(|n| n == expected), "missing icon {expected}");
        }

        // Each pill draws a surface + a 1px border.
        // Rich-input pills are flat (no gray inset box), so no borders are drawn.
        let strokes = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        assert_eq!(strokes, 0, "expected flat pills without border boxes, got {strokes}");
    }

    #[test]
    fn composer_renders_harness_dir_branch_pills() {
        use crate::elements::PaintContext;
        use crate::render::{RenderCommand, Renderer};

        let app = AppContext::default();
        let mut composer = ChatComposer::new()
            .with_harness_label("grok build")
            .with_path_label("/work/project")
            .with_branch_label("main")
            .with_harness_menu(
                vec![PopupMenuItem::new("grok build")],
                Rc::new(RefCell::new(false)),
                |_| {},
            )
            .with_dir_menu(
                vec![PopupMenuItem::new("/work/project")],
                Rc::new(RefCell::new(false)),
                |_| {},
            )
            .with_branch_menu(
                vec![PopupMenuItem::new("main")],
                Rc::new(RefCell::new(false)),
                |_| {},
            );

        let size = composer.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);
        assert!(size.y > 0.0);

        let mut paint_ctx = PaintContext::new(Renderer::new());
        composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();

        let icons: Vec<String> = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawIcon { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        // `computer` renders the agentmode glyph; folder + git-branch are the
        // new atlas entries.
        for expected in ["agentmode", "folder", "git-branch"] {
            assert!(icons.iter().any(|n| n == expected), "missing icon {expected}");
        }

        // Rich-input pills are flat (no gray inset box), so no borders are drawn.
        let strokes = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        assert_eq!(strokes, 0, "expected flat pills without border boxes, got {strokes}");
    }

    #[test]
    fn composer_context_menu_opens_and_paints_panel() {
        use crate::elements::PaintContext;
        use crate::render::{RenderCommand, Renderer};

        let app = AppContext::default();
        let open = Rc::new(RefCell::new(true));
        let mut composer = ChatComposer::new()
            .with_harness_label("grok build")
            .with_harness_menu(
                vec![
                    PopupMenuItem::new("grok build"),
                    PopupMenuItem::new("claude"),
                ],
                open,
                |_| {},
            );

        let size = composer.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert!(size.x > 0.0);

        let mut paint_ctx = PaintContext::new(Renderer::new());
        composer.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();

        // An opened context selector draws its dropdown panel (a bordered
        // surface), confirming the rich-input pills actually open.
        let strokes = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { .. }))
            .count();
        assert!(strokes >= 1, "opened context menu should paint its panel border");
    }
}
