//! The composer's anchored popover layer: a panel painted **above** the input
//! without taking a row of the pane's layout.
//!
//! The slash-command list used to be a column child over the composer, so
//! opening it shrank the transcript, moved the composer up and (because the
//! editor is rebuilt under it) could move the caret. The owner's contract is
//! the opposite: the menu is an overlay. It hangs over the transcript from the
//! composer's top edge, the composer keeps the same rect and the transcript
//! keeps the same scroll position whether the list is up or down.
//!
//! This wrapper is that overlay. It lays the composer out exactly as the column
//! would and reports the composer's own size, then lays the panel out at the
//! composer's width and queues it on [`AppContext::hover_chips`] — the same
//! last-painted layer tooltips and the usage popover ride — so the panel paints
//! over the composer, the transcript and the pane's chrome instead of being
//! clipped by them. The wrapper keeps the shared panel handle so pointer events
//! inside the panel still reach the menu's rows; the layer only draws it.
//!
//! The panel is anchored above the composer's top edge and clamped into the
//! window: it never runs off the leading/trailing edge, and when the pane is
//! too short above the input it stops at the window's inset rather than being
//! pushed below the input (a command list under the editor would cover the very
//! field it filters). It is a menu, so it must stay above the composer.

use std::cell::RefCell;
use std::rc::Rc;

use crate::elements::interactive::contains;
use crate::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use crate::event::DispatchedEvent;
use crate::geometry::{rectf, vec2f, RectF, Vector2F};

/// The air the popover keeps from the window's top edge when the pane has no
/// room above the composer. The window is the last resort; an ordinary pane
/// has the whole transcript to hang the card over.
const WINDOW_INSET: f32 = 12.0;

/// What the view needs to route pointer events to the floating panel before the
/// transcript sees them.
///
/// The panel is painted by the last layer, so it is not a sibling in the tree
/// that the column's dispatch order already reaches first. The view holds this
/// cell next to the element and checks it at the top of its own
/// `dispatch_event`: a press on the card is the menu's, not a row's behind it.
#[derive(Default)]
pub(super) struct OverlayPanelState {
    panel: Option<Rc<RefCell<Box<dyn Element>>>>,
    rect: Option<RectF>,
}

impl OverlayPanelState {
    /// The panel at `position`, when the open panel's painted rect holds it.
    /// The caller dispatches the event into the returned handle.
    pub(super) fn panel_at(&self, position: Vector2F) -> Option<Rc<RefCell<Box<dyn Element>>>> {
        let rect = self.rect?;
        contains(rect, position)
            .then(|| self.panel.clone())
            .flatten()
    }
}

/// A panel anchored above a composer-sized trigger.
///
/// The trigger is laid out normally and is the wrapper's reported size: the
/// panel contributes nothing to the pane's column. The wrapper only positions
/// and queues the panel; its contents (the slash card's rows, menu claim and
/// keyboard highlight) are the panel's own.
pub(super) struct ComposerOverlay {
    /// The composer (or any content-sized row) the panel hangs from.
    trigger: Box<dyn Element>,
    /// The panel built while it is open. Shared with the last-painted layer,
    /// which draws it while this wrapper keeps the same handle to lay it out
    /// and to dispatch pointer events into it. `None` while closed.
    panel: Option<Rc<RefCell<Box<dyn Element>>>>,
    /// The gap between the panel's bottom and the trigger's top. The view passes
    /// the column's own spacing here, so the card sits exactly where the
    /// in-flow band used to: the overlay changed how the menu is painted, not
    /// where the anchors put it.
    gap: f32,
    panel_size: Option<Vector2F>,
    /// The panel's window-space rect from the last paint. Pointer presses are
    /// read against it, so the clamped position is also the hit-test position.
    panel_rect: Option<RectF>,
    /// The width the wrapper's column offered, so an over-wide panel can be
    /// pulled back inside the pane.
    available_width: Option<f32>,
    /// The view's own handle on the panel and its painted rect, so the view's
    /// dispatch can give the floating card the first chance at a pointer event.
    shared: Option<Rc<RefCell<OverlayPanelState>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ComposerOverlay {
    /// Wrap `trigger` with the panel it anchors. `panel` is `None` while the
    /// menu is closed, so a closed overlay pays for neither layout nor paint.
    pub(super) fn new(trigger: Box<dyn Element>, panel: Option<Box<dyn Element>>) -> Self {
        Self {
            trigger,
            panel: panel.map(|panel| Rc::new(RefCell::new(panel))),
            gap: 0.0,
            panel_size: None,
            panel_rect: None,
            available_width: None,
            shared: None,
            size: None,
            origin: None,
        }
    }

    /// The air between the panel and the trigger's top edge. Defaults to zero
    /// (the panel's own padding is then the whole gap).
    pub(super) fn with_gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Share the painted panel and its rect with the view, so a press on the
    /// card can be routed to it before the transcript's rows are tried.
    pub(super) fn with_shared_state(mut self, shared: Rc<RefCell<OverlayPanelState>>) -> Self {
        self.shared = Some(shared);
        self
    }

    /// Where the panel was painted in window coordinates, when it is open.
    /// Exposed for the regression test that compares the closed and open
    /// frames; the view itself does not need it.
    #[cfg(test)]
    pub(super) fn panel_bounds(&self) -> Option<RectF> {
        self.panel_rect
    }
}

impl Element for ComposerOverlay {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // The trigger is the only child that contributes: the composer keeps the
        // rect the closed frame gives it, so the transcript above it keeps its
        // height and scroll position too.
        let size = self.trigger.layout(constraint, ctx, app);
        self.size = Some(size);
        self.available_width = constraint.max.x.is_finite().then_some(constraint.max.x);

        self.panel_size = None;
        if let Some(panel) = self.panel.as_ref() {
            // DSH's menu band spans the input block. The composer's own width is
            // the band's width, so the card lines up with the input card under
            // it instead of reading as a different surface.
            let width = size.x.max(0.0);
            let max_height = if constraint.max.y.is_finite() {
                constraint.max.y.max(0.0)
            } else {
                f32::INFINITY
            };
            let panel_size = panel.borrow_mut().layout(
                SizeConstraint::new(vec2f(width, 0.0), vec2f(width, max_height)),
                ctx,
                app,
            );
            self.panel_size = Some(panel_size);
        }
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        // The composer paints first, in its normal place.
        self.trigger.paint(origin, ctx, app);

        let (Some(panel), Some(panel_size)) = (self.panel.as_ref(), self.panel_size) else {
            self.panel_rect = None;
            if let Some(shared) = self.shared.as_ref() {
                *shared.borrow_mut() = OverlayPanelState::default();
            }
            return;
        };

        // Above the trigger, with the gap the view asks for. The x is the
        // trigger's own edge (the band already carries the card's inset), and a
        // pane narrower than the band pulls it back so it never spills past the
        // trailing edge.
        let mut at = vec2f(origin.x, origin.y - panel_size.y - self.gap);
        if at.y < WINDOW_INSET {
            at.y = WINDOW_INSET;
        }
        if let Some(available) = self.available_width {
            at.x = at.x.clamp(0.0, (available - panel_size.x).max(0.0));
        }
        let panel_rect = rectf(at.x, at.y, panel_size.x, panel_size.y);
        self.panel_rect = Some(panel_rect);
        if let Some(shared) = self.shared.as_ref() {
            *shared.borrow_mut() = OverlayPanelState {
                panel: Some(Rc::clone(panel)),
                rect: Some(panel_rect),
            };
        }
        // The last layer draws it, after the whole tree, so the menu is above
        // the composer and the transcript. Shared, because the wrapper still
        // needs to dispatch the menu's own clicks and hovers.
        app.hover_chips
            .borrow_mut()
            .push_shared(at, Rc::clone(panel));
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
        // A pointer event on the floating panel belongs to the menu: forward it
        // to the same panel the last layer painted. A press on the card's empty
        // casing is still the menu's, not a click-through to the transcript.
        let pointer = match event {
            DispatchedEvent::MouseDown { position, .. }
            | DispatchedEvent::MouseUp { position, .. }
            | DispatchedEvent::MouseMove { position } => Some(*position),
            _ => None,
        };
        if let (Some(position), Some(panel_rect), Some(panel)) =
            (pointer, self.panel_rect, self.panel.as_ref())
        {
            if contains(panel_rect, position) {
                // The card owns every pointer event on its surface, even one
                // its rows do not use (the casing between rows), so nothing
                // behind it reacts to the same press.
                panel.borrow_mut().dispatch_event(event, ctx, app);
                return true;
            }
        }
        // Everything else is the composer's: the editor keeps the caret and the
        // keyboard, including the keys that move the command list's selection.
        self.trigger.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{Empty, HoverChipLayer};
    use crate::geometry::vec2f;
    use crate::render::Renderer;

    fn panel() -> Box<dyn Element> {
        Empty::new().with_size(vec2f(200.0, 80.0)).finish()
    }

    /// Lay out and paint one frame, then drain the last layer, returning the
    /// wrapper's size and the panel's painted rect.
    fn frame(overlay: &mut ComposerOverlay) -> (Vector2F, Option<RectF>) {
        let app = AppContext::default();
        let size = overlay.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        overlay.paint(vec2f(0.0, 300.0), &mut ctx, &app);
        let queued = app.hover_chips.borrow().len();
        let mut layer_ctx = PaintContext::default();
        HoverChipLayer::new().paint(vec2f(0.0, 0.0), &mut layer_ctx, &app);
        assert_eq!(queued, 1, "an open panel queues itself for the last layer");
        (size, overlay.panel_bounds())
    }

    #[test]
    fn the_panel_takes_no_layout_height_and_hangs_above_the_trigger() {
        let mut overlay = ComposerOverlay::new(
            Empty::new().with_size(vec2f(600.0, 120.0)).finish(),
            Some(panel()),
        )
        .with_gap(8.0);
        let (size, rect) = frame(&mut overlay);
        assert_eq!(
            size,
            vec2f(600.0, 120.0),
            "the wrapper reports the trigger's size, not the panel's"
        );
        let rect = rect.expect("the open panel records its painted rect");
        assert!(
            (rect.max_y() - (300.0 - 8.0)).abs() < 0.01,
            "the panel's bottom is the trigger's top minus the gap: {rect:?}"
        );
    }

    #[test]
    fn a_closed_overlay_queues_nothing() {
        let mut overlay =
            ComposerOverlay::new(Empty::new().with_size(vec2f(600.0, 120.0)).finish(), None);
        let app = AppContext::default();
        let size = overlay.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        overlay.paint(vec2f(0.0, 300.0), &mut PaintContext::default(), &app);
        assert_eq!(size, vec2f(600.0, 120.0));
        assert!(app.hover_chips.borrow().is_empty());
        assert!(overlay.panel_bounds().is_none());
    }

    #[test]
    fn a_pane_too_short_still_hangs_the_panel_above_the_trigger() {
        let mut overlay = ComposerOverlay::new(
            Empty::new().with_size(vec2f(600.0, 120.0)).finish(),
            Some(Empty::new().with_size(vec2f(600.0, 300.0)).finish()),
        )
        .with_gap(8.0);
        let app = AppContext::default();
        overlay.layout(
            SizeConstraint::loose(vec2f(600.0, 400.0)),
            &mut LayoutContext::default(),
            &app,
        );
        overlay.paint(vec2f(0.0, 180.0), &mut PaintContext::default(), &app);
        let rect = overlay.panel_bounds().expect("the panel is open");
        assert!(
            (rect.min_y() - WINDOW_INSET).abs() < 0.01,
            "a panel with no room above is clamped to the window's inset: {rect:?}"
        );
        assert!(
            rect.max_y() <= 312.0 + 0.01,
            "the clamped panel stays inside the window: {rect:?}"
        );
        // It still rides the last layer, so even an unavoidable overlap is
        // painted above the composer rather than behind it.
        assert_eq!(overlay.panel_size, Some(vec2f(600.0, 300.0)));
    }
}
