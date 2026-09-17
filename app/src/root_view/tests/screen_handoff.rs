//! C4: an agent's `screen:handoff` lands as the card in the conversation and
//! leaves the screen sheet — the manual broadcast/computer-use surface — exactly
//! as the user had it, a clicked screen link still opens the sheet and keeps the
//! selection a handoff in the same frame would take (U2), and the card names the
//! desktop and the holder driving it.

use super::*;
use goble_screen_core::{ControlHolder, MockCapturer, MockController, ScreenFrame};

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

fn pane_source(root: &RootView, pane_id: u64) -> Option<String> {
    root.state_rc()
        .borrow()
        .pane_runtime
        .get(&pane_id)
        .and_then(|rt| rt.inline_screen_source.clone())
}

/// Put pane 1 on a conversation and hand it a mock desktop, the way the
/// translator's `screen:handoff` does.
fn hand_off(root: &mut RootView, bus: &CollectingEventBus, desktop: &DesktopState) {
    desktop.screen_registry().register_capturer(Arc::new(
        MockCapturer::new(SOURCE).with_frame(ScreenFrame::blank(SOURCE, 4, 4)),
    ));
    {
        let state = root.state_rc();
        let mut state = state.borrow_mut();
        state
            .pane_sessions
            .get_mut(&1)
            .expect("the default pane session exists")
            .conversation_id = "conv-1".to_string();
        // The pane shows its conversation, so the card is drawn in its
        // transcript.
        state.enter_agent_view(1, "conv-1", "handoff");
    }
    bus.emit(
        "screen:handoff",
        serde_json::json!({ "chat_id": "conv-1", "source": SOURCE }),
    );
    root.drain_events();
}

/// The text one frame of the mounted root draws inside the pane's area.
fn pane_texts(root: &mut Box<dyn Element>) -> Vec<String> {
    use goble_ui::geometry::vec2f;
    use goble_ui::render::RenderCommand;
    use goble_ui::test_util::render_element;

    render_element(root, vec2f(1024.0, 768.0), &AppContext::default())
        .iter()
        .filter_map(|c| match c {
            RenderCommand::DrawText { origin, text, .. }
                if origin.x >= crate::ui::SIDEBAR_WIDTH =>
            {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_handoff_lands_on_the_card_and_leaves_the_sheet_alone() {
    let (mut root, bus, desktop, _dir) = root_with_backend();
    hand_off(&mut root, &bus, &desktop);

    let screen = root.screen_state_rc();
    let screen = screen.borrow();
    assert!(
        !screen.open,
        "an agent's handoff does not throw the sheet open: the card is the surface"
    );
    assert_eq!(
        screen.selected_source, SOURCE,
        "the handoff's source is the one the app holds frames for"
    );
    drop(screen);
    assert_eq!(
        pane_source(&root, 1).as_deref(),
        Some(SOURCE),
        "the conversation shows the handed-off desktop as its card"
    );

    // A sheet the user opened is theirs: a later handoff neither opens nor
    // closes it.
    root.screen_state_rc().borrow_mut().open = true;
    bus.emit(
        "screen:handoff",
        serde_json::json!({ "chat_id": "conv-1", "source": SOURCE }),
    );
    root.drain_events();
    assert!(
        root.screen_state_rc().borrow().open,
        "a handoff leaves the sheet exactly as the user had it"
    );
}

#[test]
fn a_clicked_screen_link_still_opens_the_sheet() {
    let (mut root, _bus, desktop, _dir) = root_with_backend();
    desktop.screen_registry().register_capturer(Arc::new(
        MockCapturer::new("vm:3389").with_frame(ScreenFrame::blank("vm:3389", 4, 4)),
    ));

    // The transcript's link card stashes the URI; the root performs the open.
    root.state_rc().borrow_mut().pending_screen_link_open = Some("rdp://vm:3389".to_string());
    root.drain_events();

    let screen = root.screen_state_rc();
    let screen = screen.borrow();
    assert!(
        screen.open,
        "a clicked screen link opens the sheet: it is the user's own click"
    );
    assert_eq!(
        screen.selected_source, "vm:3389",
        "and selects the source the URI names"
    );
}

/// A clicked BYOH link and an agent's `screen:handoff` that land in one drain:
/// the user's own click is the source the frame ends on, and the handoff still
/// lands as the card in the conversation.
#[test]
fn a_handoff_in_the_same_frame_does_not_steal_a_clicked_link() {
    let (mut root, bus, desktop, _dir) = root_with_backend();
    const CLICKED: &str = "vm:3389";
    for source in [CLICKED, SOURCE] {
        desktop.screen_registry().register_capturer(Arc::new(
            MockCapturer::new(source).with_frame(ScreenFrame::blank(source, 4, 4)),
        ));
    }

    // The transcript's link card stashes the clicked URI and the agent's tool
    // call becomes a `screen:handoff` on the bus; the one drain below carries
    // both, so they are the same frame's work.
    root.state_rc().borrow_mut().pending_screen_link_open = Some("rdp://vm:3389".to_string());
    bus.emit(
        "screen:handoff",
        serde_json::json!({ "chat_id": "conv-1", "source": SOURCE }),
    );
    root.drain_events();

    let screen = root.screen_state_rc();
    let screen = screen.borrow();
    assert!(
        screen.open,
        "the click still opens the sheet: it is the user's own click"
    );
    assert_eq!(
        screen.selected_source, CLICKED,
        "the source the user clicked is the one the frame ends on"
    );
    assert!(
        screen.sources.iter().any(|s| s.source == SOURCE),
        "the handoff's source is still listed: {:?}",
        screen.sources
    );
    drop(screen);
    assert_eq!(
        pane_source(&root, 1).as_deref(),
        Some(SOURCE),
        "and the handoff still lands as the card in the conversation"
    );
}

/// The companion of the case above: with no click of the user's own in the
/// frame, a handoff still moves the selection to its own source, even when the
/// list carries another desktop.
#[test]
fn a_handoff_alone_still_selects_its_own_source() {
    let (mut root, bus, desktop, _dir) = root_with_backend();
    desktop.screen_registry().register_capturer(Arc::new(
        MockCapturer::new("vm:3389").with_frame(ScreenFrame::blank("vm:3389", 4, 4)),
    ));
    hand_off(&mut root, &bus, &desktop);

    assert_eq!(
        root.screen_state_rc().borrow().selected_source,
        SOURCE,
        "a handoff with no competing click selects its own source"
    );
}

#[test]
fn the_card_names_the_desktop_and_who_is_driving_it() {
    let (mut root, bus, desktop, _dir) = root_with_backend();
    hand_off(&mut root, &bus, &desktop);
    let screen = root.screen_state_rc();
    let mut root: Box<dyn Element> = Box::new(root);

    // A handed-off desktop is view-only until someone takes it, and the card
    // names it: the host the source runs on, and who holds its input.
    let drawn = pane_texts(&mut root);
    assert!(
        drawn.iter().any(|t| t == "vm:3389 · view only"),
        "a fresh handoff is named and shown as view-only: {drawn:?}"
    );

    // The registry is the card's source of truth, one writer at a time.
    let ctl: Arc<dyn goble_screen_core::ScreenController> = Arc::new(MockController::new(SOURCE));
    desktop
        .screen_registry()
        .take_control(SOURCE, ControlHolder::User, Arc::clone(&ctl))
        .expect("the user takes the desktop");
    let drawn = pane_texts(&mut root);
    assert!(
        drawn.iter().any(|t| t == "vm:3389 · you are driving"),
        "the card says the user is driving: {drawn:?}"
    );

    desktop
        .screen_registry()
        .release_control(SOURCE, ControlHolder::User)
        .expect("the user releases");
    desktop
        .screen_registry()
        .take_control(SOURCE, ControlHolder::Agent, ctl)
        .expect("the agent takes the released desktop");
    let drawn = pane_texts(&mut root);
    assert!(
        drawn.iter().any(|t| t == "vm:3389 · the agent is driving"),
        "the card says the agent is driving: {drawn:?}"
    );

    assert!(
        !screen.borrow().open,
        "every one of those states was drawn with the sheet closed"
    );
}
