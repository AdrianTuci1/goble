//! Root-view acceptance cases: the app's own event drain, the topbar cue and
//! the sub-agent rows, one module per surface.
//!
//! [`drain`] covers the live-event path, [`sub_agent_events`] the parent's
//! transcript row, [`topbar`] the live count cue, [`child_view`] entering a
//! child's conversation and [`nested_child`] a child entered from inside a
//! child view. The fixtures every case shares live here.

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
mod nested_child;
mod sub_agent_events;
mod topbar;
