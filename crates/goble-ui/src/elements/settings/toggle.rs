//! The settings toggle: the square switch the owner asked Goble to keep.
//!
//! DSH and T3 both draw a rounded pill; Goble's existing switch is square and
//! the owner's decision is to keep that, so this element is the settings row's
//! switch: a 32×18 square track with a square thumb, drawn in the same tokens
//! as the rest of the surface. It is a row control, not a row: focus is drawn by
//! the row (see [`super::row::SettingsRow::with_focused`]) and the app's keyboard
//! model toggles it, while a pointer press runs the callback the page supplies.

use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};
use crate::theme::ColorToken;

use super::{
    control_border, dispatch_click, mix, ClickCallback, SETTINGS_TOGGLE_HEIGHT,
    SETTINGS_TOGGLE_WIDTH,
};

/// A square switch. `disabled` draws it in the muted pair and refuses the press;
/// `focused` adds the 2px focus ring the design document puts on the control
/// when the caret is in it.
pub struct SettingsToggle {
    checked: bool,
    disabled: bool,
    focused: bool,
    on_change: Option<ClickCallback>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsToggle {
    pub fn new(checked: bool) -> Self {
        Self {
            checked,
            disabled: false,
            focused: false,
            on_change: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn with_on_change<F: FnMut() + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(std::rc::Rc::new(std::cell::RefCell::new(callback)));
        self
    }

    pub fn checked(&self) -> bool {
        self.checked
    }

    /// The thumb's rect at `origin`, for the tests that assert the square pair.
    fn thumb_rect(&self, origin: Vector2F) -> crate::geometry::RectF {
        let pad = 2.0;
        let thumb = SETTINGS_TOGGLE_HEIGHT - pad * 2.0;
        let x = if self.checked {
            origin.x + SETTINGS_TOGGLE_WIDTH - thumb - pad
        } else {
            origin.x + pad
        };
        rectf(x, origin.y + pad, thumb, thumb)
    }
}

impl Element for SettingsToggle {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        self.size = Some(Vector2F::new(SETTINGS_TOGGLE_WIDTH, SETTINGS_TOGGLE_HEIGHT));
        self.size.unwrap()
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let bg = app.theme.color(ColorToken::Bg);
        // The checked track is the accent; disabled drops it toward the
        // background, and unchecked is the control surface with a hairline.
        let track = if self.disabled {
            mix(app, ColorToken::SurfaceRaised, ColorToken::Muted, 0.25)
        } else if self.checked {
            app.theme.color(ColorToken::Accent)
        } else {
            super::control_fill(app)
        };
        let track_rect = rectf(
            origin.x,
            origin.y,
            SETTINGS_TOGGLE_WIDTH,
            SETTINGS_TOGGLE_HEIGHT,
        );
        renderer.fill_rect(track_rect, track);
        if !self.checked && !self.disabled {
            renderer.stroke_rect(track_rect, control_border(app), 1.0, 0.0);
        }
        let thumb = if self.checked && !self.disabled {
            bg
        } else if self.disabled {
            app.theme
                .color(ColorToken::Bg)
                .mix(&app.theme.color(ColorToken::Text), 0.4)
        } else {
            app.theme.color(ColorToken::Muted)
        };
        renderer.fill_rect(self.thumb_rect(origin), thumb);
        if self.focused {
            renderer.stroke_rect(track_rect, app.theme.color(ColorToken::Focus), 2.0, 0.0);
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
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        if self.disabled {
            return false;
        }
        let bounds = self.bounds();
        if dispatch_click(event, bounds, &self.on_change) {
            self.checked = !self.checked;
            return true;
        }
        false
    }
}
