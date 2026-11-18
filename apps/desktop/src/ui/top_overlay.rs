//! The shared top-center overlay placement.
//!
//! Two surfaces hang under the topbar, centered, without taking a slot in the
//! tree they are mounted in: the inbox's turn-finished announcement and the
//! session menu's transient toast. Both are window-level overlays (the panes
//! below would paint over a normal child), so they paint themselves at the top
//! and forward every event to their content, whose controls hit-test
//! themselves.

use goble_ui::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{vec2f, Vector2F};

use super::shell::TOPBAR_HEIGHT;

/// The room the banner keeps from the window's edges.
const EDGE_MARGIN: f32 = 8.0;

/// The default content width: wide enough for a two-word title and its source
/// (the inbox announcement's own width).
const DEFAULT_MAX_WIDTH: f32 = 380.0;

/// Centers its content under the topbar and paints it in the overlay layer.
pub(crate) struct TopBanner {
    content: Box<dyn Element>,
    /// The widest the content may lay out; the inbox's banner and the session
    /// toast differ here.
    max_width: f32,
    window: Option<Vector2F>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TopBanner {
    pub(crate) fn new(content: Box<dyn Element>) -> Self {
        Self {
            content,
            max_width: DEFAULT_MAX_WIDTH,
            window: None,
            size: None,
            origin: None,
        }
    }

    /// Set the widest the content may lay out.
    pub(crate) fn with_max_width(mut self, max_width: f32) -> Self {
        self.max_width = max_width.max(0.0);
        self
    }

    fn place(&self, size: Vector2F) -> Vector2F {
        let Some(window) = self.window else {
            return vec2f(0.0, TOPBAR_HEIGHT + EDGE_MARGIN);
        };
        let x = ((window.x - size.x) / 2.0)
            .min(window.x - size.x - EDGE_MARGIN)
            .max(EDGE_MARGIN);
        vec2f(x, TOPBAR_HEIGHT + EDGE_MARGIN)
    }
}

impl Element for TopBanner {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.window = constraint.max.x.is_finite().then_some(constraint.max);
        let size = self.content.layout(
            SizeConstraint::loose(vec2f(self.max_width, f32::INFINITY)),
            ctx,
            app,
        );
        self.size = Some(size);
        size
    }

    fn paint(&mut self, _origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        let Some(size) = self.size else {
            return;
        };
        let at = self.place(size);
        self.origin = Some(Point::from_vec2f(at, Default::default()));
        self.content.paint(at, ctx, app);
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
        self.content.dispatch_event(event, ctx, app)
    }
}

impl TopBanner {
    /// Box the overlay for a parent's child list.
    pub(crate) fn finish(self) -> Box<dyn Element> {
        Box::new(self)
    }
}
