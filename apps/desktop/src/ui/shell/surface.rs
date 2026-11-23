//! The shell's one rounded workspace surface.
//!
//! The shell owns the sheet the main panel and the side panel sit on: it paints
//! the `Surface` fill, the rounded outer corners and the free margin of the
//! shell's own `Surface` band around it. The panels are flat children inside it — they
//! paint their own backgrounds but no casing, border or radius of their own.
//! The shared main/side-panel boundary is square and carries only the one
//! hairline separator the split layout paints.

use goble_ui::elements::{
    AppContext, Container, EdgeInsets, Element, EventContext, LayoutContext, PaintContext, Point,
    SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{rectf, Vector2F};
use goble_ui::theme::ColorToken;
use goble_ui::ColorU;

/// The free margin the workspace surface keeps from the shell (sidebar,
/// topbar, window edges). It is painted with the shell's own `Surface`, the same
/// grey as the topbar and the sidebar, so the content space reads as one light
/// grey material around the sheet rather than a dark frame.
pub const WORKSPACE_MARGIN: f32 = 6.0;

/// The workspace surface's outer corner radius. It is larger than a pane
/// card's because it wraps BOTH panels; the shared main/side-panel boundary is
/// square.
const WORKSPACE_RADIUS: f32 = 12.0;

/// The one rounded sheet the whole workspace (main panel + side panel) lives
/// on. The shell renders this; the panels are flat children inside it.
struct WorkspaceSurface {
    child: Box<dyn Element>,
    fill: ColorU,
    backdrop: ColorU,
    radius: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl WorkspaceSurface {
    fn new(child: Box<dyn Element>, app: &AppContext) -> Self {
        // The content space reads as the shell's grey. Dark themes use the
        // sidebar's `Surface`; the light theme's `Surface` is pure white, so it
        // uses its own light grey (`Bg`) instead of a white frame.
        // One flat dark surface for the whole pane area, edges included: no
        // rounded sheet and no band, so the resize zone reads as the same
        // black material as the panels.
        // The 6 pt shell margin is the grey `Surface`; the 12 pt rounded sheet
        // inside it is the black content material.
        let backdrop = app.theme.color(ColorToken::Surface);
        Self {
            child,
            fill: app.theme.color(ColorToken::ContentBg),
            backdrop,
            radius: WORKSPACE_RADIUS,
            size: None,
            origin: None,
        }
    }
}

impl Element for WorkspaceSurface {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        let Some(size) = self.size else { return };
        let sheet = rectf(origin.x, origin.y, size.x, size.y);
        if let Some(renderer) = ctx.renderer.as_mut() {
            renderer.fill_rounded_rect(sheet, self.fill, self.radius);
        }
        self.child.paint(origin, ctx, app);
        if let Some(renderer) = ctx.renderer.as_mut() {
            // Cut what the children painted past the rounded path back to the
            // outer shell colour. The band is one margin wide: the deepest
            // spandrel of this radius is under 5 pt, so it stays in the free
            // margin and never reaches the sidebar or topbar.
            let band = WORKSPACE_MARGIN;
            renderer.stroke_rect(
                rectf(
                    sheet.min_x() - band,
                    sheet.min_y() - band,
                    sheet.width() + band * 2.0,
                    sheet.height() + band * 2.0,
                ),
                self.backdrop,
                band,
                self.radius + band,
            );
            // Flat workspace surface: no Border, no shadow/elevation. The
            // sheet is part of the shell; panels sit flat inside it, separated
            // only by the 1 pt hairline.
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn wants_animation(&self) -> bool {
        self.child.wants_animation()
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

/// Wrap `content` in the shell's workspace sheet and its free margin. The
/// main and side panels are handed in as flat children.
pub fn build_workspace_surface(app: &AppContext, content: Box<dyn Element>) -> Box<dyn Element> {
    let sheet = WorkspaceSurface::new(content, app).finish();
    Container::new(sheet)
        .with_padding(EdgeInsets::uniform(WORKSPACE_MARGIN))
        .finish()
}
