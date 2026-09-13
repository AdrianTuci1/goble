//! Cmd+K command palette overlay.
//!
//! Lists the app's commands (new space, splits, close pane, switch space, new
//! conversation, settings/projects, key toggles) as a keyboard-scrollable
//! overlay. Arrow keys move the selection, Enter runs the selected command,
//! Escape / backdrop click closes it without running anything, and typing
//! filters the list. The selection index and filter text live in app state so
//! they survive the per-frame element rebuild.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Axis, Button, ButtonVariant, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, EventContext, Expanded, Fill, Flex, LayoutContext, MainAxisSize, PaintContext, Point,
    Scrollable, SizeConstraint, Spacer, Text,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
use goble_ui::elements::SlashMenuItem;
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::Dialog;

use super::actions::UiActions;
use super::snapshot::UiSnapshot;

/// Width of the command palette panel.
pub const CMD_PALETTE_WIDTH: f32 = 520.0;

/// A single command shown in the palette: a label, an optional shortcut hint,
/// and the action to run when activated (Enter or click).
#[derive(Clone)]
pub struct PaletteCommand {
    pub label: String,
    pub shortcut: String,
    pub action: Rc<RefCell<dyn FnMut()>>,
}

impl PaletteCommand {
    pub fn new(
        label: impl Into<String>,
        shortcut: impl Into<String>,
        action: impl FnMut() + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            shortcut: shortcut.into(),
            action: Rc::new(RefCell::new(action)),
        }
    }

    /// Override the display label + shortcut hint.
    pub fn with_label(mut self, label: impl Into<String>, shortcut: impl Into<String>) -> Self {
        self.label = label.into();
        self.shortcut = shortcut.into();
        self
    }
}

/// The indices (into `commands`) of the commands matching `query`, in order.
fn filter_commands(commands: &[PaletteCommand], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    commands
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            if q.is_empty() || c.label.to_lowercase().contains(&q) {
                Some(i)
            } else {
                None
            }
        })
        .collect()
}

/// One command a rich input offers when its draft starts with `/`: the name as
/// it is typed after the slash, what it does, and what running it calls.
#[derive(Clone)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    pub action: Rc<RefCell<dyn FnMut()>>,
}

impl SlashCommand {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        action: impl FnMut() + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            action: Rc::new(RefCell::new(action)),
        }
    }
}

/// The commands a rich input offers when its draft starts with `/`, in the
/// shape grok-build lists them: a name the draft runs, and what it does. The
/// list is the session and workspace gestures this input can actually reach —
/// nothing is offered here that would go nowhere — and one entry per configured
/// model, written the way grok-build writes `/model <name>`.
///
/// The chord that runs a command is deliberately not repeated here: the
/// instruction strip under the input carries the gestures, and a command is run
/// from the list itself.
pub fn slash_commands(state: &UiSnapshot, actions: &UiActions) -> Vec<SlashCommand> {
    let run = |action: &Rc<RefCell<dyn FnMut()>>| {
        let act = action.clone();
        move || (act.borrow_mut())()
    };
    let pane_id = state.active_pane_id;
    let mut commands = vec![
        SlashCommand::new(
            "new",
            "Start a fresh conversation",
            run(&actions.on_create_submit),
        ),
        SlashCommand::new(
            "clear",
            "Clear this conversation's transcript",
            run(&actions.on_clear_transcript),
        ),
        SlashCommand::new(
            "fork",
            "Branch this conversation into a new one",
            {
                let act = actions.on_fork_conversation.clone();
                move || (act.borrow_mut())(pane_id)
            },
        ),
        SlashCommand::new("stop", "Stop the running turn", run(&actions.on_stop)),
        SlashCommand::new("shortcuts", "Show every keyboard shortcut", {
            run(&actions.on_toggle_shortcuts_help)
        }),
        SlashCommand::new("settings", "Open settings", run(&actions.on_settings)),
        SlashCommand::new("space", "Open a new space", run(&actions.on_add_space)),
        SlashCommand::new("split", "Split this pane to the right", {
            run(&actions.on_split_right)
        }),
        SlashCommand::new("terminal", "Open a new terminal pane", {
            run(&actions.on_new_terminal)
        }),
        SlashCommand::new("close", "Close this pane", run(&actions.on_close_pane)),
        SlashCommand::new("sidebar", "Toggle the right sidebar", {
            run(&actions.on_toggle_right_sidebar)
        }),
        SlashCommand::new("fullscreen", "Toggle full screen", {
            run(&actions.on_toggle_fullscreen)
        }),
        SlashCommand::new("projects", "Open the project list", run(&actions.on_projects)),
        SlashCommand::new("scheduled", "Open the scheduled tasks", {
            run(&actions.on_open_crons)
        }),
        SlashCommand::new("tasks", "Open tasks and workflows", {
            run(&actions.on_toggle_task_workflow)
        }),
        SlashCommand::new("workflows", "Open the workflow catalog", {
            run(&actions.on_workflows)
        }),
        SlashCommand::new("mcps", "Open the MCP servers", run(&actions.on_mcps)),
    ];
    {
        // The same toggles Settings carries; either surface flips them, and the
        // choice persists.
        let toggle_dark = actions.on_toggle_dark_mode.clone();
        let dark_mode = state.settings_dark_mode;
        commands.push(SlashCommand::new(
            "theme",
            "Switch between the dark and light theme",
            move || (toggle_dark.borrow_mut())(!dark_mode),
        ));
        let toggle_vim = actions.on_toggle_vim_mode.clone();
        let vim_mode = state.vim_mode;
        commands.push(SlashCommand::new(
            "vim-mode",
            "Toggle vim-style editing",
            move || (toggle_vim.borrow_mut())(!vim_mode),
        ));
        // grok-build's permission-mode command, applied to the pane that typed
        // it: running it while it is on turns it back off.
        let toggle_approve = actions.on_toggle_auto_approve.clone();
        let auto_approve = state
            .pane_controls
            .get(&pane_id)
            .map(|controls| controls.auto_approve)
            .unwrap_or(state.auto_approve);
        commands.push(SlashCommand::new(
            "always-approve",
            "Stop asking before running commands",
            move || (toggle_approve.borrow_mut())(pane_id, !auto_approve),
        ));
    }
    // One entry per configured model: `/model <name>`, so `/` lists them and
    // the name filters them. A pick applies to the pane that typed it.
    for name in &state.models {
        let act = actions.on_model_select.clone();
        let model = name.clone();
        commands.push(SlashCommand::new(
            format!("model {model}"),
            "Run this pane on another model",
            move || (act.borrow_mut())(pane_id, model.clone()),
        ));
    }
    commands
}

/// The commands whose name contains `query`, in order. An empty query matches
/// every one, so a bare `/` lists the whole set.
pub fn matching_slash_commands(commands: &[SlashCommand], query: &str) -> Vec<SlashCommand> {
    let query = query.trim().to_lowercase();
    commands
        .iter()
        .filter(|command| query.is_empty() || command.name.to_lowercase().contains(&query))
        .cloned()
        .collect()
}

/// A command list as the menu draws it: the name and what it does.
pub fn slash_items(commands: &[SlashCommand]) -> Vec<SlashMenuItem> {
    commands
        .iter()
        .map(|command| {
            SlashMenuItem::new(command.name.clone(), command.description.clone())
        })
        .collect()
}

/// Run the command at `index` of `commands` and put the menu away. The index is
/// the row the menu reported, which is a position in the list it was handed.
pub fn slash_accept(
    commands: Vec<SlashCommand>,
    close: Rc<RefCell<dyn FnMut()>>,
) -> impl FnMut(usize) + 'static {
    move |index: usize| {
        if let Some(command) = commands.get(index) {
            (command.action.borrow_mut())();
        }
        (close.borrow_mut())();
    }
}

/// The query a slash draft carries: what was typed after the `/`, up to the
/// first word. The rest of the line is the command's own text, not its name.
pub fn slash_query(draft: &str) -> Option<String> {
    let trimmed = draft.trim_start();
    let rest = trimmed.strip_prefix('/')?;
    Some(
        rest.split_whitespace()
            .next()
            .unwrap_or("")
            .to_string(),
    )
}

/// Build the Cmd+K command palette overlay. When `open` is false the overlay
/// occupies no space and ignores events.
#[allow(clippy::too_many_arguments)]
pub fn build_command_palette(
    _app: &AppContext,
    open: bool,
    query: &str,
    index: usize,
    commands: Vec<PaletteCommand>,
    on_change: Rc<RefCell<dyn FnMut(String)>>,
    on_move: Rc<RefCell<dyn FnMut(usize)>>,
    on_close: Rc<RefCell<dyn FnMut()>>,
) -> Box<dyn Element> {
    let content = PaletteContent {
        open,
        query: query.to_string(),
        index,
        commands,
        on_change,
        on_move,
        on_close: on_close.clone(),
        filtered: Vec::new(),
        root: None,
        size: None,
        origin: None,
    };
    Dialog::new(Box::new(content))
        .with_open(open)
        .with_width(CMD_PALETTE_WIDTH)
        .with_on_close(move || (on_close.borrow_mut())())
        .finish()
}

struct PaletteContent {
    open: bool,
    query: String,
    index: usize,
    commands: Vec<PaletteCommand>,
    on_change: Rc<RefCell<dyn FnMut(String)>>,
    on_move: Rc<RefCell<dyn FnMut(usize)>>,
    on_close: Rc<RefCell<dyn FnMut()>>,
    // Rebuilt on layout so dispatch can resolve the selected command.
    filtered: Vec<usize>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl PaletteContent {
    fn filtered_indices(&self) -> Vec<usize> {
        filter_commands(&self.commands, &self.query)
    }

    /// Clamp the selected index to the current filtered list length.
    fn clamped_index(&self) -> usize {
        let len = self.filtered.len();
        if len == 0 {
            0
        } else {
            self.index.min(len - 1)
        }
    }

    /// Run the command at filtered position `pos`.
    fn run_filtered(&mut self, pos: usize) {
        if let Some(&idx) = self.filtered.get(pos) {
            if let Some(cmd) = self.commands.get(idx) {
                (cmd.action.borrow_mut())();
            }
        }
    }

    fn rebuild(&mut self, app: &AppContext) {
        let sm = app.theme.spacing_px(SpacingToken::Sm);
        let md = app.theme.spacing_px(SpacingToken::Md);
        self.filtered = self.filtered_indices();
        let sel = self.clamped_index();

        let header = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(sm)
            .with_child(
                Text::new("Commands")
                    .with_font_size(13.0)
                    .with_theme_color(ColorToken::Text, app)
                    .finish(),
            )
            .with_child(Spacer::new().finish())
            .with_child(
                Text::new("Esc/click-outside to close")
                    .with_font_size(10.0)
                    .with_theme_color(ColorToken::Muted, app)
                    .finish(),
            )
            .finish();

        let display = if self.query.is_empty() {
            "Type to filter commands…".to_string()
        } else {
            self.query.clone()
        };
        let search_box = Container::new(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(sm)
                .with_child(Spacer::new().finish())
                .with_child(
                    Text::new(display)
                        .with_font_size(12.0)
                        .with_theme_color(
                            if self.query.is_empty() {
                                ColorToken::Muted
                            } else {
                                ColorToken::Text
                            },
                            app,
                        )
                        .finish(),
                )
                .with_child(Spacer::new().finish())
                .finish(),
        )
        .with_padding(EdgeInsets::uniform(md))
        .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
        .with_border(app.theme.color(ColorToken::Accent).into())
        .finish();

        let mut list = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(4.0);

        if self.filtered.is_empty() {
            list = list.with_child(
                Container::new(
                    Text::new("No commands match")
                        .with_font_size(12.0)
                        .with_theme_color(ColorToken::Muted, app)
                        .finish(),
                )
                .with_padding(EdgeInsets::uniform(md))
                .finish(),
            );
        } else {
            let commands = self.commands.clone();
            for (pos, &idx) in self.filtered.iter().enumerate() {
                let cmd = &commands[idx];
                let text = if cmd.shortcut.is_empty() {
                    cmd.label.clone()
                } else {
                    format!("{}\t{}", cmd.label, cmd.shortcut)
                };
                let variant = if pos == sel {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                };
                let action = cmd.action.clone();
                let button = Button::new(
                    Text::new(text)
                        .with_font_size(12.0)
                        .with_theme_color(ColorToken::Text, app)
                        .finish(),
                )
                .with_variant(variant)
                .with_on_click(move || (action.borrow_mut())())
                .finish();
                list = list.with_child(button);
            }
        }

        let column = Flex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(sm)
            .with_child(header)
            .with_child(Divider::horizontal().finish())
            .with_child(search_box)
            .with_child(Divider::horizontal().finish())
            .with_child(
                Expanded::new(Scrollable::new(list.finish(), Axis::Vertical).finish()).finish(),
            );

        let panel = Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::SurfaceRaised)))
            .with_padding(EdgeInsets::uniform(md))
            .with_corner_radius(app.theme.radius_px());
        self.root = Some(panel.finish());
    }
}

impl Element for PaletteContent {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        self.filtered = self.filtered_indices();
        let size = if self.open {
            self.rebuild(app);
            self.root.as_mut().unwrap().layout(constraint, ctx, app)
        } else {
            Vector2F::zero()
        };
        self.size = Some(size);
        size
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, Default::default()));
        if !self.open {
            return;
        }
        self.root.as_mut().unwrap().paint(origin, ctx, app);
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        if !self.open {
            return false;
        }
        match event {
            DispatchedEvent::KeyDown { key, .. } => {
                match key.as_str() {
                    "ArrowDown" => {
                        let len = self.filtered.len();
                        let target = if len == 0 { 0 } else { (self.clamped_index() + 1).min(len - 1) };
                        (self.on_move.borrow_mut())(target);
                    }
                    "ArrowUp" => {
                        let len = self.filtered.len();
                        let target = if len == 0 { 0 } else { self.clamped_index().saturating_sub(1) };
                        (self.on_move.borrow_mut())(target);
                    }
                    "Enter" => {
                        // Recompute so the selection reflects the current query.
                        self.filtered = self.filtered_indices();
                        let sel = self.clamped_index();
                        self.run_filtered(sel);
                    }
                    "Escape" => (self.on_close.borrow_mut())(),
                    "Backspace" => {
                        let mut q = self.query.clone();
                        q.pop();
                        (self.on_change.borrow_mut())(q);
                    }
                    c if c.chars().count() == 1 => {
                        let mut q = self.query.clone();
                        q.push_str(c);
                        (self.on_change.borrow_mut())(q);
                    }
                    _ => {}
                }
                true
            }
            // Mouse interactions (clicking a row) are handled by the row
            // Buttons, so forward them to the built tree.
            _ => self.root.as_mut().map(|r| r.dispatch_event(event, ctx, app)).unwrap_or(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(label: &str) -> PaletteCommand {
        PaletteCommand::new(label, "", || {})
    }

    #[test]
    fn filter_commands_matches_substring_ignoring_case() {
        let commands = vec![
            cmd("Split right"),
            cmd("Close pane"),
            cmd("New space"),
        ];
        let idx = filter_commands(&commands, "split");
        assert_eq!(idx, vec![0]);
        let idx = filter_commands(&commands, "PANE");
        assert_eq!(idx, vec![1]);
    }

    #[test]
    fn empty_query_returns_all_commands_in_order() {
        let commands = vec![cmd("a"), cmd("b"), cmd("c")];
        assert_eq!(filter_commands(&commands, ""), vec![0, 1, 2]);
        assert_eq!(filter_commands(&commands, " "), vec![0, 1, 2]);
    }

    #[test]
    fn no_match_returns_empty() {
        let commands = vec![cmd("Split right")];
        assert!(filter_commands(&commands, "zzz").is_empty());
    }

    #[test]
    fn run_filtered_triggers_the_selected_commands_action() {
        use std::cell::Cell;
        let fired_a = Rc::new(Cell::new(0usize));
        let fired_b = Rc::new(Cell::new(0usize));
        let a = PaletteCommand::new("New space", "⌘⇧N", {
            let fired_a = Rc::clone(&fired_a);
            move || fired_a.set(fired_a.get() + 1)
        });
        let b = PaletteCommand::new("Split right", "⌘ ", {
            let fired_b = Rc::clone(&fired_b);
            move || fired_b.set(fired_b.get() + 1)
        });
        let mut content = PaletteContent {
            open: true,
            query: String::new(),
            index: 0,
            commands: vec![a, b],
            on_change: Rc::new(RefCell::new(|_: String| {})),
            on_move: Rc::new(RefCell::new(|_: usize| {})),
            on_close: Rc::new(RefCell::new(|| {})),
            // "Split right" is the only filtered match at position 0.
            filtered: vec![1],
            root: None,
            size: None,
            origin: None,
        };
        content.run_filtered(0);
        assert_eq!(fired_b.get(), 1, "selected command should run");
        assert_eq!(fired_a.get(), 0, "unselected command should not run");
    }
}
