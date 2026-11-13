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

use goble_ui::elements::SlashMenuItem;
use goble_ui::elements::{
    AppContext, Axis, Button, ButtonVariant, Container, CrossAxisAlignment, Divider, EdgeInsets,
    Element, EventContext, Expanded, Fill, Flex, LayoutContext, MainAxisSize, PaintContext, Point,
    Scrollable, SizeConstraint, Spacer, Text,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::Vector2F;
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

/// What taking a row of the input's band does. A command runs something in the
/// app, on the pane's behalf; a skill or a path is part of the draft, because
/// that is where the harness reads it from.
#[derive(Clone)]
pub enum CommandEffect {
    Run(Rc<RefCell<dyn FnMut()>>),
    Insert(String),
}

/// One entry a rich input offers when its draft starts with a trigger prefix:
/// the name as it is typed after it, what taking it does, and the prefix it is
/// typed with (`/` a command, `$` a skill, `@` a path).
#[derive(Clone)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    pub prefix: char,
    pub effect: CommandEffect,
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
            prefix: '/',
            effect: CommandEffect::Run(Rc::new(RefCell::new(action))),
        }
    }

    /// A row that puts its token in the draft instead of running something: a
    /// skill or a path the next prompt carries.
    fn insert(
        name: impl Into<String>,
        description: impl Into<String>,
        prefix: char,
        text: String,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            prefix,
            effect: CommandEffect::Insert(text),
        }
    }
}

/// The list the draft's own trigger asks for: the harness's commands for `/`,
/// the skills it advertises for `$`, the working directory's paths for `@`.
///
/// The trigger is read from the draft the same way the band's open predicate
/// reads it, so the list the view draws and the list it filters can never
/// disagree.
pub fn draft_commands(state: &UiSnapshot, actions: &UiActions, draft: &str) -> Vec<SlashCommand> {
    match goble_ui::elements::DraftTrigger::from_draft(draft) {
        Some(goble_ui::elements::DraftTrigger::Command) => slash_commands(state, actions),
        Some(goble_ui::elements::DraftTrigger::Skill) => skill_commands(state),
        Some(goble_ui::elements::DraftTrigger::Mention) => mention_commands(state),
        None => Vec::new(),
    }
}

/// The app's own gestures, by the id the harness advertises a command under.
/// The harness owns the names and the descriptions; this table owns what they
/// do, so a command the app has no gesture for draws no row.
fn command_actions(
    state: &UiSnapshot,
    actions: &UiActions,
) -> std::collections::HashMap<String, Rc<RefCell<dyn FnMut()>>> {
    let run = |action: &Rc<RefCell<dyn FnMut()>>| {
        let act = action.clone();
        move || (act.borrow_mut())()
    };
    let pane_id = state.active_pane_id;
    let mut table: std::collections::HashMap<String, Rc<RefCell<dyn FnMut()>>> =
        std::collections::HashMap::new();
    let mut push = |id: &str, action: Rc<RefCell<dyn FnMut()>>| {
        table.insert(id.to_string(), action);
    };
    push("new", Rc::new(RefCell::new(run(&actions.on_create_submit))));
    push(
        "clear",
        Rc::new(RefCell::new(run(&actions.on_clear_transcript))),
    );
    {
        let act = actions.on_fork_conversation.clone();
        push(
            "fork",
            Rc::new(RefCell::new(move || (act.borrow_mut())(pane_id))),
        );
    }
    push("stop", Rc::new(RefCell::new(run(&actions.on_stop))));
    push(
        "shortcuts",
        Rc::new(RefCell::new(run(&actions.on_toggle_shortcuts_help))),
    );
    push("settings", Rc::new(RefCell::new(run(&actions.on_settings))));
    push("space", Rc::new(RefCell::new(run(&actions.on_add_space))));
    push(
        "split",
        Rc::new(RefCell::new(run(&actions.on_split_right))),
    );
    push(
        "terminal",
        Rc::new(RefCell::new(run(&actions.on_new_terminal))),
    );
    push("close", Rc::new(RefCell::new(run(&actions.on_close_pane))));
    push(
        "sidebar",
        Rc::new(RefCell::new(run(&actions.on_toggle_right_sidebar))),
    );
    push(
        "fullscreen",
        Rc::new(RefCell::new(run(&actions.on_toggle_fullscreen))),
    );
    {
        let act = actions.on_toggle_pane_maximized.clone();
        push(
            "maximize",
            Rc::new(RefCell::new(move || (act.borrow_mut())(pane_id))),
        );
    }
    push(
        "projects",
        Rc::new(RefCell::new(run(&actions.on_projects))),
    );
    push(
        "scheduled",
        Rc::new(RefCell::new(run(&actions.on_open_crons))),
    );
    push(
        "tasks",
        Rc::new(RefCell::new(run(&actions.on_toggle_workflow_view))),
    );
    push(
        "workflows",
        Rc::new(RefCell::new(run(&actions.on_workflows))),
    );
    push("mcps", Rc::new(RefCell::new(run(&actions.on_mcps))));
    {
        // The same toggles Settings carries; either surface flips them, and the
        // choice persists.
        let toggle_dark = actions.on_toggle_dark_mode.clone();
        let dark_mode = state.settings_dark_mode;
        push(
            "theme",
            Rc::new(RefCell::new(move || {
                (toggle_dark.borrow_mut())(!dark_mode)
            })),
        );
        let toggle_vim = actions.on_toggle_vim_mode.clone();
        let vim_mode = state.vim_mode;
        push(
            "vim-mode",
            Rc::new(RefCell::new(move || (toggle_vim.borrow_mut())(!vim_mode))),
        );
        // grok-build's permission-mode command. The mode is app-wide, so the
        // command writes the same policy the composer menu and Settings do.
        let select_mode = actions.on_select_approval_mode.clone();
        let current_mode = state.approval_mode;
        push(
            "always-approve",
            Rc::new(RefCell::new(move || {
                let next = if current_mode == goble_ui::ApprovalMode::AlwaysApprove {
                    goble_ui::ApprovalMode::Manual
                } else {
                    goble_ui::ApprovalMode::AlwaysApprove
                };
                (select_mode.borrow_mut())(pane_id, next);
            })),
        );
    }
    table
}

/// The commands a rich input offers when its draft starts with `/`: the list the
/// harness advertises, with each entry mapped to the app gesture that runs it.
/// One entry per configured model comes from the same catalog (`/model <id>`),
/// so a model the configuration dropped stops being offered.
///
/// The chord that runs a command is deliberately not repeated here: the
/// instruction strip under the input carries the gestures, and a command is run
/// from the list itself.
pub fn slash_commands(state: &UiSnapshot, actions: &UiActions) -> Vec<SlashCommand> {
    let table = command_actions(state, actions);
    let pane_id = state.active_pane_id;
    let mut commands: Vec<SlashCommand> = state
        .composer_catalog
        .commands
        .iter()
        .filter(|command| command.trigger == goble_harness_types::ComposerTrigger::Slash)
        .filter_map(|command| {
            let effect = if let Some(model) = command.action.strip_prefix("model:") {
                // `/model <id>` is one entry per configured model: the catalog
                // names the model, the press applies it to the pane.
                let model = model.to_string();
                let act = actions.on_model_select.clone();
                let pane_id = pane_id;
                CommandEffect::Run(Rc::new(RefCell::new(move || {
                    (act.borrow_mut())(pane_id, model.clone())
                })))
            } else {
                CommandEffect::Run(table.get(&command.action)?.clone())
            };
            Some(SlashCommand {
                name: command.name.clone(),
                description: command.description.clone(),
                prefix: '/',
                effect,
            })
        })
        .collect();
    if cfg!(debug_assertions) {
        // Dev builds can re-enter onboarding from the slash menu, beside the
        // `GOBLE_ONBOARDING=1` startup switch, so the flow can be iterated on.
        let replay = actions.on_onboarding.clone();
        commands.push(SlashCommand::new(
            "onboarding",
            "Replay the first-run setup (dev build)",
            move || (replay.borrow_mut())(crate::ui::OnboardingAction::Replay),
        ));
    }
    // A CLI harness owns its own model and approval policy, so the app's
    // switches for them do not belong in that conversation: nothing the input
    // could write would reach the harness.
    if active_conversation_harness(state).is_some() {
        commands.retain(|command| {
            command.name != "always-approve" && !command.name.starts_with("model ")
        });
    }
    commands
}

/// The skills the harness advertises, as rows a `$` draft lists. Taking one
/// writes its token into the draft: the skill is part of the next prompt, and
/// the harness reads it from what the user typed.
pub fn skill_commands(state: &UiSnapshot) -> Vec<SlashCommand> {
    state
        .composer_catalog
        .commands
        .iter()
        .filter(|command| command.trigger == goble_harness_types::ComposerTrigger::Skill)
        .map(|command| {
            SlashCommand::insert(
                command.name.clone(),
                command.description.clone(),
                '$',
                format!("${} ", command.name),
            )
        })
        .collect()
}

/// The paths a `@` draft lists: what sits in the pane's working directory, read
/// while the draft asks for it. Taking one writes `@<path>` into the draft.
pub fn mention_commands(state: &UiSnapshot) -> Vec<SlashCommand> {
    state
        .mention_paths
        .iter()
        .map(|path| {
            SlashCommand::insert(
                path.clone(),
                "Add this path to the prompt",
                '@',
                format!("@{path} "),
            )
        })
        .collect()
}

/// The CLI harness the active pane's conversation belongs to, when it belongs
/// to one. The native harness is `None`: its model and approval policy are the
/// app's, and their commands stay in the list.
fn active_conversation_harness(
    state: &UiSnapshot,
) -> Option<&'static goble_core::harness::HarnessSpec> {
    let conversation_id = state
        .pane_chat
        .get(&state.active_pane_id)
        .map(|chat| chat.conversation_id.as_str())
        .filter(|conversation_id| !conversation_id.is_empty())?;
    let entry = state
        .conversations
        .iter()
        .find(|entry| entry.id == conversation_id)?;
    goble_core::harness::harness_for(&entry.harness).filter(|spec| spec.id != "goble")
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

/// A command list as the menu draws it: the name under its own prefix, and what
/// it does.
pub fn slash_items(commands: &[SlashCommand]) -> Vec<SlashMenuItem> {
    commands
        .iter()
        .map(|command| {
            SlashMenuItem::new(command.name.clone(), command.description.clone())
                .with_prefix(command.prefix)
        })
        .collect()
}

/// Take the row at `index` of `commands`. A command runs and the bar it was
/// typed in is spent, so the menu closes with the draft cleared; a skill or a
/// path is written into the draft by `insert`, because it is part of the next
/// prompt and the menu's query was only how it was picked.
pub fn slash_accept(
    commands: Vec<SlashCommand>,
    close: Rc<RefCell<dyn FnMut()>>,
    insert: Rc<RefCell<dyn FnMut(String)>>,
) -> impl FnMut(usize) + 'static {
    move |index: usize| {
        let Some(command) = commands.get(index) else {
            (close.borrow_mut())();
            return;
        };
        match command.effect.clone() {
            CommandEffect::Run(action) => {
                (action.borrow_mut())();
                (close.borrow_mut())();
            }
            CommandEffect::Insert(text) => (insert.borrow_mut())(text),
        }
    }
}

/// The query a trigger draft carries: what was typed after its prefix, up to the
/// first word. The rest of the line is the command's own text, not its name.
pub fn draft_query(draft: &str) -> Option<String> {
    goble_ui::elements::DraftTrigger::from_draft(draft).map(|trigger| trigger.query(draft))
}

/// The paths a `@` draft lists: one level under `dir`, directories first and
/// then files, both alphabetical. Dot entries are left out — they are noise in a
/// prompt — and a directory that cannot be read lists nothing, so a pane that
/// lost its shell never breaks the band.
pub fn mention_paths(dir: &str, limit: usize) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut directories: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            // The slash says the row is a directory and invites the next
            // segment, the way a shell completes one.
            directories.push(format!("{name}/"));
        } else {
            files.push(name);
        }
    }
    directories.sort();
    files.sort();
    directories
        .into_iter()
        .chain(files)
        .take(limit)
        .collect()
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
        .with_border(app.theme.color(ColorToken::Focus).into())
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
                        let target = if len == 0 {
                            0
                        } else {
                            (self.clamped_index() + 1).min(len - 1)
                        };
                        (self.on_move.borrow_mut())(target);
                    }
                    "ArrowUp" => {
                        let len = self.filtered.len();
                        let target = if len == 0 {
                            0
                        } else {
                            self.clamped_index().saturating_sub(1)
                        };
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
            _ => self
                .root
                .as_mut()
                .map(|r| r.dispatch_event(event, ctx, app))
                .unwrap_or(false),
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
        let commands = vec![cmd("Split right"), cmd("Close pane"), cmd("New space")];
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
