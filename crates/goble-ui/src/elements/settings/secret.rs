//! The secret field: a write-only control for a value the core never returns.
//!
//! The `ConfigView` projection carries a `SecretState { set }` for an inline API
//! key, a worker token or a remote agent credential — never the string. So the
//! settings page cannot "show" a secret; it can only say whether one is set and
//! accept a replacement. This element is that contract in widget form:
//!
//! - when a value is configured and the field holds no draft, the placeholder
//!   reads "configured — type to replace" and the value paints as dots;
//! - while the user types, the draft paints as one dot per character
//!   (write-only, so a shoulder-surfer sees length only);
//! - committing writes the draft as a config op; clearing unsets the key.
//!
//! The mask itself is [`crate::elements::TextInput`]'s `masked` mode, so keyboard
//! editing, the caret and the placeholder are the same code as every other
//! field rather than a second input implementation.

use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
    TextInput,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::theme::ColorToken;

use super::{
    control_border, control_fill, SETTINGS_CONTROL_HEIGHT, SETTINGS_CONTROL_MIN_WIDTH,
    SETTINGS_CONTROL_RADIUS, SETTINGS_CONTROL_TEXT_SIZE,
};

/// A write-only secret control.
pub struct SettingsSecretField {
    configured: bool,
    draft: String,
    focused: bool,
    disabled: bool,
    width: Option<f32>,
    on_change: Option<std::rc::Rc<std::cell::RefCell<dyn FnMut(String)>>>,
    on_submit: Option<std::rc::Rc<std::cell::RefCell<dyn FnMut()>>>,
    field: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsSecretField {
    pub fn new() -> Self {
        Self {
            configured: false,
            draft: String::new(),
            focused: false,
            disabled: false,
            width: None,
            on_change: None,
            on_submit: None,
            field: None,
            size: None,
            origin: None,
        }
    }

    /// Whether a value is already configured (the projection's `SecretState`).
    pub fn with_configured(mut self, configured: bool) -> Self {
        self.configured = configured;
        self
    }

    /// The replacement being typed. It is kept in app state so the field can be
    /// rebuilt every frame without losing the draft.
    pub fn with_draft(mut self, draft: impl Into<String>) -> Self {
        self.draft = draft.into();
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
        self.width = Some(width.max(SETTINGS_CONTROL_MIN_WIDTH));
        self
    }

    pub fn with_on_change<F: FnMut(String) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(std::rc::Rc::new(std::cell::RefCell::new(callback)));
        self
    }

    pub fn with_on_submit<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_submit = Some(std::rc::Rc::new(std::cell::RefCell::new(callback)));
        self
    }

    fn placeholder(&self) -> String {
        if self.draft.is_empty() && self.configured {
            "Configured — type to replace".to_string()
        } else if self.configured {
            String::new()
        } else {
            "Not configured — paste a value".to_string()
        }
    }

    fn build(&mut self) {
        let mut field = TextInput::new()
            .with_value(self.draft.clone())
            .with_placeholder(self.placeholder())
            .with_masked(true)
            .with_plain(true)
            .with_full_width(true)
            .with_focused(self.focused && !self.disabled);
        if !self.disabled {
            if let Some(callback) = self.on_change.clone() {
                field = field.with_on_change(move |value| {
                    (callback.borrow_mut())(value);
                });
            }
            if let Some(callback) = self.on_submit.clone() {
                field = field.with_on_submit(move || {
                    (callback.borrow_mut())();
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

    const INSET: f32 = 10.0;
}

impl Default for SettingsSecretField {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for SettingsSecretField {
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
        let child_height = self.field_mut().size().map(|size| size.y).unwrap_or(0.0);
        let y = origin.y + ((size.y - child_height).max(0.0)) / 2.0;
        self.field_mut()
            .paint(vec2f(origin.x + Self::INSET, y), ctx, app);
        let _ = SETTINGS_CONTROL_TEXT_SIZE;
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
