//! Routing the pane's keys and pointer events to the pty and its own scrollback.

use goble_terminal::{KeyEncoder, MouseAction, TermMode};
use goble_ui::Element;
use goble_ui::elements::TerminalGrid;
use goble_ui::elements::interactive::contains;
use goble_ui::event::ModifiersState;
use goble_ui::geometry::Vector2F;

use crate::terminal::{
    classify_key, mouse_report, update_input_mirror, TerminalKeyAction, TerminalMode,
};

use super::TerminalView;
use super::{FONT_SIZE, LINE_HEIGHT};

impl TerminalView {
    /// The encoding the running program negotiated, straight from the session.
    fn input_mode(&self) -> TermMode {
        self.terminal
            .borrow()
            .sessions
            .get(&self.pane_id)
            .map(|session| session.input_mode())
            .unwrap_or(TermMode::NONE)
    }

    pub(super) fn cell_at(&self, position: Vector2F) -> Option<(usize, usize)> {
        self.geometry.borrow().as_ref()?.cell_at(position)
    }

    /// The conversation whose card is under `position`, if any. A card sits on
    /// top of the grid, so its click is the conversation's, not the program's.
    pub(super) fn card_at(&self, position: Vector2F) -> Option<String> {
        let cards = self.cards.borrow();
        cards
            .iter()
            .rev()
            .find(|card| card.contains(position))
            .map(|card| card.conversation_id.clone())
    }

    /// Whether the pointer was last seen inside this pane. A wheel event has no
    /// position, so this is how it is attributed; a wheel event only ever
    /// follows a pointer that is over the window.
    pub(super) fn pointer_inside(&self) -> bool {
        match (self.pointer, self.bounds()) {
            (Some(position), Some(bounds)) => contains(bounds, position),
            _ => false,
        }
    }

    /// Write a pointer report for the program, if it asked for one.
    pub(super) fn report_mouse(&mut self, action: MouseAction, cell: (usize, usize)) {
        let Some(bytes) = mouse_report(action, cell, self.input_mode()) else {
            return;
        };
        if let Some(session) = self.terminal.borrow_mut().sessions.get_mut(&self.pane_id) {
            session.write(&bytes);
        }
    }

    pub(super) fn handle_mouse_move(&mut self, position: Vector2F) {
        let Some(cell) = self.cell_at(position) else {
            return;
        };
        match self.pressed {
            Some(button) => self.report_mouse(MouseAction::Drag(button), cell),
            None => self.report_mouse(MouseAction::Move, cell),
        }
    }

    /// The wheel: a pointer report when the program listens for one, otherwise
    /// the pane's own scrollback.
    pub(super) fn handle_wheel(&mut self, delta_y: f32) {
        if delta_y == 0.0 {
            return;
        }
        let down = delta_y > 0.0;
        if self.input_mode().mouse_reported() {
            let cell = self.last_cell.unwrap_or((0, 0));
            self.report_mouse(
                if down {
                    MouseAction::WheelDown
                } else {
                    MouseAction::WheelUp
                },
                cell,
            );
            return;
        }

        let row_pitch = TerminalGrid::row_pitch(FONT_SIZE, LINE_HEIGHT).max(1.0);
        let lines = (delta_y.abs() / row_pitch).round().max(1.0) as i32;
        // Wheel down walks back towards the live screen.
        let lines = if down { -lines } else { lines };
        let mut reg = self.terminal.borrow_mut();
        if let Some(session) = reg.sessions.get_mut(&self.pane_id) {
            session.scroll(lines);
        }
    }

    /// Tell the program this pane hosts whether the window is focused. A
    /// program only hears about it after asking (mode 1004), and only the active
    /// pane is the one the user is looking at.
    pub(super) fn report_focus(&mut self, gained: bool) -> bool {
        if !self.active {
            return false;
        }
        let Some(bytes) = KeyEncoder::focus(gained, self.input_mode()) else {
            return false;
        };
        if let Some(session) = self.terminal.borrow_mut().sessions.get_mut(&self.pane_id) {
            session.write(&bytes);
        }
        true
    }

    pub(super) fn handle_key(&mut self, key: &str, modifiers: ModifiersState) -> bool {
        match classify_key(key, modifiers, self.input_mode()) {
            TerminalKeyAction::Forward(bytes) => {
                let mut reg = self.terminal.borrow_mut();
                // A plain Enter submits the current input line. If it names a
                // TUI agent (codex/claude/...), switch the pane to agent mode so
                // the agent takes over and keeps its native input. This must run
                // before `update_input_mirror` clears the line on Enter.
                if bytes.as_slice() == b"\r" {
                    reg.update_mode_from_input(self.pane_id);
                }
                if let Some(session) = reg.sessions.get_mut(&self.pane_id) {
                    session.write(&bytes);
                }
                let input = reg.input.entry(self.pane_id).or_default();
                update_input_mirror(input, key, modifiers);
                true
            }
            TerminalKeyAction::RouteToAgent => {
                // Native-first: while a real TUI agent owns the pane, its own
                // line editor handles the submit. Let the enter through instead
                // of routing to the headless harness, so the agent's TUI stays.
                let mode = self.terminal.borrow().mode(self.pane_id);
                if matches!(mode, TerminalMode::Agent(_)) {
                    let mut reg = self.terminal.borrow_mut();
                    if let Some(session) = reg.sessions.get_mut(&self.pane_id) {
                        session.write(b"\r");
                    }
                    return true;
                }
                let input = {
                    let reg = self.terminal.borrow();
                    reg.input(self.pane_id)
                };
                // Activate the harness for this pane: from now on the pane's
                // rich input owns the keys until Esc.
                (self.on_harness_mode.borrow_mut())(self.pane_id, true);
                if !input.trim().is_empty() {
                    // Drop the shell's pending line so the same text is not left
                    // sitting in the shell buffer after it went to the agent.
                    if let Some(session) =
                        self.terminal.borrow_mut().sessions.get_mut(&self.pane_id)
                    {
                        session.write(b"\x15");
                    }
                    (self.on_route.borrow_mut())(self.pane_id, input);
                }
                self.terminal
                    .borrow_mut()
                    .set_input(self.pane_id, String::new());
                true
            }
            TerminalKeyAction::Ignore => false,
        }
    }
}
