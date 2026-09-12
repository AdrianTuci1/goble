//! Mounting the root and the state accessors the render tests drive it through.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::{CollectingEventBus, DesktopState};
use goble_ui::elements::Empty;
use goble_ui::AppContext;

use crate::ai::AiState;
use crate::media::MediaState;
use crate::projects::ProjectsState;
use crate::screen::ScreenState;
use crate::state::UiState;

use super::RootView;

impl RootView {
    pub fn new(
        app: &AppContext,
        desktop: &Arc<DesktopState>,
        event_bus: Option<CollectingEventBus>,
    ) -> Self {
        // The app always runs on the real backend store; there is no mock
        // fallback when a store is unavailable (see `main`).
        let state = Rc::new(RefCell::new(UiState::from_desktop(desktop)));
        let ai_state = Rc::new(RefCell::new(AiState::from_desktop(desktop)));
        let projects_state = Rc::new(RefCell::new(ProjectsState::from_desktop(desktop)));
        let media_state = Rc::new(RefCell::new(MediaState::from_desktop(desktop)));
        let screen_state = Rc::new(RefCell::new(ScreenState::from_desktop(desktop)));

        let mut view = Self {
            element: Box::new(Empty::new()),
            state,
            ai_state,
            projects_state,
            media_state,
            screen_state,
            desktop: Some(desktop.clone()),
            event_bus,
            window_control: app.window_control.clone(),
            ui_zoom: app.ui_zoom.clone(),
            app_context: None,
            actions: None,
            size: None,
            origin: None,
        };
        view.rebuild(app);
        view
    }

    /// Provide the shared `AppContext` so the live theme can be applied each
    /// frame. Called by `main`; skipped by test/render harnesses.
    pub fn with_app_context(mut self, app_context: Rc<RefCell<AppContext>>) -> Self {
        self.app_context = Some(app_context);
        self
    }

    /// Expose the backing UI state so integration render tests can drive
    /// first-run flags (e.g. the model-key banner overlay) before mounting.
    #[doc(hidden)]
    pub fn state_rc(&self) -> Rc<RefCell<UiState>> {
        Rc::clone(&self.state)
    }

    /// Expose the backing screen state so integration render tests can open the
    /// screen sheet (broadcast + computer-use) before mounting.
    #[doc(hidden)]
    pub fn screen_state_rc(&self) -> Rc<RefCell<ScreenState>> {
        Rc::clone(&self.screen_state)
    }
}
