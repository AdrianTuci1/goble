//! Mounting the root and the state accessors the render tests drive it through.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use goble_desktop_service::{CollectingEventBus, DesktopState};
use goble_ui::elements::{Empty, HoverChipLayer};
use goble_ui::AppContext;

use crate::ai::AiState;
use crate::media::MediaState;
use crate::projects::ProjectsState;
use crate::state::UiState;
use crate::ui::explorer::ExplorerCache;
use crate::ui::file_view::{DiffFileCache, FileCache};
use crate::ui::pickers::PickerCache;

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

        // The restored layout keeps its active tab on screen, and that tab
        // carries the environment it was opened on. The selector and the
        // sidebar filter read the window's selected medium, so point them at
        // the tab before the first frame; otherwise a restored local tab would
        // start on whatever environment the media tree defaults to.
        let space_medium = {
            let s = state.borrow();
            s.spaces.get(s.active_space).map(|space| space.medium.clone())
        };
        if let Some(medium) = space_medium.filter(|medium| !medium.trim().is_empty()) {
            let _ = media_state.borrow_mut().select_medium(&medium);
            let mut s = state.borrow_mut();
            if s.workspace_routing.is_none() {
                s.workspace_routing = Some(crate::ui::WorkspaceRouting::from_routing(
                    crate::media::medium_routing(&medium),
                ));
            }
        }

        let mut view = Self {
            element: Box::new(Empty::new()),
            state,
            ai_state,
            projects_state,
            media_state,
            desktop: Some(desktop.clone()),
            event_bus,
            window_control: app.window_control.clone(),
            ui_zoom: app.ui_zoom.clone(),
            app_context: None,
            pickers: Rc::new(RefCell::new(PickerCache::default())),
            explorer: Rc::new(RefCell::new(ExplorerCache::default())),
            file_cache: Rc::new(RefCell::new(FileCache::default())),
            diff_files: Rc::new(RefCell::new(DiffFileCache::default())),
            actions: None,
            #[cfg(unix)]
            control: None,
            hover_chips: HoverChipLayer::new(),
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

    /// Attach the control socket's server. Its UI verbs are answered by
    /// [`Self::process_control_requests`] on this thread.
    #[cfg(unix)]
    pub fn with_control(mut self, control: crate::control::ControlBridge) -> Self {
        self.control = Some(control);
        self
    }

    /// Expose the backing UI state so integration render tests can drive
    /// first-run flags (e.g. the model-key banner overlay) before mounting.
    #[doc(hidden)]
    pub fn state_rc(&self) -> Rc<RefCell<UiState>> {
        Rc::clone(&self.state)
    }

    /// Expose the backing environment state so integration render tests can
    /// assert which environment is the active/default one (the environment new
    /// spaces start in).
    #[doc(hidden)]
    pub fn media_state_rc(&self) -> Rc<RefCell<MediaState>> {
        Rc::clone(&self.media_state)
    }
}
