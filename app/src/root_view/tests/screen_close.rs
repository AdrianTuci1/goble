//! Closing a handed-off desktop through the app's own drain: the `screen:closed`
//! the host emits leaves no pane, no source-list row and no held frame behind.

use super::*;
use goble_screen_core::{MockCapturer, ScreenFrame};

/// The source id a host-side handoff registers.
const SOURCE: &str = "remote-xrdp:vm:3389";

/// Like [`root_with_bus`], handing the backend back too, so a case can seed the
/// screen registry the root reads (no live xrdp host exists here).
fn root_with_backend() -> (
    RootView,
    CollectingEventBus,
    Arc<DesktopState>,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().expect("temp thread-store dir");
    let desktop = DesktopState::new(
        Store::open_in_memory().expect("in-memory store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    );
    let bus = CollectingEventBus::new();
    let view = RootView::new(&AppContext::default(), &desktop, Some(bus.clone()));
    (view, bus, desktop, dir)
}

/// Land the handoff on the conversation's pane, the way the translator's
/// `screen:handoff` does, and hold a live frame for the source.
fn hand_off(root: &mut RootView, bus: &CollectingEventBus, desktop: &DesktopState, pane_id: u64) {
    root.state_rc()
        .borrow_mut()
        .pane_sessions
        .get_mut(&pane_id)
        .expect("the default pane session exists")
        .conversation_id = "conv-1".to_string();
    bus.emit(
        "screen:handoff",
        serde_json::json!({ "chat_id": "conv-1", "source": SOURCE }),
    );
    root.drain_events();
    // The sheet open and broadcasting is what holds a live frame; `tick` is the
    // path the per-frame rebuild takes.
    let screen = root.screen_state_rc();
    let mut screen = screen.borrow_mut();
    screen.open = true;
    screen.broadcast = true;
    screen.tick(Some(desktop), false);
}

fn pane_source(root: &RootView, pane_id: u64) -> Option<String> {
    root.state_rc()
        .borrow()
        .pane_runtime
        .get(&pane_id)
        .and_then(|rt| rt.inline_screen_source.clone())
}

#[test]
fn a_closed_desktop_leaves_the_pane_the_source_list_and_the_held_frame() {
    let (mut root, bus, desktop, _dir) = root_with_backend();
    desktop.screen_registry().register_capturer(Arc::new(
        MockCapturer::new(SOURCE).with_frame(ScreenFrame::blank(SOURCE, 4, 4)),
    ));
    hand_off(&mut root, &bus, &desktop, 1);
    assert_eq!(pane_source(&root, 1).as_deref(), Some(SOURCE));
    assert_eq!(root.screen_state_rc().borrow().selected_source, SOURCE);
    assert_eq!(
        root.screen_state_rc()
            .borrow()
            .frame
            .as_ref()
            .map(|f| f.source.clone()),
        Some(SOURCE.to_string()),
        "the handoff's frame is held while the desktop is open"
    );

    // The close itself is the app's action (`close_remote_screen`), which
    // removes the registry entry and announces it; the drain is what the app
    // does with the announcement.
    assert!(desktop.close_remote_screen(SOURCE));
    bus.emit("screen:closed", serde_json::json!({ "source": SOURCE }));
    root.drain_events();

    assert_eq!(
        pane_source(&root, 1),
        None,
        "the pane stops drawing the closed desktop"
    );
    let screen = root.screen_state_rc();
    let screen = screen.borrow();
    assert!(
        screen.frame.is_none(),
        "the frame held for the closed desktop is dropped"
    );
    assert!(
        !screen.sources.iter().any(|s| s.source == SOURCE),
        "the closed source left the list: {:?}",
        screen.sources
    );
    assert!(
        desktop.screen_registry().capturer(SOURCE).is_none(),
        "the closed desktop is gone from the registry"
    );
}

/// A desktop closed while another pane still named it leaves no card behind
/// there either.
#[test]
fn a_closed_desktop_leaves_no_card_in_another_pane_that_named_it() {
    let (mut root, bus, desktop, _dir) = root_with_backend();
    desktop.screen_registry().register_capturer(Arc::new(
        MockCapturer::new(SOURCE).with_frame(ScreenFrame::blank(SOURCE, 4, 4)),
    ));
    hand_off(&mut root, &bus, &desktop, 1);
    root.state_rc()
        .borrow_mut()
        .pane_runtime
        .entry(3)
        .or_default()
        .inline_screen_source = Some(SOURCE.to_string());

    bus.emit("screen:closed", serde_json::json!({ "source": SOURCE }));
    root.drain_events();

    assert_eq!(pane_source(&root, 1), None);
    assert_eq!(
        pane_source(&root, 3),
        None,
        "no pane keeps a card for a desktop that is gone"
    );
}
