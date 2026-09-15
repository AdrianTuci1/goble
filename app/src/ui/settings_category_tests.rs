use std::cell::RefCell;
use std::rc::Rc;

use goble_ui::elements::{
    AppContext, Element, EventContext, LayoutContext, PaintContext, SizeConstraint,
};
use goble_ui::event::{DispatchedEvent, ModifiersState};
use goble_ui::geometry::{vec2f, RectF, Vector2F};
use goble_ui::render::RenderCommand;

use super::*;

/// The seven grok-build pages keep their labels and their order: they are still
/// exactly what the rail names once the page this app adds is taken out of it.
#[test]
fn categories_match_grok_build_labels() {
    let labels: Vec<&str> = SettingsCategory::ALL
        .iter()
        .filter(|c| **c != SettingsCategory::Connections)
        .map(|c| c.label())
        .collect();
    assert_eq!(
        labels,
        vec![
            "Appearance",
            "Mouse",
            "Editor & Input",
            "Agent & Approval",
            "Environment",
            "Models",
            "Advanced",
        ]
    );
}

/// Connections is the page between them: it sits directly after Environment,
/// where the machine's own SSH state belongs, and nothing before it moved.
#[test]
fn connections_sits_after_environment() {
    let labels: Vec<&str> = SettingsCategory::ALL.iter().map(|c| c.label()).collect();
    let at = labels
        .iter()
        .position(|label| *label == "Connections")
        .expect("the Connections page is in the rail");
    assert_eq!(labels[at - 1], "Environment");
    assert_eq!(
        labels[..at],
        ["Appearance", "Mouse", "Editor & Input", "Agent & Approval", "Environment"]
    );
    assert_eq!(&labels[at..], ["Connections", "Models", "Advanced"]);
}

/// A whole app tree with the settings tab open, plus its live state.
fn settings_root(
    category: SettingsCategory,
) -> (
    Box<dyn Element>,
    Rc<RefCell<crate::state::UiState>>,
    tempfile::TempDir,
) {
    let (root, state, dir, _desktop) = settings_root_with_desktop(category);
    (root, state, dir)
}

/// The same tree, with the service handle kept: the tests that assert what was
/// persisted (a switch in the store, a group of secrets in the store) need to
/// read the store back rather than the pane's own view of it.
///
/// The tab is opened through `open_settings_tab` — the app's own entry point —
/// so the tests drive the real path, and then pointed at the page they are
/// about. The sidebar is hidden, so the settings pane is the whole main column
/// and its geometry is the window's.
fn settings_root_with_desktop(
    category: SettingsCategory,
) -> (
    Box<dyn Element>,
    Rc<RefCell<crate::state::UiState>>,
    tempfile::TempDir,
    std::sync::Arc<goble_desktop_service::DesktopState>,
) {
    use crate::root_view::RootView;
    use goble_core::store::Store;
    use goble_desktop_service::{DesktopState, ThreadStore};

    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    );
    // The Environment page's groups live in `~/.goble/environment.toml`, so the
    // page reads and writes a file: point it inside the test's own directory
    // rather than at the user's real home.
    desktop.set_environment_path(dir.path().join("environment.toml"));
    let root = RootView::new(&AppContext::default(), &desktop, None);
    let state = root.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.right_sidebar_open = false;
        s.crons_open = false;
        s.sidebar_visible = false;
        s.open_settings_tab(Some(&desktop));
        s.settings_set_page(category);
    }
    (Box::new(root) as Box<dyn Element>, state, dir, desktop)
}

fn key(name: &str) -> DispatchedEvent {
    DispatchedEvent::KeyDown {
        key: name.to_string(),
        modifiers: ModifiersState::none(),
    }
}

/// The same key with modifiers held, for the window-level chords.
fn chord(name: &str, modifiers: ModifiersState) -> DispatchedEvent {
    DispatchedEvent::KeyDown {
        key: name.to_string(),
        modifiers,
    }
}

/// Dispatch one key through the whole app tree, after a frame, so a handler
/// that captured state when the tree was built sees the last key's effect (and
/// the pane knows its own origin, which it learns while painting). The running
/// app rebuilds and repaints between events the same way.
fn press(root: &mut Box<dyn Element>, app: &AppContext, name: &str) -> bool {
    press_event(root, app, &key(name))
}

/// The same round trip for a key carrying modifiers.
fn press_event(
    root: &mut Box<dyn Element>,
    app: &AppContext,
    event: &DispatchedEvent,
) -> bool {
    let constraint = SizeConstraint::loose(vec2f(1024.0, 768.0));
    let _ = root.layout(constraint, &mut LayoutContext::default(), app);
    let mut paint_ctx = PaintContext::new(goble_ui::render::Renderer::new());
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    let mut ctx = EventContext::default();
    root.dispatch_event(event, &mut ctx, app)
}

#[test]
fn the_arrow_keys_move_the_page_and_escape_leaves_the_tab_open() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Appearance);

    assert!(press(&mut root, &app, "ArrowDown"));
    assert_eq!(state.borrow().settings_page(), SettingsCategory::Mouse);
    press(&mut root, &app, "ArrowDown");
    assert_eq!(state.borrow().settings_page(), SettingsCategory::EditorInput);
    press(&mut root, &app, "ArrowUp");
    assert_eq!(state.borrow().settings_page(), SettingsCategory::Mouse);

    // The page is not a copy that can drift: the rail moved the pane's own
    // leaf, so the page travels with the pane.
    {
        let s = state.borrow();
        let (space, id) = s.settings_pane().expect("the tab is open");
        assert_eq!(
            s.spaces[space].leaf_kind(id),
            Some(&PaneKind::Settings {
                page: SettingsCategory::Mouse
            })
        );
    }

    // Stepping up from the first page wraps to the last and back.
    state.borrow_mut().settings_set_page(SettingsCategory::Appearance);
    press(&mut root, &app, "ArrowUp");
    assert_eq!(state.borrow().settings_page(), SettingsCategory::Advanced);
    press(&mut root, &app, "ArrowDown");
    assert_eq!(state.borrow().settings_page(), SettingsCategory::Appearance);

    // `Esc` from the rail is not the tab's key: it falls through the tree and
    // the tab is left exactly as it was.
    let before = state.borrow().spaces.len();
    assert!(
        !press(&mut root, &app, "Escape"),
        "the rail lets Esc through"
    );
    let s = state.borrow();
    assert_eq!(s.spaces.len(), before, "the tab is still open");
    assert!(s.settings_pane().is_some());
    assert_eq!(s.current_tab, AppTab::Chat);
}

/// A modified arrow is a pane-navigation shortcut (Ctrl/Cmd+Arrow), not a
/// settings navigation key, so the page must let it through.
#[test]
fn a_modified_arrow_does_not_move_the_page() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);
    let mut ctx = EventContext::default();
    root.dispatch_event(
        &DispatchedEvent::KeyDown {
            key: "ArrowDown".to_string(),
            modifiers: ModifiersState {
                command: true,
                ..Default::default()
            },
        },
        &mut ctx,
        &app,
    );
    assert_eq!(
        state.borrow().settings_page(),
        SettingsCategory::Mouse,
        "Cmd+Arrow belongs to pane navigation"
    );
}

/// The app's own actions over the same live state the tree reads, so a test can
/// press a control the way the app does (`on_settings`, a tab's ✕) as well as
/// dispatch a key.
fn actions_for(
    state: &Rc<RefCell<crate::state::UiState>>,
    desktop: &std::sync::Arc<goble_desktop_service::DesktopState>,
) -> crate::ui::UiActions {
    use crate::media::MediaState;
    use goble_ui::platform::WindowControl;
    crate::actions::make_actions(
        Rc::clone(state),
        Some(std::sync::Arc::clone(desktop)),
        Rc::new(RefCell::new(MediaState::mock())),
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    )
}

// ---- The settings tab: one space whose root is a settings leaf -------------

/// A layout whose space holds a settings leaf draws the rail's page labels and
/// the rows of the page, inside the pane the tab is.
#[test]
fn a_settings_leaf_draws_the_rail_and_the_page() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, _state, _dir) = settings_root(SettingsCategory::Mouse);
    let commands = frame(&mut root, &app, window);

    for page in SettingsCategory::ALL {
        assert!(
            pane_text(&commands, page.label()).is_some(),
            "the rail draws {:?}",
            page.label()
        );
    }
    assert!(
        pane_text(&commands, "Invert scroll").is_some(),
        "the page draws its own rows"
    );
    assert!(pane_text(&commands, "Scroll speed").is_some());
    assert!(
        !pane_text(&commands, "Font size").is_some(),
        "and only the selected page's"
    );
    // The tab itself is an ordinary entry in the strip.
    assert!(drawn(&commands, "Settings"), "the tab is in the strip");
}

/// Pressing Settings twice never opens a second tab, and it does not even move
/// the keyboard: the second press lands on the settings leaf that is already
/// the active pane.
#[test]
fn pressing_settings_twice_keeps_one_tab() {
    let app = AppContext::default();
    let (mut root, state, _dir, desktop) = settings_root_with_desktop(SettingsCategory::Mouse);
    let actions = actions_for(&state, &desktop);
    let _ = frame(&mut root, &app, vec2f(1024.0, 768.0));
    let spaces = state.borrow().spaces.len();
    let (space, pane) = state.borrow().settings_pane().expect("the tab is open");

    (actions.on_settings.borrow_mut())();
    {
        let s = state.borrow();
        assert_eq!(s.spaces.len(), spaces, "no second tab");
        assert_eq!(s.active_space, space, "and the same space stays active");
        assert_eq!(s.active_pane_id, pane, "on the same pane");
        assert_eq!(
            s.settings_page(),
            SettingsCategory::Mouse,
            "on the page it was left on"
        );
    }

    // A third press changes nothing either.
    (actions.on_settings.borrow_mut())();
    let s = state.borrow();
    assert_eq!(s.spaces.len(), spaces);
    assert_eq!(s.active_space, space);
    assert_eq!(s.active_pane_id, pane);
}

/// With the settings tab in a background space, the press switches to that
/// space — and to the chat mode the strip is drawn in when the app was
/// elsewhere, so the tab it brings forward is one the user can see.
#[test]
fn a_settings_tab_in_a_background_space_is_brought_to_the_front() {
    let app = AppContext::default();
    let (mut root, state, _dir, desktop) = settings_root_with_desktop(SettingsCategory::Models);
    let actions = actions_for(&state, &desktop);
    let _ = frame(&mut root, &app, vec2f(1024.0, 768.0));
    let (space, pane) = state.borrow().settings_pane().expect("the tab is open");

    // Leave the tab: another space is active and the app is in another mode.
    {
        let mut s = state.borrow_mut();
        s.active_space = 0;
        s.active_pane_id = s.spaces[0].root.first_leaf_id();
        s.current_tab = AppTab::Projects;
        s.sync_active_view();
    }

    (actions.on_settings.borrow_mut())();
    let s = state.borrow();
    assert_eq!(s.current_tab, AppTab::Chat, "the strip is drawn in Chat");
    assert_eq!(s.active_space, space, "the space holding the tab is active");
    assert_eq!(s.active_pane_id, pane, "with the settings leaf focused");
    assert_eq!(
        s.settings_page(),
        SettingsCategory::Models,
        "on the page it was left on"
    );
    assert_eq!(s.settings_focus, SettingsFocus::Rail, "ready for the arrows");
}

/// Closing the tab takes its space with it, so the next press opens exactly one
/// new one.
#[test]
fn after_the_tab_is_closed_the_press_adds_exactly_one_space() {
    let app = AppContext::default();
    let (mut root, state, _dir, desktop) = settings_root_with_desktop(SettingsCategory::Mouse);
    let actions = actions_for(&state, &desktop);
    let _ = frame(&mut root, &app, vec2f(1024.0, 768.0));
    let (space, _) = state.borrow().settings_pane().expect("the tab is open");

    (actions.on_close_space.borrow_mut())(space);
    assert!(
        state.borrow().settings_pane().is_none(),
        "closing the tab closes the space"
    );

    let before = state.borrow().spaces.len();
    (actions.on_settings.borrow_mut())();
    let s = state.borrow();
    assert_eq!(s.spaces.len(), before + 1, "one new tab, not two");
    let (space, pane) = s.settings_pane().expect("the settings tab is back");
    assert_eq!(s.active_space, space);
    assert_eq!(s.active_pane_id, pane);
}

/// A layout that somehow holds two settings leaves focuses the first and
/// normalises nothing: both panes draw, each on its own page, and the second is
/// left exactly where the user put it.
#[test]
fn a_layout_with_two_settings_leaves_focuses_the_first_and_normalises_nothing() {
    let app = AppContext::default();
    let (mut root, state, _dir, desktop) = settings_root_with_desktop(SettingsCategory::Mouse);
    let actions = actions_for(&state, &desktop);

    // A hand-built layout: the settings space's root becomes a split of two
    // settings leaves, the first on Appearance and the second still active.
    let (space, first) = state.borrow().settings_pane().expect("the tab is open");
    let second = {
        let mut s = state.borrow_mut();
        let second = s.next_pane_id;
        let split = second + 1;
        s.next_pane_id += 2;
        s.spaces[space].root = Pane::Split {
            id: split,
            dir: SplitDir::Horizontal,
            ratio: 0.5,
            first: Box::new(Pane::Leaf {
                id: first,
                kind: PaneKind::Settings {
                    page: SettingsCategory::Appearance,
                },
            }),
            second: Box::new(Pane::Leaf {
                id: second,
                kind: PaneKind::Settings {
                    page: SettingsCategory::Mouse,
                },
            }),
        };
        s.active_pane_id = second;
        second
    };

    (actions.on_settings.borrow_mut())();
    {
        let s = state.borrow();
        assert_eq!(s.settings_pane(), Some((space, first)), "the first leaf wins");
        assert_eq!(s.active_pane_id, first, "and it takes the keyboard");
        assert_eq!(
            s.settings_page(),
            SettingsCategory::Appearance,
            "the page is the first leaf's"
        );
    }

    // Nothing was normalised: the split still holds both leaves.
    {
        let s = state.borrow();
        match &s.spaces[space].root {
            Pane::Split { first: a, second: b, .. } => {
                assert!(matches!(
                    **a,
                    Pane::Leaf { kind: PaneKind::Settings { page: SettingsCategory::Appearance }, .. }
                ));
                assert!(matches!(
                    **b,
                    Pane::Leaf {
                        id,
                        kind: PaneKind::Settings { page: SettingsCategory::Mouse }
                    } if id == second
                ));
            }
            other => panic!("the hand-built layout was rewritten: {other:?}"),
        }
    }

    // And both panes draw, each on the page its own leaf carries.
    let commands = frame(&mut root, &app, vec2f(1024.0, 768.0));
    let count = |text: &str| {
        commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::DrawText { text: run, .. } if run == text))
            .count()
    };
    assert_eq!(count("Dark mode"), 1, "the first pane draws Appearance");
    assert_eq!(count("Invert scroll"), 1, "the second draws Mouse");
    // Each pane carries its own "Settings" title; the strip still draws one tab
    // for the space, its label above the panes.
    let tabs = commands
        .iter()
        .filter(|c| {
            matches!(
                c,
                RenderCommand::DrawText { text, origin, .. }
                    if text == "Settings" && origin.y < crate::ui::shell::TOPBAR_HEIGHT
            )
        })
        .count();
    assert_eq!(tabs, 1, "and the strip still holds one tab");
}

/// The settings body fills its pane: the rail keeps its own fixed width hard
/// against the pane's left edge, the rule sits beside it, and the page scrolls
/// in the viewport the header band and the footer leave. Widths are
/// arithmetic: `NAV_WIDTH` rail + 1 rule + 2×12 padding + the rest of the pane
/// for the viewport; its height is the pane's minus the two fixed bands, which
/// are the pane's own furniture (the header's padding and title row, the
/// footer's hint line, each closed by a rule) and do not shrink when the window
/// does. Checked at the 1280×800 default, at 1024×768, at the 640×400 logical
/// viewport the largest zoom (2.0) leaves, and in a narrow 560×400 window.
#[test]
fn the_settings_body_fills_the_pane() {
    use crate::ui::settings::{NAV_WIDTH, RULE_WIDTH};
    use goble_ui::theme::SpacingToken;

    let app = AppContext::default();
    let md = app.theme.spacing_px(SpacingToken::Md);
    let (mut root, _state, _dir) = settings_root(SettingsCategory::Mouse);
    // The header and footer bands, remembered from the first window and held
    // fixed for the rest.
    let mut bands: Option<(f32, f32)> = None;

    for window in [
        vec2f(1280.0, 800.0),
        vec2f(1024.0, 768.0),
        vec2f(640.0, 400.0),
        vec2f(560.0, 400.0),
    ] {
        let commands = frame(&mut root, &app, window);
        // The pane's own surface: the band the settings body paints inside the
        // pane, told apart from the window's background by being shorter than
        // it (the sidebar is hidden, so the pane is the whole main column).
        let panel = pane_panel(&commands, &app, window);
        assert_eq!(panel.min_x(), 0.0, "the settings pane is the main column");
        assert_eq!(panel.width(), window.x, "and as wide as the window");
        assert_eq!(
            panel.max_y(),
            window.y,
            "and it reaches the window's bottom"
        );

        // The rail draws at its own fixed width, hard against the pane's left
        // edge.
        let nav = rail_fill(&commands, &app, panel).expect("the rail paints its own background");
        assert_eq!(nav.width(), NAV_WIDTH);
        assert_eq!(nav.min_x(), panel.min_x());

        // The page scrolls in a clipped viewport: rail + rule + the page's
        // padding + the viewport is the pane's width, and the viewport's band
        // sits below the header and above the footer.
        let clip = clip_in_panel(&commands, &app, window);
        assert_eq!(
            clip.min_x(),
            nav.max_x() + RULE_WIDTH + md,
            "the viewport starts after the rail, the rule and the page's padding"
        );
        assert_eq!(
            clip.width(),
            panel.width() - NAV_WIDTH - RULE_WIDTH - md * 2.0,
            "rail + rule + paddings + viewport is the pane's width"
        );
        // The viewport takes exactly the height the two fixed bands leave, and
        // those bands are the pane's own furniture (the header's padding and
        // title row, the footer's hint line, each closed by a rule): they do
        // not shrink when the pane does.
        let header_band = clip.min_y() - panel.min_y();
        let footer_band = panel.max_y() - clip.max_y();
        assert!(
            header_band > 0.0 && footer_band > 0.0 && header_band + footer_band < panel.height(),
            "the header and the footer leave the viewport a positive height: \
             {header_band} + {footer_band} in {}",
            panel.height()
        );
        match bands {
            None => bands = Some((header_band, footer_band)),
            Some(seen) => assert_eq!(
                (header_band, footer_band),
                seen,
                "the header and footer bands are fixed while the pane resizes"
            ),
        }
        assert_eq!(
            clip.height(),
            panel.height() - header_band - footer_band,
            "the viewport takes the height the header and the footer leave"
        );
        // Both fixed bands are drawn: the pane's own header above the viewport
        // and the footer's hints below it.
        let header = pane_text(&commands, "Settings").expect("the pane draws its title");
        assert!(
            header.y < clip.min_y(),
            "the header is above the viewport: {header:?}"
        );
        let footer = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText { origin, .. } if origin.y > clip.max_y() => Some(*origin),
                _ => None,
            })
            .count();
        assert!(footer > 0, "the footer is drawn below the viewport");
    }
}

/// The settings tab's own keys answer while it is up, and the window-level
/// chords are the window's: `⌘K` and `⌘⇧W` still answer, a plain `Enter`
/// reaches the page and nothing under it, and the tab stays open throughout.
#[test]
fn the_window_chords_still_answer_while_the_settings_tab_is_open() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);
    let spaces = state.borrow().spaces.len();

    // Enter is the page's: it moves the keyboard into the pane rather than
    // reaching a composer underneath.
    let before = state.borrow().settings_pane_focus;
    assert!(press(&mut root, &app, "Enter"), "the page consumes Enter");
    assert_eq!(state.borrow().settings_focus, SettingsFocus::Pane);
    assert_eq!(state.borrow().settings_pane_focus, before);
    assert!(!state.borrow().command_palette_open);

    let command = ModifiersState {
        command: true,
        ..Default::default()
    };
    assert!(press_event(&mut root, &app, &chord("k", command)));
    assert!(
        state.borrow().command_palette_open,
        "⌘K opens the palette over the settings tab"
    );
    press_event(&mut root, &app, &chord("k", command));

    let shift_command = ModifiersState {
        command: true,
        shift: true,
        ..Default::default()
    };
    assert!(press_event(&mut root, &app, &chord("w", shift_command)));
    assert!(
        state.borrow().task_workflow_open,
        "⌘⇧W opens the tasks & workflows overlay while the settings tab is up"
    );
    let s = state.borrow();
    assert_eq!(s.spaces.len(), spaces, "and the settings tab stays open");
    assert!(s.settings_pane().is_some());
}

/// The settings pane, told apart from the window's own background: the `Bg`
/// band the settings body paints inside it, shorter than the window.
fn pane_panel(commands: &[RenderCommand], app: &AppContext, window: Vector2F) -> RectF {
    bg_fills(commands, app)
        .into_iter()
        .find(|rect| rect.min_x() == 0.0 && rect.height() < window.y - 0.5)
        .unwrap_or_else(|| {
            panic!(
                "the settings body paints its pane in {window:?}: {:?}",
                bg_fills(commands, app)
            )
        })
}

/// The rail's own background inside the pane: the `Surface` band at the pane's
/// own fixed width, hard against the pane's left edge. The pane paints
/// `Surface` behind everything as well, so the width is what tells them apart.
fn rail_fill(commands: &[RenderCommand], app: &AppContext, panel: RectF) -> Option<RectF> {
    use crate::ui::settings::NAV_WIDTH;
    let surface = app.theme.color(goble_ui::theme::ColorToken::Surface);
    commands.iter().find_map(|c| match c {
        RenderCommand::FillRect { rect, color, .. }
            if *color == surface
                && rect.min_x() == panel.min_x()
                && (rect.width() - NAV_WIDTH).abs() < 0.01
                && rect.min_y() >= panel.min_y()
                && rect.max_y() <= panel.max_y() =>
        {
            Some(*rect)
        }
        _ => None,
    })
}

/// Lay out and paint one frame, returning its render commands. A frame has to
/// happen before a dispatch: the pane learns its own origin while painting.
fn frame(root: &mut Box<dyn Element>, app: &AppContext, window: Vector2F) -> Vec<RenderCommand> {
    let constraint = SizeConstraint::loose(window);
    let _ = root.layout(constraint, &mut LayoutContext::default(), app);
    let mut paint_ctx = PaintContext::new(goble_ui::render::Renderer::new());
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    paint_ctx
        .renderer
        .map(|r| r.commands().to_vec())
        .unwrap_or_default()
}

/// Appearance draws exactly one color wheel — one hue ring and one
/// saturation/value square — and the square reports into the channel the
/// swatch column selected, not into all three at once.
#[test]
fn the_appearance_pane_has_one_wheel_wired_to_the_selected_channel() {
    use crate::ui::color_picker::SV_ROWS;

    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir) = settings_root(SettingsCategory::Appearance);
    state.borrow_mut().theme_color_target = crate::ui::color_picker::ColorTarget::Accent;
    let commands = frame(&mut root, &app, window);

    let rings: Vec<_> = commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawImage { rect, source, .. } if source.contains("ring") => Some(*rect),
            _ => None,
        })
        .collect();
    assert_eq!(rings.len(), 1, "one wheel, not one per theme channel");
    let rows = commands
        .iter()
        .filter(|c| matches!(c, RenderCommand::FillRectFadeRight { .. }))
        .count();
    assert_eq!(rows, SV_ROWS, "one saturation/value square");

    // Click the bright, saturated corner of the square: it must land on the
    // selected channel (Accent) and leave the others untouched.
    let ring = rings[0];
    let click = vec2f(
        ring.min_x() + ring.width() * 0.5 + 40.0,
        ring.min_y() + ring.height() * 0.5 - 40.0,
    );
    let mut ctx = EventContext::default();
    assert!(
        root.dispatch_event(
            &DispatchedEvent::MouseDown {
                position: click,
                button: 0
            },
            &mut ctx,
            &app
        ),
        "a click inside the square is handled"
    );
    let state = state.borrow();
    let accent = state.theme_accent.clone().expect("Accent got the new color");
    assert_ne!(accent, "#000000", "the picked color is a real color");
    assert!(state.theme_primary.is_none(), "Primary was not touched");
    assert!(state.theme_secondary.is_none(), "Secondary was not touched");
}

/// The swatch column retargets the one wheel: clicking a channel selects it,
/// and the wheel then edits that channel's stored color.
#[test]
fn clicking_a_swatch_retargets_the_wheel() {
    use crate::ui::color_picker::ColorTarget;

    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir) = settings_root(SettingsCategory::Appearance);
    let commands = frame(&mut root, &app, window);
    assert_eq!(
        state.borrow().theme_color_target,
        ColorTarget::Primary,
        "Primary is the default target"
    );

    // The swatch rows are the only place the channel labels are drawn in the
    // wheel block; take the "Accent" one and click on it.
    let accent = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. } if text == "Accent" => Some(*origin),
            _ => None,
        })
        .expect("the Accent swatch row is drawn");
    let mut ctx = EventContext::default();
    // Buttons fire on release, so the click is a down/up pair on the row.
    let at = accent + vec2f(2.0, 2.0);
    assert!(root.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: at,
            button: 0
        },
        &mut ctx,
        &app
    ));
    assert!(
        root.dispatch_event(
            &DispatchedEvent::MouseUp {
                position: at,
                button: 0
            },
            &mut ctx,
            &app
        ),
        "a click on the swatch row is handled"
    );
    assert_eq!(state.borrow().theme_color_target, ColorTarget::Accent);
}


// ---- The settings tab's two-region keyboard model --------------------------

/// The rail's arrows still move the page; `Right` moves the keyboard into
/// the pane, where the arrows move between the controls the pane drew, and
/// `Left` walks back out.
#[test]
fn the_arrows_move_the_region_that_holds_the_keyboard() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);

    // The rail, as before: Down/Up step the page.
    assert!(press(&mut root, &app, "ArrowDown"));
    assert_eq!(state.borrow().settings_page(), SettingsCategory::EditorInput);
    state.borrow_mut().settings_select_category(SettingsCategory::Mouse);

    // Right moves into the pane, onto its first control.
    assert!(press(&mut root, &app, "ArrowRight"));
    {
        let s = state.borrow();
        assert_eq!(s.settings_focus, SettingsFocus::Pane);
        assert_eq!(s.settings_pane_focus, 0);
        assert_eq!(
            s.settings_focused_control(),
            Some(SettingsControl::InvertScroll)
        );
    }

    // Down steps the pane's own order and leaves the page alone.
    press(&mut root, &app, "ArrowDown");
    {
        let s = state.borrow();
        assert_eq!(s.settings_focused_control(), Some(SettingsControl::ScrollSpeed));
        assert_eq!(
            s.settings_page(),
            SettingsCategory::Mouse,
            "the pane's arrows do not move the page"
        );
    }

    // Down at the last control stops there rather than wrapping.
    press(&mut root, &app, "ArrowDown");
    assert_eq!(state.borrow().settings_pane_focus, 1);
    press(&mut root, &app, "ArrowUp");
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::InvertScroll)
    );

    // The switch holds no discrete value, so Left is "back to the rail".
    press(&mut root, &app, "ArrowLeft");
    assert_eq!(state.borrow().settings_focus, SettingsFocus::Rail);
    assert!(state.borrow().settings_pane().is_some(), "the tab stayed open");

    // Tab goes back in, and a page change resets the pane's focus.
    press(&mut root, &app, "Tab");
    press(&mut root, &app, "ArrowDown");
    assert_eq!(state.borrow().settings_pane_focus, 1);
    state.borrow_mut().settings_select_category(SettingsCategory::EditorInput);
    assert_eq!(
        state.borrow().settings_pane_focus,
        0,
        "a new page starts the pane's focus at its first control"
    );
}

/// `Enter` on a focused switch flips it, and the flip is persisted: the
/// assertion reads the setting back out of the store, not out of the pane.
#[test]
fn enter_flips_a_switch_and_the_setting_is_persisted() {
    let app = AppContext::default();
    let (mut root, state, _dir, desktop) =
        settings_root_with_desktop(SettingsCategory::AgentApproval);
    assert!(!desktop.get_vim_mode(), "vim mode starts off");

    press(&mut root, &app, "ArrowRight"); // into the pane: Auto-approve
    press(&mut root, &app, "ArrowDown"); // Vim mode
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::VimMode)
    );
    assert!(press(&mut root, &app, "Enter"));
    assert!(state.borrow().vim_mode, "the switch flipped");
    assert!(desktop.get_vim_mode(), "and the flip was persisted");

    // A second Enter flips it back, and that is persisted too.
    press(&mut root, &app, "Enter");
    assert!(!state.borrow().vim_mode);
    assert!(!desktop.get_vim_mode());
}

/// `Left`/`Right` step a control that holds a discrete value (the stepper),
/// and a control that does not is left alone by `Right`.
#[test]
fn left_and_right_adjust_the_focused_stepper() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);
    press(&mut root, &app, "ArrowRight"); // the switch
    press(&mut root, &app, "ArrowDown"); // the stepper

    let before = state.borrow().settings_scroll_speed;
    assert!(press(&mut root, &app, "ArrowRight"));
    assert_eq!(state.borrow().settings_scroll_speed, before + 1);
    assert!(press(&mut root, &app, "ArrowLeft"));
    assert_eq!(state.borrow().settings_scroll_speed, before);
    assert_eq!(
        state.borrow().settings_focus,
        SettingsFocus::Pane,
        "an adjustable control keeps the keyboard in the pane"
    );
    assert_eq!(state.borrow().settings_page(), SettingsCategory::Mouse);

    // The switch above it has no discrete value, so Right does nothing to it
    // (Left there returns to the rail, asserted in the test above).
    press(&mut root, &app, "ArrowUp");
    let clicked = state.borrow().settings_invert_scroll;
    press(&mut root, &app, "ArrowRight");
    assert_eq!(
        state.borrow().settings_invert_scroll,
        clicked,
        "Right does not toggle a switch"
    );
    assert_eq!(state.borrow().settings_focus, SettingsFocus::Pane);
}

/// `Enter` on a field puts the caret in it, a second `Enter` commits what was
/// typed, and `Esc` puts back what the field held — the edit half of the
/// contract. A field the user typed into keeps every printable key, `Space`
/// included.
#[test]
fn a_pane_text_field_takes_the_caret_and_escape_puts_the_value_back() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Environment);

    press(&mut root, &app, "ArrowRight");
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentGroupName)
    );
    assert!(!state.borrow().settings_pane_field_active, "no caret yet");

    // Enter puts the caret in the field, and typing reaches it — including
    // Space, which is the page's own key outside a field.
    press(&mut root, &app, "Enter");
    assert!(state.borrow().settings_pane_field_active);
    for c in ["p", "r", "o", "d"] {
        press(&mut root, &app, c);
    }
    press(&mut root, &app, " ");
    assert_eq!(state.borrow().settings_environment_group_draft, "prod ");

    // Escape cancels the edit: what the field held when the caret went in is
    // put back, the caret goes, and the tab stays open on the same row.
    press(&mut root, &app, "Escape");
    assert!(!state.borrow().settings_pane_field_active);
    assert!(
        state.borrow().settings_environment_group_draft.is_empty(),
        "a cancelled edit leaves nothing behind"
    );
    assert_eq!(state.borrow().settings_focus, SettingsFocus::Pane);
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentGroupName)
    );
    assert!(state.borrow().settings_pane().is_some(), "the tab is still open");

    // Enter again edits, and a second Enter commits: what was typed stays.
    press(&mut root, &app, "Enter");
    assert!(state.borrow().settings_pane_field_active, "the caret is back");
    for c in ["p", "r", "o", "d"] {
        press(&mut root, &app, c);
    }
    press(&mut root, &app, "Enter");
    assert!(!state.borrow().settings_pane_field_active, "committed");
    assert_eq!(state.borrow().settings_environment_group_draft, "prod");
    assert_eq!(
        state.borrow().settings_focus,
        SettingsFocus::Pane,
        "committing keeps the keyboard on the row"
    );
    assert!(state.borrow().settings_pane().is_some());

    // Then Escape steps back out to the rail — and stops there: from the rail
    // the key is not the tab's, so the tab stays open.
    press(&mut root, &app, "Escape");
    assert_eq!(state.borrow().settings_focus, SettingsFocus::Rail);
    let spaces = state.borrow().spaces.len();
    assert!(!press(&mut root, &app, "Escape"));
    let s = state.borrow();
    assert_eq!(s.spaces.len(), spaces, "the tab does not close on Esc");
    assert!(s.settings_pane().is_some());
}

/// `Enter` is what a row does: a stepper advances one step (the same step
/// `Right` takes), and a single-choice row selects the choice it names.
#[test]
fn enter_advances_a_stepper_and_selects_a_choice() {
    let app = AppContext::default();
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);
    press(&mut root, &app, "ArrowRight"); // into the pane: Invert scroll
    press(&mut root, &app, "ArrowDown"); // Scroll speed
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::ScrollSpeed)
    );
    let before = state.borrow().settings_scroll_speed;
    assert!(press(&mut root, &app, "Enter"));
    assert_eq!(state.borrow().settings_scroll_speed, before + 1);
    assert!(
        !state.borrow().settings_invert_scroll,
        "Enter did not reach the switch above the stepper"
    );

    // The font-size stepper is the other one: a tenth per press.
    let (mut root, state, _dir) = settings_root(SettingsCategory::EditorInput);
    press(&mut root, &app, "ArrowRight");
    let before = state.borrow().settings_font_size;
    press(&mut root, &app, "Enter");
    assert!((state.borrow().settings_font_size - (before + 0.1)).abs() < 1e-5);

    // A routing row selects the choice it names.
    let (mut root, state, _dir) = settings_root(SettingsCategory::AgentApproval);
    press(&mut root, &app, "ArrowRight"); // Auto-approve
    press(&mut root, &app, "ArrowDown"); // Vim mode
    press(&mut root, &app, "ArrowDown"); // Local
    press(&mut root, &app, "ArrowDown"); // Remote
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::Route(WorkspaceRouting::Remote))
    );
    press(&mut root, &app, "Enter");
    assert_eq!(
        state.borrow().workspace_routing,
        Some(WorkspaceRouting::Remote)
    );
}

/// The footer is the page's key map, drawn: each region and each kind of row
/// answers with its own words, and the drawn line is that list.
#[test]
fn the_footer_says_what_the_focused_row_answers() {
    use crate::ui::settings::footer_hints;

    let words = |hints: &[goble_ui::elements::ShortcutHint]| -> Vec<String> {
        hints.iter().map(|hint| hint.label().to_string()).collect()
    };
    // The rail names only what it answers: `Esc` is not its key any more.
    let rail = footer_hints(SettingsFocus::Rail, None, false);
    assert_eq!(words(&rail), vec!["page", "open the page"]);

    let toggle = footer_hints(
        SettingsFocus::Pane,
        Some(&SettingsControl::DarkMode),
        false,
    );
    assert_eq!(words(&toggle), vec!["row", "toggle", "back"]);

    let stepper = footer_hints(
        SettingsFocus::Pane,
        Some(&SettingsControl::ScrollSpeed),
        false,
    );
    assert_eq!(words(&stepper), vec!["row", "change", "increase", "back"]);

    let choice = footer_hints(SettingsFocus::Pane, Some(&SettingsControl::Route(WorkspaceRouting::Local)), false);
    assert_eq!(words(&choice), vec!["row", "choose", "select", "back"]);

    let field = footer_hints(
        SettingsFocus::Pane,
        Some(&SettingsControl::EnvironmentGroupName),
        false,
    );
    assert_eq!(words(&field), vec!["row", "edit", "back"]);

    // While the caret is in a field only the two reserved keys are the page's.
    let editing = footer_hints(
        SettingsFocus::Pane,
        Some(&SettingsControl::EnvironmentGroupName),
        true,
    );
    assert_eq!(words(&editing), vec!["commit", "cancel the edit"]);

    let list = footer_hints(
        SettingsFocus::Pane,
        Some(&SettingsControl::EnvironmentGroup("g".to_string())),
        false,
    );
    assert_eq!(words(&list), vec!["row", "open", "back"]);

    let wheel = footer_hints(
        SettingsFocus::Pane,
        Some(&SettingsControl::ColorWheel),
        false,
    );
    assert_eq!(words(&wheel), vec!["row", "back"]);

    // And the drawn footer follows the keyboard: the rail's words while the
    // rail holds it — with no `Esc` cap among them, because the rail answers no
    // `Esc` — and the switch's once the pane does.
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);
    let commands = frame(&mut root, &app, window);
    for label in words(&rail) {
        assert!(drawn(&commands, &label), "the footer says {label:?}");
    }
    let footer = footer_text(&commands, &app, window);
    assert!(
        !footer.iter().any(|run| run == "Esc"),
        "the rail promises no Esc: {footer:?}"
    );
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, RenderCommand::DrawIcon { name, .. } if name == "key-arrow-up")),
        "the move keys are drawn as the strip's own caps"
    );

    assert!(press(&mut root, &app, "ArrowRight"));
    state.borrow_mut().settings_pane_focus = 0;
    let commands = frame(&mut root, &app, window);
    assert!(drawn(&commands, "toggle"), "the switch's own verb");
    assert!(
        !drawn(&commands, "open the page"),
        "the rail's words are gone once the pane has the keyboard"
    );
    // The pane's own `Esc` is drawn: it is the "back" key there.
    assert!(drawn(&commands, "Esc"));
}

/// A page's focus order is the order it draws its rows in: stepping the pane's
/// slot walks the ring down the sheet, or across when two rows share a line
/// (the routing buttons sit side by side), never back up. (The pane's own
/// `FocusOrder` asserts each row is the one the order holds where it is drawn;
/// this asserts the reader's view of it.)
#[test]
fn the_focus_order_walks_down_the_rows_the_page_draws() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    for category in [
        SettingsCategory::Appearance,
        SettingsCategory::Mouse,
        SettingsCategory::EditorInput,
        SettingsCategory::AgentApproval,
        SettingsCategory::Models,
    ] {
        let (mut root, state, _dir) = settings_root(category);
        let controls = state.borrow().settings_pane_controls();
        assert!(!controls.is_empty(), "{category:?} has a row to focus");
        let mut previous: Option<(f32, f32)> = None;
        for slot in 0..controls.len() {
            {
                let mut s = state.borrow_mut();
                s.settings_focus = SettingsFocus::Pane;
                s.settings_pane_focus = slot;
            }
            let commands = frame(&mut root, &app, window);
            let ring = focused_row_ring(&commands, &app, window);
            if let Some((previous_y, previous_x)) = previous {
                assert!(
                    ring.min_y() > previous_y
                        || (ring.min_y() == previous_y && ring.min_x() > previous_x),
                    "{category:?}: {:?} is drawn after the row before it",
                    controls[slot]
                );
            }
            previous = Some((ring.min_y(), ring.min_x()));
        }
    }
}

/// A page taller than the viewport still scrolls inside the pane: the wheel
/// over the pane moves the content under a viewport that does not move.
#[test]
fn a_long_page_scrolls_inside_the_pane() {
    use crate::ui::settings::NAV_WIDTH;

    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir, _desktop) = settings_root_with_desktop(SettingsCategory::Mouse);
    frame(&mut root, &app, window);
    assert_eq!(
        state.borrow().settings_scroll.borrow().max_offset(),
        0.0,
        "a short page has nothing to scroll"
    );

    let (mut root, state, _dir, desktop) =
        settings_root_with_desktop(SettingsCategory::Environment);
    // Enough groups that the page outruns the height the header and the footer
    // leave of the pane at this window.
    let names = [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
        "lambda", "mu", "nu", "xi",
    ];
    for name in names {
        desktop.create_environment_group(name).expect("create group");
    }
    state.borrow_mut().refresh_environment_groups(Some(&desktop));
    assert_eq!(
        state.borrow().settings_environment_groups.len(),
        names.len()
    );

    // The pointer sits over the page's viewport for the frame that precedes the
    // wheel: the region remembers where the pointer was while painting.
    let at = vec2f(NAV_WIDTH + 40.0, 300.0);
    let commands = frame_at(&mut root, &app, window, at);
    let before = text_y(&commands, "New group").expect("the page draws its rows");
    let max = state.borrow().settings_scroll.borrow().max_offset();
    assert!(max > 0.0, "the page is taller than the viewport");
    let clip = clip_in_panel(&commands, &app, window);
    assert!(clip.max_y() <= window.y, "the viewport is inside the window");

    let mut ctx = EventContext::default();
    assert!(
        root.dispatch_event(
            &DispatchedEvent::Scroll {
                delta: vec2f(0.0, -30.0),
            },
            &mut ctx,
            &app
        ),
        "the wheel over the page is consumed"
    );
    assert_eq!(state.borrow().settings_scroll.borrow().offset(), 30.0);

    // The page moved; the viewport did not.
    let commands = frame_at(&mut root, &app, window, at);
    let after = text_y(&commands, "New group").expect("its rows are drawn, clipped or not");
    assert!(
        (before - after - 30.0).abs() < 0.5,
        "the page scrolled by the wheel's delta: {before} -> {after}"
    );
    let scrolled_clip = clip_in_panel(&commands, &app, window);
    assert_eq!(scrolled_clip, clip, "the viewport itself did not move");
}

/// The mouse keeps its paths: the rail's rows select a page, a switch flips, a
/// stepper's `+` steps, and the pane's own ✕ closes the tab.
#[test]
fn the_settings_tab_still_answers_the_mouse() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir) = settings_root(SettingsCategory::Mouse);
    let commands = frame(&mut root, &app, window);

    // A rail row: the label is inside the row's hit target.
    let models = pane_text(&commands, "Models").expect("the rail draws its pages");
    click(&mut root, &app, models + vec2f(2.0, 2.0));
    assert_eq!(state.borrow().settings_page(), SettingsCategory::Models);

    // A switch: the 44x24 track in the content pane.
    state.borrow_mut().settings_select_category(SettingsCategory::Mouse);
    let commands = frame(&mut root, &app, window);
    let switch = switch_track(&commands, window).expect("the pane draws its switch");
    click(&mut root, &app, switch.center().to_vector());
    assert!(state.borrow().settings_invert_scroll, "the switch flipped");

    // A stepper: its `+`.
    let plus = pane_text(&commands, "+").expect("the stepper draws its buttons");
    let before = state.borrow().settings_scroll_speed;
    click(&mut root, &app, plus + vec2f(2.0, 2.0));
    assert_eq!(state.borrow().settings_scroll_speed, before + 1);

    // The pane's own ✕: the close control in the pane's header band, below the
    // strip (whose tabs carry an ✕ of their own at the top of the window).
    let spaces = state.borrow().spaces.len();
    let close = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawIcon { origin, name, size, .. }
                if name == "x-close" && origin.y > crate::ui::shell::TOPBAR_HEIGHT =>
            {
                Some(*origin + vec2f(size * 0.5, size * 0.5))
            }
            _ => None,
        })
        .expect("the settings pane draws its close control");
    click(&mut root, &app, close);
    let s = state.borrow();
    assert!(s.settings_pane().is_none(), "the ✕ closed the settings tab");
    assert_eq!(s.spaces.len(), spaces - 1, "and it took its space with it");
}

/// One click: the down/up pair a `Button` and a `HoverRow` both complete on.
fn click(root: &mut Box<dyn Element>, app: &AppContext, at: Vector2F) {
    for event in [
        DispatchedEvent::MouseDown {
            position: at,
            button: 0,
        },
        DispatchedEvent::MouseUp {
            position: at,
            button: 0,
        },
    ] {
        press_event(root, app, &event);
    }
}

/// Whether a run of text is painted.
fn drawn(commands: &[RenderCommand], text: &str) -> bool {
    commands.iter().any(|c| matches!(c, RenderCommand::DrawText { text: run, .. } if run == text))
}

/// The origin of a run of text.
fn drawn_text(commands: &[RenderCommand], text: &str) -> Option<Vector2F> {
    commands.iter().find_map(|c| match c {
        RenderCommand::DrawText { origin, text: run, .. } if run == text => Some(*origin),
        _ => None,
    })
}

/// The runs the footer draws: everything painted below the page's viewport.
fn footer_text(commands: &[RenderCommand], app: &AppContext, window: Vector2F) -> Vec<String> {
    let clip = clip_in_panel(commands, app, window);
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. } if origin.y > clip.max_y() => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

/// The origin of a run of text drawn inside the settings pane — below the
/// topbar band the strip draws in, where a label on the page or on the rail
/// cannot be confused with a tab's own.
fn pane_text(commands: &[RenderCommand], text: &str) -> Option<Vector2F> {
    commands.iter().find_map(|c| match c {
        RenderCommand::DrawText { origin, text: run, .. }
            if run == text && origin.y > PANE_TOP =>
        {
            Some(*origin)
        }
        _ => None,
    })
}

/// The y of a run of text.
fn text_y(commands: &[RenderCommand], text: &str) -> Option<f32> {
    drawn_text(commands, text).map(|origin| origin.y)
}

/// The focused row's ring: the page column's only `ColorToken::Focus` stroke.
/// The threshold keeps a rail row's own ring out of it, and every ring is
/// right of the rail by construction.
fn focused_row_ring(commands: &[RenderCommand], app: &AppContext, window: Vector2F) -> RectF {
    let focus = app.theme.color(goble_ui::theme::ColorToken::Focus);
    commands
        .iter()
        .rev()
        .find_map(|c| match c {
            RenderCommand::StrokeRect { rect, color, .. }
                if *color == focus && rect.min_x() >= pane_left(window) =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the focused row wears the focus ring")
}

/// The page's scroll viewport: the clip band the pane draws inside its own
/// bounds.
fn clip_in_panel(commands: &[RenderCommand], app: &AppContext, window: Vector2F) -> RectF {
    let panel = pane_panel(commands, app, window);
    commands
        .iter()
        .rev()
        .find_map(|c| match c {
            RenderCommand::ClipRect(rect)
                if rect.min_x() >= panel.min_x() && rect.max_x() <= panel.max_x() =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("the page scrolls in a clipped viewport")
}

/// Every `Bg`-coloured fill the frame paints.
fn bg_fills(commands: &[RenderCommand], app: &AppContext) -> Vec<RectF> {
    let bg = app.theme.color(goble_ui::theme::ColorToken::Bg);
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::FillRect { rect, color, .. } if *color == bg => Some(*rect),
            _ => None,
        })
        .collect()
}

/// The top of the settings pane: below the topbar's own band, which is all the
/// strip and the toolbar draw in. The harness hides the sidebar, so the pane is
/// the whole main column.
const PANE_TOP: f32 = crate::ui::shell::TOPBAR_HEIGHT;

/// The x of the page column's left edge: the pane's own left edge (the window's,
/// with the sidebar hidden) plus the rail and the rule beside it.
fn pane_left(_window: Vector2F) -> f32 {
    crate::ui::settings::NAV_WIDTH + crate::ui::settings::RULE_WIDTH
}

/// The switch's 44x24 track, the only one inside the page.
fn switch_track(commands: &[RenderCommand], window: Vector2F) -> Option<RectF> {
    commands.iter().find_map(|c| match c {
        RenderCommand::FillRect { rect, .. }
            if rect.width() == 44.0 && rect.height() == 24.0 && rect.min_x() >= pane_left(window) =>
        {
            Some(*rect)
        }
        _ => None,
    })
}

/// Lay out and paint one frame with the pointer at `at`, so a scroll region
/// records that the wheel belongs to it, and return its commands.
fn frame_at(
    root: &mut Box<dyn Element>,
    app: &AppContext,
    window: Vector2F,
    at: Vector2F,
) -> Vec<RenderCommand> {
    let constraint = SizeConstraint::loose(window);
    let _ = root.layout(constraint, &mut LayoutContext::default(), app);
    let mut paint_ctx = PaintContext::new(goble_ui::render::Renderer::new());
    paint_ctx.cursor_inside = true;
    paint_ctx.cursor_position = at;
    root.paint(vec2f(0.0, 0.0), &mut paint_ctx, app);
    paint_ctx
        .renderer
        .map(|r| r.commands().to_vec())
        .unwrap_or_default()
}

// ---- Settings -> Environment ----------------------------------------------

/// The Environment pane's whole lifecycle from the keyboard, asserted against
/// the store: a group is created by name, a secret is added to it, the secret
/// is removed, and then the group is deleted with nothing left behind.
#[test]
fn the_environment_pane_creates_adds_and_deletes_through_the_store() {
    let app = AppContext::default();
    let (mut root, state, _dir, desktop) =
        settings_root_with_desktop(SettingsCategory::Environment);

    // Create a group: type its name, then activate "Create group".
    press(&mut root, &app, "Tab"); // into the pane, on the name field
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentGroupName)
    );
    press(&mut root, &app, "Enter");
    for c in ["p", "r", "o", "d"] {
        press(&mut root, &app, c);
    }
    press(&mut root, &app, "Enter"); // commit the name
    press(&mut root, &app, "ArrowDown"); // Create group
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentCreateGroup)
    );
    press(&mut root, &app, "Enter");

    let groups = desktop.environment_groups().expect("read the store back");
    assert_eq!(groups.len(), 1, "a group row was written");
    assert_eq!(groups[0].name, "prod");
    let group_id = groups[0].id.clone();
    assert_eq!(
        state.borrow().settings_environment_open_group.as_deref(),
        Some(group_id.as_str()),
        "the new group is the one the pane shows"
    );

    // The open group adds its own slots: its row, its delete control, the two
    // fields and the save button.
    press(&mut root, &app, "ArrowDown"); // Group(prod)
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentGroup(group_id.clone()))
    );
    press(&mut root, &app, "ArrowDown"); // DeleteGroup(prod)
    press(&mut root, &app, "ArrowDown"); // the secret-name field
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentSecretName)
    );
    press(&mut root, &app, "Enter");
    for c in ["A", "P", "I", "_", "K", "E", "Y"] {
        press(&mut root, &app, c);
    }
    press(&mut root, &app, "Enter"); // commit the name
    press(&mut root, &app, "ArrowDown"); // the secret-value field
    press(&mut root, &app, "Enter");
    for c in ["s", "k", "-", "1"] {
        press(&mut root, &app, c);
    }
    press(&mut root, &app, "Enter"); // commit the value
    press(&mut root, &app, "ArrowDown"); // Save secret
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentSaveSecret)
    );
    press(&mut root, &app, "Enter");

    let groups = desktop.environment_groups().expect("read the store back");
    assert_eq!(groups[0].entries.len(), 1, "the secret row was written");
    assert_eq!(groups[0].entries[0].name, "API_KEY");
    assert_eq!(groups[0].entries[0].value, "sk-1");
    let entry_id = groups[0].entries[0].id.clone();
    assert!(
        state.borrow().settings_environment_secret_name.is_empty(),
        "the fields are cleared once the secret is stored"
    );

    // Activating the secret's row loads it back for editing...
    press(&mut root, &app, "Enter"); // (the save button again is a no-op: empty name)
    press(&mut root, &app, "ArrowDown"); // Secret(API_KEY)
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentSecret(entry_id.clone()))
    );
    press(&mut root, &app, "Enter");
    assert_eq!(state.borrow().settings_environment_secret_name, "API_KEY");
    assert_eq!(state.borrow().settings_environment_secret_value, "sk-1");
    assert_eq!(
        state.borrow().settings_environment_editing.as_deref(),
        Some("API_KEY")
    );

    // ...and its own delete control removes just that entry.
    press(&mut root, &app, "ArrowDown");
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentDeleteSecret(entry_id))
    );
    press(&mut root, &app, "Enter");
    let groups = desktop.environment_groups().expect("read the store back");
    assert_eq!(groups.len(), 1, "the group is still there");
    assert!(groups[0].entries.is_empty(), "and the secret is gone");

    // The group's own delete control takes the group and leaves nothing. The
    // row the keyboard was on is gone, so the pane's order is the seven slots
    // left: walk back up to the group's delete control.
    press(&mut root, &app, "ArrowUp");
    press(&mut root, &app, "ArrowUp");
    press(&mut root, &app, "ArrowUp");
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::EnvironmentDeleteGroup(group_id.clone()))
    );
    press(&mut root, &app, "Enter");
    assert!(
        desktop.environment_groups().expect("read the store back").is_empty(),
        "the group and everything in it is gone"
    );
    assert!(state.borrow().settings_environment_open_group.is_none());
}

/// A click on a group row and on its delete control still do what the keys do:
/// the two paths are one implementation.
#[test]
fn the_environment_rows_answer_the_mouse_as_well_as_the_keys() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir, desktop) =
        settings_root_with_desktop(SettingsCategory::Environment);
    let group = desktop.create_environment_group("staging").expect("create");
    state
        .borrow_mut()
        .refresh_environment_groups(Some(&desktop));
    assert_eq!(
        state.borrow().settings_environment_groups.len(),
        1,
        "the pane was reloaded from the store"
    );
    assert!(state.borrow().settings_environment_open_group.is_none());

    // Click the group's own label: the group opens (the pane shows its
    // secrets, whose fields join the order).
    let commands = frame(&mut root, &app, window);
    let at = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. } if text == "staging" => Some(*origin),
            _ => None,
        })
        .expect("the group row is drawn");
    let mut ctx = EventContext::default();
    let click = at + vec2f(2.0, 2.0);
    root.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: click,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    root.dispatch_event(
        &DispatchedEvent::MouseUp {
            position: click,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    assert_eq!(
        state.borrow().settings_environment_open_group.as_deref(),
        Some(group.id.as_str()),
        "the click opened the group"
    );
}

// ---- Settings -> Connections ----------------------------------------------

/// Two concrete blocks, one of them in `known_hosts` and one of them naming a
/// key that is not on disk, so a test can tell a known host from an unused one
/// and an existing key from a missing one.
const TWO_HOSTS: &str = "\
Host web
    HostName web.example.com
    User deploy
    Port 2222
    IdentityFile ~/.ssh/id_web

Host db
    HostName db.example.com
    User admin
    Port 5432
    IdentityFile ~/.ssh/id_db
";

/// A settings tab on the Connections page reading a fixture `~/.ssh` under a
/// temp home. The state is pointed at the fixture before the first frame — the
/// frame that shows the page is the one that reads it — so the user's real home
/// is never touched.
fn connections_root(
    files: &[(&str, &str)],
) -> (
    Box<dyn Element>,
    Rc<RefCell<crate::state::UiState>>,
    tempfile::TempDir,
) {
    let (root, state, dir) = settings_root(SettingsCategory::Connections);
    for (rel, contents) in files {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().expect("the fixture file has a parent"))
            .expect("fixture dir");
        std::fs::write(path, contents).expect("fixture file");
    }
    state.borrow_mut().settings_ssh_home = Some(dir.path().to_path_buf());
    (root, state, dir)
}

/// Every run of text a frame paints.
fn drawn_runs(commands: &[RenderCommand]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// Whether any run of text the frame paints contains `needle`. A long line is
/// one run of text, so a needle that spans words is found where the page drew
/// it.
fn any_run_contains(commands: &[RenderCommand], needle: &str) -> bool {
    drawn_runs(commands).iter().any(|run| run.contains(needle))
}

/// Whether the run of text is painted muted, the way the page's own prose and
/// its findings are.
fn muted_text(commands: &[RenderCommand], app: &AppContext, text: &str) -> bool {
    let muted = app.theme.color(goble_ui::theme::ColorToken::Muted);
    commands.iter().any(|c| {
        matches!(c, RenderCommand::DrawText { text: run, color, .. } if run == text && *color == muted)
    })
}

/// One row per concrete host, with the target `user@hostname:port` a connection
/// needs to be chosen, the key the block names, and whether this machine has
/// used the host — plus one row per key file, by name and existence.
#[test]
fn the_connections_page_draws_a_row_per_host_with_its_target_and_key() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir) = connections_root(&[
        (".ssh/config", TWO_HOSTS),
        (
            ".ssh/id_web",
            "-----BEGIN OPENSSH PRIVATE KEY-----\nKEYMATERIAL\n",
        ),
        (".ssh/known_hosts", "web.example.com ssh-ed25519 AAAA\n"),
    ]);
    let commands = frame(&mut root, &app, window);

    {
        let s = state.borrow();
        let ssh = s.settings_ssh_hosts.as_ref().expect("the page read it");
        assert_eq!(
            ssh.hosts.iter().map(|h| h.alias.as_str()).collect::<Vec<_>>(),
            vec!["db", "web"],
            "one row per concrete block, sorted by alias"
        );
        assert!(ssh.findings.is_empty(), "two hosts were found");
    }

    for text in [
        "web",
        "deploy@web.example.com:2222",
        "~/.ssh/id_web",
        "db",
        "admin@db.example.com:5432",
        "~/.ssh/id_db",
    ] {
        assert!(drawn(&commands, text), "the host rows draw {text:?}");
    }
    // `web.example.com` is in `known_hosts`; `db.example.com` is not.
    assert!(drawn(&commands, "known"));
    assert!(drawn(&commands, "not used yet"));

    // One row per key file: the `id_*` file that is there, and the key the `db`
    // block names, which is not.
    assert!(drawn(&commands, "id_web"));
    assert!(drawn(&commands, "exists"));
    assert!(drawn(&commands, "missing"));

    // The page's own words: `Enter` names a target, it does not connect.
    assert!(any_run_contains(&commands, "Enter selects a connection"));
    assert!(any_run_contains(&commands, "never key contents"));
}

/// `Host *`, `Host !x`, a `Match` block and an `Include` are rules, not
/// connections: they draw no row, and neither do the keywords they carry.
#[test]
fn wildcard_negated_match_and_include_blocks_draw_no_rows() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir) = connections_root(&[(
        ".ssh/config",
        "\
Include ~/.ssh/conf.d/*

Host *
    ServerAliveInterval 60

Host !prod
    User nobody

Host *.internal
    User ops

Match host *.internal
    HostName ignored.example.com
    User ignored-user

Host real
    HostName real.example.com
    User me
",
    )]);
    let commands = frame(&mut root, &app, window);

    {
        let s = state.borrow();
        let ssh = s.settings_ssh_hosts.as_ref().expect("the page read it");
        assert_eq!(
            ssh.hosts.iter().map(|h| h.alias.as_str()).collect::<Vec<_>>(),
            vec!["real"],
            "only the concrete block is a connection"
        );
    }
    assert!(drawn(&commands, "real"));
    assert!(drawn(&commands, "me@real.example.com:22"));
    for absent in [
        "!prod",
        "*.internal",
        "ignored.example.com",
        "ignored-user",
        "nobody",
        "ops",
        "ServerAliveInterval",
        "Include ~/.ssh",
    ] {
        assert!(
            !any_run_contains(&commands, absent),
            "{absent:?} is a rule, and rules draw no row"
        );
    }
}

/// When `~/.ssh` is not there, has no `config`, or holds only rules, the page
/// draws the finding as a muted line naming what is missing and the path — a
/// normal machine state, not an error and not an empty page.
#[test]
fn the_missing_directory_and_config_less_cases_draw_their_muted_lines() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);

    // No `~/.ssh` at all.
    let (mut root, state, dir) = connections_root(&[]);
    let commands = frame(&mut root, &app, window);
    {
        let s = state.borrow();
        let ssh = s.settings_ssh_hosts.as_ref().expect("the page read it");
        assert_eq!(ssh.findings.len(), 1);
        let line = ssh.findings[0].message();
        assert!(
            line.contains(&dir.path().join(".ssh").display().to_string()),
            "the line names the path: {line}"
        );
        assert!(
            muted_text(&commands, &app, &line),
            "the finding draws as a muted line: {line}"
        );
    }
    // Not an empty page: the reload row is there, and it is the focus order.
    assert!(drawn(&commands, "Reload from ~/.ssh"));
    assert_eq!(
        state.borrow().settings_pane_controls(),
        vec![SettingsControl::ReloadSshHosts]
    );

    // The directory exists with no `config`: its own line, and the keys.
    let (mut root, state, dir) = connections_root(&[(".ssh/id_ed25519", "KEY\n")]);
    let commands = frame(&mut root, &app, window);
    {
        let s = state.borrow();
        let ssh = s.settings_ssh_hosts.as_ref().expect("the page read it");
        assert_eq!(ssh.findings.len(), 1);
        let line = ssh.findings[0].message();
        assert!(
            line.contains(&dir.path().join(".ssh/config").display().to_string()),
            "the line names the config path: {line}"
        );
        assert!(muted_text(&commands, &app, &line));
    }
    assert!(drawn(&commands, "id_ed25519"));
    assert!(drawn(&commands, "exists"));

    // A config that declares no concrete host.
    let (mut root, state, _dir) = connections_root(&[(".ssh/config", "Host *\n    User root\n")]);
    let commands = frame(&mut root, &app, window);
    let s = state.borrow();
    let ssh = s.settings_ssh_hosts.as_ref().expect("the page read it");
    assert_eq!(ssh.findings.len(), 1);
    let line = ssh.findings[0].message();
    assert!(
        line.contains("no concrete host"),
        "the line says what is missing: {line}"
    );
    assert!(muted_text(&commands, &app, &line));
}

/// A `known_hosts` holding only hashed entries reports a count: those names are
/// not in the file, and the page does not guess them.
#[test]
fn hashed_known_hosts_is_reported_as_a_count_not_names() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir) = connections_root(&[
        (
            ".ssh/config",
            "Host box\n    HostName box.example.com\n    User me\n",
        ),
        (
            ".ssh/known_hosts",
            "|1|AAA=|BBB= ssh-ed25519 KEYONE\n|1|CCC=|DDD= ssh-rsa KEYTWO\n",
        ),
    ]);
    let commands = frame(&mut root, &app, window);

    assert!(
        any_run_contains(&commands, "2 known_hosts entries are hashed"),
        "the count is what is drawn"
    );
    {
        let s = state.borrow();
        let ssh = s.settings_ssh_hosts.as_ref().expect("the page read it");
        assert_eq!(ssh.hashed_known_hosts, 2);
        assert!(
            ssh.hosts.iter().all(|h| !h.known),
            "a hashed entry cannot make a host known"
        );
    }
    // The host is not marked from the file, and nothing of the file is drawn.
    assert!(!drawn(&commands, "known"));
    assert!(drawn(&commands, "not used yet"));
    for absent in ["AAA=", "BBB=", "CCC=", "KEYONE", "KEYTWO"] {
        assert!(
            !any_run_contains(&commands, absent),
            "{absent:?} is not a name this page can read"
        );
    }
}

/// No key material reaches the screen: not a key's contents, not a passphrase,
/// not a `ProxyJump` command — paths and names only.
#[test]
fn no_drawn_text_carries_key_material() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, _state, _dir) = connections_root(&[
        (
            ".ssh/config",
            "\
Host secret
    HostName secret.example.com
    User me
    IdentityFile ~/.ssh/id_secret
    ProxyJump jump.example.com
",
        ),
        (
            ".ssh/id_secret",
            "-----BEGIN OPENSSH PRIVATE KEY-----\nSECRETMATERIAL\n-----END OPENSSH PRIVATE KEY-----\n",
        ),
        (".ssh/id_other", "SECRETMATERIAL\n"),
    ]);
    let commands = frame(&mut root, &app, window);

    for absent in [
        "SECRETMATERIAL",
        "BEGIN OPENSSH",
        "PRIVATE KEY",
        "jump.example.com",
    ] {
        assert!(
            !any_run_contains(&commands, absent),
            "{absent:?} is key material or a jump command, and is never drawn"
        );
    }
    // The key is on the page as a name and a path, and nothing more.
    assert!(drawn(&commands, "~/.ssh/id_secret"));
    assert!(drawn(&commands, "id_secret"));
}

/// The page's rows are its focus order, drawn in that order, and `Enter` on a
/// connection row selects it and names it. It connects nothing: no space, no
/// pane and no terminal session is created.
#[test]
fn the_connection_rows_are_focusable_in_order_and_enter_selects() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, _dir) = connections_root(&[(".ssh/config", TWO_HOSTS)]);
    frame(&mut root, &app, window);

    let controls = state.borrow().settings_pane_controls();
    assert_eq!(
        controls,
        vec![
            SettingsControl::SshHost("db".to_string()),
            SettingsControl::SshHost("web".to_string()),
            SettingsControl::ReloadSshHosts,
        ],
        "the hosts in the reader's order, then the reload row"
    );

    let mut previous: Option<(f32, f32)> = None;
    for slot in 0..controls.len() {
        {
            let mut s = state.borrow_mut();
            s.settings_focus = SettingsFocus::Pane;
            s.settings_pane_focus = slot;
        }
        let commands = frame(&mut root, &app, window);
        let ring = focused_row_ring(&commands, &app, window);
        if let Some((previous_y, previous_x)) = previous {
            assert!(
                ring.min_y() > previous_y || (ring.min_y() == previous_y && ring.min_x() > previous_x),
                "{:?} is drawn after the row before it",
                controls[slot]
            );
        }
        previous = Some((ring.min_y(), ring.min_x()));
    }

    // The footer names what the row does, and it is not "connect".
    {
        let mut s = state.borrow_mut();
        s.settings_focus = SettingsFocus::Pane;
        s.settings_pane_focus = 0;
    }
    let commands = frame(&mut root, &app, window);
    assert!(drawn(&commands, "select"), "the row's own verb");
    assert!(!drawn(&commands, "connect"), "no row offers a connection");
    assert!(
        any_run_contains(&commands, "This build opens no session over SSH"),
        "and the page says so in its own words"
    );

    let spaces = state.borrow().spaces.len();
    press(&mut root, &app, "Enter");
    let commands = frame(&mut root, &app, window);
    assert_eq!(
        state.borrow().settings_ssh_selected.as_deref(),
        Some("db"),
        "the row is selected"
    );
    assert!(
        any_run_contains(&commands, "Selected db — admin@db.example.com:5432"),
        "and it is named on the page"
    );
    {
        let s = state.borrow();
        assert_eq!(s.spaces.len(), spaces, "no tab was opened");
        assert!(
            s.terminal.borrow().sessions.is_empty(),
            "and no session was opened over SSH"
        );
    }

    // The row itself is the hit target: a click selects what `Enter` selects.
    let at = pane_text(&commands, "web").expect("the web row is drawn");
    click(&mut root, &app, at + vec2f(2.0, 2.0));
    assert_eq!(state.borrow().settings_ssh_selected.as_deref(), Some("web"));
}

/// The directory is read once, when the page is shown, and again when the
/// reload row runs: a frame that changes nothing reads nothing.
#[test]
fn the_reload_row_re_reads_the_directory() {
    let app = AppContext::default();
    let window = vec2f(1024.0, 768.0);
    let (mut root, state, dir) = connections_root(&[(
        ".ssh/config",
        "Host one\n    HostName one.example.com\n    User me\n",
    )]);
    let commands = frame(&mut root, &app, window);
    assert!(drawn(&commands, "me@one.example.com:22"));

    // The file grows under the page. The next frame still shows the cached
    // read: the page does not re-read the directory while it draws.
    std::fs::write(
        dir.path().join(".ssh/config"),
        "\
Host one
    HostName one.example.com
    User me

Host two
    HostName two.example.com
    User me
",
    )
    .expect("rewrite the fixture config");
    let commands = frame(&mut root, &app, window);
    assert!(
        !any_run_contains(&commands, "two.example.com"),
        "no read happens for a frame that changed nothing"
    );

    // The reload row re-reads it.
    {
        let mut s = state.borrow_mut();
        s.settings_focus = SettingsFocus::Pane;
        s.settings_pane_focus = 1;
    }
    assert_eq!(
        state.borrow().settings_focused_control(),
        Some(SettingsControl::ReloadSshHosts)
    );
    press(&mut root, &app, "Enter");
    let commands = frame(&mut root, &app, window);
    assert!(drawn(&commands, "two"));
    assert!(drawn(&commands, "me@two.example.com:22"));
    assert_eq!(
        state
            .borrow()
            .settings_ssh_hosts
            .as_ref()
            .expect("the reload read it")
            .hosts
            .len(),
        2
    );
}
