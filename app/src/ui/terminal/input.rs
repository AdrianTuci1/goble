//! Routing the pane's keys and pointer events to the pty and its own scrollback.

use std::time::Instant;

use goble_terminal::{KeyEncoder, MouseAction, TermMode};
use goble_ui::elements::interactive::contains;
use goble_ui::elements::TerminalGrid;
use goble_ui::event::ModifiersState;
use goble_ui::geometry::Vector2F;
use goble_ui::Element;

use crate::terminal::{
    classify_copy_key, classify_find_key, classify_key, is_copy_mode_chord, mouse_report,
    update_input_mirror, CopyAction, FindAction, TerminalKeyAction, TerminalMode,
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

    /// The block-text cell under `position`: the flattened line of the block
    /// line the pointer is over and the character column in it. `None` when the
    /// pointer is not over a block line (a header, a card, the padding).
    pub(super) fn block_cell_at(&self, position: Vector2F) -> Option<crate::terminal::Cell> {
        let geometry = self.block_geometry.borrow();
        for line in geometry.iter() {
            let height = line.size.y.max(1.0);
            if position.y < line.origin.y || position.y >= line.origin.y + height {
                continue;
            }
            let text_x = line.origin.x + line.prefix_columns as f32 * line.column_width;
            let offset = ((position.x - text_x) / line.column_width.max(1.0)).floor();
            let column = if line.text_columns == 0 {
                0
            } else {
                (offset.max(0.0) as usize).min(line.text_columns - 1)
            };
            return Some(crate::terminal::Cell::new(line.base + line.line, column));
        }
        None
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
        if self.dragging_block_selection() {
            if let Some(cell) = self.block_cell_at(position) {
                self.terminal
                    .borrow_mut()
                    .extend_block_selection(self.pane_id, cell);
            }
            return;
        }
        let Some(cell) = self.cell_at(position) else {
            return;
        };
        if self.dragging_selection() {
            if let Some(cell) = self.selection_cell((cell.0, cell.1)) {
                self.terminal
                    .borrow_mut()
                    .extend_selection(self.pane_id, cell);
            }
            return;
        }
        match self.pressed {
            Some(button) => self.report_mouse(MouseAction::Drag(button), cell),
            None => self.report_mouse(MouseAction::Move, cell),
        }
    }

    /// Whether a mouse drag is currently extending a cell selection.
    pub(super) fn dragging_selection(&self) -> bool {
        self.terminal
            .borrow()
            .pane_ui(self.pane_id)
            .is_some_and(|ui| ui.drag_anchor.is_some())
    }

    /// Whether a mouse drag is currently extending a block-text selection.
    pub(super) fn dragging_block_selection(&self) -> bool {
        self.terminal
            .borrow()
            .pane_ui(self.pane_id)
            .is_some_and(|ui| ui.block_drag_anchor.is_some())
    }

    /// Whether a plain mouse gesture selects cells instead of being reported
    /// to the program. A full-screen program that negotiated mouse reporting
    /// keeps the pointer; copy mode and the find bar take it for the grid.
    pub(super) fn mouse_selects(&self) -> bool {
        let (copy, find) = {
            let reg = self.terminal.borrow();
            (reg.copy_active(self.pane_id), reg.find_open(self.pane_id))
        };
        copy || find || !self.input_mode().mouse_reported()
    }

    /// The absolute content cell under a viewport cell.
    fn selection_cell(&self, (row, column): (usize, usize)) -> Option<crate::terminal::Cell> {
        self.terminal
            .borrow()
            .screen_cell(self.pane_id, row, column)
    }

    /// Start a mouse selection; a double- or triple-click resolves to a word or
    /// the line. Returns whether a cell was under the pointer.
    pub(super) fn begin_mouse_selection(&mut self, position: Vector2F) -> bool {
        let Some((row, column)) = self.cell_at(position) else {
            return false;
        };
        let Some(cell) = self.selection_cell((row, column)) else {
            return false;
        };
        self.last_cell = Some((row, column));
        self.terminal
            .borrow_mut()
            .begin_selection(self.pane_id, cell, Instant::now());
        true
    }

    /// Start a selection over the block view's text; a double- or triple-click
    /// resolves to a word or the line. Returns whether a block line was under
    /// the pointer.
    pub(super) fn begin_block_mouse_selection(&mut self, position: Vector2F) -> bool {
        let Some(cell) = self.block_cell_at(position) else {
            return false;
        };
        self.last_cell = None;
        self.terminal
            .borrow_mut()
            .begin_block_selection(self.pane_id, cell, Instant::now());
        true
    }

    /// End a mouse selection drag. Returns whether one was in progress.
    pub(super) fn end_mouse_selection(&mut self) -> bool {
        self.terminal
            .borrow_mut()
            .end_selection(self.pane_id)
    }

    /// End a block-text selection drag. Returns whether one was in progress.
    pub(super) fn end_block_mouse_selection(&mut self) -> bool {
        self.terminal
            .borrow_mut()
            .end_block_selection(self.pane_id)
    }

    /// Copy what a finished drag selected, the way grok-build does: the
    /// selection is on the clipboard as soon as the button comes up, with no
    /// Cmd+C. A drag that covers one cell is a click and copies nothing.
    pub(super) fn copy_dragged_selection(&mut self) {
        let text = self
            .terminal
            .borrow()
            .dragged_selection_text(self.pane_id);
        if let Some(text) = text {
            (self.on_copy.borrow_mut())(text);
        }
    }

    /// The wheel: a pointer report when the program listens for one, otherwise
    /// the pane's own scrollback. Returns whether the view moved.
    ///
    /// A trackpad reports pixels while the scrollback (and a program's wheel
    /// reports) move in rows, so the fraction short of a row is carried to the
    /// next event: rounding every pixel event up to a row made a flick jump a
    /// row at a time instead of travelling through the scrollback.
    pub(super) fn handle_wheel(&mut self, delta_y: f32, precise: bool) -> bool {
        if delta_y == 0.0 {
            return false;
        }
        let row_pitch = TerminalGrid::row_pitch(FONT_SIZE, LINE_HEIGHT).max(1.0);
        let rows = {
            let mut reg = self.terminal.borrow_mut();
            let ui = reg.pane_ui_mut(self.pane_id);
            // A line delta is reported in points with one notch at 20 (the
            // platform's own line height); a precise one carries the distance
            // the fingers travelled, which is read in rows.
            let distance = delta_y / if precise { row_pitch } else { 20.0 };
            let total = ui.wheel_remainder + distance;
            let whole = total.trunc();
            ui.wheel_remainder = total - whole;
            whole as i32
        };
        if rows == 0 {
            return false;
        }
        // The wheel reports how far the content should move, and positive is
        // down (winit's contract): a positive delta pulls the content down, so
        // the view walks back into the scrollback — wheel *up* in a program's
        // terms, the button a pager calls "scroll up".
        if self.input_mode().mouse_reported() {
            let cell = self.last_cell.unwrap_or((0, 0));
            let action = if delta_y < 0.0 {
                MouseAction::WheelDown
            } else {
                MouseAction::WheelUp
            };
            // The program hears one report per row, the rate a wheel gives it,
            // however many pixel events the gesture arrived in.
            for _ in 0..rows.abs() {
                self.report_mouse(action, cell);
            }
            return false;
        }
        let mut reg = self.terminal.borrow_mut();
        let Some(session) = reg.sessions.get_mut(&self.pane_id) else {
            return false;
        };
        let before = session.display_offset();
        session.scroll(rows);
        session.display_offset() != before
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

    /// Whether the running program owns the screen. A full-screen program paints
    /// its own layout, so the pane draws no block list and offers no filter to
    /// type into.
    fn program_owns_screen(&self) -> bool {
        self.terminal.borrow().program_owns_screen(self.pane_id)
    }

    /// Whether the pane's whole-output filter bar holds the caret.
    pub(super) fn filter_field_focused(&self) -> bool {
        self.plumbing
            .global_filter()
            .is_some_and(|filter| *filter.focused.borrow())
    }

    /// Selection, copy mode and the find bar, handed the key before the tree
    /// and the shell so neither can swallow a chord the pane means for itself.
    ///
    /// * Cmd/Ctrl+C copies the mouse selection, or copy mode's selection.
    /// * Cmd/Ctrl+Shift+C or Cmd/Ctrl+Shift+Space enters/leaves copy mode.
    /// * While the find bar is open it is modal: Esc closes it, Enter/`n` and
    ///   Shift+Enter/`N` walk the matches, and the query field takes the rest.
    /// * Copy mode is modal too: its motions, `v`, `y`, `/` and `q`/Esc never
    ///   reach the program in the pane.
    pub(super) fn handle_copy_key(&mut self, key: &str, modifiers: ModifiersState) -> bool {
        let cmd = modifiers.ctrl || modifiers.command;

        if is_copy_mode_chord(key, modifiers) {
            self.terminal
                .borrow_mut()
                .toggle_copy_mode(self.pane_id);
            return true;
        }

        if cmd && !modifiers.shift && !modifiers.alt && key.eq_ignore_ascii_case("c") {
            let text = self.terminal.borrow().copy_text(self.pane_id);
            if let Some(text) = text {
                (self.on_copy.borrow_mut())(text);
                return true;
            }
            return false;
        }

        let find_open = self.terminal.borrow().find_open(self.pane_id);
        if find_open {
            let field_focused = self
                .terminal
                .borrow()
                .pane_ui(self.pane_id)
                .is_some_and(|ui| ui.find.focused);
            match classify_find_key(key, modifiers, field_focused) {
                FindAction::Close => {
                    self.terminal.borrow_mut().close_find(self.pane_id);
                }
                FindAction::Next => self.terminal.borrow_mut().find_next(self.pane_id),
                FindAction::Previous => {
                    self.terminal.borrow_mut().find_previous(self.pane_id);
                }
                FindAction::ToggleRegex => {
                    self.terminal
                        .borrow_mut()
                        .toggle_find_regex(self.pane_id);
                }
                // Typing belongs to the search field, which is in the tree.
                FindAction::Input => return false,
                FindAction::Ignore => {}
            }
            return true;
        }

        let copy_active = self.terminal.borrow().copy_active(self.pane_id);
        if !copy_active {
            return false;
        }
        match classify_copy_key(key, modifiers) {
            CopyAction::Move(motion) => {
                self.terminal.borrow_mut().move_copy(self.pane_id, motion);
            }
            CopyAction::Visual => self
                .terminal
                .borrow_mut()
                .toggle_copy_visual(self.pane_id),
            CopyAction::Yank => {
                let text = self.terminal.borrow().copy_mode_yank_text(self.pane_id);
                self.terminal.borrow_mut().finish_copy_yank(self.pane_id);
                if let Some(text) = text {
                    (self.on_copy.borrow_mut())(text);
                }
            }
            CopyAction::Search => {
                self.terminal.borrow_mut().open_find(self.pane_id);
            }
            CopyAction::Exit => {
                self.terminal.borrow_mut().exit_copy_mode(self.pane_id);
            }
            CopyAction::Ignore => {}
        }
        true
    }

    /// Whether the find bar's query field is the pane's typing surface.
    pub(super) fn find_open(&self) -> bool {
        self.terminal.borrow().find_open(self.pane_id)
    }

    /// The pane's own filter keys, handed the key before the tree so the rich
    /// input cannot swallow them. `true` when the pane took the key:
    ///
    /// * Cmd/Ctrl+F opens or closes the scrollback find bar.
    /// * Cmd/Ctrl+Alt+F shows or hides the whole-output filter bar over every
    ///   block the pane draws — the general filter.
    /// * Cmd/Ctrl+Shift+F shows or hides the filter bar of the block under the
    ///   pointer — warp's per-block filter, the same chord it uses.
    /// * Escape puts the whole-output filter away and restores the output.
    pub(super) fn handle_filter_key(&mut self, key: &str, modifiers: ModifiersState) -> bool {
        if key == "Escape" {
            let Some(filter) = self.plumbing.global_filter() else {
                return false;
            };
            if !filter.is_open() {
                return false;
            }
            filter.close_bar();
            return true;
        }
        let cmd = modifiers.ctrl || modifiers.command;
        if !cmd || !key.eq_ignore_ascii_case("f") {
            return false;
        }
        if !modifiers.alt && !modifiers.shift {
            // The find bar works over a full-screen program's screen too: it
            // is the pane's own surface, and only while it is open do the
            // keys stop reaching that program.
            self.terminal.borrow_mut().toggle_find(self.pane_id);
            return true;
        }
        // A full-screen program owns the screen, so there is no block list to
        // filter and no bar to show.
        if self.program_owns_screen() {
            return false;
        }
        if modifiers.shift && !modifiers.alt {
            return self.plumbing.toggle_hovered_filter();
        }
        if modifiers.alt && !modifiers.shift {
            return self.plumbing.toggle_global_filter();
        }
        false
    }

    pub(super) fn handle_key(&mut self, key: &str, modifiers: ModifiersState) -> bool {
        match classify_key(key, modifiers, self.input_mode()) {
            TerminalKeyAction::Forward(bytes) => {
                // Enter submits the line the mirror holds; capture it before
                // the mirror clears so an `ssh` line or `exit` typed in the
                // grid binds the pane the way the rich input bar does.
                let submitted = (bytes.as_slice() == b"\r")
                    .then(|| self.terminal.borrow().input(self.pane_id))
                    .filter(|line| !line.trim().is_empty());
                {
                    let mut reg = self.terminal.borrow_mut();
                    // A plain Enter submits the current input line. If it names
                    // a TUI agent (codex/claude/...), switch the pane to agent
                    // mode so the agent takes over and keeps its native input.
                    // This must run before `update_input_mirror` clears the
                    // line on Enter.
                    if bytes.as_slice() == b"\r" {
                        reg.update_mode_from_input(self.pane_id);
                    }
                    if let Some(session) = reg.sessions.get_mut(&self.pane_id) {
                        session.write(&bytes);
                    }
                    let input = reg.input.entry(self.pane_id).or_default();
                    update_input_mirror(input, key, modifiers);
                }
                if let Some(line) = submitted {
                    (self.on_shell_submit.borrow_mut())(self.pane_id, line);
                }
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
