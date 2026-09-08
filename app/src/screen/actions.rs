//! Callback wiring for the screen domain: turns app state + backend into
//! [`crate::ui::ScreenActions`] closures.
//!
//! Broadcast toggling drives a best-effort grab through the desktop service's
//! [`goble_screen_core::ScreenRegistry`]; the source list is reloaded from the
//! same registry. The recorder/replayer live in [`super::state::ScreenState`].

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::DesktopState;

use crate::ui::ScreenActions;

use super::state::ScreenState;

pub fn make_screen_actions(
    state: Rc<RefCell<ScreenState>>,
    desktop: Option<Arc<DesktopState>>,
) -> ScreenActions {
    let on_open = Rc::clone(&state);
    let on_close = Rc::clone(&state);
    let on_toggle_broadcast = Rc::clone(&state);
    let on_toggle_computer_use = Rc::clone(&state);
    let on_select_source = Rc::clone(&state);
    let on_refresh_sources = Rc::clone(&state);
    let on_record_start = Rc::clone(&state);
    let on_record_stop = Rc::clone(&state);
    let on_replay_once = Rc::clone(&state);
    let on_replay_loop = Rc::clone(&state);
    let on_replay_stop = Rc::clone(&state);
    let on_clear_recording = Rc::clone(&state);
    let on_screen_click = Rc::clone(&state);
    let on_screen_type = Rc::clone(&state);
    let on_screen_scroll = Rc::clone(&state);

    let desktop_broadcast = desktop.clone();
    let desktop_refresh = desktop.clone();
    let desktop_replay_once = desktop.clone();
    let desktop_replay_loop = desktop.clone();
    let desktop_click = desktop.clone();
    let desktop_type = desktop.clone();
    let desktop_scroll = desktop.clone();

    ScreenActions {
        on_open: Rc::new(RefCell::new(move || {
            on_open.borrow_mut().open = true;
        })),
        on_close: Rc::new(RefCell::new(move || {
            on_close.borrow_mut().open = false;
        })),
        on_toggle_broadcast: Rc::new(RefCell::new(move |enabled: bool| {
            let desktop = desktop_broadcast.clone();
            on_toggle_broadcast
                .borrow_mut()
                .toggle_broadcast(enabled, desktop.as_deref());
        })),
        on_toggle_computer_use: Rc::new(RefCell::new(move |enabled: bool| {
            on_toggle_computer_use
                .borrow_mut()
                .toggle_computer_use(enabled);
        })),
        on_select_source: Rc::new(RefCell::new(move |source: String| {
            on_select_source.borrow_mut().select_source(&source);
        })),
        on_refresh_sources: Rc::new(RefCell::new(move || {
            if let Some(desktop) = &desktop_refresh {
                on_refresh_sources.borrow_mut().refresh_sources(desktop);
            }
        })),
        on_record_start: Rc::new(RefCell::new(move || {
            on_record_start.borrow_mut().start_recording();
        })),
        on_record_stop: Rc::new(RefCell::new(move || {
            on_record_stop.borrow_mut().stop_recording();
        })),
        // Replay once / loop. The desktop handle is only used to dispatch input
        // events through the registry when their timing is reached.
        on_replay_once: Rc::new(RefCell::new(move || {
            let desktop = desktop_replay_once.clone();
            on_replay_once.borrow_mut().start_replay(false);
            if let Some(desktop) = desktop {
                // Immediately run the first frame so a zero-offset event shows.
                on_replay_once.borrow_mut().step_replay(0, Some(&desktop));
            }
        })),
        on_replay_loop: Rc::new(RefCell::new(move || {
            let desktop = desktop_replay_loop.clone();
            on_replay_loop.borrow_mut().start_replay(true);
            if let Some(desktop) = desktop {
                on_replay_loop.borrow_mut().step_replay(0, Some(&desktop));
            }
        })),
        on_replay_stop: Rc::new(RefCell::new(move || {
            on_replay_stop.borrow_mut().stop_replay();
        })),
        on_clear_recording: Rc::new(RefCell::new(move || {
            let mut state = on_clear_recording.borrow_mut();
            state.stop_replay();
            state.recorded.clear();
            state.replay_index = 0;
            state.replay_elapsed_ms = 0;
            state.replay_status = Some("recording cleared".to_string());
        })),
        on_screen_click: Rc::new(RefCell::new(move |x: u32, y: u32| {
            let desktop = desktop_click.clone();
            on_screen_click.borrow_mut().click(x, y, desktop.as_deref());
        })),
        on_screen_type: Rc::new(RefCell::new(move |text: String| {
            let desktop = desktop_type.clone();
            on_screen_type
                .borrow_mut()
                .type_text(&text, desktop.as_deref());
        })),
        on_screen_scroll: Rc::new(RefCell::new(move |dx: i32, dy: i32| {
            let desktop = desktop_scroll.clone();
            on_screen_scroll
                .borrow_mut()
                .scroll(dx, dy, desktop.as_deref());
        })),
    }
}
