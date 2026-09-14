use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::geometry::{vec2f, Vector2F};
use crate::platform::text_atlas::FontWeight;
use crate::theme::{ColorToken, FontFamily};

/// The spinner's frames: the circle quadrants a running tool call already uses
/// (`chat_message_bubble`), so the footer's spinner and the transcript's running
/// glyph agree on what "running" looks like.
pub const SPINNER_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];

/// A status indicator that signals an ongoing operation.
///
/// It paints one frame of a spinner. The phase is supplied by the caller: the
/// element layer has no clock and no frame counter, so the app that owns the
/// observed turn start passes the elapsed time and the frame advances with it
/// (see [`crate::elements::TurnStatusFooter`]).
pub struct RunningIndicator {
    size: f32,
    /// Phase in `0..1`; the frame advances through [`SPINNER_FRAMES`].
    phase: f32,
    color: ColorToken,
    size_cache: Option<Vector2F>,
    origin: Option<Point>,
}

impl RunningIndicator {
    pub fn new() -> Self {
        Self {
            size: 12.0,
            phase: 0.0,
            color: ColorToken::Accent,
            size_cache: None,
            origin: None,
        }
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    /// Set the phase in `0..1`. The frame shown is `phase * SPINNER_FRAMES.len()`.
    pub fn with_phase(mut self, phase: f32) -> Self {
        self.phase = phase;
        self
    }

    pub fn with_color_token(mut self, color: ColorToken) -> Self {
        self.color = color;
        self
    }

    /// The glyph this indicator draws for its current phase.
    pub fn frame(&self) -> &'static str {
        let count = SPINNER_FRAMES.len();
        let index = (self.phase.rem_euclid(1.0) * count as f32) as usize;
        SPINNER_FRAMES[index.min(count - 1)]
    }
}

impl Default for RunningIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl Element for RunningIndicator {
    fn layout(
        &mut self,
        _constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = vec2f(self.size, self.size);
        self.size_cache = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size_cache else { return };
        if size.x <= 0.0 || size.y <= 0.0 {
            return;
        }
        let color = app.theme.color(self.color);
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.draw_text_with_font(
                origin,
                self.frame(),
                size.y,
                color,
                size.x,
                size.y,
                FontWeight::Regular,
                FontFamily::Mono,
                false,
            );
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size_cache
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::AppContext;
    use crate::render::{RenderCommand, Renderer};

    #[test]
    fn indicator_has_requested_size() {
        let app = AppContext::default();
        let mut indicator = RunningIndicator::new().with_size(12.0);
        let size = indicator.layout(
            SizeConstraint::loose(vec2f(100.0, 100.0)),
            &mut LayoutContext,
            &app,
        );
        assert_eq!(size, vec2f(12.0, 12.0));
    }

    #[test]
    fn indicator_paints_a_spinner_frame() {
        let app = AppContext::default();
        let mut indicator: Box<dyn Element> = RunningIndicator::new().with_size(12.0).finish();
        indicator.layout(
            SizeConstraint::loose(vec2f(100.0, 100.0)),
            &mut LayoutContext,
            &app,
        );
        let mut paint_ctx = PaintContext::new(Renderer::new());
        indicator.paint(vec2f(4.0, 8.0), &mut paint_ctx, &app);
        let commands = paint_ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();

        assert!(
            commands.iter().any(|c| matches!(
                c,
                RenderCommand::DrawText { text, .. } if text == "◐"
            )),
            "the indicator must actually draw a spinner frame: {commands:?}"
        );
    }

    #[test]
    fn the_phase_advances_the_frame() {
        assert_eq!(RunningIndicator::new().with_phase(0.0).frame(), "◐");
        assert_eq!(RunningIndicator::new().with_phase(0.3).frame(), "◓");
        assert_eq!(RunningIndicator::new().with_phase(0.6).frame(), "◑");
        assert_eq!(RunningIndicator::new().with_phase(0.9).frame(), "◒");
        // A phase past the end wraps rather than drawing nothing.
        assert_eq!(RunningIndicator::new().with_phase(1.4).frame(), "◓");
    }
}
