//! Building the pane's tree: the header, the sections, the bar and the cards.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use goble_terminal::blocks::BlockView;
use goble_terminal::Palette;
use goble_ui::elements::{
    AppContext, Axis, Container, CrossAxisAlignment, Divider, EdgeInsets, Element, Empty, Expanded,
    Fill, Flex, MainAxisAlignment, MainAxisSize, Scrollable, SizeConstraint, Stack,
    TerminalBlockPlumbing, TerminalGrid, Text,
};
use goble_ui::geometry::Vector2F;
use goble_ui::theme::{ColorToken, FontFamily, SpacingToken};

use crate::terminal::{TerminalMode, TerminalRegistry};
use crate::ui::chat;

use super::TerminalView;
use super::blocks::section_data;
use super::cards::{CardProbe, build_card};
use super::grid::GridProbe;
use super::{FONT_SIZE, LINE_HEIGHT};

/// Build the terminal pane content for `pane_id`.
///
/// The pane draws one of two surfaces, both shared with the chat workspace:
///
/// * While this pane's harness (agent session) is open, it draws the very agent
///   view a chat pane draws — [`chat::build_agent_chat`], the same function the
///   `PaneKind::Chat` leaf mounts — with Escape wired back to the shell. There
///   is no terminal-only agent view.
/// * Otherwise it draws the shell: the commands it ran as sections (the block
///   view — one section per command, headed by its directory and duration), with
///   the shared rich input bar pinned to the bottom of the pane. The bar is the
///   chat surface's own composer element (`chat::build_terminal_composer`), wired
///   to this pane's own session.
///
/// A full-screen program owns its own screen, so the alternate screen — and a
/// shell whose integration has not delimited a block yet — keeps the cell grid
/// instead of the sections.
///
/// Cmd+Enter opens this pane's harness and Esc (at the agent view) returns it
/// to the plain pty, so the pane keeps its own switch.
/// The pane's `view` is the filter over its block list: the shell's own history
/// (the terminal) or one conversation's agent view. A conversation card left in
/// the terminal is clickable to reopen its view.
pub fn build_terminal(
    app: &AppContext,
    state: &crate::ui::UiSnapshot,
    actions: &crate::ui::UiActions,
    pane_id: u64,
    cwd: String,
    active: bool,
) -> Box<dyn Element> {
    let view = state
        .pane_controls
        .get(&pane_id)
        .map(|c| c.view.clone())
        .unwrap_or(BlockView::Terminal);

    // The open harness is the shared agent view. Esc is its way back to the
    // shell: the harness closes and the bar's editor is left unfocused, so the
    // grid takes the keys.
    if matches!(view, BlockView::Agent { .. }) {
        let on_harness_mode = actions.on_set_pane_harness_mode.clone();
        let on_composer_focus = actions.on_composer_focus_change.clone();
        let leave: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(move || {
            (on_harness_mode.borrow_mut())(pane_id, false);
            (on_composer_focus.borrow_mut())(false);
        }));
        return chat::build_agent_chat(app, state, actions, pane_id, active, Some(leave));
    }

    // The shell view wears the pane's one topbar, the same bar the harness's
    // agent view draws — without the `esc`, which is only the way back *from*
    // the harness.
    let header = chat::build_agent_header(app, state, actions, pane_id, false);
    // The rich input is this pane's only typing surface: it holds the keyboard
    // whenever the pane is active and the screen still belongs to the shell. A
    // full-screen program (a TUI agent, or anything in the alternate screen)
    // takes the keys themselves, which is the native-first rule the pane
    // already follows for a claimed TUI agent.
    let owner = state.terminal.borrow();
    let bar_focused = active && !owner.program_owns_screen(pane_id);
    let bar = chat::build_terminal_composer(state, actions, pane_id, bar_focused);
    drop(owner);
    let shell = TerminalView::new(
        state.terminal.clone(),
        pane_id,
        cwd,
        active,
        view,
        bar_focused,
        bar,
        actions.on_pane_activate.clone(),
        actions.on_terminal_command.clone(),
        actions.on_set_pane_harness_mode.clone(),
        actions.on_open_agent_view.clone(),
    )
    // The pane's history scrolls on the app's own per-pane state, and each
    // section draws through the same block plumbing the transcript uses, so a
    // filter tray opened on a block survives the per-frame rebuild wherever the
    // block is shown.
    .with_scroll(
        state
            .pane_terminal_scroll
            .get(&pane_id)
            .cloned()
            .unwrap_or_else(|| Rc::new(RefCell::new(goble_ui::ScrollState::following()))),
    )
    .with_block_plumbing(TerminalBlockPlumbing::new(
        state.terminal_filters.clone(),
        state.terminal_global_filters.get(&pane_id).cloned(),
        Some(actions.on_copy_terminal.clone()),
    ))
    .finish();
    // The pane's topbar floats on its own layer above the shell, so the tray
    // the dots open hangs over the output instead of being painted over by it.
    // The column below reserves the bar's height, so the shell starts under it.
    let content = Flex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(
            Empty::new()
                .with_size(Vector2F::new(0.0, super::super::shell::TOPBAR_HEIGHT))
                .finish(),
        )
        .with_child(Expanded::new(shell).finish())
        .finish();
    Stack::new()
        .with_children(vec![content])
        .with_overlay(header, Vector2F::zero())
        .finish()
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
    pub(super) fn new(
        terminal: Rc<RefCell<TerminalRegistry>>,
        pane_id: u64,
        cwd: String,
        active: bool,
        view: BlockView,
        composer_focused: bool,
        bar: Box<dyn Element>,
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
            view,
            bar: Some(bar),
            composer_focused,
            on_activate,
            on_route,
            on_harness_mode,
            on_open_agent_view,
            geometry: Rc::new(RefCell::new(None)),
            cards: Rc::new(RefCell::new(Vec::new())),
            scroll: Rc::new(RefCell::new(goble_ui::ScrollState::following())),
            plumbing: TerminalBlockPlumbing::new(
                Rc::new(RefCell::new(HashMap::new())),
                None,
                None,
            ),
            pressed: None,
            last_cell: None,
            pointer: None,
            root: None,
            size: None,
            origin: None,
        }
    }

    /// Attach the app-owned scroll state of the pane's command sections.
    pub(super) fn with_scroll(mut self, scroll: Rc<RefCell<goble_ui::ScrollState>>) -> Self {
        self.scroll = scroll;
        self
    }

    /// Attach how this pane draws a terminal block: the app-owned per-block
    /// filter map, the pane's whole-history filter and the copy handler.
    pub(super) fn with_block_plumbing(mut self, plumbing: TerminalBlockPlumbing) -> Self {
        self.plumbing = plumbing;
        self
    }

    /// Ensure the session exists, tell it the grid size, and (re)build the
    /// subtree for this frame.
    pub(super) fn rebuild(&mut self, app: &AppContext, constraint: SizeConstraint) {
        let palette = terminal_palette(app);
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let md = app.theme.spacing_px(SpacingToken::Md);
        let row_pitch = TerminalGrid::row_pitch(FONT_SIZE, LINE_HEIGHT);

        // One badge line per active surface, plus the shared input bar pinned to
        // the bottom. The grid takes what is left.
        let badge_lines = if matches!(
            self.terminal.borrow().mode(self.pane_id),
            TerminalMode::Agent(_)
        ) {
            1
        } else {
            0
        };
        // The bar's own minimum, the figures `ChatComposer` lays out: a 48px
        // editor, a 28px footer row, its padding and the column's spacing.
        let bar_height = 48.0 + 28.0 + sm * 3.0 + md * 2.0;
        let chrome = badge_lines as f32 * row_pitch + bar_height;
        let rows = (((constraint.max.y - sm * 2.0 - chrome) / row_pitch).floor() as usize).max(1);
        let cols = ((constraint.max.x - sm * 2.0) / TerminalGrid::cell_width(FONT_SIZE))
            .floor()
            .max(1.0) as usize;

        let mut reg = self.terminal.borrow_mut();
        reg.ensure_session(self.pane_id, &self.cwd);
        let mode = reg.mode(self.pane_id);
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

        // The shell's own view is the block list: one section per command, with
        // the directory it ran in and how long it took on the section's top line
        // (warp-new's block view), and a card where a conversation happened. The
        // sections are drawn inside one following viewport, so the newest command
        // sits on the rich input and the history scrolls above it.
        //
        // A full-screen program owns its screen instead — a TUI paints a layout,
        // not a list of commands — so the alternate screen keeps the live grid,
        // and so does a shell whose integration has not delimited a block yet.
        let mut list = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm);
        let mut listed = 0usize;
        if !screen.alt_screen {
            for block in &blocks {
                if let Some(card) = &block.card {
                    list = list.with_child(
                        CardProbe {
                            inner: build_card(card),
                            cards: Rc::clone(&self.cards),
                            conversation_id: card.conversation_id.clone(),
                            size: None,
                            origin: None,
                        }
                        .finish(),
                    );
                    listed += 1;
                    continue;
                }
                if let Some(data) = section_data(block) {
                    list = list.with_child(self.plumbing.element(&data));
                    listed += 1;
                }
            }
        }

        // An open harness draws the shared agent surface instead of this list,
        // so the pane's own view here is the shell's history. A session that has
        // not drawn yet reads as a terminal only if we say where it is: the
        // pane shows the shell's directory and a prompt marker until the shell
        // itself paints over it.
        let body: Box<dyn Element> = if listed > 0 {
            // No grid means no cell under the pointer: a section is not a
            // terminal cell, so the program hears no pointer reports from here.
            *self.geometry.borrow_mut() = None;
            Scrollable::new(list.finish(), Axis::Vertical)
                .with_state(Rc::clone(&self.scroll))
                .finish()
        } else if screen.has_content {
            GridProbe {
                // The grid is this pane's output; the pane's input is the rich
                // input bar below it, and that bar owns the caret. A cursor
                // painted in the grid too would read as a second, competing
                // insertion point.
                inner: TerminalGrid::new(screen.rows, screen.cursor)
                    .with_palette(palette)
                    .with_font_size(FONT_SIZE)
                    .with_line_height(LINE_HEIGHT)
                    // The shell's history grows upwards from the rich input
                    // below it (warp-new); a full-screen program painted its
                    // own layout, so its rows stay where it put them.
                    .with_bottom_anchor(!screen.alt_screen)
                    .with_cursor_visible(false),
                geometry: Rc::clone(&self.geometry),
            }
            .finish()
        } else {
            *self.geometry.borrow_mut() = None;
            // The shell's directory hugs the bottom of the body, just above the
            // rich input, so an empty pane reads as a prompt line waiting for the
            // command the user is about to type rather than as output at the top.
            let hint = Text::new(format!("{}$", crate::state::display_path(&self.cwd)))
                .with_font_size(FONT_SIZE)
                .with_line_height(LINE_HEIGHT)
                .with_font_family(FontFamily::Mono)
                .with_theme_color(ColorToken::Muted, app)
                .finish();
            Flex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::End)
                .with_child(hint)
                .finish()
        };
        column = column.with_child(Expanded::new(body).finish());

        // The shared rich input bar, pinned to the bottom of the pane under the
        // grid and the conversation cards. It is the chat surface's own composer
        // element, taken by this frame's build (the tree is rebuilt per frame).
        // A rule separates it from the shell's output, so the input reads as its
        // own surface rather than another line of the transcript.
        if let Some(bar) = self.bar.take() {
            column = column.with_child(Divider::horizontal().finish());
            column = column.with_child(bar);
        }

        let content = Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Bg)))
            .with_padding(EdgeInsets::new(sm, sm, sm, sm))
            .finish();
        self.root = Some(content);
    }
}
