//! The row controls: text field, path field, select, and the small button.
//!
//! These are the settings-sized versions of the app's own controls — 32px tall,
//! 8px radius, a hairline `--input` border and a 13px value — so a page's rows
//! line up without each page re-inventing the geometry. Everything is drawn
//! from tokens; nothing here hardcodes a colour.
//!
//! The select is a *cycle* control: clicking it advances to the next option and
//! the app's keyboard model steps it with Left/Right. The renderer has no
//! portal layer for a popup that would survive the per-frame rebuild, and a
//! popup that closes with the frame it opened in would be worse than an honest
//! cycle; the page marks the row's control as a select (chevron) so the
//! affordance reads, and the footer names the keys that change it.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint, Text,
    TextInput,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::ColorToken;

use super::{
    control_border, control_fill, SETTINGS_CONTROL_HEIGHT, SETTINGS_CONTROL_MIN_WIDTH,
    SETTINGS_CONTROL_RADIUS, SETTINGS_CONTROL_TEXT_SIZE,
};

type TextCallback = Rc<RefCell<dyn FnMut(String)>>;
type SubmitCallback = Rc<RefCell<dyn FnMut()>>;
type CycleCallback = Rc<RefCell<dyn FnMut(usize)>>;
type ClickCallback = Rc<RefCell<dyn FnMut()>>;
type FocusCallback = Rc<RefCell<dyn FnMut(bool)>>;

/// A settings text field: a 32px control with the value at 13px.
pub struct SettingsTextField {
    value: String,
    placeholder: String,
    focused: bool,
    disabled: bool,
    write_only: bool,
    width: Option<f32>,
    on_change: Option<TextCallback>,
    on_submit: Option<SubmitCallback>,
    on_focus_change: Option<FocusCallback>,
    field: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsTextField {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            placeholder: String::new(),
            focused: false,
            disabled: false,
            write_only: false,
            width: None,
            on_change: None,
            on_submit: None,
            on_focus_change: None,
            field: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into();
        self
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Draw the value write-only: one dot per character, never the text. A
    /// secret the core never hands out is still typed here and committed as an
    /// op; the mask is what keeps it off the screen.
    pub fn write_only(mut self) -> Self {
        self.write_only = true;
        self
    }

    /// The field's fixed width; `None` takes the width the row gives it up to
    /// the control minimum.
    pub fn with_width(mut self, width: f32) -> Self {
        self.width = Some(width.max(SETTINGS_CONTROL_MIN_WIDTH));
        self
    }

    pub fn with_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_submit<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_submit = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn with_on_focus_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_focus_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn build(&mut self) {
        let on_change = self.on_change.clone();
        let mut field = TextInput::new()
            .with_value(self.value.clone())
            .with_placeholder(self.placeholder.clone())
            .with_plain(true)
            .with_full_width(true)
            .with_masked(self.write_only)
            .with_focused(self.focused && !self.disabled);
        if !self.disabled {
            if let Some(callback) = on_change {
                field = field.with_on_change(move |value| {
                    (callback.borrow_mut())(value);
                });
            }
            if let Some(callback) = self.on_submit.clone() {
                field = field.with_on_submit(move || {
                    (callback.borrow_mut())();
                });
            }
            if let Some(callback) = self.on_focus_change.clone() {
                field = field.with_on_focus_change(move |focused| {
                    (callback.borrow_mut())(focused);
                });
            }
        }
        self.field = Some(field.finish());
    }

    fn field_mut(&mut self) -> &mut Box<dyn Element> {
        if self.field.is_none() {
            self.build();
        }
        self.field.as_mut().unwrap()
    }

    /// The field's child inset: the value sits 10px inside the control.
    const INSET: f32 = 10.0;
}

impl Default for SettingsTextField {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for SettingsTextField {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let width = self
            .width
            .unwrap_or_else(|| constraint.max.x.max(SETTINGS_CONTROL_MIN_WIDTH));
        let inner = SizeConstraint::new(
            vec2f((width - Self::INSET * 2.0).max(0.0), 0.0),
            vec2f(
                (width - Self::INSET * 2.0).max(0.0),
                SETTINGS_CONTROL_HEIGHT,
            ),
        );
        let child = self.field_mut().layout(inner, ctx, app);
        let size = vec2f(width, SETTINGS_CONTROL_HEIGHT.max(child.y));
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let border = if self.focused && !self.disabled {
            app.theme.color(ColorToken::Focus)
        } else if self.disabled {
            control_border(app).mix(&app.theme.color(ColorToken::Bg), 0.5)
        } else {
            control_border(app)
        };
        let fill = if self.disabled {
            control_fill(app).mix(&app.theme.color(ColorToken::Bg), 0.5)
        } else {
            control_fill(app)
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            let rect = rectf(origin.x, origin.y, size.x, size.y);
            renderer.fill_rounded_rect(rect, fill, SETTINGS_CONTROL_RADIUS);
            renderer.stroke_rect(rect, border, 1.0, SETTINGS_CONTROL_RADIUS);
        }
        // The field's text is vertically centred in the 32px control.
        let child_height = self.field_mut().size().map(|size| size.y).unwrap_or(0.0);
        let y = origin.y + ((size.y - child_height).max(0.0)) / 2.0;
        self.field_mut()
            .paint(vec2f(origin.x + Self::INSET, y), ctx, app);
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
        if self.disabled {
            return false;
        }
        self.field_mut().dispatch_event(event, ctx, app)
    }
}

/// A path field is a text field with the same geometry; the difference is the
/// page's copy ("Folder") and, later, a browse affordance this build does not
/// have. Keeping the type means a page says what the value is.
pub type SettingsPathField = SettingsTextField;

/// A settings select trigger: a 32px control with the current option and a
/// chevron. Clicking cycles; the app keyboard steps it.
pub struct SettingsSelect {
    options: Vec<String>,
    selected: usize,
    focused: bool,
    disabled: bool,
    width: f32,
    on_change: Option<CycleCallback>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsSelect {
    pub fn new(options: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            selected: 0,
            focused: false,
            disabled: false,
            width: SETTINGS_CONTROL_MIN_WIDTH,
            on_change: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width.max(SETTINGS_CONTROL_MIN_WIDTH);
        self
    }

    pub fn with_on_change<F: FnMut(usize) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    fn label(&self) -> &str {
        self.options
            .get(self.selected)
            .map(String::as_str)
            .unwrap_or("")
    }
}

impl Element for SettingsSelect {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(self.width, SETTINGS_CONTROL_HEIGHT);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let border = if self.focused && !self.disabled {
            app.theme.color(ColorToken::Focus)
        } else if self.disabled {
            control_border(app).mix(&app.theme.color(ColorToken::Bg), 0.5)
        } else {
            control_border(app)
        };
        let fill = if self.disabled {
            control_fill(app).mix(&app.theme.color(ColorToken::Bg), 0.5)
        } else {
            control_fill(app)
        };
        if let Some(renderer) = ctx.renderer.as_mut() {
            let rect = rectf(origin.x, origin.y, size.x, size.y);
            renderer.fill_rounded_rect(rect, fill, SETTINGS_CONTROL_RADIUS);
            renderer.stroke_rect(rect, border, 1.0, SETTINGS_CONTROL_RADIUS);
        }
        let text_color = if self.disabled {
            app.theme.color(ColorToken::Muted)
        } else {
            app.theme.color(ColorToken::Text)
        };
        // The label is laid out by hand: a bare Text run at the control's
        // inset, vertically centred, then the chevron at the trailing inset.
        let label = Text::new(self.label().to_string())
            .with_font_size(SETTINGS_CONTROL_TEXT_SIZE)
            .with_line_height(1.4)
            .with_color(text_color)
            .finish();
        let mut label = label;
        let inner = SizeConstraint::loose(vec2f((size.x - 34.0).max(0.0), SETTINGS_CONTROL_HEIGHT));
        let label_size = label.layout(inner, &mut LayoutContext::default(), app);
        let y = origin.y + (size.y - label_size.y).max(0.0) / 2.0;
        label.paint(vec2f(origin.x + 10.0, y), ctx, app);
        let mut chevron = crate::elements::Icon::new("chevron-down")
            .with_size(12.0)
            .with_color(if self.disabled {
                app.theme.color(ColorToken::Muted)
            } else {
                app.theme.color(ColorToken::Muted)
            })
            .finish();
        let _ = chevron.layout(
            SizeConstraint::tight(vec2f(12.0, 12.0)),
            &mut LayoutContext::default(),
            app,
        );
        chevron.paint(
            vec2f(
                origin.x + size.x - 10.0 - 12.0,
                origin.y + (size.y - 12.0) / 2.0,
            ),
            ctx,
            app,
        );
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
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        if self.disabled || self.options.is_empty() {
            return false;
        }
        // The select decides the next value itself, so the press is recognized
        // here rather than through the shared `dispatch_click` helper.
        let bounds = match self.bounds() {
            Some(bounds) => bounds,
            None => return false,
        };
        if let DispatchedEvent::MouseDown {
            position,
            button: 0,
        } = event
        {
            if bounds.contains(crate::geometry::PointF::new(position.x, position.y)) {
                self.selected = (self.selected + 1) % self.options.len();
                if let Some(callback) = self.on_change.as_ref() {
                    (callback.borrow_mut())(self.selected);
                }
                return true;
            }
        }
        false
    }
}

/// A small button in T3's `size="xs"` register: 28px tall, a 13px label, an
/// optional leading icon.
pub struct SettingsButton {
    label: String,
    icon: Option<&'static str>,
    primary: bool,
    destructive: bool,
    ghost: bool,
    disabled: bool,
    focused: bool,
    on_click: Option<ClickCallback>,
    size: Option<Vector2F>,
    origin: Option<Point>,
    button: Option<Box<dyn Element>>,
}

impl SettingsButton {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            primary: false,
            destructive: false,
            ghost: false,
            disabled: false,
            focused: false,
            on_click: None,
            size: None,
            origin: None,
            button: None,
        }
    }

    pub fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// A low-emphasis text button: no raised fill or outline, muted text. Used
    /// for a list row's secondary action (a palette's "Use" / "Selected").
    pub fn ghost(mut self) -> Self {
        self.ghost = true;
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_click = Some(Rc::new(RefCell::new(callback)));
        self
    }

    fn build(&mut self, app: &AppContext) {
        let color = if self.disabled {
            app.theme.color(ColorToken::Muted)
        } else if self.destructive {
            app.theme.color(ColorToken::Error)
        } else if self.primary {
            app.theme.color(ColorToken::Bg)
        } else if self.ghost {
            app.theme.color(ColorToken::Muted)
        } else {
            app.theme.color(ColorToken::Text)
        };
        let mut parts: Vec<Box<dyn Element>> = Vec::new();
        if let Some(icon) = self.icon {
            let icon_color = if self.disabled {
                app.theme.color(ColorToken::Muted)
            } else if self.primary {
                app.theme.color(ColorToken::Bg)
            } else if self.destructive {
                app.theme.color(ColorToken::Error)
            } else {
                app.theme.color(ColorToken::Muted)
            };
            parts.push(
                crate::elements::Icon::new(icon)
                    .with_size(12.0)
                    .with_color(icon_color)
                    .finish(),
            );
        }
        parts.push(
            Text::new(self.label.clone())
                .with_font_size(12.0)
                .with_line_height(1.3)
                .with_weight(FontWeight::Medium)
                .with_color(color)
                .finish(),
        );
        let content = crate::elements::Flex::row()
            .with_cross_axis_alignment(crate::elements::CrossAxisAlignment::Center)
            .with_spacing(5.0)
            .with_children(parts)
            .finish();
        let variant = if self.primary {
            crate::elements::ButtonVariant::Primary
        } else if self.ghost {
            crate::elements::ButtonVariant::Ghost
        } else {
            crate::elements::ButtonVariant::Default
        };
        let on_click = self.on_click.clone();
        let button = crate::elements::Button::new(content)
            .with_variant(variant)
            .with_corner_radius(8.0)
            .with_disabled(self.disabled)
            .with_on_click(move || {
                if let Some(callback) = on_click.as_ref() {
                    (callback.borrow_mut())();
                }
            })
            .finish();
        let button = crate::elements::ConstrainedBox::new(button)
            .with_min_height(28.0)
            .finish();
        let button = if self.focused {
            Box::new(super::FocusOutline::new(
                button,
                app.theme.color(ColorToken::Focus),
                2.0,
                1.0,
                9.0,
            ))
        } else {
            button
        };
        self.button = Some(button);
    }

    fn root_mut(&mut self, app: &AppContext) -> &mut Box<dyn Element> {
        if self.button.is_none() {
            self.build(app);
        }
        self.button.as_mut().unwrap()
    }
}

impl Element for SettingsButton {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.root_mut(app).layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root_mut(app).paint(origin, ctx, app);
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
        if self.disabled {
            return false;
        }
        self.root_mut(app).dispatch_event(event, ctx, app)
    }
}

/// The one-line value text a read-only row shows where a control would go.
pub fn read_only_value(app: &AppContext, value: impl Into<String>) -> Box<dyn Element> {
    crate::elements::ConstrainedBox::new(
        Text::new(value)
            .with_font_size(SETTINGS_CONTROL_TEXT_SIZE)
            .with_line_height(1.4)
            .with_color(app.theme.color(ColorToken::Muted))
            .finish(),
    )
    .with_max_width(SETTINGS_CONTROL_MIN_WIDTH)
    .finish()
}
