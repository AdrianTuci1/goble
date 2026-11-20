//! The settings toggle: DSH/T3's rounded pill switch.
//!
//! A 32×18 capsule track with a 14px circular thumb inset 2px from the edge.
//! Checked fills the track with the accent and draws the thumb in the page
//! background; unchecked keeps the control surface and its hairline border.
//! Hover lifts the track toward the hover pair, focus draws the 2px ring, and
//! disabled dims the whole control. It is a row control, not a row: focus is
//! drawn by the row (see [`super::row::SettingsRow::with_focused`]) and the
//! app's keyboard model toggles it, while a pointer press runs the callback the
//! page supplies.

use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, RectF, Vector2F};
use crate::theme::ColorToken;

use super::{
    control_border, dispatch_click, mix, ClickCallback, SETTINGS_TOGGLE_HEIGHT,
    SETTINGS_TOGGLE_WIDTH,
};

/// The track's radius: half its height, so the switch is a full capsule.
const TRACK_RADIUS: f32 = SETTINGS_TOGGLE_HEIGHT / 2.0;
/// The thumb's diameter: track height less the 2px inset at each end.
const THUMB_SIZE: f32 = SETTINGS_TOGGLE_HEIGHT - 4.0;
/// The thumb's inset from the track's edge.
const THUMB_INSET: f32 = 2.0;
/// The focus ring's width and the gap it leaves around the track.
const FOCUS_RING_WIDTH: f32 = 2.0;
const FOCUS_RING_GAP: f32 = 2.0;

/// A rounded switch. `disabled` draws it in the muted pair and refuses the
/// press; `focused` adds the 2px focus ring the design document puts on the
/// control when the caret is in it.
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

    /// The thumb's rect at `origin`, for tests that assert the circular pair.
    fn thumb_rect(&self, origin: Vector2F) -> RectF {
        let x = if self.checked {
            origin.x + SETTINGS_TOGGLE_WIDTH - THUMB_SIZE - THUMB_INSET
        } else {
            origin.x + THUMB_INSET
        };
        rectf(
            x,
            origin.y + (SETTINGS_TOGGLE_HEIGHT - THUMB_SIZE) / 2.0,
            THUMB_SIZE,
            THUMB_SIZE,
        )
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
        let track_rect = rectf(
            origin.x,
            origin.y,
            SETTINGS_TOGGLE_WIDTH,
            SETTINGS_TOGGLE_HEIGHT,
        );
        let hovered = !self.disabled && ctx.hovered(track_rect);
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let bg = app.theme.color(ColorToken::Bg);
        // The checked track is the accent; unchecked is the control surface
        // with a hairline. Hover moves either one a step while disabled keeps
        // the pair muted.
        let track = if self.disabled {
            mix(app, ColorToken::SurfaceRaised, ColorToken::Muted, 0.25)
        } else if self.checked {
            if hovered {
                mix(app, ColorToken::Accent, ColorToken::Text, 0.08)
            } else {
                app.theme.color(ColorToken::Accent)
            }
        } else if hovered {
            mix(app, ColorToken::SurfaceRaised, ColorToken::Hover, 0.7)
        } else {
            super::control_fill(app)
        };
        renderer.fill_rounded_rect(track_rect, track, TRACK_RADIUS);
        if self.disabled {
            renderer.stroke_rect(
                track_rect,
                control_border(app).mix(&bg, 0.5),
                1.0,
                TRACK_RADIUS,
            );
        } else if !self.checked {
            renderer.stroke_rect(track_rect, control_border(app), 1.0, TRACK_RADIUS);
        }
        let thumb = if self.checked && !self.disabled {
            bg
        } else if self.disabled {
            bg.mix(&app.theme.color(ColorToken::Text), 0.4)
        } else {
            app.theme.color(ColorToken::Muted)
        };
        renderer.fill_rounded_rect(self.thumb_rect(origin), thumb, THUMB_SIZE / 2.0);
        if self.focused {
            let ring = rectf(
                origin.x - FOCUS_RING_GAP,
                origin.y - FOCUS_RING_GAP,
                SETTINGS_TOGGLE_WIDTH + FOCUS_RING_GAP * 2.0,
                SETTINGS_TOGGLE_HEIGHT + FOCUS_RING_GAP * 2.0,
            );
            renderer.stroke_rect(
                ring,
                app.theme.color(ColorToken::Focus),
                FOCUS_RING_WIDTH,
                TRACK_RADIUS + FOCUS_RING_GAP,
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
        let bounds = self.bounds();
        if dispatch_click(event, bounds, &self.on_change) {
            self.checked = !self.checked;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{Element, LayoutContext, SizeConstraint};
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;
    use crate::test_util::render_element;
    use crate::theme::Theme;

    fn app() -> AppContext {
        let mut app = AppContext::default();
        app.theme = Theme::dark();
        app
    }

    fn fills(commands: &[RenderCommand]) -> Vec<(RectF, crate::color::ColorU, f32)> {
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color,
                    corner_radius,
                } => Some((*rect, *color, *corner_radius)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_switch_is_a_rounded_pill_with_a_14px_knob() {
        let app = app();
        let mut toggle: Box<dyn Element> = Box::new(SettingsToggle::new(false));
        let commands = render_element(&mut toggle, vec2f(100.0, 40.0), &app);
        let track = fills(&commands)
            .into_iter()
            .find(|(rect, _, _)| (rect.width() - SETTINGS_TOGGLE_WIDTH).abs() < 0.01)
            .expect("the track is 32 wide");
        assert!((track.0.height() - SETTINGS_TOGGLE_HEIGHT).abs() < 0.01);
        assert!(
            (track.2 - SETTINGS_TOGGLE_HEIGHT / 2.0).abs() < 0.01,
            "full radius"
        );
        let knob = fills(&commands)
            .into_iter()
            .find(|(rect, _, _)| (rect.width() - 14.0).abs() < 0.01)
            .expect("a 14px thumb");
        assert!((knob.2 - 7.0).abs() < 0.01, "the thumb is circular");
        assert!((knob.0.min_x() - 2.0).abs() < 0.01, "2px inset when off");
        assert!((knob.0.min_y() - 2.0).abs() < 0.01);
    }

    #[test]
    fn the_checked_thumb_sits_at_the_trailing_inset() {
        let app = app();
        let mut toggle: Box<dyn Element> = Box::new(SettingsToggle::new(true));
        let commands = render_element(&mut toggle, vec2f(100.0, 40.0), &app);
        assert!(
            fills(&commands)
                .iter()
                .any(|(_, color, _)| *color == app.theme.color(ColorToken::Accent)),
            "the checked track is the accent"
        );
        let knob = fills(&commands)
            .into_iter()
            .find(|(rect, _, _)| (rect.width() - 14.0).abs() < 0.01)
            .expect("a 14px thumb");
        assert!((knob.0.max_x() - (SETTINGS_TOGGLE_WIDTH - 2.0)).abs() < 0.01);
    }

    #[test]
    fn a_focused_switch_draws_the_2px_ring() {
        let app = app();
        let mut toggle: Box<dyn Element> = Box::new(SettingsToggle::new(false).with_focused(true));
        let commands = render_element(&mut toggle, vec2f(100.0, 40.0), &app);
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::StrokeRect { color, width, .. }
                    if *color == app.theme.color(ColorToken::Focus) && (*width - 2.0).abs() < 0.01
            )),
            "the focused switch rings in Focus at 2px"
        );
    }

    #[test]
    fn a_disabled_switch_refuses_the_press() {
        let app = app();
        let mut toggle: Box<dyn Element> = Box::new(SettingsToggle::new(false).with_disabled(true));
        let _ = toggle.layout(
            SizeConstraint::loose(vec2f(100.0, 40.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = EventContext::default();
        assert!(!toggle.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(5.0, 5.0),
                button: 0,
            },
            &mut ctx,
            &app,
        ));
    }
}
