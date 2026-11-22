//! The settings slider: a bounded value control for the appearance rows.
//!
//! T3 draws a range input beside a monospace `<output>`; goble has no platform
//! range control, so the element paints the track and the knob itself and the
//! page pairs it with a value output. The pointer sets the value by pressing
//! anywhere on the track; the keyboard steps it through the app's `adjust`
//! action.

use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint, Text,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, RectF, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::ColorToken;

use super::{control_fill, ClickCallback, SETTINGS_TOGGLE_HEIGHT};

/// The slider's track height, and the knob's size.
pub const SLIDER_TRACK_HEIGHT: f32 = 4.0;
pub const SLIDER_KNOB: f32 = 14.0;
/// The default track width: T3's `sm:w-52` (208) narrowed for the settings
/// column, while pages that want the full reference width pass `with_width`.
pub const SLIDER_DEFAULT_WIDTH: f32 = 160.0;

/// A horizontal value slider.
pub struct SettingsSlider {
    value: f32,
    min: f32,
    max: f32,
    width: f32,
    disabled: bool,
    focused: bool,
    /// Called with the value a press mapped to.
    on_change: Option<std::rc::Rc<std::cell::RefCell<dyn FnMut(f32)>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SettingsSlider {
    pub fn new(value: f32, min: f32, max: f32) -> Self {
        Self {
            value,
            min,
            max,
            width: SLIDER_DEFAULT_WIDTH,
            disabled: false,
            focused: false,
            on_change: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
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

    pub fn with_on_change<F: FnMut(f32) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(std::rc::Rc::new(std::cell::RefCell::new(callback)));
        self
    }

    fn fraction(&self) -> f32 {
        if (self.max - self.min).abs() < f32::EPSILON {
            return 0.0;
        }
        ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    fn track(&self, origin: Vector2F) -> RectF {
        rectf(
            origin.x,
            origin.y + (SETTINGS_TOGGLE_HEIGHT - SLIDER_TRACK_HEIGHT) / 2.0,
            self.width,
            SLIDER_TRACK_HEIGHT,
        )
    }
}

impl Element for SettingsSlider {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        self.size = Some(vec2f(self.width, SETTINGS_TOGGLE_HEIGHT));
        self.size.unwrap()
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let track = self.track(origin);
        let track_color = if self.disabled {
            control_fill(app).mix(&app.theme.color(ColorToken::Bg), 0.4)
        } else {
            app.theme.color(ColorToken::Border)
        };
        renderer.fill_rounded_rect(track, track_color, SLIDER_TRACK_HEIGHT / 2.0);
        let fraction = self.fraction();
        let filled = rectf(
            track.min_x(),
            track.min_y(),
            track.width() * fraction,
            track.height(),
        );
        let accent = if self.disabled {
            app.theme.color(ColorToken::Muted)
        } else {
            app.theme.color(ColorToken::Accent)
        };
        renderer.fill_rounded_rect(filled, accent, SLIDER_TRACK_HEIGHT / 2.0);
        let knob_x = origin.x + self.width * fraction - SLIDER_KNOB / 2.0;
        let max_knob_x = (origin.x + self.width - SLIDER_KNOB).max(origin.x);
        let knob = rectf(
            knob_x.clamp(origin.x, max_knob_x),
            origin.y + (SETTINGS_TOGGLE_HEIGHT - SLIDER_KNOB) / 2.0,
            SLIDER_KNOB,
            SLIDER_KNOB,
        );
        let knob_fill = if self.disabled {
            app.theme.color(ColorToken::Muted)
        } else {
            app.theme.color(ColorToken::Text)
        };
        renderer.fill_rounded_rect(knob, knob_fill, SLIDER_KNOB / 2.0);
        if self.focused {
            renderer.stroke_rect(
                rectf(
                    origin.x - 2.0,
                    origin.y - 1.0,
                    self.width + 4.0,
                    SETTINGS_TOGGLE_HEIGHT + 2.0,
                ),
                app.theme.color(ColorToken::Focus),
                2.0,
                3.0,
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
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        if self.disabled {
            return false;
        }
        let DispatchedEvent::MouseDown { position, button } = event else {
            return false;
        };
        if *button != 0 {
            return false;
        }
        let Some(bounds) = self.bounds() else {
            return false;
        };
        if !bounds.contains(crate::geometry::PointF::new(position.x, position.y)) {
            return false;
        }
        let fraction = ((position.x - bounds.min_x()) / bounds.width().max(1.0)).clamp(0.0, 1.0);
        let value = self.min + fraction * (self.max - self.min);
        self.value = value;
        if let Some(callback) = self.on_change.as_ref() {
            (callback.borrow_mut())(value);
        }
        true
    }
}

/// The `<output>` box beside a slider: `min-w-12 rounded-md bg-muted px-2 py-1
/// text-center font-mono text-xs`. Returns the box and the value text.
pub fn slider_output(app: &AppContext, text: impl Into<String>) -> Box<dyn Element> {
    let label = Text::new(text)
        .with_font_size(12.0)
        .with_line_height(1.3)
        .with_weight(FontWeight::Medium)
        .with_color(app.theme.color(ColorToken::Text))
        .finish();
    let label = crate::elements::ConstrainedBox::new(label)
        .with_min_width(28.0)
        .finish();
    crate::elements::Container::new(
        crate::elements::Flex::row()
            .with_main_axis_alignment(crate::elements::MainAxisAlignment::Center)
            .with_child(label)
            .finish(),
    )
    .with_padding(crate::elements::EdgeInsets::new(4.0, 8.0, 4.0, 8.0))
    .with_background(crate::elements::Fill::Solid(super::over_bg(
        app,
        ColorToken::SurfaceRaised,
        0.7,
    )))
    .with_corner_radius(6.0)
    .finish()
}

/// A slider and its output box in the row's control group.
pub fn slider_with_output(
    app: &AppContext,
    slider: Box<dyn Element>,
    text: impl Into<String>,
) -> Box<dyn Element> {
    crate::elements::Flex::row()
        .with_cross_axis_alignment(crate::elements::CrossAxisAlignment::Center)
        .with_spacing(8.0)
        .with_child(slider_output(app, text))
        .with_child(slider)
        .finish()
}

/// Keep the click-callback alias used by the module docs honest.
#[allow(dead_code)]
fn _shared(_: Option<ClickCallback>) {}
