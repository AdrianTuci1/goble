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
/// The chip never affects layout. Its visibility is decided at paint time from
/// the render-time cursor position (see [`PaintContext::hovered`]), because the
/// element tree is rebuilt every frame and element-local hover state would be
/// reset before it is drawn.
///
/// A hovered tooltip does not draw its box in its own paint pass — anything
/// painted after the wrapped child (the pane to the right of a sidebar tab, for
/// one) would cover it. It queues the box in the frame's hover-chip registry
/// instead, and the root's last-painted layer
/// ([`HoverChipLayer`](crate::elements::HoverChipLayer)) draws it above the
/// whole tree.
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

        // The panel moves to the frame's chip layer, which the root paints after
        // the whole tree. `layout` built it for this frame; the next rebuild
        // builds it again.
        let panel = match self.panel.take() {
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

        app.hover_chips.borrow_mut().push(vec2f(x, y), panel);
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
    use crate::elements::{Empty, HoverChipLayer, SizeConstraint};
    use crate::geometry::vec2f;
    use crate::render::RenderCommand;

    fn child_box() -> Box<dyn Element> {
        Empty::new().with_size(vec2f(40.0, 32.0)).finish()
    }

    /// What one frame paints: the commands the tooltip's own paint pass emitted,
    /// the commands the root's chip layer emitted after it, and how many chips
    /// the tooltip queued.
    struct Frame {
        own: Vec<RenderCommand>,
        layer: Vec<RenderCommand>,
        queued: usize,
    }

    fn paint_frame(tooltip: &mut Tooltip, cursor: Vector2F, cursor_inside: bool) -> Frame {
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
        let queued = app.hover_chips.borrow().len();

        let mut renderer = paint_ctx.renderer.take().expect("renderer");
        let own = renderer.commands().to_vec();
        renderer.clear();
        paint_ctx.renderer = Some(renderer);

        HoverChipLayer::new().paint(vec2f(0.0, 0.0), &mut paint_ctx, &app);
        let layer = paint_ctx
            .renderer
            .take()
            .expect("renderer")
            .commands()
            .to_vec();
        Frame { own, layer, queued }
    }

    /// The chip's box, as the layer drew it.
    fn chip_box(frames: &Frame, app: &AppContext) -> Option<crate::geometry::RectF> {
        let bg = app.theme.color(ColorToken::SurfaceRaised);
        frames.layer.iter().find_map(|command| match command {
            RenderCommand::FillRect { rect, color, .. } if *color == bg => Some(*rect),
            _ => None,
        })
    }

    #[test]
    fn tooltip_queues_its_chip_only_while_hovered() {
        let app = AppContext::default();

        let mut tooltip = Tooltip::new(child_box(), "Run");
        let idle = paint_frame(&mut tooltip, vec2f(10.0, 10.0), false);
        assert_eq!(idle.queued, 0, "the pointer outside the window hovers nothing");
        assert!(chip_box(&idle, &app).is_none(), "no chip is drawn");

        let hovering = paint_frame(&mut tooltip, vec2f(20.0, 16.0), true);
        assert_eq!(hovering.queued, 1, "the hovered tooltip queues its box");
        assert!(chip_box(&hovering, &app).is_some(), "the layer draws the chip");
    }

    #[test]
    fn tooltip_never_draws_its_panel_in_its_own_paint_pass() {
        let app = AppContext::default();
        let mut tooltip = Tooltip::new(child_box(), "Run");
        let bg = app.theme.color(ColorToken::SurfaceRaised);

        let hovering = paint_frame(&mut tooltip, vec2f(20.0, 16.0), true);
        assert!(
            !hovering
                .own
                .iter()
                .any(|c| matches!(c, RenderCommand::FillRect { color, .. } if *color == bg)),
            "anything painted after the child would cover an inline panel, so the \
             tooltip must only queue it"
        );
    }

    #[test]
    fn tooltip_chip_keeps_its_place_below_the_child() {
        let app = AppContext::default();
        let mut tooltip = Tooltip::new(child_box(), "Run");
        let hovering = paint_frame(&mut tooltip, vec2f(20.0, 16.0), true);

        let rect = chip_box(&hovering, &app).expect("the layer draws the chip");
        assert_eq!(rect.min_y(), 32.0 + TOOLTIP_GAP, "6 pt under the child");
        assert_eq!(
            rect.min_x(),
            (40.0 - rect.width()) / 2.0,
            "centered over the child"
        );
        assert!(rect.width() <= TOOLTIP_MAX_WIDTH);
        assert!(rect.height() <= 200.0);
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
