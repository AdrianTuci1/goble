//! The finished-turn announcement at the top of the window: what raises it,
//! and the frame clock that runs its deadline out.

use super::*;

use goble_ui::elements::{LayoutContext, SizeConstraint};
use goble_ui::geometry::vec2f;

use crate::state::InboxKind;

/// A banner is timed, so it needs frames: the clock stays at the active rate
/// while one is out, and lets go once it is gone.
#[test]
fn an_announcement_keeps_the_frame_clock_running_until_it_is_gone() {
    let (view, _bus, _dir) = root_with_bus();
    let state = view.state_rc();
    assert!(
        !view.wants_animation(),
        "a window with nothing in flight idles"
    );

    state
        .borrow_mut()
        .raise_inbox(1, "c1", InboxKind::Done, "Turn finished", "2 files");
    assert!(
        view.wants_animation(),
        "the banner's deadline is only reached on a frame"
    );

    state.borrow_mut().dismiss_announcement();
    assert!(
        !view.wants_animation(),
        "a dismissed banner no longer asks for frames"
    );
}

/// The deadline is the frame's own work: a frame after it puts the banner away
/// whether or not anything asked for it to be dismissed.
#[test]
fn a_frame_past_the_deadline_puts_the_announcement_away() {
    let (view, _bus, _dir) = root_with_bus();
    let state = view.state_rc();
    state.borrow_mut().raise_inbox(
        1,
        "c1",
        InboxKind::Error,
        "Turn failed",
        "the model refused",
    );
    state.borrow_mut().announcement_until = Some(std::time::Instant::now());
    assert!(state.borrow().announcement.is_some());

    let app = AppContext::default();
    let mut element: Box<dyn Element> = Box::new(view);
    let _ = element.layout(
        SizeConstraint::loose(vec2f(1024.0, 768.0)),
        &mut LayoutContext::default(),
        &app,
    );
    assert!(
        state.borrow().announcement.is_none(),
        "the frame ran the banner's deadline out"
    );
    assert!(
        state.borrow().inbox.len() == 1,
        "the report itself stays in the inbox"
    );
}
