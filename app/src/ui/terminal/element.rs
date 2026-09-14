//! The [`Element`] impl: the pane's layout, painting and event dispatch.

use goble_terminal::MouseAction;
use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;

use super::TerminalView;
use super::mouse_button;

impl Element for TerminalView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.rebuild(app, constraint);
        let width = constraint.max.x.max(constraint.min.x);
        // Fill the viewport height so the pane is a solid, clickable surface.
        let height = constraint.max.y.max(constraint.min.y);
        let size = Vector2F::new(width, height);
        if let Some(root) = self.root.as_mut() {
            let _ = root.layout(
                SizeConstraint::new(Vector2F::zero(), Vector2F::new(width, height)),
                ctx,
                app,
            );
        }
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if let Some(root) = self.root.as_mut() {
            root.paint(origin, ctx, app);
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
        match event {
            DispatchedEvent::MouseDown { position, button } => {
                if let Some(bounds) = self.bounds() {
                    if !contains(bounds, *position) {
                        return false;
                    }
                } else {
                    return false;
                }
                (self.on_activate.borrow_mut())(self.pane_id);
                // The shared input bar is a child of the pane and owns its own
                // gestures — its editor, its footer buttons and the menus those
                // buttons open (which paint above the bar, so a bounds test on
                // the bar alone would miss them). A click it does not take is a
                // click on the shell, handled below. The bar's editor is
                // configured not to blur on a click outside it (the rich input
                // is the pane's only typing surface), so a click on the output
                // leaves the keyboard where it is.
                if let Some(root) = self.root.as_mut() {
                    if root.dispatch_event(event, ctx, app) {
                        return true;
                    }
                }
                // A click on a conversation card reopens that conversation's
                // agent view. The card sits on top of the grid, so it consumes
                // the press instead of reporting it to the program. (The card's
                // own probe never consumes a click, so it was checked here.)
                if let Some(conversation_id) = self.card_at(*position) {
                    (self.on_open_agent_view.borrow_mut())(self.pane_id, conversation_id);
                    return true;
                }
                let Some(button) = mouse_button(*button) else {
                    return true;
                };
                if let Some(cell) = self.cell_at(*position) {
                    self.last_cell = Some(cell);
                    self.pressed = Some(button);
                    self.report_mouse(MouseAction::Press(button), cell);
                }
                true
            }
            DispatchedEvent::MouseUp { position, button } => {
                // The bar's own gesture (its buttons fire on release) if the
                // release is over it; otherwise the release is the grid's.
                if let Some(root) = self.root.as_mut() {
                    if root.dispatch_event(event, ctx, app) {
                        return true;
                    }
                }
                // A release is reported even when the pointer has already left
                // the pane: the program is still holding the button down.
                let Some(cell) = self.cell_at(*position).or(self.last_cell) else {
                    return false;
                };
                self.last_cell = Some(cell);
                self.pressed = None;
                if let Some(button) = mouse_button(*button) {
                    self.report_mouse(MouseAction::Release(button), cell);
                }
                true
            }
            DispatchedEvent::MouseMove { position } => {
                self.pointer = Some(*position);
                // Hover belongs to whatever is under the pointer: the bar's
                // controls, or the grid's cells (a drag/move report).
                if let Some(root) = self.root.as_mut() {
                    let _ = root.dispatch_event(event, ctx, app);
                }
                self.handle_mouse_move(*position);
                // Motion is never consumed: the pane does not own the pointer.
                false
            }
            DispatchedEvent::Scroll { delta } => {
                if !self.pointer_inside() {
                    return false;
                }
                // The block view's own viewport takes the wheel first (it
                // consumes it only while the pointer is over it); a grid — a
                // full-screen program, or a shell with no block yet — scrolls
                // the session's scrollback instead.
                if let Some(root) = self.root.as_mut() {
                    if root.dispatch_event(event, ctx, app) {
                        return true;
                    }
                }
                self.handle_wheel(delta.y);
                true
            }
            // The window gained or lost the focus. Only the active pane is the
            // one the user is looking at, so only it reports the change.
            DispatchedEvent::Focus { gained } => {
                self.report_focus(*gained);
                false
            }
            DispatchedEvent::KeyDown { key, modifiers } => {
                // Only the active terminal pane receives keystrokes; a background
                // pane must not steal them from the active pane.
                if !self.active {
                    return false;
                }
                // While the shared bar holds the keyboard its editor gets the
                // keys (typing, Backspace, Enter to submit); the shell gets them
                // again as soon as the editor is not the focused one.
                if self.composer_focused {
                    if let Some(root) = self.root.as_mut() {
                        return root.dispatch_event(event, ctx, app);
                    }
                    return false;
                }
                self.handle_key(key, *modifiers)
            }
            _ => false,
        }
    }
}
