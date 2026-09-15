use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::theme::{ColorToken, SpacingToken};

pub struct Switch {
    state: InteractiveState,
    checked: bool,
    disabled: bool,
    size: Vector2F,
    on_change: Option<Rc<RefCell<dyn FnMut(bool) + 'static>>>,
    layout_size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Switch {
    pub fn new() -> Self {
        Self {
            state: InteractiveState::default(),
            checked: false,
            disabled: false,
            size: vec2f(44.0, 24.0),
            on_change: None,
            layout_size: None,
            origin: None,
        }
    }

    pub fn with_checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_size(mut self, size: Vector2F) -> Self {
        self.size = size;
        self
    }

    pub fn with_on_change<F: FnMut(bool) + 'static>(mut self, callback: F) -> Self {
        self.on_change = Some(Rc::new(RefCell::new(callback)));
        self
    }

    pub fn checked(&self) -> bool {
        self.checked
    }
}

impl Default for Switch {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for Switch {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        self.layout_size = Some(self.size);
        self.size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));

        let track_rect = rectf(origin.x, origin.y, self.size.x, self.size.y);
        let track_color = if self.disabled {
            app.theme.color(ColorToken::SurfaceRaised)
        } else if self.checked {
            app.theme.color(ColorToken::Accent)
        } else {
            app.theme.color(ColorToken::Border)
        };
        // Square corners: the track and the thumb draw right angles in every
        // state (off/on/disabled), so neither takes a radius.
        ctx.renderer
            .as_mut()
            .unwrap()
            .fill_rect(track_rect, track_color);

        let padding = app.theme.spacing_px(SpacingToken::Xs);
        let thumb_size = self.size.y - padding * 2.0;
        let thumb_x = if self.checked {
            origin.x + self.size.x - thumb_size - padding
        } else {
            origin.x + padding
        };
        let thumb_y = origin.y + padding;
        let thumb_rect = rectf(thumb_x, thumb_y, thumb_size, thumb_size);
        let thumb_color = app.theme.color(ColorToken::Text);
        ctx.renderer
            .as_mut()
            .unwrap()
            .fill_rect(thumb_rect, thumb_color);
    }

    fn size(&self) -> Option<Vector2F> {
        self.layout_size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        if self.disabled {
            return false;
        }
        let bounds = match self.bounds() {
            Some(b) => b,
            None => return false,
        };

        let change = self.on_change.clone();
        let mut toggle = || {
            self.checked = !self.checked;
            if let Some(cb) = change.as_ref() {
                (cb.borrow_mut())(self.checked);
            }
        };

        handle_mouse_event(&mut self.state, event, bounds, ctx, &mut toggle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::DispatchedEvent;
    use crate::geometry::RectF;
    use crate::render::{RenderCommand, Renderer};
    use crate::test_util::render_element;

    /// The switch's own track and thumb, as `(rect, colour, corner_radius)`.
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

    /// Every command the switch draws with a non-zero corner radius — a
    /// rounded track, thumb or border.
    fn rounded(commands: &[RenderCommand]) -> Vec<String> {
        commands
            .iter()
            .filter(|command| match command {
                RenderCommand::FillRect { corner_radius, .. }
                | RenderCommand::FillRectFadeRight { corner_radius, .. }
                | RenderCommand::StrokeRect { corner_radius, .. } => *corner_radius > 0.0,
                _ => false,
            })
            .map(|command| format!("{command:?}"))
            .collect()
    }

    /// Paints `element` headlessly, optionally with the pointer inside it.
    fn painted_with_cursor(
        element: &mut Switch,
        app: &AppContext,
        cursor: Option<Vector2F>,
    ) -> Vec<RenderCommand> {
        element.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        if let Some(position) = cursor {
            ctx.cursor_position = position;
            ctx.cursor_inside = true;
        }
        element.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    /// The user asked for square switches: no command the switch draws in any
    /// state — off, on, disabled — may carry a corner radius, and squaring it
    /// must not have moved or recoloured the track and thumb.
    #[test]
    fn switch_draws_square_corners_in_every_state() {
        let app = AppContext::default();
        let padding = app.theme.spacing_px(SpacingToken::Xs);
        let thumb_size = 24.0 - padding * 2.0;

        for (checked, disabled) in [(false, false), (true, false), (false, true), (true, true)] {
            let state = format!("checked={checked}, disabled={disabled}");
            let mut element = Switch::new()
                .with_checked(checked)
                .with_disabled(disabled)
                .finish();
            let commands = render_element(&mut element, vec2f(200.0, 200.0), &app);

            assert!(
                rounded(&commands).is_empty(),
                "switch ({state}) draws square corners, got {:?}",
                rounded(&commands)
            );

            let fills = fills(&commands);
            assert_eq!(fills.len(), 2, "switch ({state}) draws a track and a thumb");
            let (track_rect, track_color, track_radius) = fills[0];
            let (thumb_rect, thumb_color, thumb_radius) = fills[1];

            assert_eq!(track_rect, rectf(0.0, 0.0, 44.0, 24.0), "track ({state})");
            assert_eq!(track_radius, 0.0, "track radius ({state})");
            let expected_track = if disabled {
                app.theme.color(ColorToken::SurfaceRaised)
            } else if checked {
                app.theme.color(ColorToken::Accent)
            } else {
                app.theme.color(ColorToken::Border)
            };
            assert_eq!(track_color, expected_track, "track colour ({state})");

            let thumb_x = if checked {
                44.0 - thumb_size - padding
            } else {
                padding
            };
            assert_eq!(
                thumb_rect,
                rectf(thumb_x, padding, thumb_size, thumb_size),
                "thumb ({state})"
            );
            assert_eq!(thumb_radius, 0.0, "thumb radius ({state})");
            assert_eq!(
                thumb_color,
                app.theme.color(ColorToken::Text),
                "thumb colour ({state})"
            );
        }
    }

    /// The switch has no state-dependent paint of its own: hovering or
    /// pressing draws exactly the same square commands as idle, and a focus
    /// ring is drawn by the host row, not here — so no state can hide a corner
    /// the idle switch forgot.
    #[test]
    fn switch_draws_the_same_square_commands_when_hovered_or_pressed() {
        let app = AppContext::default();
        let bounds_position = vec2f(10.0, 10.0);

        let idle = {
            let mut switch = Switch::new().with_checked(true);
            painted_with_cursor(&mut switch, &app, None)
        };

        let hovered = {
            let mut switch = Switch::new().with_checked(true);
            let mut event_ctx = EventContext::default();
            // A first paint sets the origin the hit test uses, as in a frame.
            let _ = painted_with_cursor(&mut switch, &app, None);
            switch.dispatch_event(
                &DispatchedEvent::MouseMove {
                    position: bounds_position,
                },
                &mut event_ctx,
                &app,
            );
            assert!(switch.state.hover);
            painted_with_cursor(&mut switch, &app, Some(bounds_position))
        };

        let pressed = {
            let mut switch = Switch::new().with_checked(true);
            let mut event_ctx = EventContext::default();
            let _ = painted_with_cursor(&mut switch, &app, None);
            switch.dispatch_event(
                &DispatchedEvent::MouseMove {
                    position: bounds_position,
                },
                &mut event_ctx,
                &app,
            );
            switch.dispatch_event(
                &DispatchedEvent::MouseDown {
                    position: bounds_position,
                    button: 0,
                },
                &mut event_ctx,
                &app,
            );
            assert!(switch.state.is_active());
            painted_with_cursor(&mut switch, &app, Some(bounds_position))
        };

        assert!(rounded(&idle).is_empty(), "idle switch draws square");
        assert_eq!(
            hovered.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>(),
            idle.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>(),
            "hovered switch draws the idle switch's square commands"
        );
        assert_eq!(
            pressed.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>(),
            idle.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>(),
            "pressed switch draws the idle switch's square commands"
        );
    }

    /// Squaring the switch must not have touched its behaviour: a click still
    /// flips it, reports the new state, and moves the squared thumb.
    #[test]
    fn switch_still_toggles_and_moves_its_squared_thumb_on_click() {
        let app = AppContext::default();
        let checked = Rc::new(RefCell::new(false));
        let checked_clone = checked.clone();
        let mut switch = Switch::new().with_on_change(move |v| *checked_clone.borrow_mut() = v);

        let before = painted_with_cursor(&mut switch, &app, None);
        assert!(!switch.checked());

        let mut event_ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        assert!(switch.dispatch_event(&down, &mut event_ctx, &app));
        assert!(switch.dispatch_event(&up, &mut event_ctx, &app));

        assert!(*checked.borrow(), "the click reported the new state");
        assert!(switch.checked(), "the switch reports its state");

        let after = painted_with_cursor(&mut switch, &app, None);
        assert!(
            rounded(&after).is_empty(),
            "the checked switch draws square"
        );
        let padding = app.theme.spacing_px(SpacingToken::Xs);
        let thumb_size = 24.0 - padding * 2.0;
        assert_eq!(
            fills(&before)[1].0,
            rectf(padding, padding, thumb_size, thumb_size)
        );
        assert_eq!(
            fills(&after)[1].0,
            rectf(44.0 - thumb_size - padding, padding, thumb_size, thumb_size)
        );
    }

    #[test]
    fn switch_toggles_on_click() {
        let checked = Rc::new(RefCell::new(false));
        let checked_clone = checked.clone();
        let mut switch = Switch::new().with_on_change(move |v| *checked_clone.borrow_mut() = v);

        let app = AppContext::default();
        switch.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        switch.paint(vec2f(0.0, 0.0), &mut PaintContext::default(), &app);

        let mut event_ctx = EventContext::default();
        let down = DispatchedEvent::MouseDown {
            position: vec2f(10.0, 10.0),
            button: 0,
        };
        let up = DispatchedEvent::MouseUp {
            position: vec2f(10.0, 10.0),
            button: 0,
        };

        assert!(switch.dispatch_event(&down, &mut event_ctx, &app));
        assert!(switch.dispatch_event(&up, &mut event_ctx, &app));
        assert!(*checked.borrow());
        assert!(switch.checked());
    }
}
