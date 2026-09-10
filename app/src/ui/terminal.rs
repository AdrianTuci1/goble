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

use goble_terminal::blocks::BlockView;
use goble_terminal::{KeyEncoder, MouseAction, MouseButton, Palette, TermMode};
use goble_ui::elements::interactive::contains;
use goble_ui::elements::{
    caret_beam, terminal_block, AppContext, Container, CrossAxisAlignment, EdgeInsets, Element,
    EventContext, Expanded, Fill, Flex, LayoutContext, MainAxisSize, PaintContext, Point,
    SizeConstraint, TerminalData, TerminalFilter, TerminalGrid, TerminalLine, TerminalStatus, Text,
};
use goble_ui::event::{DispatchedEvent, ModifiersState};
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, FontFamily, SpacingToken};

use crate::emulator::{AgentViewCard, VisibleBlock};
use crate::terminal::{
    classify_input, classify_key, is_agent_submit, mouse_report, update_input_mirror, InputClass,
    TerminalKeyAction, TerminalMode, TerminalRegistry, TerminalSession,
};

const FONT_SIZE: f32 = 12.0;
const LINE_HEIGHT: f32 = 1.35;

/// Build the terminal pane content for `pane_id`.
///
/// `harness_mode` is this pane's own rich-input mode: when it is on, the pane
/// shows a harness input line at the bottom and routes turns to the agent
/// instead of the shell (Cmd+Enter activates it, Esc returns to the plain pty).
/// The pane's `view` is the filter over its block list: the shell's own history
/// (the terminal) or one conversation's agent view. A conversation card left in
/// the terminal is clickable to reopen its view.
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
    let on_open_agent_view = actions.on_open_agent_view.clone();
    let view = state
        .pane_controls
        .get(&pane_id)
        .map(|c| c.view.clone())
        .unwrap_or(BlockView::Terminal);
    TerminalView::new(
        state.terminal.clone(),
        pane_id,
        cwd,
        active,
        harness_mode,
        view,
        on_activate,
        on_route,
        on_harness_mode,
        on_open_agent_view,
    )
    .finish()
}

/// The block a pane's executed command is drawn as: the live session's output,
/// titled for the pane.
///
/// This is the pane's own block path, and the transcript reuses it: a command
/// that ran in the pane is drawn with the same terminal block the transcript
/// draws a command the agent ran with, so one command is one block in both
/// places. `None` while the session has drawn nothing.
pub fn executed_command_block(session: &TerminalSession) -> Option<TerminalData> {
    let snapshot = session.snapshot(48);
    if snapshot.lines.is_empty() {
        return None;
    }
    let lines = snapshot
        .lines
        .iter()
        .map(|line| {
            let text = line.trim_end().to_string();
            if text.is_empty() {
                TerminalLine::info(" ")
            } else {
                TerminalLine::output(text)
            }
        })
        .collect();
    Some(TerminalData::new("terminal", lines).with_status(TerminalStatus::Success))
}

struct TerminalView {
    terminal: Rc<RefCell<TerminalRegistry>>,
    pane_id: u64,
    cwd: String,
    active: bool,
    harness_mode: bool,
    /// The filter this pane's block list is drawn through: the terminal, or one
    /// conversation's agent view.
    view: BlockView,
    on_activate: Rc<RefCell<dyn FnMut(u64)>>,
    on_route: Rc<RefCell<dyn FnMut(u64, String)>>,
    on_harness_mode: Rc<RefCell<dyn FnMut(u64, bool)>>,
    on_open_agent_view: Rc<RefCell<dyn FnMut(u64, String)>>,
    /// Where the grid ended up, written by the grid as it paints; a pointer
    /// event is turned into a cell against this, not against a second guess at
    /// the flex layout.
    geometry: Rc<RefCell<Option<GridGeometry>>>,
    /// The conversation cards drawn in the terminal view this frame, with where
    /// each landed, so a click on a card reopens its conversation.
    cards: Rc<RefCell<Vec<CardRect>>>,
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

/// Where one conversation card landed in the terminal view, in window
/// coordinates, so a click on the card is a click on the conversation it names.
#[derive(Debug, Clone)]
struct CardRect {
    conversation_id: String,
    origin: Vector2F,
    size: Vector2F,
}

impl CardRect {
    fn contains(&self, position: Vector2F) -> bool {
        position.x >= self.origin.x
            && position.x <= self.origin.x + self.size.x
            && position.y >= self.origin.y
            && position.y <= self.origin.y + self.size.y
    }
}

/// The row a conversation card draws: the card's label and how to open it.
fn build_card(app: &AppContext, card: &AgentViewCard) -> Box<dyn Element> {
    let sm = app.theme.spacing_px(SpacingToken::Sm);
    let row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new("▸")
                .with_font_size(FONT_SIZE)
                .with_font_family(FontFamily::Mono)
                .with_theme_color(ColorToken::Accent, app)
                .finish(),
        )
        .with_child(
            Text::new(format!("agent view · {}", card.label))
                .with_font_size(FONT_SIZE)
                .with_line_height(LINE_HEIGHT)
                .with_font_family(FontFamily::Mono)
                .with_theme_color(ColorToken::Text, app)
                .with_max_lines(1)
                .finish(),
        );
    Container::new(row.finish())
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(app.theme.color(ColorToken::Border).into())
        .with_corner_radius(app.theme.radius_px())
        .with_padding(EdgeInsets::new(sm * 0.5, sm, sm * 0.5, sm))
        .finish()
}

/// Draws a conversation card and records where it landed, so the pane can turn
/// a pointer event into a click on the card rather than on the grid behind it.
struct CardProbe {
    inner: Box<dyn Element>,
    cards: Rc<RefCell<Vec<CardRect>>>,
    conversation_id: String,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Element for CardProbe {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.inner.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        if let Some(size) = self.size {
            self.cards.borrow_mut().push(CardRect {
                conversation_id: self.conversation_id.clone(),
                origin,
                size,
            });
        }
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        self.inner.paint(origin, ctx, app);
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
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
        view: BlockView,
        on_activate: Rc<RefCell<dyn FnMut(u64)>>,
        on_route: Rc<RefCell<dyn FnMut(u64, String)>>,
        on_harness_mode: Rc<RefCell<dyn FnMut(u64, bool)>>,
        on_open_agent_view: Rc<RefCell<dyn FnMut(u64, String)>>,
    ) -> Self {
        Self {
            terminal,
            pane_id,
            cwd,
            active,
            harness_mode,
            view,
            on_activate,
            on_route,
            on_harness_mode,
            on_open_agent_view,
            geometry: Rc::new(RefCell::new(None)),
            cards: Rc::new(RefCell::new(Vec::new())),
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
        let mut screen = crate::terminal::TerminalViewState::empty();
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
            screen = session.view();
        }
        drop(reg);

        // The block list, filtered by this pane's view, is what the agent view
        // draws and what its conversation cards name. The cards are reset here
        // and recorded again as they paint.
        let blocks = self
            .terminal
            .borrow()
            .visible_blocks(self.pane_id, &self.view);
        self.cards.borrow_mut().clear();

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

        // The pane's own view. An agent view draws the conversation's blocks
        // instead of the shell grid; the terminal view keeps the grid, and the
        // conversation cards the list holds are drawn under it. A session that
        // has not drawn yet reads as a terminal only if we say where it is: the
        // pane shows the shell's directory and a prompt marker until the shell
        // itself paints over it.
        let body: Box<dyn Element> = if let BlockView::Agent { .. } = &self.view {
            *self.geometry.borrow_mut() = None;
            self.build_agent_view(&blocks, app)
        } else if screen.has_content {
            GridProbe {
                inner: TerminalGrid::new(screen.rows, screen.cursor)
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

        // The cards the conversations left behind: each is a block in the list,
        // drawn here and clickable to reopen its agent view.
        if self.view == BlockView::Terminal {
            for block in &blocks {
                let Some(card) = &block.card else { continue };
                column = column.with_child(
                    CardProbe {
                        inner: build_card(app, card),
                        cards: Rc::clone(&self.cards),
                        conversation_id: card.conversation_id.clone(),
                        size: None,
                        origin: None,
                    }
                    .finish(),
                );
            }
        }

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

    /// The conversation whose card is under `position`, if any. A card sits on
    /// top of the grid, so its click is the conversation's, not the program's.
    fn card_at(&self, position: Vector2F) -> Option<String> {
        let cards = self.cards.borrow();
        cards
            .iter()
            .rev()
            .find(|card| card.contains(position))
            .map(|card| card.conversation_id.clone())
    }

    /// The agent view's body: the blocks the conversation's filter draws, each
    /// through the one terminal-block renderer the transcript also uses. The
    /// conversation's own card is hidden by the filter, so it is not one of
    /// them.
    fn build_agent_view(&self, blocks: &[VisibleBlock], app: &AppContext) -> Box<dyn Element> {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let mut list = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm);
        let mut any = false;
        for block in blocks
            .iter()
            .filter(|block| block.card.is_none() && !block.command.is_empty())
        {
            any = true;
            let status = if block.failed {
                TerminalStatus::Error
            } else {
                TerminalStatus::Success
            };
            let data = TerminalData::for_command(block.command.clone(), &block.output, status);
            list = list.with_child(terminal_block(&data, TerminalFilter::default(), None, None));
        }
        if !any {
            list = list.with_child(
                Text::new("the agent has not run a command in this conversation yet")
                    .with_font_size(FONT_SIZE)
                    .with_line_height(LINE_HEIGHT)
                    .with_font_family(FontFamily::Mono)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            );
        }
        list.finish()
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
                // A click on a conversation card reopens that conversation's
                // agent view. The card sits on top of the grid, so it consumes
                // the press instead of reporting it to the program.
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
    use crate::emulator::Emulator;
    use goble_core::harness::ToolCallStatus;
    use goble_ui::elements::{
        terminal_block, ChatFragment, ChatMessageBubble, ChatRole, TerminalFilter, ToolCall,
    };
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;
    use goble_ui::ColorU;

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
            BlockView::Terminal,
            Rc::new(RefCell::new(|_: u64| {})),
            Rc::new(RefCell::new(move |_: u64, text: String| {
                routed_cb.borrow_mut().push(text)
            })),
            Rc::new(RefCell::new(move |_: u64, on: bool| {
                harness_cb.borrow_mut().push(on)
            })),
            Rc::new(RefCell::new(|_: u64, _: String| {})),
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
            BlockView::Terminal,
            Rc::new(RefCell::new(|_: u64| {})),
            Rc::new(RefCell::new(move |_: u64, text: String| {
                routed_cb.borrow_mut().push(text)
            })),
            Rc::new(RefCell::new(move |_: u64, on: bool| {
                harness_cb.borrow_mut().push(on)
            })),
            Rc::new(RefCell::new(|_: u64, _: String| {})),
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
            BlockView::Terminal,
            Rc::new(RefCell::new(|_: u64| {})),
            Rc::new(RefCell::new(|_: u64, _: String| {})),
            Rc::new(RefCell::new(|_: u64, _: bool| {})),
            Rc::new(RefCell::new(|_: u64, _: String| {})),
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
                BlockView::Terminal,
                Rc::new(RefCell::new(|_: u64| {})),
                Rc::new(RefCell::new(|_: u64, _: String| {})),
                Rc::new(RefCell::new(|_: u64, _: bool| {})),
                Rc::new(RefCell::new(|_: u64, _: String| {})),
            )
        };

        // No session, so the mode is unknown and the encoder declines either way.
        assert!(!view(true).report_focus(true), "1004 is not on");
        assert!(
            !view(false).report_focus(true),
            "a background pane is not the focused one"
        );
    }

    /// The drawn text runs as `(text, colour, family, size)`, so two elements
    /// can be compared without depending on where each was laid out.
    fn text_runs(commands: &[RenderCommand]) -> Vec<(String, ColorU, FontFamily, f32)> {
        commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText {
                    text,
                    color,
                    font_family,
                    font_size,
                    ..
                } => Some((text.clone(), *color, *font_family, *font_size)),
                _ => None,
            })
            .collect()
    }

    /// Paint an element headlessly at a fixed size.
    fn paint(element: Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
        let mut element = element;
        render_element(&mut element, vec2f(600.0, 200.0), app)
    }

    /// Q8: a command is one block, drawn by the same element in the transcript
    /// and in the terminal pane. The pane's executed-command block renders
    /// identically in the transcript, and a command the agent ran is that same
    /// block built from the command the bridge produced for it.
    #[test]
    fn a_command_block_renders_identically_in_the_transcript_and_the_pane() {
        let app = AppContext::default();

        // The pane's block, from a session that ran `echo hi`.
        let mut emulator = Emulator::new(80, 24);
        emulator.feed(b"echo hi\r\nhi\r\n");
        let session = TerminalSession::with_emulator(emulator);
        let pane_data = executed_command_block(&session).expect("the pane drew its block");
        let pane_runs = text_runs(&paint(
            terminal_block(&pane_data, TerminalFilter::default(), None, None),
            &app,
        ));
        assert!(
            !pane_runs.is_empty(),
            "the pane's block draws the session's output"
        );

        // The transcript draws the pane's block as the same element.
        let transcript = ChatMessageBubble::new(
            ChatRole::Tool,
            vec![ChatFragment::terminal(pane_data.clone())],
        );
        let transcript_runs = text_runs(&paint(Box::new(transcript), &app));
        assert_eq!(
            transcript_runs, pane_runs,
            "the same block must render in the transcript"
        );

        // A command the agent ran is that block, built from the command and its
        // output rather than re-drawn as agent output.
        let command = TerminalData::for_command("echo hi", "hi", TerminalStatus::Success);
        let command_runs = text_runs(&paint(
            terminal_block(&command, TerminalFilter::default(), None, None),
            &app,
        ));
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "run_command".to_string(),
            arguments: r#"{"command":"echo hi"}"#.to_string(),
            status: ToolCallStatus::Finished,
            result: Some("hi".to_string()),
        };
        let bubble =
            ChatMessageBubble::new(ChatRole::Assistant, Vec::new()).with_tool_calls(vec![call]);
        let bubble_runs = text_runs(&paint(Box::new(bubble), &app));
        assert_eq!(
            &bubble_runs[2..],
            command_runs.as_slice(),
            "the agent's command segment must be the terminal block"
        );
    }

    /// A7: the agent view over the block list. Cmd+Enter pushes the
    /// conversation's card and points the pane's filter at its agent view; Esc
    /// returns the filter to the terminal, leaving the card behind; and clicking
    /// that card reopens the view. The whole round trip goes through the pane's
    /// own events and the app's own actions.
    #[test]
    fn cmd_enter_esc_and_a_card_click_round_trip_the_agent_view() {
        use crate::actions::make_actions;
        use crate::media::MediaState;
        use crate::state::UiState;
        use goble_ui::platform::WindowControl;

        let state = Rc::new(RefCell::new(UiState::mock()));
        // Pane 1 is bound to conversation `c1`; give it a live block list (a
        // detached session: no pty) so the card has a list to go into.
        state
            .borrow()
            .terminal
            .borrow_mut()
            .sessions
            .insert(1, TerminalSession::with_emulator(Emulator::new(80, 24)));
        let actions = make_actions(
            Rc::clone(&state),
            None,
            Rc::new(RefCell::new(MediaState::mock())),
            WindowControl::default(),
            Rc::new(RefCell::new(1.0)),
        );
        let app = AppContext::default();
        let mut ctx = EventContext::default();

        // The app rebuilds the pane from state every frame, so each step below
        // builds the pane the frame after the previous step would.
        let build = || {
            let controls = state.borrow().pane_controls(1);
            TerminalView::new(
                Rc::clone(&state.borrow().terminal),
                1,
                "/tmp".to_string(),
                true,
                controls.harness_mode,
                controls.view.clone(),
                Rc::new(RefCell::new(|_: u64| {})),
                Rc::new(RefCell::new(|_: u64, _: String| {})),
                actions.on_set_pane_harness_mode.clone(),
                actions.on_open_agent_view.clone(),
            )
        };

        // Cmd+Enter enters the agent view: the card is pushed and the filter
        // names the conversation.
        let mut entered = build();
        let cmd_enter = ModifiersState {
            command: true,
            ..ModifiersState::default()
        };
        assert!(entered.dispatch_event(&key("Enter", cmd_enter), &mut ctx, &app));
        assert_eq!(
            state.borrow().pane_view(1),
            BlockView::Agent {
                conversation_id: "c1".to_string()
            },
            "Cmd+Enter switches the pane's view filter to the agent view"
        );
        let cards: Vec<String> = state
            .borrow()
            .pane_terminal_view(1)
            .into_iter()
            .filter_map(|block| block.card.map(|card| card.conversation_id))
            .collect();
        assert_eq!(
            cards,
            vec!["c1".to_string()],
            "the card stands for the conversation in the terminal list"
        );
        assert!(
            state.borrow().pane_agent_view(1, "c1").is_empty(),
            "the conversation's own view is the other filter over the list"
        );

        // Esc returns to the terminal; the card is left behind.
        let mut left = build();
        assert!(left.dispatch_event(&key("Escape", ModifiersState::none()), &mut ctx, &app));
        assert_eq!(
            state.borrow().pane_view(1),
            BlockView::Terminal,
            "Esc returns the pane to the terminal"
        );
        assert!(
            state
                .borrow()
                .pane_terminal_view(1)
                .iter()
                .any(|block| block.card.is_some()),
            "the card stays in the terminal scrollback"
        );

        // The card is clickable to come back: paint the pane so the card records
        // where it landed, then click its centre.
        let pane = build();
        let drawn = Rc::clone(&pane.cards);
        let mut pane: Box<dyn Element> = Box::new(pane);
        render_element(&mut pane, vec2f(600.0, 200.0), &app);
        let rect = drawn
            .borrow()
            .first()
            .cloned()
            .expect("the terminal view drew the card");
        assert_eq!(rect.conversation_id, "c1");
        let centre = Vector2F::new(
            rect.origin.x + rect.size.x * 0.5,
            rect.origin.y + rect.size.y * 0.5,
        );
        let click = DispatchedEvent::MouseDown {
            position: centre,
            button: 0,
        };
        assert!(pane.dispatch_event(&click, &mut ctx, &app));
        assert_eq!(
            state.borrow().pane_view(1),
            BlockView::Agent {
                conversation_id: "c1".to_string()
            },
            "clicking the card reopens the conversation's agent view"
        );
    }
}
