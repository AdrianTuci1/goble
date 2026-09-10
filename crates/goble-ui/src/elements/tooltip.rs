use crate::elements::{
    AppContext, Container, EdgeInsets, Element, EventContext, Fill, LayoutContext, PaintContext,
    Point, SizeConstraint,
};
use crate::elements::Text;
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, Vector2F};
use crate::theme::ColorToken;

const TOOLTIP_FONT_SIZE: f32 = 12.0;
const TOOLTIP_PADDING_H: f32 = 8.0;
const TOOLTIP_PADDING_V: f32 = 4.0;
const TOOLTIP_GAP: f32 = 6.0;
const TOOLTIP_MAX_WIDTH: f32 = 240.0;

/// Where a [`Tooltip`] is drawn relative to the wrapped child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TooltipPosition {
    Above,
    Below,
}

impl Default for TooltipPosition {
    fn default() -> Self {
        Self::Below
    }
}

/// Wraps a child and shows a short message box while the pointer hovers it.
///
/// The tooltip is drawn on top of the child and never affects layout. Its
/// visibility is decided at paint time from the render-time cursor position
/// (see [`PaintContext::hovered`]), because the element tree is rebuilt every
/// frame and element-local hover state would be reset before it is drawn.
pub struct Tooltip {
    child: Box<dyn Element>,
    message: String,
    position: TooltipPosition,
    padding: EdgeInsets,
    panel: Option<Box<dyn Element>>,
    panel_size: Option<Vector2F>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Tooltip {
    pub fn new(child: Box<dyn Element>, message: impl Into<String>) -> Self {
        Self {
            child,
            message: message.into(),
            position: TooltipPosition::default(),
            padding: EdgeInsets::new(TOOLTIP_PADDING_H, TOOLTIP_PADDING_V, TOOLTIP_PADDING_H, TOOLTIP_PADDING_V),
            panel: None,
            panel_size: None,
            size: None,
            origin: None,
        }
    }

    pub fn with_position(mut self, position: TooltipPosition) -> Self {
        self.position = position;
        self
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self.panel = None;
        self
    }

    fn ensure_panel(&mut self, app: &AppContext) {
        if self.panel.is_some() {
            return;
        }
        let text = Text::new(self.message.clone())
            .with_font_size(TOOLTIP_FONT_SIZE)
            .with_theme_color(ColorToken::Text, app)
            .finish();
        self.panel = Some(
            Container::new(text)
                .with_padding(self.padding)
                .with_corner_radius(4.0)
                .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
                .finish(),
        );
    }
}

impl Element for Tooltip {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.ensure_panel(app);
        let child_size = self.child.layout(constraint, ctx, app);
        self.size = Some(child_size);

        // Measure the panel so it can be positioned on paint. The panel sizes
        // to its content, clamped to a max width so long messages wrap.
        let panel = self.panel.as_mut().expect("panel built");
        let panel_size = panel.layout(
            SizeConstraint::loose(vec2f(TOOLTIP_MAX_WIDTH, 200.0)),
            ctx,
            app,
        );
        self.panel_size = Some(panel_size);
        child_size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.child.paint(origin, ctx, app);

        let child_size = self.size.unwrap_or(Vector2F::zero());
        let child_bounds = rectf(origin.x, origin.y, child_size.x, child_size.y);
        if !ctx.hovered(child_bounds) {
            return;
        }

        let panel = match self.panel.as_mut() {
            Some(p) => p,
            None => return,
        };
        let panel_size = match self.panel_size {
            Some(s) => s,
            None => return,
        };

        // Centered over the child horizontally, kept within the window.
        let x = (origin.x + (child_size.x - panel_size.x) / 2.0).max(0.0);
        let y = match self.position {
            TooltipPosition::Above => origin.y - panel_size.y - TOOLTIP_GAP,
            TooltipPosition::Below => origin.y + child_size.y + TOOLTIP_GAP,
        }
        .max(0.0);

        panel.paint(vec2f(x, y), ctx, app);
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
        self.child.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{Empty, SizeConstraint};
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;

    fn child_box() -> Box<dyn Element> {
        Empty::new().with_size(vec2f(40.0, 32.0)).finish()
    }

    fn painted_commands(
        tooltip: &mut Tooltip,
        cursor: Vector2F,
        cursor_inside: bool,
    ) -> Vec<RenderCommand> {
        let app = AppContext::default();
        tooltip.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut paint_ctx = PaintContext::default();
        paint_ctx.cursor_position = cursor;
        paint_ctx.cursor_inside = cursor_inside;
        tooltip.paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        paint_ctx.renderer.take().unwrap().commands().to_vec()
    }

    #[test]
    fn tooltip_only_paints_its_panel_when_hovered() {
        let mut tooltip = Tooltip::new(child_box(), "Run");
        let bg = AppContext::default().theme.color(ColorToken::SurfaceRaised);

        let idle = painted_commands(&mut tooltip, vec2f(10.0, 10.0), false);
        assert!(
            !idle
                .iter()
                .any(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == bg)),
            "tooltip should not paint its panel when the cursor is not inside the window"
        );

        let hovering = painted_commands(&mut tooltip, vec2f(20.0, 16.0), true);
        assert!(
            hovering
                .iter()
                .any(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == bg)),
            "tooltip should paint its panel when the cursor is over the child"
        );
    }

    #[test]
    fn tooltip_layout_matches_child() {
        let app = AppContext::default();
        let mut tooltip = Tooltip::new(child_box(), "Run");
        let size = tooltip.layout(
            SizeConstraint::loose(vec2f(200.0, 200.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size, vec2f(40.0, 32.0));
    }
}
