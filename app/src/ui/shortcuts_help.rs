//! The keyboard shortcuts panel: every chord the workspace answers, grouped
//! the way grok-build's cheatsheet groups its own (`Ctrl+.` in its TUI).
//!
//! The list is this GUI's own binding table, not a copy of grok-build's: a row
//! is drawn only where the app really binds the chord it names, so the panel
//! never promises a key nothing answers — the same rule the input's instruction
//! strip follows. The global chords live in `root_view/element.rs`, the input's
//! in the composer, and the palette's in `palette.rs`.
//!
//! The rows are the strip's own key caps ([`ShortcutHint::caps`]), in a fixed
//! key column so the names line up down the panel; the caps themselves are the
//! ones the rich input draws, so a chord looks the same in both places.
//!
//! The filter is the panel's search mode, the one grok-build's cheatsheet
//! carries: while the panel is up the keyboard belongs to it — printable keys
//! edit the filter, `Backspace` deletes, `Ctrl/Cmd+/` empties it, `Esc` empties
//! it and only closes the panel once it is already empty, and `↑`/`↓` walk the
//! rows the filter leaves.

use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Axis, ConstrainedBox, Container, CrossAxisAlignment, Divider, EdgeInsets, Element,
    EventContext, Fill, Flex, HoverRow, Icon, KeyHandler, LayoutContext, MainAxisSize,
    PaintContext, Point, ScrollState, Scrollable, ShortcutHint, ShortcutHints, SizeConstraint,
    Spacer, Text, TopbarButton, caret_beam,
};
use goble_ui::event::DispatchedEvent;
use goble_ui::geometry::{Vector2F, vec2f};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::UiActions;
use super::snapshot::UiSnapshot;

/// The panel's width: wide enough for the longest chord plus its name, and
/// narrower than a settings page so it reads as a sheet over the workspace.
pub const SHORTCUTS_PANEL_WIDTH: f32 = 560.0;

/// The key column: every chord's caps are drawn into this width, so the names
/// start at the same x on every row. The widest chord here is three caps.
const KEY_COLUMN_WIDTH: f32 = 108.0;

/// The panel's content: a section name and its rows, in the order they are
/// drawn. Every row is a chord the app answers:
///
/// - **Workspace** — the global chords in `root_view/element.rs`, which are
///   handled before the tree so a focused composer cannot swallow them.
/// - **Input** — the composer's own gestures, the set the instruction strip
///   above the editor carries.
/// - **Palette** — the command palette's keys (`palette.rs`), reachable from
///   the Workspace section's `⌘K`.
pub(crate) fn sections() -> Vec<(&'static str, Vec<ShortcutHint>)> {
    vec![
        (
            "Workspace",
            vec![
                ShortcutHint::new(&["⌘", "K"], "command palette"),
                ShortcutHint::new(&["⌘", "⇧", "T"], "new terminal"),
                ShortcutHint::new(&["⌘", "Space"], "split right"),
                ShortcutHint::new(&["⌘", "⇧", "D"], "split down"),
                ShortcutHint::new(&["⌘", "W"], "close pane"),
                ShortcutHint::new(&["⌘", "⇧", "W"], "workflow runs"),
                ShortcutHint::new(&["⌘", "←", "→"], "previous / next pane"),
                ShortcutHint::new(&["⌘", "↑", "↓"], "pane above / below"),
                ShortcutHint::new(&["⌃", "⇥"], "next / previous space"),
            ],
        ),
        (
            "Input",
            vec![
                ShortcutHint::new(&["↵"], "send (queues while a turn runs)"),
                ShortcutHint::new(&["⌘", "↵"], "new conversation"),
                ShortcutHint::new(&["!"], "shell command"),
            ],
        ),
        (
            "Palette",
            vec![
                ShortcutHint::new(&["↑", "↓"], "move the selection"),
                ShortcutHint::new(&["↵"], "run the selected command"),
                ShortcutHint::new(&["Esc"], "close the palette"),
            ],
        ),
    ]
}

/// How far `PageUp`/`PageDown` walk the highlight: a screenful of rows.
const PAGE_STEP: i32 = 10;

/// The list's own viewport height. The panel is content-sized, so its column
/// hands the list an unbounded height — the list caps itself here, high enough
/// for the sections at a glance and low enough that the panel stays a sheet
/// over the workspace, and the rows scroll inside it past that.
const LIST_MAX_HEIGHT: f32 = 360.0;

/// The words a key cap can be filtered by, for the keys the bundled text faces
/// cannot spell: those caps are drawn as icons (`⌘`, `⇧`, `↵`), so `cmd k` has
/// to find `⌘ K`. A cap the font carries — `K`, `Esc`, `!` — needs no words.
fn cap_words(key: &str) -> &'static str {
    match key {
        "⌘" => "cmd command",
        "⇧" => "shift",
        "⌥" => "option alt",
        "⌃" => "ctrl control",
        "↵" | "⏎" => "return enter",
        "⌫" => "backspace delete",
        "⇥" => "tab",
        "↑" => "up",
        "↓" => "down",
        "←" => "left",
        "→" => "right",
        _ => "",
    }
}

/// Every string a filter term may match for one row: the row's name, each of
/// its caps both as drawn and as words, and the chord the caps spell out —
/// joined (`⌘K`) and written out (`cmd k`) — so a chord can also be typed as a
/// single term.
fn haystacks(hint: &ShortcutHint) -> Vec<String> {
    let mut stacks = vec![hint.label().to_lowercase()];
    for key in hint.keys() {
        stacks.push(key.to_lowercase());
        let words = cap_words(key);
        if !words.is_empty() {
            stacks.push(words.to_string());
        }
    }
    stacks.push(hint.keys().join("").to_lowercase());
    stacks.push(
        hint.keys()
            .iter()
            .map(|key| {
                cap_words(key)
                    .split_whitespace()
                    .next()
                    .unwrap_or(key)
                    .to_string()
            })
            .collect::<Vec<String>>()
            .join(" "),
    );
    stacks
}

/// Whether the row survives `terms`. The filter is folded to lower case and
/// split on whitespace, and **every** term has to land somewhere in the row —
/// in its name or in one of its caps — so `split`, `⌘K`, `cmd k` and `palette`
/// each narrow the list, and a second word narrows it further. No terms (an
/// empty filter, or only spaces) keeps every row.
fn matches(hint: &ShortcutHint, terms: &[String]) -> bool {
    let stacks = haystacks(hint);
    terms
        .iter()
        .all(|term| stacks.iter().any(|stack| stack.contains(term)))
}

/// The sections with only the rows `filter` keeps, in draw order. A section
/// whose rows all filter out is dropped, so no heading outlives its rows.
pub(crate) fn filtered_sections(filter: &str) -> Vec<(&'static str, Vec<ShortcutHint>)> {
    let terms: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
    sections()
        .into_iter()
        .filter_map(|(title, hints)| {
            let rows: Vec<ShortcutHint> = hints
                .into_iter()
                .filter(|hint| matches(hint, &terms))
                .collect();
            (!rows.is_empty()).then_some((title, rows))
        })
        .collect()
}

/// How many rows `filter` leaves — the length of the list the highlight moves
/// through.
pub(crate) fn visible_row_count(filter: &str) -> usize {
    filtered_sections(filter)
        .into_iter()
        .map(|(_, rows)| rows.len())
        .sum()
}

/// A section heading: the name in the muted caption weight the panels use.
fn heading(app: &AppContext, title: &str) -> Box<dyn Element> {
    Text::new(title)
        .with_font_size(11.0)
        .with_theme_color(ColorToken::Muted, app)
        .finish()
}

/// One row: the chord's caps in the key column, then the name. The column is a
/// fixed width, so the names start at the same x on every row — a `Flex::row`
/// shrinks to its content, so the caps ride in a full-width row inside the
/// `ConstrainedBox` to hold the column open.
fn row(app: &AppContext, hint: ShortcutHint) -> Box<dyn Element> {
    let label = hint.label().to_string();
    let key_column = ConstrainedBox::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_child(hint.caps(app))
            .finish(),
    )
    .with_width(KEY_COLUMN_WIDTH)
    .finish();
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(app.theme.spacing_px(SpacingToken::Sm))
        .with_child(key_column)
        .with_child(
            Text::new(label)
                .with_font_size(11.0)
                .with_theme_color(ColorToken::Muted, app)
                .with_max_lines(1)
                .finish(),
        )
        .finish()
}

/// The panel: a header (title, ✕), a filter line and a scrollable column of
/// sections, with the panel's own keys under them. It is an overlay — the
/// workspace stays mounted underneath — and its keys are intercepted before any
/// child sees them, so the filter takes them and Escape steps back through it.
pub fn build_shortcuts_help(
    app: &AppContext,
    state: &UiSnapshot,
    actions: &UiActions,
) -> Box<dyn Element> {
    let spacing = app.theme.spacing_px(SpacingToken::Md);
    let sm = app.theme.spacing_px(SpacingToken::Sm);

    let on_close = actions.on_close_shortcuts_help.clone();
    let close_button = TopbarButton::new(
        Icon::new("close")
            .with_size(16.0)
            .with_theme_color(ColorToken::Muted, app)
            .finish(),
    )
    .with_size(28.0)
    .with_corner_radius(0.0)
    .with_on_click(move || (on_close.borrow_mut())())
    .finish();

    let header = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(sm)
        .with_child(
            Text::new("Keyboard shortcuts")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Text, app)
                .finish(),
        )
        .with_child(Spacer::new().finish())
        .with_child(close_button)
        .finish();

    // The filter line: the panel's own search field, drawn like the palette's —
    // the placeholder in the muted weight while it is empty, the typed text
    // otherwise, and the caret and ring in the focus colour. The panel owns the
    // keyboard while it is open, so the field is always the focused one.
    let empty = state.shortcuts_help_filter.is_empty();
    let display = if empty {
        "Filter shortcuts".to_string()
    } else {
        state.shortcuts_help_filter.clone()
    };
    let filter_line = Container::new(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Text::new(display)
                    .with_font_size(12.0)
                    .with_theme_color(
                        if empty {
                            ColorToken::Muted
                        } else {
                            ColorToken::Text
                        },
                        app,
                    )
                    .with_max_lines(1)
                    .finish(),
            )
            .with_child(caret_beam(app))
            .finish(),
    )
    .with_padding(EdgeInsets::uniform(sm))
    .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
    .with_border(app.theme.color(ColorToken::Focus).into())
    .finish();

    let visible = filtered_sections(&state.shortcuts_help_filter);
    let row_count: usize = visible.iter().map(|(_, rows)| rows.len()).sum();
    // The highlight is a position in the filtered list, so a filter that shrank
    // it can never leave the highlight pointing at a row that is not drawn.
    let highlight = (row_count > 0).then(|| state.shortcuts_help_index.min(row_count - 1));

    let mut items: Vec<Box<dyn Element>> = Vec::new();
    let mut highlighted_item = None;
    let mut row_seen = 0;
    for (title, rows) in visible {
        items.push(heading(app, title));
        for hint in rows {
            let selected = highlight == Some(row_seen);
            if selected {
                highlighted_item = Some(items.len());
            }
            items.push(
                HoverRow::new(row(app, hint))
                    .with_padding(EdgeInsets::new(0.0, 2.0, 0.0, 2.0))
                    .with_selected(selected)
                    .finish(),
            );
            row_seen += 1;
        }
    }

    let body: Box<dyn Element> = if items.is_empty() {
        // Nothing matches: the panel says so instead of drawing an empty box,
        // in the palette's own empty-state wording.
        Container::new(
            Text::new("No shortcuts match")
                .with_font_size(12.0)
                .with_theme_color(ColorToken::Muted, app)
                .finish(),
        )
        .with_padding(EdgeInsets::uniform(spacing))
        .finish()
    } else {
        ShortcutList::new(items, highlighted_item, sm, state.shortcuts_help_scroll.clone())
            .finish()
    };

    // The panel's own keys, in the caps the rows draw: what answers right now.
    let footer = ShortcutHints::new(vec![
        ShortcutHint::new(&["⌘", "/"], "clear the filter"),
        ShortcutHint::new(&["↑", "↓"], "move the highlight"),
        ShortcutHint::new(&["Esc"], "clear / close"),
    ])
    .finish(app);

    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(filter_line);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(body);
    column = column.with_child(footer);

    let on_escape = actions.on_shortcuts_help_escape.clone();
    let on_type = actions.on_shortcuts_help_type.clone();
    let on_backspace = actions.on_shortcuts_help_backspace.clone();
    let on_clear = actions.on_shortcuts_help_clear_filter.clone();
    let on_move = actions.on_shortcuts_help_move.clone();
    KeyHandler::new(
        Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::uniform(spacing))
            .finish(),
        move |key: &str, modifiers| {
            if modifiers.ctrl || modifiers.command {
                // The filter's own chord. Every other modified key is a global
                // chord, handled before the tree, and falls through here.
                if key == "/" {
                    (on_clear.borrow_mut())();
                    return true;
                }
                return false;
            }
            if modifiers.alt {
                return false;
            }
            match key {
                "Escape" => {
                    (on_escape.borrow_mut())();
                    true
                }
                "Backspace" => {
                    (on_backspace.borrow_mut())();
                    true
                }
                "ArrowUp" => {
                    (on_move.borrow_mut())(-1);
                    true
                }
                "ArrowDown" => {
                    (on_move.borrow_mut())(1);
                    true
                }
                "PageUp" => {
                    (on_move.borrow_mut())(-PAGE_STEP);
                    true
                }
                "PageDown" => {
                    (on_move.borrow_mut())(PAGE_STEP);
                    true
                }
                // A row is a chord the user presses out in the workspace, not an
                // action the panel can take, so Return runs nothing — it is
                // swallowed all the same, so it cannot reach the composer under
                // the panel.
                "Enter" => true,
                _ => {
                    let mut chars = key.chars();
                    match (chars.next(), chars.next()) {
                        // Everything else the panel can print is filter text.
                        (Some(ch), None) if !ch.is_control() => {
                            (on_type.borrow_mut())(ch);
                            true
                        }
                        _ => false,
                    }
                }
            }
        },
    )
    .finish()
}

/// The panel's list: its items in draw order — a heading or a row — over a
/// [`Scrollable`], which is what scrolls the highlighted row into view. The
/// list measures its own items as it lays them out, because the tree is rebuilt
/// every frame and the geometry cannot be kept anywhere else; the offset the
/// wheel and the highlight write to lives in app state.
struct ShortcutList {
    items: Vec<Box<dyn Element>>,
    /// The item to keep in view, if any.
    highlight: Option<usize>,
    spacing: f32,
    scroll: Rc<RefCell<ScrollState>>,
    root: Option<Box<dyn Element>>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl ShortcutList {
    fn new(
        items: Vec<Box<dyn Element>>,
        highlight: Option<usize>,
        spacing: f32,
        scroll: Rc<RefCell<ScrollState>>,
    ) -> Self {
        Self {
            items,
            highlight,
            spacing,
            scroll,
            root: None,
            size: None,
            origin: None,
        }
    }
}

impl Element for ShortcutList {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        // Built once per frame: the items are measured, then handed over to the
        // viewport, so a later layout call only relays.
        if let Some(root) = self.root.as_mut() {
            let size = root.layout(constraint, ctx, app);
            self.size = Some(size);
            return size;
        }
        let width = constraint.max.x.max(0.0);
        let mut spans = Vec::with_capacity(self.items.len());
        let mut height = 0.0;
        for item in &mut self.items {
            let size = item.layout(
                SizeConstraint::new(vec2f(width, 0.0), vec2f(width, f32::INFINITY)),
                ctx,
                app,
            );
            spans.push((height, size.y));
            height += size.y + self.spacing;
        }
        let content_height = (height - self.spacing).max(0.0);

        let mut column = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(self.spacing);
        for item in self.items.drain(..) {
            column = column.with_child(item);
        }
        // The panel is content-sized, so its column leaves the height open: the
        // viewport is the shorter of the content and the list's own cap, so the
        // list is as tall as it needs to be until the rows no longer fit.
        let viewport_height = content_height.min(LIST_MAX_HEIGHT);
        let mut scrollable = Scrollable::new(column.finish(), Axis::Vertical)
            .with_state(Rc::clone(&self.scroll));
        let size = scrollable.layout(
            SizeConstraint::new(
                vec2f(width, viewport_height),
                vec2f(width, viewport_height),
            ),
            ctx,
            app,
        );
        // Keep the highlighted row in view, moving the shortest way to it and
        // never past the row itself. The viewport's metrics are the ones this
        // layout just measured, so the clamp is against this frame's list.
        if let Some((top, extent)) = self.highlight.and_then(|item| spans.get(item).copied()) {
            let viewport = size.y;
            let offset = self.scroll.borrow().offset();
            let target = if top < offset {
                top
            } else if top + extent > offset + viewport {
                top + extent - viewport
            } else {
                offset
            };
            self.scroll.borrow_mut().scroll_by(target - offset);
        }
        self.root = Some(scrollable.finish());
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
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.root
            .as_mut()
            .map(|root| root.dispatch_event(event, ctx, app))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root_view::RootView;
    use crate::state::UiState;
    use crate::ui::Pane;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};
    use goble_ui::event::{DispatchedEvent, ModifiersState};
    use goble_ui::geometry::vec2f;
    use goble_ui::render::{RenderCommand, Renderer};
    use goble_ui::{Element, EventContext, LayoutContext, PaintContext, SizeConstraint};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    /// A whole app tree over a real store, with the workspace mounted and every
    /// overlay shut, so the panel is the only thing that can appear.
    fn workspace() -> (Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let root = RootView::new(&AppContext::default(), &desktop, None);
        let state = root.state_rc();
        {
            let mut s = state.borrow_mut();
            s.show_workspace_choice = false;
            s.show_llm_key_banner = false;
            s.right_sidebar_open = false;
            s.crons_open = false;
            s.task_workflow_open = false;
            s.shortcuts_help_open = false;
        }
        (Box::new(root), state, dir)
    }

    /// One frame through the whole app, returning what it painted.
    fn frame(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
        let _ = root.layout(
            SizeConstraint::loose(vec2f(1024.0, 768.0)),
            &mut LayoutContext::default(),
            app,
        );
        let mut ctx = PaintContext::new(Renderer::new());
        root.paint(vec2f(0.0, 0.0), &mut ctx, app);
        ctx.renderer
            .take()
            .map(|renderer| renderer.commands().to_vec())
            .unwrap_or_default()
    }

    fn chord(
        root: &mut Box<dyn Element>,
        app: &AppContext,
        key: &str,
        modifiers: ModifiersState,
    ) -> bool {
        let mut ctx = EventContext::default();
        root.dispatch_event(
            &DispatchedEvent::KeyDown {
                key: key.to_string(),
                modifiers,
            },
            &mut ctx,
            app,
        )
    }

    /// The panel's own chord: Ctrl+.
    fn toggle(root: &mut Box<dyn Element>, app: &AppContext) -> bool {
        chord(
            root,
            app,
            ".",
            ModifiersState {
                ctrl: true,
                ..Default::default()
            },
        )
    }

    fn drawn(commands: &[RenderCommand], text: &str) -> bool {
        commands.iter().any(|command| {
            matches!(command, RenderCommand::DrawText { text: run, .. } if run == text)
        })
    }

    /// The leaves of a pane tree — one per visible pane, splits included.
    fn leaves(pane: &Pane) -> usize {
        match pane {
            Pane::Leaf { .. } => 1,
            Pane::Split { first, second, .. } => leaves(first) + leaves(second),
        }
    }

    fn pane_count(state: &Rc<RefCell<UiState>>) -> usize {
        state
            .borrow()
            .spaces
            .iter()
            .map(|space| leaves(&space.root))
            .sum()
    }

    /// The section the panel draws under `title`.
    fn section(title: &str) -> Vec<ShortcutHint> {
        sections()
            .into_iter()
            .find(|(name, _)| *name == title)
            .unwrap_or_else(|| panic!("the panel has a {title} section"))
            .1
    }

    /// The chord a row names, in the vocabulary the handlers receive. The caps
    /// are display labels — `⌘`, `←`, `Space`, `Esc` — while the window hands a
    /// handler `"ArrowLeft"`, `" "` and `"Escape"`
    /// (`platform/window.rs::logical_key_string`), so the two are mapped here.
    /// A row may name two keys (`⌘ ← →`), and both answer the gesture.
    fn chord_of(hint: &ShortcutHint) -> (ModifiersState, Vec<String>) {
        let mut modifiers = ModifiersState::none();
        let mut keys = Vec::new();
        for key in hint.keys() {
            match key.as_str() {
                "⌘" => modifiers.command = true,
                "⇧" => modifiers.shift = true,
                "⌃" => modifiers.ctrl = true,
                "⇥" => keys.push("Tab".to_string()),
                "←" => keys.push("ArrowLeft".to_string()),
                "→" => keys.push("ArrowRight".to_string()),
                "↑" => keys.push("ArrowUp".to_string()),
                "↓" => keys.push("ArrowDown".to_string()),
                "Space" => keys.push(" ".to_string()),
                "Esc" => keys.push("Escape".to_string()),
                other => keys.push(other.to_string()),
            }
        }
        (modifiers, keys)
    }

    /// The panel lists the chords the app really answers — the global ones, the
    /// composer's and the palette's — over a workspace that stays mounted, and
    /// every route closes it: the chord again, Escape, the ✕ and the backdrop.
    #[test]
    fn the_panel_lists_the_workspaces_own_chords_and_every_route_closes_it() {
        let app = AppContext::default();
        let (mut root, state, _dir) = workspace();

        // Shut, none of it is drawn.
        let commands = frame(&mut root, &app);
        assert!(!drawn(&commands, "Keyboard shortcuts"));
        assert!(!drawn(&commands, "command palette"));

        // Ctrl+. opens it.
        assert!(toggle(&mut root, &app), "the chord is consumed");
        assert!(state.borrow().shortcuts_help_open, "Ctrl+. opens the panel");

        let commands = frame(&mut root, &app);
        for (title, hints) in sections() {
            assert!(drawn(&commands, title), "the panel draws {title:?}");
            for hint in hints {
                assert!(
                    drawn(&commands, hint.label()),
                    "the panel draws {:?}",
                    hint.label()
                );
            }
        }
        // The caps are drawn too: the modifier and arrow keys as the icons the
        // strip draws them from, and the words and punctuation the text font can
        // carry as runs.
        for cap in ["Space", "Esc", "!"] {
            assert!(drawn(&commands, cap), "the panel draws the {cap} cap");
        }
        for (cap, icon) in [
            ("⌘", "key-command"),
            ("⇧", "key-shift"),
            ("←", "key-arrow-left"),
            ("→", "key-arrow-right"),
            ("↑", "key-arrow-up"),
            ("↓", "key-arrow-down"),
        ] {
            assert!(
                commands.iter().any(|command| matches!(
                    command,
                    RenderCommand::DrawIcon { name, .. } if name == icon
                )),
                "the {cap} cap is the {icon} icon, as on the strip"
            );
        }
        assert!(
            commands.iter().any(|command| matches!(
                command,
                RenderCommand::DrawIcon { name, .. } if name == "key-return"
            )),
            "the Return key's cap is the icon, as on the strip"
        );

        // The workspace is still mounted underneath: the overlay covers it
        // instead of replacing it. (The tab holds an agent with no conversation
        // subject yet, so it reads "New Agent".)
        assert!(drawn(&commands, "New Agent"), "the workspace stays mounted");
        assert!(
            drawn(&commands, "Ask anything..."),
            "the pane's composer stays mounted under the panel"
        );

        // Nothing overflows: every row the panel draws lands inside the centered
        // panel's band and above the window's bottom edge, so a row added later
        // cannot silently fall off the panel.
        let panel_left = (1024.0 - SHORTCUTS_PANEL_WIDTH) * 0.5;
        let mut panel_texts: Vec<String> = vec!["Keyboard shortcuts".to_string()];
        for (title, hints) in sections() {
            panel_texts.push(title.to_string());
            for hint in hints {
                panel_texts.push(hint.label().to_string());
                // A cap that draws from the icon set has no run of its own
                // (`key_has_icon`); the icon assertions above cover it.
                panel_texts.extend(
                    hint.keys()
                        .iter()
                        .filter(|key| !goble_ui::elements::key_has_icon(key))
                        .cloned(),
                );
            }
        }
        for text in &panel_texts {
            let lines: Vec<f32> = commands
                .iter()
                .filter_map(|command| match command {
                    RenderCommand::DrawText { text: run, origin, .. } if run == text => {
                        Some(origin.x)
                    }
                    _ => None,
                })
                .collect();
            assert!(
                lines
                    .iter()
                    .any(|x| *x >= panel_left && *x <= panel_left + SHORTCUTS_PANEL_WIDTH),
                "{text:?} is drawn inside the panel's band (found x={lines:?}, band starts at {panel_left})"
            );
        }
        let bottoms: Vec<f32> = commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { origin, .. } => Some(origin.y),
                _ => None,
            })
            .collect();
        assert!(
            bottoms.iter().all(|y| *y < 768.0),
            "every run is inside the window's height"
        );

        // Escape closes it.
        assert!(
            chord(&mut root, &app, "Escape", ModifiersState::none()),
            "Escape is consumed"
        );
        assert!(!state.borrow().shortcuts_help_open, "Escape closes the panel");

        // The chord toggles it shut again.
        assert!(toggle(&mut root, &app));
        assert!(state.borrow().shortcuts_help_open, "the chord re-opens it");
        assert!(toggle(&mut root, &app));
        assert!(
            !state.borrow().shortcuts_help_open,
            "the same chord closes the panel"
        );

        // The ✕ closes it.
        assert!(toggle(&mut root, &app));
        let commands = frame(&mut root, &app);
        let close = commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawIcon { origin, name, .. } if name == "close" => Some(*origin),
                _ => None,
            })
            .expect("the panel draws its close control");
        let mut ctx = EventContext::default();
        for event in [
            DispatchedEvent::MouseDown {
                position: close,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position: close,
                button: 0,
            },
        ] {
            root.dispatch_event(&event, &mut ctx, &app);
        }
        assert!(!state.borrow().shortcuts_help_open, "the ✕ closes the panel");

        // And a click on the backdrop, left of the centered panel, closes it.
        assert!(toggle(&mut root, &app));
        frame(&mut root, &app);
        let mut ctx = EventContext::default();
        root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: vec2f(10.0, 400.0),
                button: 0,
            },
            &mut ctx,
            &app,
        );
        assert!(
            !state.borrow().shortcuts_help_open,
            "the backdrop closes the panel"
        );
    }

    /// Every row the panel draws under **Workspace** is a chord the app answers:
    /// the panel never promises a key nothing does. The rows are matched by
    /// their label, so a row this test does not know how to prove fails it
    /// instead of passing silently — a new claim arrives with its own proof.
    #[test]
    fn every_chord_the_panel_names_is_one_the_app_answers() {
        let app = AppContext::default();
        for hint in section("Workspace") {
            let (modifiers, keys) = chord_of(&hint);
            let (mut root, state, _dir) = workspace();
            let label = hint.label().to_string();
            match label.as_str() {
                "command palette" => {
                    assert!(chord(&mut root, &app, &keys[0], modifiers), "⌘K answers");
                    assert!(
                        state.borrow().command_palette_open,
                        "⌘K opens the command palette"
                    );
                }
                "new terminal" | "split right" | "split down" => {
                    let before = pane_count(&state);
                    assert!(
                        chord(&mut root, &app, &keys[0], modifiers),
                        "{label} answers"
                    );
                    assert_eq!(pane_count(&state), before + 1, "{label} adds a pane");
                }
                "close pane" => {
                    // Two panes, so closing one leaves a workspace behind.
                    chord(
                        &mut root,
                        &app,
                        " ",
                        ModifiersState {
                            command: true,
                            ..Default::default()
                        },
                    );
                    assert_eq!(pane_count(&state), 2, "the split landed");
                    assert!(chord(&mut root, &app, &keys[0], modifiers), "⌘W answers");
                    assert_eq!(pane_count(&state), 1, "⌘W closes a pane");
                }
                "workflow runs" => {
                    assert!(
                        chord(&mut root, &app, &keys[0], modifiers),
                        "⌘⇧W answers"
                    );
                    assert!(
                        state.borrow().task_workflow_open,
                        "⌘⇧W opens the workflow-runs overlay"
                    );
                }
                "previous / next pane" | "pane above / below" => {
                    // A neighbour in the direction the row names: "split right"
                    // lays the two panes side by side, "split down" stacks them,
                    // so each row's arrows have somewhere to go.
                    let split = if label == "previous / next pane" {
                        (
                            " ",
                            ModifiersState {
                                command: true,
                                ..Default::default()
                            },
                        )
                    } else {
                        (
                            "d",
                            ModifiersState {
                                command: true,
                                shift: true,
                                ..Default::default()
                            },
                        )
                    };
                    chord(&mut root, &app, split.0, split.1);
                    assert_eq!(pane_count(&state), 2, "the split landed");
                    for key in &keys {
                        let from = state.borrow().active_pane_id;
                        assert!(chord(&mut root, &app, key, modifiers), "{key} answers");
                        assert_ne!(
                            state.borrow().active_pane_id,
                            from,
                            "{label}: {key} moves the focus off pane {from}"
                        );
                    }
                }
                "next / previous space" => {
                    // The chord walks the tab strip, so there has to be a second
                    // tab: a second space, with the first one on screen.
                    {
                        let mut s = state.borrow_mut();
                        let id = 9;
                        s.spaces.push(crate::ui::Space::unnamed(Pane::Leaf {
                            id,
                            kind: crate::ui::PaneKind::Terminal,
                        }));
                        s.active_space = 0;
                        s.active_pane_id = s.spaces[0].root.first_leaf_id();
                    }
                    assert_eq!(keys, vec!["Tab"], "the row's key is Tab");
                    assert!(
                        chord(&mut root, &app, &keys[0], modifiers),
                        "Ctrl+Tab answers"
                    );
                    assert_eq!(state.borrow().active_space, 1, "Ctrl+Tab moves a tab on");
                    let backwards = ModifiersState {
                        shift: true,
                        ..modifiers
                    };
                    assert!(
                        chord(&mut root, &app, &keys[0], backwards),
                        "Ctrl+Shift+Tab answers"
                    );
                    assert_eq!(
                        state.borrow().active_space,
                        0,
                        "Ctrl+Shift+Tab moves a tab back"
                    );
                    assert!(
                        chord(&mut root, &app, &keys[0], backwards),
                        "Ctrl+Shift+Tab answers at the first tab too"
                    );
                    assert_eq!(
                        state.borrow().active_space,
                        1,
                        "Ctrl+Shift+Tab wraps to the last tab"
                    );
                }
                other => panic!("the panel names {other:?}: prove it or drop the row"),
            }
        }
    }

    /// The panel's own table: every row, in the order it is drawn. The input and
    /// palette rows name keys their own surfaces answer (the composer and the
    /// palette), so they are pinned here by the table itself — a row added,
    /// removed or reworded has to be deliberate.
    #[test]
    fn the_panel_s_rows_are_the_chords_it_draws() {
        let titles: Vec<&str> = sections().into_iter().map(|(title, _)| title).collect();
        assert_eq!(titles, vec!["Workspace", "Input", "Palette"]);
        let rows: Vec<(String, String)> = sections()
            .into_iter()
            .flat_map(|(_, hints)| {
                hints
                    .into_iter()
                    .map(|hint| (hint.keys().join(" "), hint.label().to_string()))
            })
            .collect();
        assert_eq!(
            rows,
            vec![
                ("⌘ K".to_string(), "command palette".to_string()),
                ("⌘ ⇧ T".to_string(), "new terminal".to_string()),
                ("⌘ Space".to_string(), "split right".to_string()),
                ("⌘ ⇧ D".to_string(), "split down".to_string()),
                ("⌘ W".to_string(), "close pane".to_string()),
                ("⌘ ⇧ W".to_string(), "workflow runs".to_string()),
                ("⌘ ← →".to_string(), "previous / next pane".to_string()),
                ("⌘ ↑ ↓".to_string(), "pane above / below".to_string()),
                ("⌃ ⇥".to_string(), "next / previous space".to_string()),
                (
                    "↵".to_string(),
                    "send (queues while a turn runs)".to_string()
                ),
                ("⌘ ↵".to_string(), "new conversation".to_string()),
                ("!".to_string(), "shell command".to_string()),
                ("↑ ↓".to_string(), "move the selection".to_string()),
                ("↵".to_string(), "run the selected command".to_string()),
                ("Esc".to_string(), "close the palette".to_string()),
            ]
        );
    }

    /// Open the panel on a fresh workspace, the way `Ctrl+.` does.
    fn open_panel() -> (Box<dyn Element>, Rc<RefCell<UiState>>, tempfile::TempDir, AppContext) {
        let app = AppContext::default();
        let (mut root, state, dir) = workspace();
        assert!(toggle(&mut root, &app), "Ctrl+. is consumed");
        frame(&mut root, &app);
        (root, state, dir, app)
    }

    /// Type `text` into the panel's filter: one key press and one frame per
    /// character, the way the window delivers them.
    fn type_text(root: &mut Box<dyn Element>, app: &AppContext, text: &str) {
        for ch in text.chars() {
            assert!(
                chord(root, app, &ch.to_string(), ModifiersState::none()),
                "the panel takes {ch:?} as filter text"
            );
            frame(root, app);
        }
    }

    /// Empty the panel's filter with its own chord.
    fn clear_filter(root: &mut Box<dyn Element>, app: &AppContext) {
        assert!(
            chord(
                root,
                app,
                "/",
                ModifiersState {
                    ctrl: true,
                    ..Default::default()
                }
            ),
            "Ctrl+/ is the filter's own chord"
        );
        frame(root, app);
    }

    /// The bands the panel paints for its highlighted row, as `(top, bottom)`
    /// pairs. The panel is the only surface in the frame that draws a
    /// `Selected` band inside its own column.
    fn highlight_bands(commands: &[RenderCommand], app: &AppContext) -> Vec<(f32, f32)> {
        let selected = app.theme.color(ColorToken::Selected);
        let panel_left = (1024.0 - SHORTCUTS_PANEL_WIDTH) * 0.5;
        commands
            .iter()
            .filter_map(|command| match command {
                RenderCommand::FillRect { color, rect, .. }
                    if *color == selected
                        && rect.min_x() >= panel_left
                        && rect.max_x() <= panel_left + SHORTCUTS_PANEL_WIDTH =>
                {
                    Some((rect.min_y(), rect.max_y()))
                }
                _ => None,
            })
            .collect()
    }

    /// The y the panel paints a row's label at.
    fn label_y(commands: &[RenderCommand], label: &str) -> f32 {
        commands
            .iter()
            .find_map(|command| match command {
                RenderCommand::DrawText { text, origin, .. } if text == label => Some(origin.y),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the panel draws {label:?}"))
    }

    /// The one row the panel highlights, as its band's `(top, bottom)`.
    fn the_highlighted_band(commands: &[RenderCommand], app: &AppContext) -> (f32, f32) {
        let bands = highlight_bands(commands, app);
        assert_eq!(bands.len(), 1, "one row is highlighted: {bands:?}");
        bands[0]
    }

    /// Typing narrows the drawn rows: the labels the filter leaves are painted
    /// and the rest are not.
    #[test]
    fn typing_filters_the_rows_the_panel_draws() {
        let (mut root, state, _dir, app) = open_panel();
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "command palette"));
        assert!(drawn(&commands, "split right"));
        assert!(drawn(&commands, "send (queues while a turn runs)"));

        type_text(&mut root, &app, "split");
        let commands = frame(&mut root, &app);
        assert_eq!(state.borrow().shortcuts_help_filter, "split");
        assert!(drawn(&commands, "split right"));
        assert!(drawn(&commands, "split down"));
        assert!(!drawn(&commands, "command palette"), "the filter narrows");
        assert!(!drawn(&commands, "send (queues while a turn runs)"));
    }

    /// The filter matches by name, by the cap as it is drawn, and by the words
    /// of a cap the fonts cannot spell: `cmd k` finds the `⌘ K` row.
    #[test]
    fn the_filter_matches_a_rows_name_its_caps_and_their_words() {
        let (mut root, state, _dir, app) = open_panel();

        // The letter of a chord, as the panel writes it.
        type_text(&mut root, &app, "k");
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "command palette"), "K is the palette's cap");
        assert!(!drawn(&commands, "split right"), "⌘Space is not ⌘K");

        // A cap that is drawn as an icon, typed as the word it stands for. The
        // two terms have to land in the same row.
        clear_filter(&mut root, &app);
        type_text(&mut root, &app, "cmd k");
        let commands = frame(&mut root, &app);
        assert_eq!(state.borrow().shortcuts_help_filter, "cmd k");
        assert!(drawn(&commands, "command palette"));
        assert!(!drawn(&commands, "new terminal"), "⌘⇧T is not ⌘K");

        // The chord joined, the way the row writes it.
        clear_filter(&mut root, &app);
        type_text(&mut root, &app, "⌘⇧t");
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "new terminal"));
        assert!(!drawn(&commands, "command palette"));

        // And the name, whatever its case.
        clear_filter(&mut root, &app);
        type_text(&mut root, &app, "SPLIT");
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "split right"));
        assert!(drawn(&commands, "split down"));
        assert!(!drawn(&commands, "close pane"));
    }

    /// A heading leaves with its rows, and a filter nothing matches says so
    /// instead of drawing an empty box.
    #[test]
    fn a_section_with_no_matches_is_dropped_and_the_empty_state_speaks() {
        let app = AppContext::default();
        let (mut root, _state, _dir) = workspace();
        // What the workspace under the panel draws on its own: a few of the
        // panel's words are affordances out there too ("new conversation" is
        // the sidebar's), and those are not the panel's to answer for.
        let workspace_texts: Vec<String> = frame(&mut root, &app)
            .iter()
            .filter_map(|command| match command {
                RenderCommand::DrawText { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(toggle(&mut root, &app));
        frame(&mut root, &app);

        // `Esc` is a cap of one row, under Palette: two headings go with it.
        type_text(&mut root, &app, "esc");
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "close the palette"));
        assert!(drawn(&commands, "Palette"));
        assert!(!drawn(&commands, "Workspace"), "a heading leaves with its rows");
        assert!(!drawn(&commands, "Input"));

        // Nothing matches: the panel is the palette's own empty state now.
        type_text(&mut root, &app, "zzz");
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "No shortcuts match"));
        for (title, hints) in sections() {
            let rows = hints.iter().map(|hint| hint.label());
            for text in std::iter::once(title).chain(rows) {
                if workspace_texts.iter().any(|drawn| drawn == text) {
                    continue;
                }
                assert!(
                    !drawn(&commands, text),
                    "nothing of the panel survives the empty state: {text:?}"
                );
            }
        }
    }

    /// Escape steps back through the filter: it empties a filter that has text
    /// and only closes the panel once the filter is already empty.
    #[test]
    fn escape_clears_the_filter_before_it_closes_the_panel() {
        let (mut root, state, _dir, app) = open_panel();
        type_text(&mut root, &app, "split");
        frame(&mut root, &app);

        assert!(
            chord(&mut root, &app, "Escape", ModifiersState::none()),
            "Escape is consumed"
        );
        assert!(state.borrow().shortcuts_help_open, "the first Escape only clears");
        assert!(state.borrow().shortcuts_help_filter.is_empty());
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "command palette"), "the whole table is back");
        assert!(
            drawn(&commands, "Filter shortcuts"),
            "the field reads as a placeholder again"
        );

        assert!(chord(&mut root, &app, "Escape", ModifiersState::none()));
        assert!(!state.borrow().shortcuts_help_open, "the second Escape closes");
    }

    /// The panel's own chord still toggles it, filter or no filter, and opening
    /// it again starts from the whole table.
    #[test]
    fn ctrl_period_still_opens_and_closes_the_panel() {
        let (mut root, state, _dir, app) = open_panel();
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "Keyboard shortcuts"));

        type_text(&mut root, &app, "split");
        assert!(
            toggle(&mut root, &app),
            "Ctrl+. is the panel's own chord, not the filter's"
        );
        assert!(!state.borrow().shortcuts_help_open, "Ctrl+. closes the panel");
        let commands = frame(&mut root, &app);
        assert!(!drawn(&commands, "Keyboard shortcuts"));

        assert!(toggle(&mut root, &app));
        assert!(state.borrow().shortcuts_help_open, "Ctrl+. opens it again");
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "Keyboard shortcuts"));
        assert!(state.borrow().shortcuts_help_filter.is_empty(), "reopened clean");
        assert_eq!(state.borrow().shortcuts_help_index, 0);
        assert!(drawn(&commands, "command palette"));
        assert!(drawn(&commands, "Filter shortcuts"));
    }

    /// Backspace edits the filter from its end.
    #[test]
    fn backspace_edits_the_filter() {
        let (mut root, state, _dir, app) = open_panel();
        type_text(&mut root, &app, "cmd k");
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "command palette"));
        assert!(!drawn(&commands, "new terminal"), "⌘⇧T is not ⌘K");

        assert!(
            chord(&mut root, &app, "Backspace", ModifiersState::none()),
            "Backspace is the panel's"
        );
        frame(&mut root, &app);
        assert_eq!(state.borrow().shortcuts_help_filter, "cmd ");
        let commands = frame(&mut root, &app);
        assert!(drawn(&commands, "command palette"), "⌘K still matches cmd");
        assert!(drawn(&commands, "new terminal"), "so does every ⌘ row");
        assert!(
            !drawn(&commands, "send (queues while a turn runs)"),
            "the bare ↵ row does not"
        );
    }

    /// The highlight moves through the rows the filter leaves and never points
    /// past them — not when the filter narrows to fewer rows than its position,
    /// and not when the filter is cleared again.
    #[test]
    fn the_highlight_stays_inside_the_rows_the_filter_leaves() {
        let (mut root, state, _dir, app) = open_panel();

        // Walk to the end of the table: the list has to scroll to follow.
        for _ in 0..14 {
            assert!(chord(&mut root, &app, "ArrowDown", ModifiersState::none()));
        }
        let commands = frame(&mut root, &app);
        let last = visible_row_count("") - 1;
        assert_eq!(state.borrow().shortcuts_help_index, last, "the end of the table");
        assert!(
            state.borrow().shortcuts_help_scroll.borrow().offset() > 0.0,
            "the list scrolled to keep the highlighted row in view"
        );
        let (top, bottom) = the_highlighted_band(&commands, &app);
        let y = label_y(&commands, "close the palette");
        assert!(
            y >= top && y <= bottom,
            "the band is drawn on the highlighted row, not at {y} vs {top}..{bottom}"
        );

        // A filter that leaves fewer rows than the highlight's position.
        type_text(&mut root, &app, "split");
        let commands = frame(&mut root, &app);
        let visible = visible_row_count("split");
        assert_eq!(visible, 2, "only the two splits are left");
        assert!(
            state.borrow().shortcuts_help_index < visible,
            "the highlight followed the shorter list"
        );
        let (top, bottom) = the_highlighted_band(&commands, &app);
        let y = label_y(&commands, "split right");
        assert!(y >= top && y <= bottom, "and is drawn on the first of them");

        // Clearing it puts the highlight back on the list it belongs to.
        clear_filter(&mut root, &app);
        let commands = frame(&mut root, &app);
        let visible = visible_row_count("");
        assert_eq!(visible, 15, "the whole table is back");
        assert!(state.borrow().shortcuts_help_index < visible);
        let (top, bottom) = the_highlighted_band(&commands, &app);
        let y = label_y(&commands, "command palette");
        assert!(y >= top && y <= bottom, "still one band, on the first row");
    }
}
