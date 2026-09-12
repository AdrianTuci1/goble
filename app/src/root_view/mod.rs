//! Root element of the native UI.
//!
//! Owns only the element tree. The data lives in [`crate::state`], the
//! callbacks live in [`crate::actions`], and runtime orchestration (where a
//! turn executes) lives in [`crate::runtime`].
//!
//! One module per surface of the root: the mount and the accessors the render
//! tests drive it through (`construct`), the backend-event drain (`events`), the
//! per-frame rebuild (`rebuild`), the live theme (`theme`) and the [`Element`]
//! impl (`element`). What the surfaces share — the root's own element and the
//! app state it renders — stays here.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::{CollectingEventBus, DesktopState};
use goble_ui::elements::Element;
use goble_ui::platform::WindowControl;
use goble_ui::{AppContext, Point, Vector2F};

use crate::ai::AiState;
use crate::media::MediaState;
use crate::projects::ProjectsState;
use crate::screen::ScreenState;
use crate::state::UiState;
use crate::ui::UiActions;

mod construct;
mod element;
mod events;
mod rebuild;
mod theme;

#[cfg(test)]
mod tests;

/// Root element that renders the app UI and drives the event loop.
pub struct RootView {
    element: Box<dyn Element>,
    state: Rc<RefCell<UiState>>,
    ai_state: Rc<RefCell<AiState>>,
    projects_state: Rc<RefCell<ProjectsState>>,
    media_state: Rc<RefCell<MediaState>>,
    screen_state: Rc<RefCell<ScreenState>>,
    desktop: Option<Arc<DesktopState>>,
    event_bus: Option<CollectingEventBus>,
    /// Handle used to request window-level changes (e.g. toggling fullscreen).
    /// Cloned from `AppContext`, so the handler installed by the platform event
    /// loop (once the window exists) is visible to the actions built each frame.
    window_control: WindowControl,
    /// The whole-app zoom shared with `AppContext.ui_zoom`. Kept here so the
    /// settings "font size" action can drive the same value the keyboard and
    /// menubar zoom use.
    ui_zoom: Rc<RefCell<f32>>,
    /// The shared `AppContext` so the live theme (from the color wheel) can be
    /// written back each frame. `None` in tests/render harnesses, which skip
    /// theme application.
    app_context: Option<Rc<RefCell<AppContext>>>,
    /// The callbacks built on the last rebuild; kept here so the root can
    /// dispatch global keyboard shortcuts (split/space) before the tree does.
    actions: Option<UiActions>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}
