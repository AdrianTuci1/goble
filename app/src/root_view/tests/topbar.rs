use super::*;

use goble_ui::event::ModifiersState;
use goble_ui::geometry::vec2f;
use goble_ui::render::RenderCommand;
use goble_ui::test_util::render_element;

/// The agent-management control follows the pointer rules of every other
/// topbar button: a move over it only lights the control, and the release
/// inside it opens the panel.
#[test]
fn the_agent_management_control_opens_on_release_not_on_hover() {
    use goble_ui::elements::EventContext;
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;
    use goble_ui::Element;

    let (root, _bus, _dir) = root_with_bus();
    let state = root.state_rc();
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.right_sidebar_open = false;
        s.crons_open = false;
    }
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(root);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    let center = commands
        .iter()
        .find_map(|command| match command {
            RenderCommand::DrawIcon {
                name, origin, size, ..
            } if name == "robot" && origin.y < crate::ui::shell::TOPBAR_HEIGHT => {
                Some(vec2f(origin.x + size / 2.0, origin.y + size / 2.0))
            }
            _ => None,
        })
        .expect("the agent management control");

    let mut ctx = EventContext;
    assert!(!root.dispatch_event(
        &DispatchedEvent::MouseMove { position: center },
        &mut ctx,
        &app,
    ));
    let _ = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    assert!(
        !state.borrow().agent_dashboard.open,
        "a pointer move over the control only lights it"
    );

    let _ = root.dispatch_event(
        &DispatchedEvent::MouseDown {
            position: center,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    let _ = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    let _ = root.dispatch_event(
        &DispatchedEvent::MouseUp {
            position: center,
            button: 0,
        },
        &mut ctx,
        &app,
    );
    assert!(
        state.borrow().agent_dashboard.open,
        "a release on the control opens the agent management panel"
    );
}

/// C3's acceptance: the topbar cue reads C1's live count and is absent at
/// zero. It is information only: a click on it is not a hit target.
#[test]
fn the_topbar_cue_reads_the_live_count_and_is_inert() {
    use goble_core::harness::ToolCallStatus;
    use goble_desktop_service::ToolCallEvent;
    use goble_ui::elements::EventContext;
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;
    use goble_ui::Element;

    let (root, _bus, _dir) = root_with_bus();
    let state = root.state_rc();
    // No first-run overlay may sit above the bar and swallow the click.
    {
        let mut s = state.borrow_mut();
        s.show_workspace_choice = false;
        s.show_llm_key_banner = false;
        s.right_sidebar_open = false;
        s.crons_open = false;
    }
    let app = AppContext::default();
    let mut root: Box<dyn Element> = Box::new(root);

    // Nothing in flight: the topbar band draws no live cue.
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    assert!(
        !commands.iter().any(|c| matches!(
            c,
            RenderCommand::DrawIcon { origin, name, .. }
                if name == "diamond" && origin.y < crate::ui::shell::TOPBAR_HEIGHT
        )),
        "an idle topbar draws no live cue"
    );

    // One in-flight tool for the active pane: the cue shows the diamond
    // next to `1`. The event names the pane's own conversation, so it is
    // attributed to that pane rather than to whichever pane has focus.
    state.borrow_mut().record_pane_conversation(1, "conv-1");
    state.borrow_mut().apply_tool_event(&ToolCallEvent {
        chat_id: "conv-1".into(),
        id: "call_1".into(),
        name: "read_file".into(),
        arguments: serde_json::json!({"path": "src/lib.rs"}),
        status: ToolCallStatus::Running,
        result: None,
    });
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    // The pane's turn-status footer draws a diamond too, so the cue's own
    // icon is the one in the topbar band.
    let icon = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawIcon { origin, name, .. }
                if name == "diamond" && origin.y < crate::ui::shell::TOPBAR_HEIGHT =>
            {
                Some(*origin)
            }
            _ => None,
        })
        .expect("the cue marks the count with the diamond icon");
    let origin = commands
        .iter()
        .find_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. }
                if text == "1" && origin.y < crate::ui::shell::TOPBAR_HEIGHT =>
            {
                Some(*origin)
            }
            _ => None,
        })
        .expect("the topbar cue draws the live count");

    // Click it, on the count and on the diamond beside it: the indicator is
    // not a hit target, so the events travel on unconsumed.
    let mut ctx = EventContext;
    for position in [origin + vec2f(2.0, 4.0), icon + vec2f(2.0, 4.0)] {
        for event in [
            DispatchedEvent::MouseDown {
                position,
                button: 0,
            },
            DispatchedEvent::MouseUp {
                position,
                button: 0,
            },
        ] {
            assert!(
                !root.dispatch_event(&event, &mut ctx, &app),
                "the topbar indicator takes no pointer event at {position:?}"
            );
        }
    }
}

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
/// for it.
fn drawn_hints(commands: &[RenderCommand]) -> Vec<String> {
    topbar_texts(commands)
        .into_iter()
        .filter(|text| (1..=8).any(|index| text == &hint(index)))
        .collect()
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

/// With the chord modifier held, every workspace tab advertises the shortcut
/// that selects it — one number per tab, in strip order — and the hint is gone
/// again the moment it is released.
#[test]
fn holding_the_chord_modifier_numbers_every_workspace_tab() {
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

    let commands = frame(&mut root, &app);
    assert!(
        drawn_hints(&commands).is_empty(),
        "no tab advertises a shortcut while the modifier is up: {:?}",
        topbar_texts(&commands)
    );

    // The modifier goes down: the platform forwards that as a bare modifier
    // change, since a modifier key reports no key press of its own.
    assert!(
        !key(
            &mut root,
            &app,
            DispatchedEvent::Modifiers {
                modifiers: chord_modifier(true),
            }
        ),
        "the modifier change is not a shortcut the app consumes"
    );
    assert!(
        state.borrow().command_held,
        "the held modifier is recorded in app state"
    );
    let commands = frame(&mut root, &app);
    assert_eq!(
        drawn_hints(&commands),
        vec![hint(1), hint(2), hint(3)],
        "each tab shows its own number, in the order the strip draws them: {:?}",
        topbar_texts(&commands)
    );

    // And up again: the tabs go back to their names and close controls.
    let _ = key(
        &mut root,
        &app,
        DispatchedEvent::KeyUp {
            key: "Meta".to_string(),
            modifiers: chord_modifier(false),
        },
    );
    assert!(
        !state.borrow().command_held,
        "the release clears the held modifier"
    );
    let commands = frame(&mut root, &app);
    assert!(
        drawn_hints(&commands).is_empty(),
        "the hints are gone once the modifier is up: {:?}",
        topbar_texts(&commands)
    );
}

/// The window losing the keyboard clears the held modifier: Cmd-Tabbing away
/// sends no release for the key that was down, and the hints would otherwise
/// stay painted over a window nobody is typing in.
#[test]
fn losing_the_keyboard_clears_the_held_modifier() {
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

    let _ = key(
        &mut root,
        &app,
        DispatchedEvent::Modifiers {
            modifiers: chord_modifier(true),
        },
    );
    let commands = frame(&mut root, &app);
    assert_eq!(
        drawn_hints(&commands),
        vec![hint(1), hint(2), hint(3)],
        "the hints are up before the focus goes"
    );

    let _ = key(&mut root, &app, DispatchedEvent::Focus { gained: false });
    assert!(
        !state.borrow().command_held,
        "a window without the keyboard holds no chord modifier"
    );
    let commands = frame(&mut root, &app);
    assert!(
        drawn_hints(&commands).is_empty(),
        "the hints go with the focus: {:?}",
        topbar_texts(&commands)
    );
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
