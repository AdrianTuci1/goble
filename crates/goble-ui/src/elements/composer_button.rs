//! The low-emphasis pill button used by the rich input.
//!
//! Borderless at rest: the control is its glyph and its label, and the pointer
//! over it adds the theme's soft rounded hover fill. The rich input's context
//! band and footer are rows of these, and the reference design keeps them flat
//! against the card they sit in — no outlines inside the composer.
//!
//! A control that fills its own box at rest — the send circle — asks for that
//! fill with [`ComposerButton::with_fill`]; a control that belongs to a card it
//! is drawn inside (the command proposal's candidates, the diff menu's trigger)
//! keeps the same flat shape, and a caller that needs the old outlined card can
//! still ask for it with [`ComposerButton::with_outline`].

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::{handle_mouse_event, InteractiveState};
use crate::elements::{
    AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint, Vector2F,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f};
use crate::theme::ColorToken;

/// The rich input's own corner radius: the controls sit inside the input's
/// segment at 24-32 pt tall, so their hover fill is a small rounded box rather
/// than the theme's card radius.
pub const COMPOSER_CONTROL_RADIUS: f32 = 6.0;

/// A refined, low-emphasis pill button used in the composer.
///
/// Borderless at rest with [`COMPOSER_CONTROL_RADIUS`] hover fill, and no
/// outline unless the caller asks for one. Fixed height with auto width for
/// icon+label content.
pub struct ComposerButton {
    child: Box<dyn Element>,
    state: InteractiveState,
    height: f32,
    /// The inset from the box's side edges to its content. A footer control
    /// keeps it small so a row of buttons stays compact.
    h_pad: f32,
    /// Whether the control outlines its own box at rest. Off by default: the
    /// composer's controls are flat, and only a caller that wants the older
    /// card look turns it back on.
    outlined: bool,
    /// The box's corner radius. The rich input's controls share
    /// [`COMPOSER_CONTROL_RADIUS`]; the send circle asks for half its height so
    /// the box is a circle.
    radius: f32,
    /// A solid fill drawn at rest, under the pointer's hover fill. `None` keeps
    /// the control flat until it is hovered (the composer's text controls); the
    /// send circle draws `Selected` at rest.
    rest_fill: Option<ColorToken>,
    /// How opaque the resting fill is. The stop circle is the error colour at
    /// 90%, so the input's surface reads through it a little.
    rest_fill_alpha: f32,
    /// The fill drawn under the pointer. Defaults to [`ColorToken::Hover`].
    hover_fill: ColorToken,
    /// A disabled control draws its content as the host built it (dimmed
    /// upstream) and takes no presses.
    disabled: bool,
    on_click: Option<Rc<RefCell<dyn FnMut() + 'static>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ComposerButton {
    pub fn new(child: Box<dyn Element>) -> Self {
        Self {
            child,
            state: InteractiveState::default(),
            height: 28.0,
            h_pad: 10.0,
            outlined: false,
            radius: COMPOSER_CONTROL_RADIUS,
            rest_fill: None,
            rest_fill_alpha: 1.0,
            hover_fill: ColorToken::Hover,
            disabled: false,
            on_click: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// The horizontal inset between the box's edges and its content. Footer
    /// controls in the rich input pass a tighter inset than the default so the
    /// row reads as one compact strip.
    pub fn with_h_pad(mut self, h_pad: f32) -> Self {
        self.h_pad = h_pad;
        self
    }

    /// Whether the control draws a resting outline. Off by default; a control
    /// that has to sit inside a card on a flat surface can turn it back on.
    pub fn with_outline(mut self, outlined: bool) -> Self {
        self.outlined = outlined;
        self
    }

    /// The corner radius of the control's own box. Zero gives the square
    /// corners the diff menu's trigger used to ask for.
    pub fn with_radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// A solid fill the control draws at rest. The send circle is `Selected`
    /// while the composer's text controls stay flat.
    pub fn with_fill(mut self, fill: ColorToken) -> Self {
        self.rest_fill = Some(fill);
        self
    }

    /// The resting fill's opacity (1.0 by default).
    pub fn with_fill_alpha(mut self, alpha: f32) -> Self {
        self.rest_fill_alpha = alpha.clamp(0.0, 1.0);
        self
    }

    /// The fill under the pointer. Defaults to [`ColorToken::Hover`].
    pub fn with_hover_fill(mut self, fill: ColorToken) -> Self {
        self.hover_fill = fill;
        self
    }

    /// A disabled control takes no presses (its content is dimmed by the
    /// caller, which knows what "disabled" means for that control).
    pub fn with_disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_on_click<F: FnMut() + 'static>(mut self, callback: F) -> Self {
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
        let h_pad = self.h_pad;
        let inner_max = vec2f(
            (constraint.max.x - h_pad * 2.0).max(0.0),
            (self.height - 4.0).max(0.0),
        );
        let child_size =
            self.child
                .layout(SizeConstraint::new(vec2f(0.0, 0.0), inner_max), ctx, app);
        let size = vec2f(child_size.x + h_pad * 2.0, self.height);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let rect = rectf(origin.x, origin.y, size.x, size.y);
        let hovered = !self.disabled && ctx.hovered(rect);
        let radius = self.radius;
        if let Some(renderer) = ctx.renderer.as_mut() {
            // A control with a resting fill keeps it under the pointer: the
            // send and stop circles are filled at rest, and swapping that fill
            // for the flat control's hover grey would turn them into holes in
            // the segment. The flat text controls are exactly the ones that
            // need the pointer's fill.
            if let Some(fill) = self.rest_fill {
                let mut color = app.theme.color(fill);
                color.a = (color.a as f32 * self.rest_fill_alpha).round() as u8;
                renderer.fill_rounded_rect(rect, color, radius);
            } else if hovered {
                renderer.fill_rounded_rect(rect, app.theme.color(self.hover_fill), radius);
            }
            if self.outlined {
                renderer.stroke_rect(rect, app.theme.color(ColorToken::Border), 1.0, radius);
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
        if self.disabled {
            return false;
        }
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
    use crate::elements::{EventContext, Icon, Text};
    use crate::event::DispatchedEvent;
    use crate::geometry::vec2f;
    use crate::test_util::render_element;

    fn app() -> AppContext {
        AppContext::default()
    }

    /// A control is borderless at rest: the composer's own card is the only
    /// outline on the surface, so the button paints no stroke of its own.
    #[test]
    fn a_composer_control_draws_no_resting_outline() {
        let app = app();
        let mut element: Box<dyn Element> =
            ComposerButton::new(Text::new("model").finish()).finish();
        let commands = render_element(&mut element, vec2f(200.0, 40.0), &app);
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, crate::render::RenderCommand::StrokeRect { .. })),
            "a flat control outlines nothing: {commands:?}"
        );
    }

    /// The send circle asks for a fill and a circle radius: the area it paints
    /// is one rounded box of the control's own height, filled at rest.
    #[test]
    fn a_filled_control_paints_its_rule_at_rest() {
        use crate::render::RenderCommand;

        let app = app();
        let mut element = Box::new(
            ComposerButton::new(Icon::new("send").with_size(16.0).finish())
                .with_height(32.0)
                .with_h_pad(8.0)
                .with_radius(16.0)
                .with_fill(ColorToken::Selected),
        ) as Box<dyn Element>;
        let commands = render_element(&mut element, vec2f(200.0, 40.0), &app);
        let fill = commands.iter().find_map(|command| match command {
            RenderCommand::FillRect {
                rect,
                color,
                corner_radius,
            } => Some((*rect, *color, *corner_radius)),
            _ => None,
        });
        let (rect, color, radius) = fill.expect("the circle fills its box");
        assert_eq!(color, app.theme.color(ColorToken::Selected));
        assert_eq!(radius, 16.0, "a 32pt box fills as a circle");
        assert!((rect.width() - 32.0).abs() < 0.5);
        assert!((rect.height() - 32.0).abs() < 0.5);
    }

    /// A disabled control takes no release, so the send circle cannot submit
    /// while the draft is empty.
    #[test]
    fn a_disabled_control_takes_no_press() {
        let app = app();
        let clicked = Rc::new(RefCell::new(false));
        let flag = clicked.clone();
        let mut element = Box::new(
            ComposerButton::new(Icon::new("send").with_size(16.0).finish())
                .with_height(32.0)
                .with_h_pad(8.0)
                .with_radius(16.0)
                .with_disabled(true)
                .with_on_click(move || *flag.borrow_mut() = true),
        ) as Box<dyn Element>;
        element.layout(
            SizeConstraint::loose(vec2f(200.0, 40.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let handled = element.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: vec2f(16.0, 16.0),
                button: 0,
            },
            &mut EventContext::default(),
            &app,
        );
        assert!(!handled, "a disabled control takes no press");
        assert!(!*clicked.borrow(), "and fires no callback");
    }
}
