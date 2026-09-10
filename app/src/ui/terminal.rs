//! Terminal pane element.
//!
//! Paints a PTY-backed shell as a real cell grid: the pane owns no blocking
//! I/O, and every layout frame it lazily ensures a [`TerminalSession`] exists
//! for the pane (spawning `$SHELL` in the pane's cwd), then hands the session
//! the grid size the pane just measured and paints the rows the session reports
//! back. Keystrokes when the pane is active are encoded for the program that is
//! running and written to the pty — except Cmd/Ctrl+Enter, which routes the
//! pane's current input to the agent harness as a NEW agent conversation
//! (warp-new) instead of executing it as a shell command. Plain Enter runs a
//! terminal command in the pane's shell.
//!
//! The pane also decides what the pointer means: when the program has asked for
//! mouse reports the events are encoded and written to the pty, and when it has
//! not the wheel scrolls the pane's own scrollback.

use std::cell::RefCell;
use std::rc::Rc;

use goble_terminal::{KeyEncoder, MouseAction, MouseButton, Palette, TermMode};
use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    caret_beam, AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext,
    Expanded, Fill, Flex, LayoutContext, MainAxisSize, PaintContext, Point, SizeConstraint,
    TerminalGrid, Text,
};
use goble_ui::event::{DispatchedEvent, ModifiersState};
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, FontFamily, SpacingToken};

use crate::terminal::{
    classify_input, classify_key, is_agent_submit, mouse_report, update_input_mirror, InputClass,
    TerminalKeyAction, TerminalMode, TerminalRegistry,
};

const FONT_SIZE: f32 = 12.0;
const LINE_HEIGHT: f32 = 1.35;

/// Build the terminal pane content for `pane_id`.
///
/// `harness_mode` is this pane's own rich-input mode: when it is on, the pane
/// shows a harness input line at the bottom and routes turns to the agent
/// instead of the shell (Cmd+Enter activates it, Esc returns to the plain pty).
pub fn build_terminal(
    _app: &AppContext,
    state: &crate::ui::UiSnapshot,
    actions: &crate::ui::UiActions,
    pane_id: u64,
    cwd: String,
    active: bool,
    harness_mode: bool,
) -> Box<dyn Element> {
    let on_activate = actions.on_pane_activate.clone();
    let on_route = actions.on_terminal_command.clone();
    let on_harness_mode = actions.on_set_pane_harness_mode.clone();
    TerminalView::new(
        state.terminal.clone(),
        pane_id,
        cwd,
        active,
        harness_mode,
        on_activate,
        on_route,
        on_harness_mode,
    )
    .finish()
}

struct TerminalView {
    terminal: Rc<RefCell<TerminalRegistry>>,
    pane_id: u64,
    cwd: String,
    active: bool,
    harness_mode: bool,
    on_activate: Rc<RefCell<dyn FnMut(u64)>>,
    on_route: Rc<RefCell<dyn FnMut(u64, String)>>,
    on_harness_mode: Rc<RefCell<dyn FnMut(u64, bool)>>,
    /// Where the grid ended up, written by the grid as it paints; a pointer
    /// event is turned into a cell against this, not against a second guess at
    /// the flex layout.
    geometry: Rc<RefCell<Option<GridGeometry>>>,
    /// The button held down, so motion is reported as a drag while it is.
    pressed: Option<MouseButton>,
    /// The last cell the pointer was over. A wheel event carries no position,
    /// and a release can arrive after the pointer left the pane.
    last_cell: Option<(usize, usize)>,
    /// Where the pointer was last seen, which is how a wheel event — which has
    /// no position of its own — is attributed to this pane.
    pointer: Option<Vector2F>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

/// The painted grid's box, in window coordinates.
#[derive(Debug, Clone, Copy)]
struct GridGeometry {
    origin: Vector2F,
    columns: usize,
    rows: usize,
}

impl GridGeometry {
    /// The cell a point falls in, or `None` when it is outside the grid.
    fn cell_at(&self, position: Vector2F) -> Option<(usize, usize)> {
        let column_pitch = TerminalGrid::cell_width(FONT_SIZE);
        let row_pitch = TerminalGrid::row_pitch(FONT_SIZE, LINE_HEIGHT);
        let x = position.x - self.origin.x;
        let y = position.y - self.origin.y;
        if x < 0.0 || y < 0.0 || column_pitch <= 0.0 || row_pitch <= 0.0 {
            return None;
        }
        let column = (x / column_pitch) as usize;
        let row = (y / row_pitch) as usize;
        (column < self.columns && row < self.rows).then_some((row, column))
    }
}

/// Paints a [`TerminalGrid`] and records where it landed.
struct GridProbe {
    inner: TerminalGrid,
    geometry: Rc<RefCell<Option<GridGeometry>>>,
}

impl Element for GridProbe {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.inner.layout(constraint, ctx, app)
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        *self.geometry.borrow_mut() = Some(GridGeometry {
            origin,
            columns: self.inner.columns(),
            rows: self.inner.rows(),
        });
        self.inner.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.inner.size()
    }

    fn origin(&self) -> Option<Point> {
        self.inner.origin()
    }
}

/// The terminal palette: the app's own text, background and caret colours over
/// the standard sixteen, so a shell in a pane matches the window it is in.
fn terminal_palette(app: &AppContext) -> Palette {
    let text = app.theme.color(ColorToken::Text);
    let background = app.theme.color(ColorToken::Bg);
    let cursor = app.theme.color(ColorToken::Accent);
    Palette::xterm().with_defaults(
        (text.r, text.g, text.b),
        (background.r, background.g, background.b),
        (cursor.r, cursor.g, cursor.b),
    )
}

impl TerminalView {
    #[allow(clippy::too_many_arguments)]
    fn new(
        terminal: Rc<RefCell<TerminalRegistry>>,
        pane_id: u64,
        cwd: String,
        active: bool,
        harness_mode: bool,
        on_activate: Rc<RefCell<dyn FnMut(u64)>>,
        on_route: Rc<RefCell<dyn FnMut(u64, String)>>,
        on_harness_mode: Rc<RefCell<dyn FnMut(u64, bool)>>,
    ) -> Self {
        Self {
            terminal,
            pane_id,
            cwd,
            active,
            harness_mode,
            on_activate,
            on_route,
            on_harness_mode,
            geometry: Rc::new(RefCell::new(None)),
            pressed: None,
            last_cell: None,
            pointer: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    /// Ensure the session exists, tell it the grid size, and (re)build the
    /// subtree for this frame.
    fn rebuild(&mut self, app: &AppContext, constraint: SizeConstraint) {
        let palette = terminal_palette(app);
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let row_pitch = TerminalGrid::row_pitch(FONT_SIZE, LINE_HEIGHT);

        // One badge line per active surface, and the harness input bar with its
        // padding. The grid takes what is left.
        let badge_lines = if matches!(
            self.terminal.borrow().mode(self.pane_id),
            TerminalMode::Agent(_)
        ) {
            1
        } else {
            0
        };
        let chrome = badge_lines as f32 * row_pitch
            + if self.harness_mode {
                row_pitch + sm * 1.5
            } else {
                0.0
            };
        let rows = (((constraint.max.y - sm * 2.0 - chrome) / row_pitch).floor() as usize).max(1);
        let cols = ((constraint.max.x - sm * 2.0) / TerminalGrid::cell_width(FONT_SIZE))
            .floor()
            .max(1.0) as usize;

        let mut reg = self.terminal.borrow_mut();
        reg.ensure_session(self.pane_id, &self.cwd);
        let mode = reg.mode(self.pane_id);
        let input = reg.input(self.pane_id);
        let mut view = crate::terminal::TerminalViewState::empty();
        if let Some(session) = reg.sessions.get_mut(&self.pane_id) {
            session.set_palette(palette);
            session.set_size(
                rows.min(u16::MAX as usize) as u16,
                cols.min(u16::MAX as usize) as u16,
                TerminalGrid::cell_width(FONT_SIZE).round() as u16,
                row_pitch.round() as u16,
            );
            // Answers to the program's questions are written from this thread,
            // so the pump runs every frame, painted or not.
            session.pump();
            view = session.view();
        }
        drop(reg);

        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        // When the pane is hosting a real TUI agent, mark it so the user knows
        // goble kept the agent's native input (native-first). The badge is the
        // first line of the pane so it reads as a header.
        if let TerminalMode::Agent(agent) = &mode {
            let badge = Text::new(format!("agent · {}", agent.label()))
                .with_font_size(FONT_SIZE)
                .with_line_height(LINE_HEIGHT)
                .with_font_family(FontFamily::Mono)
                .with_theme_color(ColorToken::Accent, app)
                .finish();
            column = column.with_child(badge);
        }

        // Harness mode is this pane's own rich input: a badge names the state
        // and tells the user how to leave it (Esc), matching the warp-new
        // Cmd+Enter / Esc switch.
        if self.harness_mode {
            let badge = Text::new("harness · Esc to return to the shell")
                .with_font_size(FONT_SIZE)
                .with_line_height(LINE_HEIGHT)
                .with_font_family(FontFamily::Mono)
                .with_theme_color(ColorToken::Accent, app)
                .finish();
            column = column.with_child(badge);
        }

        // A session that has not drawn yet reads as a terminal only if we say
        // where it is: the pane shows the shell's directory and a prompt marker
        // until the shell itself paints over it.
        let body: Box<dyn Element> = if view.has_content {
            GridProbe {
                inner: TerminalGrid::new(view.rows, view.cursor)
                    .with_palette(palette)
                    .with_font_size(FONT_SIZE)
                    .with_line_height(LINE_HEIGHT)
                    .with_cursor_visible(self.active),
                geometry: Rc::clone(&self.geometry),
            }
            .finish()
        } else {
            *self.geometry.borrow_mut() = None;
            let hint = Text::new(format!("{}$", self.cwd))
                .with_font_size(FONT_SIZE)
                .with_line_height(LINE_HEIGHT)
                .with_font_family(FontFamily::Mono)
                .with_theme_color(ColorToken::Muted, app)
                .finish();
            hint
        };
        column = column.with_child(Expanded::new(body).finish());

        if self.harness_mode {
            // The rich input line is pinned to the bottom of the pane, under the
            // transcript, the way the warp-new prompt line sits under the
            // terminal output. The caret is the same insertion beam the text
            // fields show, and it is drawn while this pane is the active one.
            let mut field = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Text::new("❯")
                        .with_font_size(FONT_SIZE)
                        .with_line_height(LINE_HEIGHT)
                        .with_font_family(FontFamily::Mono)
                        .with_theme_color(ColorToken::Accent, app)
                        .finish(),
                );
            if !input.is_empty() {
                field = field.with_child(
                    Text::new(input.clone())
                        .with_font_size(FONT_SIZE)
                        .with_line_height(LINE_HEIGHT)
                        .with_font_family(FontFamily::Mono)
                        .with_theme_color(ColorToken::Text, app)
                        .with_max_lines(1)
                        .finish(),
                );
            }
            if self.active {
                field = field.with_child(caret_beam(app));
            }
            let bar = Container::new(field.finish())
                .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
                .with_border(app.theme.color(ColorToken::Accent).into())
                .with_corner_radius(app.theme.radius_px())
                .with_padding(EdgeInsets::new(sm * 0.75, sm * 0.5, sm * 0.75, sm * 0.5))
                .finish();
            column = column.with_child(bar);
        }

        let content = Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_padding(EdgeInsets::new(sm, sm, sm, sm))
            .finish();
        self.root = Some(content);
    }

    /// The encoding the running program negotiated, straight from the session.
    fn input_mode(&self) -> TermMode {
        self.terminal
            .borrow()
            .sessions
            .get(&self.pane_id)
            .map(|session| session.input_mode())
            .unwrap_or(TermMode::NONE)
    }

    fn cell_at(&self, position: Vector2F) -> Option<(usize, usize)> {
        self.geometry.borrow().as_ref()?.cell_at(position)
    }

    /// Whether the pointer was last seen inside this pane. A wheel event has no
    /// position, so this is how it is attributed; a wheel event only ever
    /// follows a pointer that is over the window.
    fn pointer_inside(&self) -> bool {
        match (self.pointer, self.bounds()) {
            (Some(position), Some(bounds)) => contains(bounds, position),
            _ => false,
        }
    }

    /// Write a pointer report for the program, if it asked for one.
    fn report_mouse(&mut self, action: MouseAction, cell: (usize, usize)) {
        let Some(bytes) = mouse_report(action, cell, self.input_mode()) else {
            return;
        };
        if let Some(session) = self.terminal.borrow_mut().sessions.get_mut(&self.pane_id) {
            session.write(&bytes);
        }
    }

    fn handle_mouse_move(&mut self, position: Vector2F) {
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
    fn handle_wheel(&mut self, delta_y: f32) {
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
    fn report_focus(&mut self, gained: bool) -> bool {
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

    fn handle_key(&mut self, key: &str, modifiers: ModifiersState) -> bool {
        // Esc leaves harness mode at the rich input and hands the pane back to
        // the plain shell (Cmd+Enter activates the harness, Esc returns).
        if self.harness_mode
            && key.eq_ignore_ascii_case("escape")
            && !modifiers.command
            && !modifiers.ctrl
            && !modifiers.alt
        {
            (self.on_harness_mode.borrow_mut())(self.pane_id, false);
            self.terminal
                .borrow_mut()
                .set_input(self.pane_id, String::new());
            return true;
        }
        if self.harness_mode {
            return self.handle_harness_key(key, modifiers);
        }
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

    /// Rich-input editing while the harness is active: printable keys and
    /// Backspace edit the pane's own input line instead of the shell, Enter
    /// sends it (an agent turn, or a shell command when prefixed with `!`), and
    /// control keys still reach the pty so Ctrl+C can interrupt a command.
    fn handle_harness_key(&mut self, key: &str, modifiers: ModifiersState) -> bool {
        let enter = key.eq_ignore_ascii_case("enter") || key.eq_ignore_ascii_case("return");
        if enter && !is_agent_submit(modifiers) {
            let input = {
                let reg = self.terminal.borrow();
                reg.input(self.pane_id)
            };
            match classify_input(&input, true) {
                Some(InputClass::TerminalCommand(command)) => {
                    let mut line = command;
                    // A carriage return is what Enter sends to a pty.
                    line.push('\r');
                    if let Some(session) =
                        self.terminal.borrow_mut().sessions.get_mut(&self.pane_id)
                    {
                        session.write(line.as_bytes());
                    }
                }
                Some(InputClass::AgentPrompt(prompt)) => {
                    (self.on_route.borrow_mut())(self.pane_id, prompt);
                }
                None => {}
            }
            self.terminal
                .borrow_mut()
                .set_input(self.pane_id, String::new());
            return true;
        }
        match classify_key(key, modifiers, self.input_mode()) {
            TerminalKeyAction::Forward(bytes) => {
                let control = bytes
                    .iter()
                    .any(|b| *b < 0x20 && *b != b'\n' && *b != b'\r' && *b != b'\t');
                if control {
                    if let Some(session) =
                        self.terminal.borrow_mut().sessions.get_mut(&self.pane_id)
                    {
                        session.write(&bytes);
                    }
                    return true;
                }
                let mut reg = self.terminal.borrow_mut();
                let input = reg.input.entry(self.pane_id).or_default();
                update_input_mirror(input, key, modifiers);
                true
            }
            TerminalKeyAction::RouteToAgent => {
                let input = {
                    let reg = self.terminal.borrow();
                    reg.input(self.pane_id)
                };
                if input.trim().is_empty() {
                    return true;
                }
                (self.on_route.borrow_mut())(self.pane_id, input);
                self.terminal
                    .borrow_mut()
                    .set_input(self.pane_id, String::new());
                true
            }
            TerminalKeyAction::Ignore => false,
        }
    }
}

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
        _ctx: &mut EventContext,
        _app: &AppContext,
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
                self.handle_mouse_move(*position);
                // Motion is never consumed: the pane does not own the pointer.
                false
            }
            DispatchedEvent::Scroll { delta } => {
                if !self.pointer_inside() {
                    return false;
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
                self.handle_key(key, *modifiers)
            }
            _ => false,
        }
    }
}

/// The mouse button an event id means: the platform layer numbers them left,
/// right, then everything else as middle.
fn mouse_button(id: u32) -> Option<MouseButton> {
    match id {
        0 => Some(MouseButton::Left),
        1 => Some(MouseButton::Right),
        2 => Some(MouseButton::Middle),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: &str, modifiers: ModifiersState) -> DispatchedEvent {
        DispatchedEvent::KeyDown {
            key: key.to_string(),
            modifiers,
        }
    }

    /// Harness mode is the pane's own rich input: keys edit the pane's input
    /// line, Enter routes it to the harness, and Esc hands the pane back to the
    /// plain shell. Nothing here reaches the pty.
    #[test]
    fn harness_mode_owns_the_keys_and_esc_returns_to_the_shell() {
        let app = AppContext::default();
        let terminal = Rc::new(RefCell::new(TerminalRegistry::default()));
        let routed: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let harness: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
        let routed_cb = Rc::clone(&routed);
        let harness_cb = Rc::clone(&harness);
        let mut view = TerminalView::new(
            Rc::clone(&terminal),
            7,
            "/tmp".to_string(),
            true,
            true,
            Rc::new(RefCell::new(|_: u64| {})),
            Rc::new(RefCell::new(move |_: u64, text: String| {
                routed_cb.borrow_mut().push(text)
            })),
            Rc::new(RefCell::new(move |_: u64, on: bool| {
                harness_cb.borrow_mut().push(on)
            })),
        );
        let mut ctx = EventContext::default();

        assert!(view.dispatch_event(&key("h", ModifiersState::none()), &mut ctx, &app));
        assert!(view.dispatch_event(&key("i", ModifiersState::none()), &mut ctx, &app));
        assert_eq!(terminal.borrow().input(7), "hi");

        assert!(view.dispatch_event(&key("Enter", ModifiersState::none()), &mut ctx, &app));
        assert_eq!(*routed.borrow(), vec!["hi".to_string()]);
        assert_eq!(terminal.borrow().input(7), "", "the line is cleared");

        assert!(view.dispatch_event(&key("Escape", ModifiersState::none()), &mut ctx, &app));
        assert_eq!(*harness.borrow(), vec![false], "Esc leaves harness mode");
    }

    /// In a plain pty pane Cmd+Enter turns the harness on for this pane only.
    #[test]
    fn cmd_enter_activates_harness_mode_on_a_shell_pane() {
        let app = AppContext::default();
        let terminal = Rc::new(RefCell::new(TerminalRegistry::default()));
        terminal
            .borrow_mut()
            .set_input(3, "summarize the diff".to_string());
        let routed: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let harness: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
        let routed_cb = Rc::clone(&routed);
        let harness_cb = Rc::clone(&harness);
        let mut view = TerminalView::new(
            Rc::clone(&terminal),
            3,
            "/tmp".to_string(),
            true,
            false,
            Rc::new(RefCell::new(|_: u64| {})),
            Rc::new(RefCell::new(move |_: u64, text: String| {
                routed_cb.borrow_mut().push(text)
            })),
            Rc::new(RefCell::new(move |_: u64, on: bool| {
                harness_cb.borrow_mut().push(on)
            })),
        );
        let mut ctx = EventContext::default();
        let cmd_enter = ModifiersState {
            command: true,
            ..ModifiersState::default()
        };
        assert!(view.dispatch_event(&key("Enter", cmd_enter), &mut ctx, &app));
        assert_eq!(*harness.borrow(), vec![true], "Cmd+Enter activates it");
        assert_eq!(*routed.borrow(), vec!["summarize the diff".to_string()]);
    }

    /// A background pane must not take keystrokes from the active one.
    #[test]
    fn an_inactive_pane_ignores_keystrokes() {
        let app = AppContext::default();
        let terminal = Rc::new(RefCell::new(TerminalRegistry::default()));
        let mut view = TerminalView::new(
            terminal,
            1,
            "/tmp".to_string(),
            false,
            false,
            Rc::new(RefCell::new(|_: u64| {})),
            Rc::new(RefCell::new(|_: u64, _: String| {})),
            Rc::new(RefCell::new(|_: u64, _: bool| {})),
        );
        let mut ctx = EventContext::default();
        assert!(!view.dispatch_event(&key("a", ModifiersState::none()), &mut ctx, &app));
    }

    /// The pointer becomes a cell against the grid that was painted, not
    /// against a second guess at the flex layout.
    #[test]
    fn a_pointer_position_maps_to_the_cell_it_is_over() {
        let geometry = GridGeometry {
            origin: Vector2F::new(10.0, 20.0),
            columns: 80,
            rows: 24,
        };
        let pitch = TerminalGrid::cell_width(FONT_SIZE);
        let row_pitch = TerminalGrid::row_pitch(FONT_SIZE, LINE_HEIGHT);

        assert_eq!(geometry.cell_at(Vector2F::new(10.0, 20.0)), Some((0, 0)));
        assert_eq!(
            geometry.cell_at(Vector2F::new(10.0 + pitch * 3.5, 20.0 + row_pitch * 4.5)),
            Some((4, 3))
        );
        // Outside the box, on either side, is not a cell.
        assert_eq!(geometry.cell_at(Vector2F::new(9.0, 20.0)), None);
        assert_eq!(geometry.cell_at(Vector2F::new(10.0, 19.0)), None);
        assert_eq!(
            geometry.cell_at(Vector2F::new(10.0 + pitch * 81.0, 20.0)),
            None
        );
        assert_eq!(
            geometry.cell_at(Vector2F::new(10.0, 20.0 + row_pitch * 25.0)),
            None
        );
    }

    #[test]
    fn mouse_button_ids_map_to_the_buttons_the_platform_numbers() {
        assert_eq!(mouse_button(0), Some(MouseButton::Left));
        assert_eq!(mouse_button(1), Some(MouseButton::Right));
        assert_eq!(mouse_button(2), Some(MouseButton::Middle));
        assert_eq!(mouse_button(7), None);
    }

    /// Focus is reported to the program only when the pane is the active one and
    /// only after the program asked for it (mode 1004).
    #[test]
    fn only_an_active_pane_with_the_mode_on_reports_focus() {
        let terminal = Rc::new(RefCell::new(TerminalRegistry::default()));
        let mut view = |active: bool| {
            TerminalView::new(
                Rc::clone(&terminal),
                1,
                "/tmp".to_string(),
                active,
                false,
                Rc::new(RefCell::new(|_: u64| {})),
                Rc::new(RefCell::new(|_: u64, _: String| {})),
                Rc::new(RefCell::new(|_: u64, _: bool| {})),
            )
        };

        // No session, so the mode is unknown and the encoder declines either way.
        assert!(!view(true).report_focus(true), "1004 is not on");
        assert!(
            !view(false).report_focus(true),
            "a background pane is not the focused one"
        );
    }
}
