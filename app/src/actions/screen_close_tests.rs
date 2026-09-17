//! Closing a handed-off desktop: the card's dismissal, the session's end and
//! the tab's close, and the rule that keeps a source two panes watch alive
//! until the second pane lets go.

use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::store::Store;
use goble_desktop_service::{CollectingEventBus, DesktopState, ThreadStore};
use goble_screen_core::{
    ClientLiveness, MockCapturer, MockController, ScreenController, ScreenFrame,
};
use goble_ui::platform::WindowControl;

use crate::media::MediaState;
use crate::state::UiState;
use crate::ui::UiActions;
use crate::ui::{Pane, PaneKind, Space};

/// The source id a host-side handoff registers, as `open_remote_screen` derives
/// it from the remote config.
const SOURCE: &str = "remote-xrdp:vm:3389";

fn desktop_state() -> (Arc<DesktopState>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = DesktopState::new(
        Store::open_in_memory().expect("store"),
        ThreadStore::new(dir.path()).expect("thread store"),
    );
    // A source standing in for the desktop the agent asked for: a capturer
    // under the handoff's source id. No live xrdp here — the frame is a mock's.
    state.screen_registry().register_capturer(Arc::new(
        MockCapturer::new(SOURCE).with_frame(ScreenFrame::blank(SOURCE, 4, 4)),
    ));
    (state, dir)
}

fn build(desktop: &Arc<DesktopState>) -> (Rc<RefCell<UiState>>, UiActions) {
    let state = Rc::new(RefCell::new(UiState::from_desktop(desktop)));
    let media = Rc::new(RefCell::new(MediaState::mock()));
    let actions = make_actions(
        Rc::clone(&state),
        Some(Arc::clone(desktop)),
        media,
        WindowControl::default(),
        Rc::new(RefCell::new(1.0)),
    );
    (state, actions)
}

/// Bind `pane_id`'s runtime to the handed-off source, the way the
/// `screen:handoff` drain does for the conversation's pane.
fn show_screen(state: &Rc<RefCell<UiState>>, pane_id: u64, source: &str) {
    state
        .borrow_mut()
        .pane_runtime
        .entry(pane_id)
        .or_default()
        .inline_screen_source = Some(source.to_string());
}

/// Whether the pane's runtime still names the source, i.e. still draws its card.
fn shows_screen(state: &Rc<RefCell<UiState>>, pane_id: u64) -> bool {
    state
        .borrow()
        .pane_runtime
        .get(&pane_id)
        .and_then(|rt| rt.inline_screen_source.clone())
        .is_some()
}

#[test]
fn dismissing_the_card_clears_its_panes_source_and_closes_the_desktop() {
    let (desktop, _dir) = desktop_state();
    let (state, actions) = build(&desktop);
    let pane_id = state.borrow().active_pane_id;
    show_screen(&state, pane_id, SOURCE);
    assert!(desktop.screen_registry().capturer(SOURCE).is_some());

    (actions.on_close_inline_screen.borrow_mut())();

    assert!(
        !shows_screen(&state, pane_id),
        "the dismissed card leaves its pane with no source"
    );
    assert!(
        desktop.screen_registry().capturer(SOURCE).is_none(),
        "the last viewer let go, so the desktop closed"
    );
    assert!(
        !desktop
            .screen_registry()
            .capturer_sources()
            .contains(&SOURCE.to_string()),
        "the closed source left the registry: {:?}",
        desktop.screen_registry().capturer_sources()
    );
    // A dismissed card stays dismissed: a second dismissal closes nothing.
    (actions.on_close_inline_screen.borrow_mut())();
    assert!(!shows_screen(&state, pane_id));
}

/// The rule: a desktop two panes are watching is closed by the **last** viewer,
/// never by the first card dismissed.
#[test]
fn a_second_pane_on_the_same_source_keeps_its_stream() {
    let (desktop, _dir) = desktop_state();
    let (state, actions) = build(&desktop);
    let first = state.borrow().active_pane_id;
    let second = 3;
    show_screen(&state, first, SOURCE);
    show_screen(&state, second, SOURCE);
    assert_eq!(state.borrow().inline_screen_viewers(SOURCE), 2);

    (actions.on_close_inline_screen.borrow_mut())();

    assert!(!shows_screen(&state, first), "the dismissed pane let go");
    assert!(
        shows_screen(&state, second),
        "the other pane keeps its card"
    );
    assert!(
        desktop.screen_registry().capturer(SOURCE).is_some(),
        "a source another conversation still watches stays open"
    );
    // It still streams: the pane the card feeds reads a frame.
    let frame = desktop
        .screen_registry()
        .capture(SOURCE)
        .expect("still capturable");
    assert_eq!(frame.source, SOURCE);

    // The last viewer is what closes it.
    state.borrow_mut().active_pane_id = second;
    (actions.on_close_inline_screen.borrow_mut())();
    assert!(!shows_screen(&state, second));
    assert!(
        desktop.screen_registry().capturer(SOURCE).is_none(),
        "the desktop closes with its last viewer"
    );
}

/// The desktop a host-side handoff leaves behind: its capturer registered and
/// its controller parked under the source in `remote_desktops`, which is what
/// the close path removes along with the frame the capturer feeds. No live xrdp
/// here — the route into a real desktop is unobserved.
fn handed_off_desktop() -> (Arc<DesktopState>, CollectingEventBus, tempfile::TempDir) {
    let (desktop, dir) = desktop_state();
    let bus = CollectingEventBus::new();
    desktop.set_event_bus(Arc::new(bus.clone()));
    desktop.record_open_desktop(
        SOURCE,
        "vm",
        3389,
        Arc::new(MockController::new(SOURCE)) as Arc<dyn ScreenController>,
        ClientLiveness::new(),
    );
    (desktop, bus, dir)
}

/// Add a second tab holding one terminal pane, the way `on_add_space` does, and
/// return that pane's id.
fn add_tab(state: &Rc<RefCell<UiState>>) -> u64 {
    let mut state = state.borrow_mut();
    let id = state.next_pane_id;
    state.next_pane_id += 1;
    state.spaces.push(Space::unnamed(Pane::Leaf {
        id,
        kind: PaneKind::Terminal,
    }));
    state.active_space = state.spaces.len() - 1;
    state.active_pane_id = id;
    id
}

fn closed_events(bus: &CollectingEventBus) -> Vec<serde_json::Value> {
    bus.events()
        .into_iter()
        .filter(|(name, _)| name == "screen:closed")
        .map(|(_, payload)| payload)
        .collect()
}

/// A tab's close is a viewer leaving like any other: the desktop its panes were
/// showing is released, so it closes with the tab rather than staying registered
/// — client up, controller parked — with nobody watching it.
#[test]
fn closing_a_tab_closes_the_desktop_it_was_showing() {
    let (desktop, bus, _dir) = handed_off_desktop();
    let (state, actions) = build(&desktop);
    let shown_in = state.borrow().active_pane_id;
    show_screen(&state, shown_in, SOURCE);
    let other_tab = add_tab(&state);
    assert!(
        desktop.remote_desktop(SOURCE).is_some(),
        "the handed-off desktop is registered before the tab closes"
    );
    assert_eq!(
        state.borrow().spaces.len(),
        2,
        "the tab closed below is not the last one, so it is a plain close"
    );

    (actions.on_close_space.borrow_mut())(0);

    assert!(
        desktop.screen_registry().capturer(SOURCE).is_none(),
        "the closed tab's last viewer let go, so the desktop closed"
    );
    assert!(
        !desktop
            .screen_registry()
            .capturer_sources()
            .contains(&SOURCE.to_string()),
        "the closed source left the registry: {:?}",
        desktop.screen_registry().capturer_sources()
    );
    assert!(
        desktop.remote_desktop(SOURCE).is_none(),
        "the desktop is no longer the host's open one"
    );
    assert!(
        desktop
            .take_screen_control(SOURCE, goble_screen_core::ControlHolder::User)
            .is_err(),
        "the parked controller went with the desktop"
    );
    assert_eq!(
        closed_events(&bus).len(),
        1,
        "the close is announced once: {:?}",
        bus.events()
    );
    assert_eq!(
        closed_events(&bus)[0]
            .get("source")
            .and_then(|v| v.as_str()),
        Some(SOURCE),
        "the close names the source it closed"
    );
    // The tab that did not close is untouched.
    assert!(!shows_screen(&state, other_tab));
    assert_eq!(state.borrow().spaces.len(), 1);
}

/// The rule crosses tabs too: a desktop a second tab still shows survives the
/// first tab's close, and the desktop closes only with the last tab showing it.
#[test]
fn closing_a_tab_leaves_a_desktop_another_tab_still_shows() {
    let (desktop, bus, _dir) = handed_off_desktop();
    let (state, actions) = build(&desktop);
    let first = state.borrow().active_pane_id;
    show_screen(&state, first, SOURCE);
    let second = add_tab(&state);
    show_screen(&state, second, SOURCE);
    assert_eq!(state.borrow().inline_screen_viewers(SOURCE), 2);

    (actions.on_close_space.borrow_mut())(0);

    assert!(
        desktop.screen_registry().capturer(SOURCE).is_some(),
        "a tab another tab still shows stays open"
    );
    assert!(desktop.remote_desktop(SOURCE).is_some());
    assert!(shows_screen(&state, second), "the other tab keeps its card");
    assert!(
        closed_events(&bus).is_empty(),
        "nothing closed, so nothing is announced: {:?}",
        bus.events()
    );
    // It still streams: the tab left showing it reads a frame.
    assert_eq!(
        desktop
            .screen_registry()
            .capture(SOURCE)
            .expect("still capturable")
            .source,
        SOURCE
    );

    // The last tab showing it is what closes it.
    (actions.on_close_space.borrow_mut())(0);

    assert!(desktop.screen_registry().capturer(SOURCE).is_none());
    assert!(desktop.remote_desktop(SOURCE).is_none());
    assert_eq!(closed_events(&bus).len(), 1, "{:?}", bus.events());
}

/// The session's end is the other trigger: the pane that held the conversation
/// goes away, so a desktop only it was showing closes with it.
#[test]
fn closing_the_last_pane_showing_a_desktop_closes_it() {
    let (desktop, _dir) = desktop_state();
    let (state, actions) = build(&desktop);
    let pane_id = state.borrow().active_pane_id;
    show_screen(&state, pane_id, SOURCE);

    (actions.on_close_pane.borrow_mut())();

    assert!(
        !shows_screen(&state, pane_id),
        "the closed pane took its runtime with it"
    );
    assert!(
        desktop.screen_registry().capturer(SOURCE).is_none(),
        "the desktop closed with the session that asked for it"
    );
}

/// A pane close is not a licence to kill another pane's stream: the source
/// survives while a pane that was not closed still shows it.
#[test]
fn closing_a_pane_leaves_a_source_another_pane_still_shows() {
    let (desktop, _dir) = desktop_state();
    let (state, actions) = build(&desktop);
    let first = state.borrow().active_pane_id;
    let second = 3;
    show_screen(&state, first, SOURCE);
    show_screen(&state, second, SOURCE);

    (actions.on_close_pane.borrow_mut())();

    assert!(
        desktop.screen_registry().capturer(SOURCE).is_some(),
        "the other pane still watches it, so the desktop stays open"
    );
    assert!(shows_screen(&state, second));
}
