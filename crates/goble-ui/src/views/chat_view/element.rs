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
        // Drag-to-copy over the transcript. The press freezes the rows the last
        // paint recorded, so the pointer keeps reading the text the user pressed
        // on while the stream moves. The events still reach the tree: a press on
        // a tool row or a link is that element's click, and a plain click with
        // no drag copies nothing.
        if let Some(selection) = self.transcript_selection.as_ref() {
            match event {
                DispatchedEvent::MouseDown { position, .. } => {
                    let inside = self
                        .transcript_viewport
                        .borrow()
                        .map(|bounds| {
                            position.x >= bounds.min().x
                                && position.x <= bounds.max().x
                                && position.y >= bounds.min().y
                                && position.y <= bounds.max().y
                        })
                        .unwrap_or(false);
                    if inside {
                        let recording = self.text_recording.borrow().clone();
                        if let Some(point) = recording.point_at(*position) {
                            selection.borrow_mut().begin(recording, point);
                        }
                    } else {
                        // A press anywhere else (the composer, the filter bar)
                        // is not a text gesture: put the highlight away.
                        selection.borrow_mut().cancel();
                    }
                }
                DispatchedEvent::MouseMove { position } => {
                    if selection.borrow().is_dragging() {
                        let point = selection.borrow().point_at(*position);
                        if let Some(point) = point {
                            selection.borrow_mut().extend(point);
                        }
                    }
                }
                DispatchedEvent::MouseUp { position, .. } => {
                    if selection.borrow().is_dragging() {
                        // The release point is the end even when the last move
                        // did not arrive.
                        let point = selection.borrow().point_at(*position);
                        if let Some(point) = point {
                            selection.borrow_mut().extend(point);
                        }
                        let copied = selection.borrow_mut().finish();
                        if let Some(text) = copied {
                            if let Some(copy) = self.on_copy_terminal.as_ref() {
                                (copy.borrow_mut())(text);
                            }
                            return true;
                        }
                    }
                }
                // The frozen rows do not follow the scroll, so a selection that
                // stayed would point at text that moved away.
                DispatchedEvent::Scroll { .. } => selection.borrow_mut().cancel(),
                _ => {}
            }
        }

        // `e` advances the transcript's tool calls one step through
        // Collapsed -> Truncated -> Expanded. It is a transcript key only while
        // the composer is not holding the keyboard, so typing an `e` into a
        // draft still inserts it.
        if let DispatchedEvent::KeyDown { key, modifiers } = event {
            let transcript_key = self.pane_active
                && !self.composer_focused
                && !modifiers.ctrl
                && !modifiers.command
                && !modifiers.alt;
            if transcript_key {
                match key.as_str() {
                    // grok-build's fold keys: Right opens the selected entry,
                    // Left closes it. The transcript has no cursor of its own,
                    // so the newest foldable row is the one they act on — the
                    // live run is what the pane tails.
                    "ArrowRight" => return self.expand_latest_fold(),
                    "ArrowLeft" => return self.collapse_latest_fold(),
                    "e" => {
                        self.advance_tool_folds();
                        return true;
                    }
                    _ => {}
                }
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
