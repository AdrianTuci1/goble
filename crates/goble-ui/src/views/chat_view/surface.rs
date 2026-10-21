//! The transcript's selectable surface: the scrollable transcript plus the
//! recording and highlight that make its text draggable.
//!
//! The tree is rebuilt every frame, so the view cannot hold the elements' line
//! boxes. This wrapper clears a shared [`SelectableText`] before painting the
//! transcript (each text element appends the rows it drew) and then paints the
//! live selection over them. It also publishes its viewport, which is what the
//! view hit-tests a press against.

use std::cell::RefCell;
use std::rc::Rc;

use crate::color::ColorU;
use crate::elements::{
    AppContext, Element, LayoutContext, PaintContext, Point, SelectableText, SizeConstraint,
    TranscriptSelection,
};
use crate::geometry::{rectf, RectF, Vector2F};
use crate::theme::ColorToken;

/// How opaque the selection is over the transcript's own background. Matches
/// the terminal pane's selection, so the two read as one gesture.
const SELECTION_ALPHA: u8 = 70;

pub(super) struct TranscriptSurface {
    child: Box<dyn Element>,
    recording: Rc<RefCell<SelectableText>>,
    selection: Rc<RefCell<TranscriptSelection>>,
    viewport: Rc<RefCell<Option<RectF>>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TranscriptSurface {
    pub(super) fn new(
        child: Box<dyn Element>,
        recording: Rc<RefCell<SelectableText>>,
        selection: Rc<RefCell<TranscriptSelection>>,
        viewport: Rc<RefCell<Option<RectF>>>,
    ) -> Self {
        Self {
            child,
            recording,
            selection,
            viewport,
            size: None,
            origin: None,
        }
    }
}

impl Element for TranscriptSurface {
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
        let Some(size) = self.size else {
            return;
        };
        let bounds = rectf(origin.x, origin.y, size.x, size.y);
        *self.viewport.borrow_mut() = Some(bounds);

        // This frame's recording replaces the last one: a row that is no longer
        // drawn must not answer a press.
        {
            let mut recording = self.recording.borrow_mut();
            recording.clear();
        }
        self.child.paint(origin, ctx, app);

        // Keep only the rows inside the viewport. The scrollable paints every
        // child, including the ones above and below the clip, and a pointer in
        // the gap between two visible rows must not fall back to one of them.
        {
            let mut recording = self.recording.borrow_mut();
            recording.lines.retain(|line| {
                line.top + line.height > bounds.min().y && line.top < bounds.max().y
            });
        }

        let rects = self.selection.borrow().highlight_rects();
        if rects.is_empty() {
            return;
        }
        let Some(renderer) = ctx.renderer.as_mut() else {
            return;
        };
        let accent = app.theme.color(ColorToken::Accent);
        let color = ColorU::new(accent.r, accent.g, accent.b, SELECTION_ALPHA);
        renderer.clip_rect(bounds);
        for rect in rects {
            renderer.fill_rect(rect, color);
        }
        renderer.pop_clip();
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &crate::event::DispatchedEvent,
        ctx: &mut crate::elements::EventContext,
        app: &AppContext,
    ) -> bool {
        // The wrapper adds recording, not behaviour: points, clicks and the
        // wheel keep reaching the transcript under it.
        self.child.dispatch_event(event, ctx, app)
    }
}
