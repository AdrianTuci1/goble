//! The [`Element`] impl: layout, paint, size and the global shortcuts.

use goble_ui::event::DispatchedEvent;
use goble_ui::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, Point, SizeConstraint, Vector2F,
};

use super::RootView;

impl Element for RootView {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // Apply the live theme (base dark/light + custom primary/secondary/
        // accent from the color wheel) before the tree is rebuilt this frame.
        self.apply_theme();
        self.rebuild(app);
        let size = self.element.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.element.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    /// Whether the frame clock still has a reason to run: an agent turn, a tool
    /// call, a pending approval or question, or a command printing into a pane.
    /// Everything else is event-driven, so an idle window idles instead of
    /// rebuilding and repainting the same frame.
    fn wants_animation(&self) -> bool {
        let state = self.state.borrow();
        let live = state.live_work();
        if live.turn_busy
            || !live.executions.is_empty()
            || !live.tool_calls.is_empty()
            || live.pending_approval.is_some()
            || live.pending_question.is_some()
        {
            return true;
        }
        let running = state.terminal.borrow().any_running_command();
        running
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        // Global shortcuts (warp-new style pane splitting + pane focus
        // navigation + the command palette). These are handled before the tree
        // so a focused composer cannot swallow the keys.
        if let Some(actions) = &self.actions {
            if let DispatchedEvent::KeyDown { key, modifiers } = event {
                let has_ctrl_cmd = modifiers.ctrl || modifiers.command;
                // Cmd+K toggles the command palette (no other modifier).
                if has_ctrl_cmd && !modifiers.shift && key.eq_ignore_ascii_case("k") {
                    (actions.on_toggle_command_palette.borrow_mut())();
                    return true;
                }
                if has_ctrl_cmd && !modifiers.shift && key == " " {
                    (actions.on_split_right.borrow_mut())();
                    return true;
                }
                if modifiers.command && modifiers.shift && key.eq_ignore_ascii_case("d") {
                    (actions.on_split_down.borrow_mut())();
                    return true;
                }
                if has_ctrl_cmd && modifiers.shift && key.eq_ignore_ascii_case("t") {
                    (actions.on_new_terminal.borrow_mut())();
                    return true;
                }
                if has_ctrl_cmd && !modifiers.shift && key.eq_ignore_ascii_case("w") {
                    (actions.on_close_pane.borrow_mut())();
                    return true;
                }
                // Pane focus navigation: Ctrl/Cmd+Arrows move between panes.
                if has_ctrl_cmd && !modifiers.shift && !modifiers.alt {
                    let dir = match key.as_str() {
                        "ArrowLeft" => Some(crate::ui::NavDir::Left),
                        "ArrowRight" => Some(crate::ui::NavDir::Right),
                        "ArrowUp" => Some(crate::ui::NavDir::Up),
                        "ArrowDown" => Some(crate::ui::NavDir::Down),
                        _ => None,
                    };
                    if let Some(dir) = dir {
                        (actions.on_pane_navigate.borrow_mut())(dir);
                        return true;
                    }
                }
            }
        }
        self.element.dispatch_event(event, ctx, app)
    }
}
