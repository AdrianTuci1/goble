//! Terminal pane element.
//!
//! Renders a PTY-backed shell as an ANSI-aware scrolling line buffer. The
//! element owns no blocking I/O: every layout frame it lazily ensures a
//! [`TerminalSession`] exists for the pane (spawning `$SHELL` in the pane's
//! cwd) and coalesces the newest output from the session's reader thread into a
//! bounded tail of lines. Keystrokes when the pane is active are forwarded to
//! the pty — except Cmd/Ctrl+Enter, which routes the pane's current input to
//! the agent harness as a NEW agent conversation (warp-new) instead of
//! executing it as a shell command. Plain Enter runs a terminal command in the
//! pane's shell.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    caret_beam, AppContext, Container, CrossAxisAlignment, EdgeInsets, Element, EventContext,
    Expanded, Fill, Flex, LayoutContext, MainAxisSize, PaintContext, Point, SizeConstraint, Text,
};
use goble_ui::event::{DispatchedEvent, ModifiersState};
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, FontFamily};

use crate::terminal::{
    classify_input, classify_key, is_agent_submit, update_input_mirror, InputClass, TerminalKeyAction,
    TerminalMode, TerminalRegistry,
};

const FONT_SIZE: f32 = 12.0;
const LINE_HEIGHT: f32 = 1.35;
/// Approximate per-character advance for the mono (Hack) font, used to decide
/// how many visible columns fit so a rendered line never wraps.
const CHAR_WIDTH: f32 = 0.62;

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
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
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
            root: None,
            size: None,
            origin: None,
        }
    }

    /// Coalesce the newest frame of output and (re)build the line-buffer subtree.
    fn rebuild(&mut self, app: &AppContext, constraint: SizeConstraint) {
        let rows = visible_rows(constraint.max.y);
        let cols = visible_cols(constraint.max.x);

        let (snapshot, mode, input) = {
            let mut reg = self.terminal.borrow_mut();
            reg.ensure_session(self.pane_id, &self.cwd);
            let mode = reg.mode(self.pane_id);
            let input = reg.input(self.pane_id);
            let snapshot = match reg.sessions.get(&self.pane_id) {
                Some(session) => session.snapshot(rows),
                None => crate::terminal::TerminalSnapshot { lines: Vec::new() },
            };
            (snapshot, mode, input)
        };

        let font_size = FONT_SIZE;
        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        // When the pane is hosting a real TUI agent, mark it so the user knows
        // goble kept the agent's native input (native-first). The badge is the
        // first line of the pane so it reads as a header.
        if let TerminalMode::Agent(agent) = &mode {
            let badge = Text::new(format!("agent · {}", agent.label()))
                .with_font_size(font_size)
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
                .with_font_size(font_size)
                .with_line_height(LINE_HEIGHT)
                .with_font_family(FontFamily::Mono)
                .with_theme_color(ColorToken::Accent, app)
                .finish();
            column = column.with_child(badge);
        }

        let mut has_content = !matches!(mode, TerminalMode::Agent(_));
        for line in snapshot.lines.iter().take(rows) {
            if !line.trim().is_empty() {
                has_content = true;
            }
            let truncated: String = line.chars().take(cols).collect();
            let text = Text::new(truncated)
                .with_font_size(font_size)
                .with_line_height(LINE_HEIGHT)
                .with_font_family(FontFamily::Mono)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish();
            column = column.with_child(text);
        }

        // First frame (or a silent shell): show where the shell started and a
        // hint so the pane reads as a terminal rather than a blank box.
        if !has_content {
            let hint = format!("{}$", self.cwd);
            let text = Text::new(hint)
                .with_font_size(font_size)
                .with_line_height(LINE_HEIGHT)
                .with_font_family(FontFamily::Mono)
                .with_theme_color(ColorToken::Muted, app)
                .finish();
            column = column.with_child(text);
        }

        let sm = app.theme.spacing_px(goble_ui::theme::SpacingToken::Sm);
        let body: Box<dyn Element> = if self.harness_mode {
            // The rich input line is pinned to the bottom of the pane, under the
            // transcript, the way the warp-new prompt line sits under the
            // terminal output. The caret is the same insertion beam the text
            // fields show, and it is drawn while this pane is the active one.
            let mut field = Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Text::new("❯")
                        .with_font_size(font_size)
                        .with_line_height(LINE_HEIGHT)
                        .with_font_family(FontFamily::Mono)
                        .with_theme_color(ColorToken::Accent, app)
                        .finish(),
                );
            if !input.is_empty() {
                field = field.with_child(
                    Text::new(input.clone())
                        .with_font_size(font_size)
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
            Flex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_child(Expanded::new(column.finish()).finish())
                .with_child(bar)
                .finish()
        } else {
            column.finish()
        };
        let content = Container::new(body)
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_padding(EdgeInsets::new(sm, sm, sm, sm))
            .finish();
        self.root = Some(content);
    }
}

fn visible_rows(height: f32) -> usize {
    let h = FONT_SIZE * LINE_HEIGHT;
    ((height / h).floor() as usize).saturating_sub(1).max(1)
}

fn visible_cols(width: f32) -> usize {
    ((width / (FONT_SIZE * CHAR_WIDTH)).floor() as usize).saturating_sub(0).max(1)
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
            DispatchedEvent::MouseDown { position, .. } => {
                if let Some(bounds) = self.bounds() {
                    if contains(bounds, *position) {
                        (self.on_activate.borrow_mut())(self.pane_id);
                        return true;
                    }
                }
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

impl TerminalView {
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
        match classify_key(key, modifiers) {
            TerminalKeyAction::Forward(bytes) => {
                let mut reg = self.terminal.borrow_mut();
                // A plain Enter submits the current input line. If it names a
                // TUI agent (codex/claude/...), switch the pane to agent mode so
                // the agent takes over and keeps its native input. This must run
                // before `update_input_mirror` clears the line on `\n`.
                if bytes.as_slice() == [b'\n'] {
                    reg.update_mode_from_input(self.pane_id);
                }
                if let Some(session) = reg.sessions.get_mut(&self.pane_id) {
                    session.write(&bytes);
                }
                let input = reg.input.entry(self.pane_id).or_default();
                update_input_mirror(input, &bytes);
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
                        session.write(b"\n");
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
                    if let Some(session) = self.terminal.borrow_mut().sessions.get_mut(&self.pane_id)
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
                    line.push('\n');
                    if let Some(session) = self.terminal.borrow_mut().sessions.get_mut(&self.pane_id)
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
        match classify_key(key, modifiers) {
            TerminalKeyAction::Forward(bytes) => {
                let control = bytes
                    .iter()
                    .any(|b| *b < 0x20 && *b != b'\n' && *b != b'\x7f' && *b != b'\t');
                if control {
                    if let Some(session) = self.terminal.borrow_mut().sessions.get_mut(&self.pane_id)
                    {
                        session.write(&bytes);
                    }
                    return true;
                }
                let mut reg = self.terminal.borrow_mut();
                let input = reg.input.entry(self.pane_id).or_default();
                update_input_mirror(input, &bytes);
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
        terminal.borrow_mut().set_input(3, "summarize the diff".to_string());
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
}


