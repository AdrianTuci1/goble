use super::*;

use goble_ui::event::ModifiersState;
use goble_ui::geometry::vec2f;
use goble_ui::render::RenderCommand;
use goble_ui::test_util::render_element;

/// The chord's modifier as the platform reports it: Command on macOS, Ctrl
/// everywhere else — the two the tab hints spell out.
fn chord_modifier(held: bool) -> ModifiersState {
    let mut modifiers = ModifiersState::default();
    if held {
        if cfg!(target_os = "macos") {
            modifiers.command = true;
        } else {
            modifiers.ctrl = true;
        }
    }
    modifiers
}

/// The chord's prefix, spelled the way the tabs spell it.
#[cfg(target_os = "macos")]
const CHORD: &str = "⌘";
#[cfg(not(target_os = "macos"))]
const CHORD: &str = "Ctrl+";

/// The shortcut hint the tab at `index` (1-based) draws.
fn hint(index: usize) -> String {
    format!("{CHORD}{index}")
}

/// Every text run the topbar band drew, in paint order.
fn topbar_texts(commands: &[RenderCommand]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. }
                if origin.y < crate::ui::shell::TOPBAR_HEIGHT =>
            {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

/// The hint runs the topbar band drew, in tab order: only the runs that are a
/// tab's own shortcut, so a tab name that happens to read like one cannot pass
/// for it. The strip paints the selected tab after its neighbours (its feet
/// have to cover their corners), so the runs are sorted by their x — the order
/// they read on screen — rather than by paint order.
fn drawn_hints(commands: &[RenderCommand]) -> Vec<String> {
    let mut hints: Vec<(f32, String)> = commands
        .iter()
        .filter_map(|command| match command {
            RenderCommand::DrawText { origin, text, .. }
                if origin.y < crate::ui::shell::TOPBAR_HEIGHT
                    && (1..=8).any(|index| text == &hint(index)) =>
            {
                Some((origin.x, text.clone()))
            }
            _ => None,
        })
        .collect();
    hints.sort_by(|a, b| a.0.total_cmp(&b.0));
    hints.into_iter().map(|(_, text)| text).collect()
}

/// Three workspaces on screen, the first one active, each holding one pane
/// whose id names its place.
fn three_workspaces(state: &Rc<RefCell<crate::state::UiState>>) {
    use crate::ui::{Pane, PaneKind, Space};

    let mut spaces = Vec::new();
    for id in 1..=3 {
        spaces.push(Space::unnamed(Pane::Leaf {
            id,
            kind: PaneKind::Terminal,
        }));
    }
    let mut s = state.borrow_mut();
    s.spaces = spaces;
    s.active_space = 0;
    s.active_pane_id = 1;
}

/// One frame of the whole app, laid out in the window the root-view cases use.
fn frame(root: &mut Box<dyn Element>, app: &AppContext) -> Vec<RenderCommand> {
    render_element(root, vec2f(1024.0, 768.0), app)
}

/// A key through the whole app tree, after a frame — so the chords are
/// dispatched through the actions the last rebuild built.
fn key(root: &mut Box<dyn Element>, app: &AppContext, event: DispatchedEvent) -> bool {
    let _ = frame(root, app);
    let mut ctx = EventContext;
    root.dispatch_event(&event, &mut ctx, app)
}

/// A modifier transition asks the window for the frame that draws it, and the
/// frame it asked for settles the request — an unchanged modifier asks for
/// nothing, so an idle window stays idle.
#[test]
fn a_modifier_transition_asks_the_window_for_one_frame() {
    let (view, _bus, _dir) = root_with_bus();
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(view);

    let _ = frame(&mut root, &app);
    assert!(
        !root.wants_animation(),
        "a window with nothing in flight idles"
    );

    let _ = key(
        &mut root,
        &app,
        DispatchedEvent::Modifiers {
            modifiers: chord_modifier(true),
        },
    );
    assert!(
        root.wants_animation(),
        "the transition pulls the next frame in at once"
    );

    let _ = frame(&mut root, &app);
    assert!(
        !root.wants_animation(),
        "the frame the transition asked for settles it"
    );

    let _ = key(
        &mut root,
        &app,
        DispatchedEvent::Modifiers {
            modifiers: chord_modifier(true),
        },
    );
    assert!(
        !root.wants_animation(),
        "a modifier change that changes nothing asks for no frame"
    );
}

/// Cmd/Ctrl+3 selects the third workspace — the tab whose hint says `3` — and a
/// digit the strip has no tab for does nothing at all.
#[test]
fn a_chord_digit_selects_the_workspace_its_hint_names() {
    let (view, _bus, _dir) = root_with_bus();
    let state = view.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.right_sidebar_open = false;
        s.crons_open = false;
    }
    three_workspaces(&state);
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(view);
    let chord = |key: &str| DispatchedEvent::KeyDown {
        key: key.to_string(),
        modifiers: chord_modifier(true),
    };

    // A bare digit is typing, not a chord.
    let _ = key(
        &mut root,
        &app,
        DispatchedEvent::KeyDown {
            key: "3".to_string(),
            modifiers: ModifiersState::none(),
        },
    );
    assert_eq!(
        state.borrow().active_space,
        0,
        "a digit with no modifier leaves the tab strip alone"
    );

    assert!(
        key(&mut root, &app, chord("3")),
        "the chord belongs to the root"
    );
    assert_eq!(
        state.borrow().active_space,
        2,
        "the third tab is the one the chord names"
    );
    assert_eq!(
        state.borrow().active_pane_id,
        3,
        "the arriving tab's first leaf takes the keyboard"
    );

    // Three tabs, no fourth: the digit names nothing, so the strip stays put.
    let _ = key(&mut root, &app, chord("4"));
    assert_eq!(
        state.borrow().active_space,
        2,
        "a digit past the last tab moves nothing"
    );
    // `9` is not a tab digit at all, as in a browser's tab strip.
    let _ = key(&mut root, &app, chord("9"));
    assert_eq!(
        state.borrow().active_space,
        2,
        "the ninth slot is not a tab shortcut"
    );
}
