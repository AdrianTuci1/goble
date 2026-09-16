//! The [`Element`] impl: layout, paint, size and the global shortcuts.

use goble_terminal::blocks::BlockView;
use goble_ui::elements::terminal_block::toggle_terminal_filter;
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
        let _ = self.hover_chips.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.element.paint(origin, ctx, app);
        // The chips hovered this frame go last, after the panes and after every
        // overlay the tree mounted, so nothing the tree painted covers them.
        self.hover_chips.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    /// Whether the frame clock still has a reason to run: an agent turn, a tool
    /// call, a pending approval or question, a command printing into a pane, or
    /// a transcript gliding to content that arrived while it was following.
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
        // A glide is over in a fraction of a second: at the idle rate (250ms a
        // frame) it would arrive in one visible step instead of travelling.
        let gliding = state
            .pane_chat_scroll
            .values()
            .chain(state.pane_terminal_scroll.values())
            .any(|scroll| scroll.borrow().is_animating());
        if gliding {
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
                // Ctrl+. opens the keyboard shortcuts panel, grok-build's own
                // binding for its cheatsheet. Shift is not part of the chord, so
                // it stays free for whatever the panes bind next.
                if modifiers.ctrl && !modifiers.shift && !modifiers.alt && key == "." {
                    (actions.on_toggle_shortcuts_help.borrow_mut())();
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
                // Cmd/Ctrl+Shift+W toggles the workflow-runs overlay: the
                // workspace stays mounted, the panel floats over it.
                if has_ctrl_cmd && modifiers.shift && key.eq_ignore_ascii_case("w") {
                    (actions.on_toggle_task_workflow.borrow_mut())();
                    return true;
                }
                if has_ctrl_cmd && !modifiers.shift && key.eq_ignore_ascii_case("w") {
                    (actions.on_close_pane.borrow_mut())();
                    return true;
                }
                // Cmd/Ctrl+F raises the active agent pane's whole-transcript
                // filter, the bar the transcript draws over its terminal blocks.
                // A shell pane answers the same chord itself, beside Cmd+Shift+F
                // and the alternate-screen rule, so the root leaves every shell
                // pane's filter keys to that pane.
                if has_ctrl_cmd && !modifiers.shift && !modifiers.alt && key.eq_ignore_ascii_case("f")
                {
                    let mut state = self.state.borrow_mut();
                    let pane_id = state.active_pane_id;
                    if matches!(state.pane_view(pane_id), BlockView::Agent { .. }) {
                        let filter = state
                            .terminal_global_filters
                            .entry(pane_id)
                            .or_default()
                            .clone();
                        drop(state);
                        toggle_terminal_filter(&filter);
                        return true;
                    }
                }
                // Ctrl+Tab / Ctrl+Shift+Tab walk the tab strip. Ctrl only: the
                // OS eats Cmd+Tab before the window sees it, and staying off
                // the Cmd family keeps the chord clear of the menus.
                if modifiers.ctrl && !modifiers.command && key == "Tab" {
                    let delta = if modifiers.shift { -1 } else { 1 };
                    (actions.on_switch_space.borrow_mut())(delta);
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

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::elements::{Axis, Scrollable};
    use goble_ui::geometry::vec2f;
    use goble_ui::{ScrollState, SizeConstraint};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    /// Mount a real root over an in-memory store, as the root-view cases do.
    fn root() -> (RootView, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let view = RootView::new(&AppContext::default(), &desktop, None);
        (view, dir)
    }

    /// A transcript glide is over in a fraction of a second, which the idle
    /// heartbeat (250ms a frame) cannot carry: while one runs, the frame clock
    /// has to stay at the active rate, and it has to let go once the glide has
    /// landed.
    #[test]
    fn a_transcript_glide_keeps_the_frame_clock_at_the_active_rate() {
        let (view, _dir) = root();
        assert!(
            !view.wants_animation(),
            "a window with nothing in flight idles"
        );

        // Pane 1's transcript, as the app itself registers it.
        let scroll = {
            let mut state = view.state.borrow_mut();
            state.ensure_pane_controls();
            state
                .pane_chat_scroll
                .entry(1)
                .or_insert_with(|| Rc::new(RefCell::new(ScrollState::following())))
                .clone()
        };
        let app = AppContext::default();
        let constraint = SizeConstraint::loose(vec2f(400.0, 200.0));
        let laid_out = |height: f32| {
            Scrollable::new(
                goble_ui::elements::Empty::new()
                    .with_size(vec2f(100.0, height))
                    .finish(),
                Axis::Vertical,
            )
            .with_state(Rc::clone(&scroll))
            .layout(constraint, &mut LayoutContext::default(), &app);
        };

        // The pane opens on its transcript, at the end of it.
        laid_out(400.0);
        assert!(!view.wants_animation(), "an opened transcript is at rest");

        // A long answer arrives in one frame: the transcript glides to it.
        laid_out(3000.0);
        assert!(
            scroll.borrow().is_animating(),
            "the new content is glided to, not jumped to"
        );
        assert!(
            view.wants_animation(),
            "the glide needs the active frame rate"
        );

        // The glide lands, and the window goes quiet again.
        while scroll.borrow().is_animating() {
            scroll.borrow_mut().advance(0.016);
        }
        assert!(
            !view.wants_animation(),
            "a landed glide no longer asks for frames"
        );
    }
}
