//! Integration tests for the screen domain: rendering the broadcast +
//! computer-use panel and verifying the broadcast/computer-use toggles are
//! wired through the real action closures and the [`RootView`].

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_app::root_view::RootView;
use goble_app::screen::{make_screen_actions, ScreenState};
use goble_app::ui::screen::build_screen_sheet;
use goble_app::ui::{ScreenFrameSnapshot, ScreenSnapshot, ScreenSourceEntry};
use goble_screen_core::{MockAction, MockCapturer, MockController, ScreenFrame};
use goble_ui::elements::AppContext;
use goble_ui::render::RenderCommand;
use goble_ui::test_util::{command_counts, render_element};
use goble_ui::{vec2f, Element};

/// Collect the human-readable text of every `DrawText` command.
fn draw_texts(commands: &[RenderCommand]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// A snapshot exposing a single local source, both toggles off.
fn screen_snapshot() -> ScreenSnapshot {
    ScreenSnapshot {
        open: true,
        broadcast: false,
        computer_use: false,
        selected_source: "local".to_string(),
        sources: vec![ScreenSourceEntry {
            source: "local".to_string(),
            capturable: true,
            controllable: true,
        }],
        last_capture: None,
        recording: false,
        replaying: false,
        replay_loop: false,
        recorded_count: 0,
        replay_status: None,
        frame: None,
    }
}

#[test]
fn screen_panel_renders_toggles_and_sources() {
    let app = AppContext::default();
    let snapshot = screen_snapshot();
    let actions = make_screen_actions(Rc::new(RefCell::new(ScreenState::mock())), None);

    let mut element = build_screen_sheet(&app, &snapshot, &actions);
    let commands = render_element(&mut element, vec2f(800.0, 600.0), &app);
    let counts = command_counts(&commands);
    assert!(counts.fill_rect > 0, "panel should paint backgrounds");

    let texts = draw_texts(&commands);
    assert!(
        texts.iter().any(|t| t == "Screen"),
        "should render the panel title, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("Live broadcast")),
        "should render the broadcast toggle label, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("Computer use")),
        "should render the computer-use toggle label, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("local")),
        "should list the capturable source, got {texts:?}"
    );
}

#[test]
fn screen_state_from_desktop_has_local_source() {
    let (desktop, _dir) = common::desktop_state();
    let state = ScreenState::from_desktop(&desktop);
    assert!(
        state.sources.iter().any(|s| s.source == "local"),
        "the seeded registry exposes the local source, got {:?}",
        state.sources
    );
    assert_eq!(state.selected_source, "local");
    assert!(!state.broadcast);
    assert!(!state.computer_use);
}

#[test]
fn broadcast_and_computer_use_toggles_are_wired() {
    let state = Rc::new(RefCell::new(ScreenState::mock()));
    let actions = make_screen_actions(Rc::clone(&state), None);

    assert!(!state.borrow().broadcast);
    assert!(!state.borrow().computer_use);

    (actions.on_toggle_broadcast.borrow_mut())(true);
    assert!(
        state.borrow().broadcast,
        "broadcast toggle should enable live capture"
    );
    assert!(
        state.borrow().last_capture.is_some(),
        "starting broadcast should record a status"
    );

    (actions.on_toggle_computer_use.borrow_mut())(true);
    assert!(
        state.borrow().computer_use,
        "computer-use toggle should enable click/type/scroll"
    );

    // Toggling off clears broadcast status.
    (actions.on_toggle_broadcast.borrow_mut())(false);
    assert!(!state.borrow().broadcast);
    assert_eq!(state.borrow().last_capture, None);
}

#[test]
fn open_close_and_select_source_actions_are_wired() {
    let state = Rc::new(RefCell::new(ScreenState::mock()));
    let actions = make_screen_actions(Rc::clone(&state), None);

    (actions.on_open.borrow_mut())();
    assert!(state.borrow().open, "open action shows the sheet");
    (actions.on_close.borrow_mut())();
    assert!(!state.borrow().open, "close action hides the sheet");

    (actions.on_select_source.borrow_mut())(String::new());
    assert_eq!(
        state.borrow().selected_source,
        "local",
        "selecting an unknown source is ignored"
    );
    assert!(
        state.borrow().sources.iter().any(|s| s.source == "local"),
        "mock exposes the local source"
    );
}

#[test]
fn screen_sheet_renders_through_root_view() {
    let app = AppContext::default();
    let view = RootView::new(&app, None, None);
    view.screen_state_rc().borrow_mut().open = true;

    let mut root: Box<dyn Element> = Box::new(view);
    let commands = render_element(&mut root, vec2f(1024.0, 768.0), &app);
    let texts = draw_texts(&commands);
    assert!(
        texts.iter().any(|t| t == "Screen"),
        "the screen sheet should render its header, got {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("Live broadcast")),
        "the screen sheet should render the broadcast toggle, got {texts:?}"
    );
}

/// A `DesktopState` whose local source is overridden with a deterministic
/// capturer (or controller) so tests never hit a live screen backend.
fn desktop_with_mock_capturer(
    width: u32,
    height: u32,
    fill: u8,
) -> (Arc<goble_desktop_service::DesktopState>, tempfile::TempDir) {
    let (desktop, dir) = common::desktop_state();
    let frame = ScreenFrame::new("local", width, height, vec![fill; (width * height * 4) as usize]);
    desktop
        .screen_registry()
        .register_capturer(Arc::new(MockCapturer::new("local").with_frame(frame)));
    (desktop, dir)
}

#[test]
fn captured_frame_renders_into_frame_view_shape() {
    let app = AppContext::default();
    let (desktop, _dir) = desktop_with_mock_capturer(2, 3, 9);
    let mut state = ScreenState::from_desktop(&desktop);
    state.open = true;
    state.toggle_broadcast(true, Some(&desktop));
    // The registered source is listed and its capture produced a held frame.
    assert!(
        state.sources.iter().any(|s| s.source == "local"),
        "the registered source should be listed, got {:?}",
        state.sources
    );
    let frame = state.frame.as_ref().expect("broadcast should hold a frame");
    assert_eq!((frame.width, frame.height), (2, 3));

    let snapshot = ScreenSnapshot {
        open: true,
        broadcast: true,
        computer_use: false,
        selected_source: "local".to_string(),
        sources: vec![ScreenSourceEntry {
            source: "local".to_string(),
            capturable: true,
            controllable: true,
        }],
        last_capture: state.last_capture.clone(),
        recording: false,
        replaying: false,
        replay_loop: false,
        recorded_count: 0,
        replay_status: None,
        frame: Some(ScreenFrameSnapshot {
            frame_seq: state.frame_seq,
            width: frame.width,
            height: frame.height,
            data: Arc::from(frame.data.clone()),
        }),
    };

    let actions = make_screen_actions(Rc::new(RefCell::new(ScreenState::mock())), None);
    let mut element = build_screen_sheet(&app, &snapshot, &actions);
    let commands = render_element(&mut element, vec2f(800.0, 600.0), &app);
    let counts = command_counts(&commands);
    assert_eq!(
        counts.draw_image,
        1,
        "the held frame should render through FrameView (one DrawImage)"
    );
}

#[test]
fn input_routes_to_controller_when_computer_use_on() {
    let (desktop, _dir) = desktop_with_mock_capturer(1, 1, 0);
    let ctl = Arc::new(MockController::new("local"));
    desktop.screen_registry().register_controller(ctl.clone());

    let mut state = ScreenState::from_desktop(&desktop);
    state.toggle_computer_use(true);
    state.click(10, 20, Some(&desktop));
    state.type_text("hi", Some(&desktop));
    state.scroll(-1, 2, Some(&desktop));

    assert_eq!(
        ctl.actions(),
        vec![
            MockAction::Click { x: 10, y: 20 },
            MockAction::TypeText { text: "hi".into() },
            MockAction::Scroll { dx: -1, dy: 2 },
        ]
    );
}

#[test]
fn input_is_ignored_when_computer_use_off() {
    let (desktop, _dir) = desktop_with_mock_capturer(1, 1, 0);
    let ctl = Arc::new(MockController::new("local"));
    desktop.screen_registry().register_controller(ctl.clone());

    let mut state = ScreenState::from_desktop(&desktop);
    assert!(!state.computer_use);
    state.click(10, 20, Some(&desktop));
    state.type_text("hi", Some(&desktop));
    state.scroll(0, -1, Some(&desktop));
    assert!(
        ctl.actions().is_empty(),
        "no input should reach the controller when computer-use is off"
    );
}
