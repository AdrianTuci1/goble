//! The terminal composer's outer surface: the pane's own bottom band.
//!
//! The rich input is one card on every host, but on a terminal pane it is not
//! a floating object: the owner asked for it to read as the bottom band of the
//! panel — full pane width, flush with the pane's sides and bottom, square
//! against the transcript above it, and rounded only where the panel itself is
//! rounded (its bottom corners). The inner input segment keeps its own radius
//! and its casing inset, so the band carries a rounded field inside it.
//!
//! A plain [`Container`](crate::elements::Container) paints one radius on all
//! four corners, so the band is its own element: it paints the card's fill as a
//! box rounded at the panel's radius, then squares the top by filling the top
//! `radius` strip with the same tone. The renderer's rounded-rect primitive
//! (and the offline command list) stay unchanged, and the shape is exactly the
//! rectangle-minus-two-bottom-corner-notches a bottom band needs.

use crate::color::ColorU;
use crate::elements::{
    AppContext, EdgeInsets, Element, EventContext, LayoutContext, PaintContext, Point,
    SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, Vector2F};

/// A surface that stretches its child with padding and paints itself as the
/// bottom band of a panel: square top corners, the bottom two corners rounded
/// at the panel's radius.
pub(super) struct BandCard {
    child: Option<Box<dyn Element>>,
    background: ColorU,
    /// The panel's corner radius: the band's bottom corners carry it, and the
    /// top `radius` strip is squared over it.
    bottom_radius: f32,
    padding: EdgeInsets,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl BandCard {
    pub(super) fn new(child: Box<dyn Element>) -> Self {
        Self {
            child: Some(child),
            background: ColorU::new(0, 0, 0, 0),
            bottom_radius: 0.0,
            padding: EdgeInsets::uniform(0.0),
            size: None,
            origin: None,
        }
    }

    pub(super) fn with_background(mut self, color: ColorU) -> Self {
        self.background = color;
        self
    }

    pub(super) fn with_bottom_radius(mut self, radius: f32) -> Self {
        self.bottom_radius = radius.max(0.0);
        self
    }

    pub(super) fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    pub(super) fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}

impl Element for BandCard {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let horizontal = self.padding.left + self.padding.right;
        let vertical = self.padding.top + self.padding.bottom;
        let inner_max = Vector2F::new(
            (constraint.max.x - horizontal).max(0.0),
            (constraint.max.y - vertical).max(0.0),
        );
        let inner_min = Vector2F::new(
            (constraint.min.x - horizontal).max(0.0),
            (constraint.min.y - vertical).max(0.0),
        );
        let child_size = match self.child.as_mut() {
            Some(child) => child.layout(SizeConstraint::new(inner_min, inner_max), ctx, app),
            None => Vector2F::zero(),
        };
        let size = Vector2F::new(child_size.x + horizontal, child_size.y + vertical);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let size = self.size.unwrap_or(Vector2F::zero());
        let rect = rectf(origin.x, origin.y, size.x, size.y);
        if let Some(renderer) = ctx.renderer.as_mut() {
            // The rounded box first, then the square top strip over the two
            // notches its top corners leave (same tone, so only the notches
            // change): the band's top edge is square against the transcript and
            // its bottom corners carry the panel's radius.
            renderer.fill_rounded_rect(rect, self.background, self.bottom_radius);
            let strip = self.bottom_radius.min(size.y);
            if strip > 0.0 {
                renderer.fill_rect(rectf(origin.x, origin.y, size.x, strip), self.background);
            }
        }
        if let Some(child) = self.child.as_mut() {
            child.paint(
                Vector2F::new(origin.x + self.padding.left, origin.y + self.padding.top),
                ctx,
                app,
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.child
            .as_mut()
            .map(|child| child.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::AppContext;

    #[test]
    fn the_band_paints_a_bottom_rounded_box_and_squares_its_top() {
        use crate::elements::Element as _;
        use crate::render::{RenderCommand, Renderer};

        let app = AppContext::default();
        let color = app.theme.color(crate::theme::ColorToken::Surface);
        let mut band = BandCard::new(
            crate::elements::Empty::new()
                .with_size(Vector2F::new(400.0, 146.0))
                .finish(),
        )
        .with_background(color)
        .with_bottom_radius(10.0)
        .finish();
        let size = band.layout(
            SizeConstraint::tight(Vector2F::new(400.0, 146.0)),
            &mut LayoutContext::default(),
            &app,
        );
        assert_eq!(size, Vector2F::new(400.0, 146.0));
        let mut ctx = PaintContext::new(Renderer::new());
        band.paint(Vector2F::zero(), &mut ctx, &app);
        let commands = ctx
            .renderer
            .take()
            .map(|r| r.commands().to_vec())
            .unwrap_or_default();
        let rounded: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color: painted,
                    corner_radius,
                } if *painted == color && *corner_radius > 0.0 => Some((*rect, *corner_radius)),
                _ => None,
            })
            .collect();
        assert_eq!(
            rounded.len(),
            1,
            "one rounded box paints the band: {commands:?}"
        );
        assert!((rounded[0].1 - 10.0).abs() < 0.01);
        assert_eq!(rounded[0].0, rectf(0.0, 0.0, 400.0, 146.0));
        let square: Vec<_> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect {
                    rect,
                    color: painted,
                    corner_radius,
                } if *painted == color && *corner_radius == 0.0 => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(
            square.len(),
            1,
            "one square strip squares the top: {commands:?}"
        );
        assert_eq!(square[0], rectf(0.0, 0.0, 400.0, 10.0));
    }
}
