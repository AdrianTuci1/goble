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

use goble_ui::elements::{
    AppContext, Axis, ConstrainedBox, Container, CrossAxisAlignment, Divider, EdgeInsets, Element,
    Fill, Flex, Icon, KeyHandler, MainAxisSize, Scrollable, ShortcutHint, Spacer, Text,
    TopbarButton,
};
use goble_ui::theme::{ColorToken, SpacingToken};

use super::UiActions;

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
                ShortcutHint::new(&["⌘", "⇧", "W"], "tasks & workflows"),
                ShortcutHint::new(&["⌘", "←", "→"], "previous / next pane"),
                ShortcutHint::new(&["⌘", "↑", "↓"], "pane above / below"),
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

/// The panel: a header (title, ✕) over a scrollable column of sections. It is
/// an overlay — the workspace stays mounted underneath — and its keys are
/// intercepted before any child sees them, so Escape closes it.
pub fn build_shortcuts_help(app: &AppContext, actions: &UiActions) -> Box<dyn Element> {
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

    let mut body = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(sm);
    for (title, hints) in sections() {
        body = body.with_child(heading(app, title));
        for hint in hints {
            body = body.with_child(row(app, hint));
        }
    }

    let mut column = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(spacing);
    column = column.with_child(header);
    column = column.with_child(Divider::horizontal().finish());
    column = column.with_child(Scrollable::new(body.finish(), Axis::Vertical).finish());

    let on_escape = actions.on_close_shortcuts_help.clone();
    KeyHandler::new(
        Container::new(column.finish())
            .with_background(Fill::Solid(app.theme.color(ColorToken::Surface)))
            .with_padding(EdgeInsets::uniform(spacing))
            .finish(),
        move |key: &str, modifiers| {
            if modifiers.ctrl || modifiers.command || modifiers.alt {
                return false;
            }
            if key == "Escape" {
                (on_escape.borrow_mut())();
                return true;
            }
            false
        },
    )
    .finish()
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
            s.settings_overlay_open = false;
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
        // instead of replacing it.
        assert!(drawn(&commands, "Space 1"), "the workspace stays mounted");
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
                "tasks & workflows" => {
                    assert!(
                        chord(&mut root, &app, &keys[0], modifiers),
                        "⌘⇧W answers"
                    );
                    assert!(
                        state.borrow().task_workflow_open,
                        "⌘⇧W opens the tasks & workflows overlay"
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
                ("⌘ ⇧ W".to_string(), "tasks & workflows".to_string()),
                ("⌘ ← →".to_string(), "previous / next pane".to_string()),
                ("⌘ ↑ ↓".to_string(), "pane above / below".to_string()),
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
}
