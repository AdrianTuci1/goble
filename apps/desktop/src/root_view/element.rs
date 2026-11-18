//! The [`Element`] impl: layout, paint, size and the global shortcuts.

use goble_terminal::blocks::BlockView;
use goble_ui::elements::terminal_block::toggle_terminal_filter;
use goble_ui::elements::MenuPointer;
use goble_ui::event::{DispatchedEvent, BUTTON_PRIMARY};
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
        // The frame being laid out is the one the request asked for.
        self.redraw_requested = false;
        let size = self.element.layout(constraint, ctx, app);
        let _ = self.hover_chips.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        // The menu that was open on the previous frame owns the pointer over its
        // own surface: seed the gate before anything paints, so the rows, pills
        // and tooltips behind it are told the pointer is not over them. The
        // menus paint-report their surface below, for the frame after this one.
        ctx.open_menu_surfaces = app.open_menus.surfaces();
        app.open_menus.begin_paint();
        // Paint-time hover is gated by the context, but a few cells are written
        // by pointer *events* as well (a workspace tab's hover, a pane header's
        // chip). While the pointer is on the open menu those cells belong to the
        // menu too, so they are cleared here, after this frame's events and
        // before anything paints from them: the surface behind the menu rests.
        if ctx.pointer_on_open_menu() {
            let mut state = self.state.borrow_mut();
            state.space_hover = None;
            for hover in state.pane_hover.values() {
                *hover.borrow_mut() = false;
            }
        }
        self.element.paint(origin, ctx, app);
        // The chips hovered this frame go last, after the panes and after every
        // overlay the tree mounted, so nothing the tree painted covers them.
        // They are a menu's own top-layer surfaces too (a row's tray, the usage
        // panel), so while the pointer is on the open menu they paint inside its
        // surface: the menu keeps its own hover when its last layer draws it,
        // instead of reading its own rows as the elements behind it.
        if ctx.pointer_on_open_menu() {
            ctx.enter_menu_surface();
        }
        self.hover_chips.paint(origin, ctx, app);
        if ctx.menu_surface_depth > 0 {
            ctx.leave_menu_surface();
        }
        // Reconcile what painted open with the one open menu (adopting an
        // action-driven open, closing stragglers), then mirror the result into
        // the app state the rest of the desktop reads.
        app.open_menus.end_paint();
        ctx.open_menu_surfaces.clear();
        let open = app.open_menus.id();
        let mut changed = false;
        {
            let mut state = self.state.borrow_mut();
            if state.open_menu != open {
                state.open_menu = open;
                changed = true;
            }
        }
        if changed {
            // A menu that closed itself while the tree painted (or one adopted
            // after an action) needs the frame that shows its absence.
            self.redraw_requested = true;
        }
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    /// Whether the frame clock still has a reason to run: an agent turn, a tool
    /// call, a running sub-agent child, a pending approval or question, a
    /// command printing into a pane, or a transcript gliding to content that
    /// arrived while it was following. Everything else is event-driven, so an
    /// idle window idles instead of rebuilding and repainting the same frame.
    fn wants_animation(&self) -> bool {
        // A transition the state made outside the frame clock — a chord
        // modifier that went down or came up, a window that lost the keyboard —
        // is due a frame now, not at the next idle heartbeat.
        if self.redraw_requested {
            return true;
        }
        let state = self.state.borrow();
        // A finished turn's banner is put away by its own deadline, which is
        // only reached on a frame: keep the clock running until it has gone.
        if state.announcement.is_some() {
            return true;
        }
        // The session menu's toast is put away by its own deadline the same
        // way: keep the clock running until it has gone.
        if state.toast.is_some() {
            return true;
        }
        let live = state.live_work();
        if live.turn_busy
            || !live.executions.is_empty()
            || !live.tool_calls.is_empty()
            // A pane whose only work is a background child still has a row and
            // a chip to keep current, so the clock stays live for it.
            || live.running_sub_agents > 0
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
            .chain(
                state
                    .pane_controls
                    .values()
                    .flat_map(|controls| [&controls.diff_scroll, &controls.diff_list_scroll]),
            )
            .any(|scroll| scroll.borrow().is_animating());
        if gliding {
            return true;
        }
        // A card's sideways swipe glides on the same clock as the column's
        // vertical one; it is kept per file, so it is asked for separately.
        let sideways = state.pane_controls.values().any(|controls| {
            controls
                .diff_hscroll
                .borrow()
                .values()
                .any(|scroll| scroll.borrow().is_animating())
        });
        if sideways {
            return true;
        }
        let running = state.terminal.borrow().any_running_command();
        if running {
            return true;
        }
        // A Workspaces row under the pointer owns two delayed effects: the
        // hover card's dwell and the clipped title's marquee crawl. Both need
        // frames while the pointer rests, so the sidebar's shared hover cell
        // keeps the clock awake.
        let sidebar_hovering = state.sidebar_hover_row.borrow().is_some();
        sidebar_hovering
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        // The chord modifier's own press and release decide whether every
        // workspace tab advertises its number (see `WorkspaceChip`). Tracked
        // before the shortcuts, so the frame this key lands in already draws
        // the hint, and tracked whatever the tree does with the event below.
        self.track_chord_modifier(event);
        // The open menu has the window's pointer and Escape before any element
        // behind it does: a press outside it is the one dismiss path for every
        // menu, and a move over its surface is not a move over what it covers.
        if self.guard_open_menu(event, app) {
            return true;
        }
        // Global shortcuts (warp-new style pane splitting + pane focus
        // navigation + the command palette). These are handled before the tree
        // so a focused composer cannot swallow the keys.
        if let Some(actions) = &self.actions {
            if let DispatchedEvent::KeyDown { .. } = event {
                // The onboarding overlay owns the keyboard while it is up: no
                // chord opens a panel or moves a pane behind it. The overlay
                // itself decides what the key means.
                if self.state.borrow().onboarding_open {
                    return self.element.dispatch_event(event, ctx, app);
                }
                let DispatchedEvent::KeyDown { key, modifiers } = event else {
                    return false;
                };
                let has_ctrl_cmd = modifiers.ctrl || modifiers.command;
                // Cmd+K opens the keyboard shortcuts panel, the one menu that
                // lists the chords. Ctrl+. is the same panel's own chord.
                if has_ctrl_cmd && !modifiers.shift && key.eq_ignore_ascii_case("k") {
                    (actions.on_toggle_shortcuts_help.borrow_mut())();
                    return true;
                }
                // Cmd/Ctrl+Shift+I toggles the global inbox: approvals,
                // questions and terminal reports from every pane.
                if has_ctrl_cmd && modifiers.shift && key.eq_ignore_ascii_case("i") {
                    (actions.on_inbox.borrow_mut())();
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
                // Cmd/Ctrl+Shift+L cycles the active space's layout through
                // BSP, master-stack, floating and carousel.
                if has_ctrl_cmd && modifiers.shift && key.eq_ignore_ascii_case("l") {
                    (actions.on_cycle_layout.borrow_mut())();
                    return true;
                }
                // Cmd/Ctrl+Shift+N opens the saved-host picker: a new terminal
                // pane bound to an arbitrary `ssh -t` host.
                if has_ctrl_cmd && modifiers.shift && key.eq_ignore_ascii_case("n") {
                    (actions.on_open_ssh_host_picker.borrow_mut())();
                    return true;
                }
                // Cmd/Ctrl+Shift+W opens the workflow runs in the active pane,
                // or closes the view already there.
                if has_ctrl_cmd && modifiers.shift && key.eq_ignore_ascii_case("w") {
                    (actions.on_toggle_workflow_view.borrow_mut())();
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
                if has_ctrl_cmd
                    && !modifiers.shift
                    && !modifiers.alt
                    && key.eq_ignore_ascii_case("f")
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
                // Cmd/Ctrl+1 … Cmd/Ctrl+8 select a workspace by the number its
                // tab advertises while the chord modifier is held. There is no
                // ninth slot, as in a browser's tab strip, and a digit past the
                // last tab does nothing. Only the modifier makes it a chord, so
                // a digit typed into a field still arrives as text.
                if has_ctrl_cmd && !modifiers.shift && !modifiers.alt {
                    if let Some(index) = workspace_digit(key) {
                        if index < self.state.borrow().spaces.len() {
                            // A chord is not a click: it never continues the
                            // pointer's multi-click run, so landing on a tab
                            // that was double-clicked a moment ago selects it
                            // instead of reopening its rename field.
                            self.state.borrow_mut().space_press_run.borrow_mut().take();
                            (actions.on_workspace_click.borrow_mut())(index);
                            return true;
                        }
                    }
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

impl RootView {
    /// The open menu's claim on the window, before the tree sees the event.
    ///
    /// Returns true when the event was the menu's and must not reach anything
    /// else. The rules are the native counterpart of a portal-rendered menu in
    /// the web:
    ///
    /// * an `Escape` closes the menu and goes no further;
    /// * a press on the menu's own surface is left to the tree (the menu's
    ///   elements handle it);
    /// * a press on the menu's own trigger is left alone as well — the menu
    ///   toggles itself through it, the way a drop-down's trigger does;
    /// * a press anywhere else closes the menu first, and then goes on to what
    ///   it landed on (the web's document `pointerdown` listener does not
    ///   swallow the click either: the page still reacts, the menu only steps
    ///   out of the way);
    /// * a pointer move over the surface of a paint-hover menu is the menu's
    ///   and is not passed on, so nothing behind it updates hover from the
    ///   event stream. Menus that move their own highlight from pointer events
    ///   (the composer's model card, the slash card) still receive them.
    fn guard_open_menu(&mut self, event: &DispatchedEvent, app: &AppContext) -> bool {
        if !app.open_menus.is_open() {
            return false;
        }
        match event {
            DispatchedEvent::KeyDown { key, modifiers } => {
                // Plain Escape only: a chorded Escape belongs to whatever binds
                // it. Menu keyboard navigation (arrows, Enter) is untouched.
                if key == "Escape"
                    && !modifiers.command
                    && !modifiers.ctrl
                    && !modifiers.alt
                    && !modifiers.shift
                {
                    app.open_menus.close();
                    self.redraw_requested = true;
                    return true;
                }
                false
            }
            DispatchedEvent::MouseDown { position, button } => {
                // Only the primary button dismisses: a right press is a gesture
                // of its own (it is what opens a context menu, and on the tab a
                // context menu already belongs to it closes that menu through
                // its own toggle), so treating it as a dismissal would have the
                // two cancel out.
                if *button != BUTTON_PRIMARY {
                    return false;
                }
                if app.open_menus.pointer_on_surface(*position)
                    || app.open_menus.pointer_on_anchor(*position)
                {
                    return false;
                }
                app.open_menus.close();
                self.redraw_requested = true;
                // The press itself is not the menu's: let the control or row it
                // landed on take it, now that the menu is out of the way.
                false
            }
            DispatchedEvent::MouseMove { position } => {
                app.open_menus.pointer() == MenuPointer::Paint
                    && app.open_menus.pointer_on_surface(*position)
            }
            _ => false,
        }
    }
}

/// The workspace a chord digit names: `1` is the first tab, `8` the eighth.
/// `9` is left out, as in a browser's tab strip, and every other key names no
/// workspace at all.
fn workspace_digit(key: &str) -> Option<usize> {
    let digit = key.parse::<usize>().ok().filter(|d| (1..=8).contains(d))?;
    Some(digit - 1)
}

/// Whether `modifiers` reports the platform's chord modifier down: Command on
/// macOS, Ctrl everywhere else — the two the tab hints spell out.
fn chord_modifier_down(modifiers: &goble_ui::event::ModifiersState) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.command
    } else {
        modifiers.ctrl
    }
}

impl RootView {
    /// Keep the chord-modifier flag in step with the keyboard. A modifier key
    /// reports itself as a bare modifier change rather than as a key press, so
    /// the platform forwards it as one; a key press or release that carries the
    /// modifiers is read too, and a window that loses the keyboard clears the
    /// flag outright — Cmd-Tabbing away sends no release for the key that was
    /// down, and the hints would otherwise stay painted.
    ///
    /// Nothing here consumes the event: a release that changes nothing is
    /// ordinary typing as far as the tree is concerned.
    fn track_chord_modifier(&mut self, event: &DispatchedEvent) {
        let held = match event {
            DispatchedEvent::Modifiers { modifiers } => chord_modifier_down(modifiers),
            DispatchedEvent::KeyDown { modifiers, .. } => chord_modifier_down(modifiers),
            DispatchedEvent::KeyUp { modifiers, .. } => chord_modifier_down(modifiers),
            DispatchedEvent::Focus { gained: false } => false,
            _ => return,
        };
        if self.state.borrow_mut().set_command_held(held) {
            self.redraw_requested = true;
        }
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
            .layout(constraint, &mut LayoutContext, &app);
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
