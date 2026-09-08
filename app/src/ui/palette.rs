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
use goble_ui::theme::{ColorToken, SpacingToken};
use goble_ui::Dialog;

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
