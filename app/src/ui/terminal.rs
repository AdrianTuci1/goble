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
    AppContext, Container, CrossAxisAlignment, Element, EventContext, Fill, Flex,
    LayoutContext, MainAxisSize, PaintContext, Point, SizeConstraint, Text,
};
use goble_ui::event::{DispatchedEvent, ModifiersState};
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, FontFamily};

use crate::terminal::{classify_key, update_input_mirror, TerminalKeyAction, TerminalRegistry};

const FONT_SIZE: f32 = 12.0;
const LINE_HEIGHT: f32 = 1.35;
/// Approximate per-character advance for the mono (Hack) font, used to decide
/// how many visible columns fit so a rendered line never wraps.
const CHAR_WIDTH: f32 = 0.62;

/// Build the terminal pane content for `pane_id`.
pub fn build_terminal(
    _app: &AppContext,
    state: &crate::ui::UiSnapshot,
    actions: &crate::ui::UiActions,
    pane_id: u64,
    cwd: String,
    active: bool,
) -> Box<dyn Element> {
    let on_activate = actions.on_pane_activate.clone();
    let on_route = actions.on_terminal_command.clone();
    TerminalView::new(
        state.terminal.clone(),
        pane_id,
        cwd,
        active,
        on_activate,
        on_route,
    )
    .finish()
}

struct TerminalView {
    terminal: Rc<RefCell<TerminalRegistry>>,
    pane_id: u64,
    cwd: String,
    active: bool,
    on_activate: Rc<RefCell<dyn FnMut(u64)>>,
    on_route: Rc<RefCell<dyn FnMut(u64, String)>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl TerminalView {
    fn new(
        terminal: Rc<RefCell<TerminalRegistry>>,
        pane_id: u64,
        cwd: String,
        active: bool,
        on_activate: Rc<RefCell<dyn FnMut(u64)>>,
        on_route: Rc<RefCell<dyn FnMut(u64, String)>>,
    ) -> Self {
        Self {
            terminal,
            pane_id,
            cwd,
            active,
            on_activate,
            on_route,
            root: None,
            size: None,
            origin: None,
        }
    }

    /// Coalesce the newest frame of output and (re)build the line-buffer subtree.
    fn rebuild(&mut self, app: &AppContext, constraint: SizeConstraint) {
        let rows = visible_rows(constraint.max.y);
        let cols = visible_cols(constraint.max.x);

        let snapshot = {
            let mut reg = self.terminal.borrow_mut();
            reg.ensure_session(self.pane_id, &self.cwd);
            match reg.sessions.get(&self.pane_id) {
                Some(session) => session.snapshot(rows),
                None => crate::terminal::TerminalSnapshot { lines: Vec::new() },
            }
        };

        let font_size = FONT_SIZE;
        let mut column = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

        let mut has_content = false;
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
        let content = Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_padding(goble_ui::EdgeInsets::new(sm, sm, sm, sm))
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
        match classify_key(key, modifiers) {
            TerminalKeyAction::Forward(bytes) => {
                let mut reg = self.terminal.borrow_mut();
                if let Some(session) = reg.sessions.get_mut(&self.pane_id) {
                    session.write(&bytes);
                }
                let input = reg.input.entry(self.pane_id).or_default();
                update_input_mirror(input, &bytes);
                true
            }
            TerminalKeyAction::RouteToAgent => {
                let input = {
                    let reg = self.terminal.borrow();
                    reg.input(self.pane_id)
                };
                if !input.trim().is_empty() {
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


