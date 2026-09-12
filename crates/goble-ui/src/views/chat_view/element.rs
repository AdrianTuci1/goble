use crate::elements::{AppContext, Element, LayoutContext, PaintContext, Point, SizeConstraint};
use crate::event::DispatchedEvent;
use crate::geometry::Vector2F;

use super::ChatView;

impl Element for ChatView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // The topbar floats above the transcript (its tray must paint over the
        // messages), so its height is measured first and reserved in the column
        // the rebuild lays out under it.
        let header_height = self
            .header
            .as_mut()
            .map(|header| header.layout(constraint, ctx, app).y)
            .unwrap_or(0.0);
        self.rebuild(app, header_height);
        let size = self.root.as_mut().unwrap().layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.root.as_mut().unwrap().paint(origin, ctx, app);
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
        app: &AppContext,
    ) -> bool {
        // `e` advances the transcript's tool calls one step through
        // Collapsed -> Truncated -> Expanded. It is a transcript key only while
        // the composer is not holding the keyboard, so typing an `e` into a
        // draft still inserts it.
        if let DispatchedEvent::KeyDown { key, modifiers } = event {
            if self.pane_active
                && !self.composer_focused
                && key == "e"
                && !modifiers.ctrl
                && !modifiers.command
                && !modifiers.alt
            {
                self.advance_tool_folds();
                return true;
            }
        }
        let consumed = self
            .root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false);
        // Escape is the way back from a read-only transcript (the sub-agent
        // child view) — but only once the tree had its chance: a composer
        // proposal consumes Escape as its reject, and that wins.
        if !consumed {
            if let DispatchedEvent::KeyDown { key, modifiers } = event {
                if self.pane_active
                    && key == "Escape"
                    && !modifiers.ctrl
                    && !modifiers.command
                    && !modifiers.alt
                {
                    if let Some(cb) = self.on_escape.as_ref() {
                        (cb.borrow_mut())();
                        return true;
                    }
                }
            }
        }
        consumed
    }
}
