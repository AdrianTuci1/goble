//! Root-view acceptance cases: the app's own event drain, the topbar cue and
//! the sub-agent rows, one module per surface.
//!
//! [`drain`] covers the live-event path, [`sub_agent_events`] the parent's
//! transcript row, [`topbar`] the live count cue, [`child_view`] entering a
//! child's conversation, [`nested_child`] a child entered from inside a
//! child view, [`return_to_shell`] the pane a conversation switch or an agent
//! delete sends back to its shell (U1), [`screen_close`] the close of a
//! handed-off desktop and [`screen_handoff`] the handoff itself (the card, and
//! the sheet it must not open), and [`worker_pane`] the viewer pane a
//! remote-routed conversation gets and its re-attach to the session it runs on.
//! The fixtures every case shares live here.

use goble_ui::event::DispatchedEvent;
use goble_ui::EventContext;

    use super::*;
    use goble_core::store::Store;
    use goble_desktop_service::{EventBus, ThreadStore};

    /// Mount a real `RootView` over an in-memory store and a collecting bus, so
    /// the scripted events go through the real [`RootView::drain_events`] path
    /// rather than the state setters.
    fn root_with_bus() -> (RootView, CollectingEventBus, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp thread-store dir");
        let desktop = Arc::new(DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        ));
        let bus = CollectingEventBus::new();
        let view = RootView::new(&AppContext::default(), &desktop, Some(bus.clone()));
        (view, bus, dir)
    }

mod child_view;
mod drain;
mod environment;
mod filter_chord;
mod hover_chip;
mod nested_child;
mod return_to_shell;
mod screen_close;
mod screen_handoff;
mod sub_agent_events;
mod topbar;
mod worker_pane;
