//! First-run onboarding and the app-wide approval policy it shares a home with.
//!
//! The onboarding is three steps — usage, friendly name, telemetry consent —
//! and it writes the same store keys the Profile settings page edits. Reaching
//! the flow again is a dev build concern: `GOBLE_ONBOARDING=1` opens it at
//! startup and `/onboarding` opens it while the app runs.

use super::*;

/// The three onboarding steps, in the order the progress dashes draw them.
pub const ONBOARDING_STEPS: usize = 3;

/// The usage cards the first step offers. The labels are also what the store
/// keeps, so the answer survives a restart and can be reported as a category.
pub const ONBOARDING_USAGE: [(&str, &str); 3] = [
    ("work", "Work"),
    ("personal", "Personal"),
    ("learning", "Learning"),
];

impl UiState {
    /// Copy an approval mode into the window and every live pane. Persisting is
    /// the caller's job; this is the in-memory half.
    pub fn apply_approval_mode(&mut self, mode: ApprovalMode) {
        self.approval_mode = mode;
        // The legacy per-pane switch and the shared setting read this flag, so
        // keep them on the same answer as the mode.
        self.auto_approve = matches!(mode, ApprovalMode::AlwaysApprove);
        for controls in self.pane_controls.values_mut() {
            controls.approval_mode = mode;
            controls.auto_approve = self.auto_approve;
        }
    }

    /// Set the app-wide approval mode and persist it, so the policy is the same
    /// in every pane and on the next launch.
    pub fn set_approval_mode(&mut self, mode: ApprovalMode, desktop: Option<&DesktopState>) {
        self.apply_approval_mode(mode);
        if let Some(desktop) = desktop {
            if let Err(error) = desktop.set_approval_mode(mode.label()) {
                log::warn!("set_approval_mode failed: {error}");
            }
        }
    }

    /// Whether the current onboarding step has an answer worth advancing with.
    pub fn onboarding_can_advance(&self) -> bool {
        match self.onboarding_step {
            0 => !self.onboarding_usage.trim().is_empty(),
            1 => !self.onboarding_name.trim().is_empty(),
            _ => true,
        }
    }

    pub fn onboarding_is_last_step(&self) -> bool {
        self.onboarding_step + 1 >= ONBOARDING_STEPS
    }

    /// Advance one step. The final step is finished by the action layer, which
    /// owns the store handle; this only guards the steps before it.
    pub fn onboarding_next_step(&mut self) {
        if self.onboarding_is_last_step() || !self.onboarding_can_advance() {
            return;
        }
        self.onboarding_step += 1;
    }

    pub fn onboarding_back(&mut self) {
        self.onboarding_step = self.onboarding_step.saturating_sub(1);
    }

    /// Finish the flow: keep the answers, apply the consent, and mark first run
    /// done so the overlay does not come back on its own.
    pub fn complete_onboarding(&mut self, desktop: Option<&DesktopState>) {
        let name = self.onboarding_name.trim().to_string();
        let usage = self.onboarding_usage.trim().to_string();
        let share = self.onboarding_share;
        self.settings_profile_name = name.clone();
        self.share_telemetry = share;
        self.onboarding_done = true;
        self.onboarding_open = false;
        self.show_workspace_choice = false;
        self.show_onboarding_tip = false;
        if let Some(desktop) = desktop {
            if let Err(error) = desktop.set_friendly_name(&name) {
                log::warn!("set_friendly_name failed: {error}");
            }
            if let Err(error) = desktop.set_usage(&usage) {
                log::warn!("set_usage failed: {error}");
            }
            if let Err(error) = desktop.set_onboarding_done() {
                log::warn!("set_onboarding_done failed: {error}");
            }
        }
        if let Some(telemetry) = goble_telemetry::global() {
            // Report what is actually in force: an environment opt-out wins
            // over the answer, and the Profile switch should say so.
            self.share_telemetry = telemetry.set_upload_consent(share);
        }
        crate::telemetry::onboarding_completed();
    }

    /// Open the flow at launch when this is the first run. The app entry point
    /// calls this, so mounting the tree in a test does not put the overlay over
    /// every other surface. A dev build can force it with `GOBLE_ONBOARDING=1`.
    pub fn open_onboarding_if_first_run(&mut self, desktop: Option<&DesktopState>) {
        self.onboarding_done = desktop.map(|d| d.onboarding_done()).unwrap_or(false);
        let forced = cfg!(debug_assertions) && std::env::var_os("GOBLE_ONBOARDING").is_some();
        if self.onboarding_done && !forced {
            return;
        }
        self.onboarding_open = true;
        self.show_llm_key_banner = false;
        self.show_workspace_choice = false;
        self.show_onboarding_tip = false;
    }

    /// Open the flow again with the answers already known, for a dev build that
    /// needs to iterate on it. The stored onboarding-done flag is left alone, so
    /// a replay that is abandoned does not ambush the next real launch.
    pub fn replay_onboarding(&mut self) {
        self.onboarding_open = true;
        self.onboarding_step = 0;
        self.onboarding_name = self.settings_profile_name.clone();
        self.onboarding_share = self.share_telemetry;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use goble_core::store::Store;
    use goble_desktop_service::ThreadStore;

    /// A real store, so the assertions read what was persisted rather than the
    /// pane's own copy.
    fn desktop(dir: &tempfile::TempDir) -> std::sync::Arc<DesktopState> {
        DesktopState::new(
            Store::open_in_memory().expect("in-memory store"),
            ThreadStore::new(dir.path()).expect("thread store"),
        )
    }

    #[test]
    fn the_flow_needs_a_usage_then_a_name() {
        let mut state = UiState::mock();
        state.onboarding_open = true;
        state.onboarding_step = 0;
        assert!(!state.onboarding_can_advance());
        state.onboarding_usage = "work".into();
        assert!(state.onboarding_can_advance());
        state.onboarding_next_step();
        assert_eq!(state.onboarding_step, 1);
        assert!(!state.onboarding_can_advance());
        state.onboarding_name = "  ".into();
        assert!(!state.onboarding_can_advance());
        state.onboarding_name = "Ada".into();
        assert!(state.onboarding_can_advance());
        state.onboarding_next_step();
        assert_eq!(state.onboarding_step, 2);
        assert!(state.onboarding_is_last_step());
        state.onboarding_next_step();
        assert_eq!(
            state.onboarding_step, 2,
            "the action layer finishes the flow"
        );
        state.onboarding_back();
        assert_eq!(state.onboarding_step, 1);
    }

    #[test]
    fn finishing_closes_the_overlay_and_keeps_the_answers() {
        let mut state = UiState::mock();
        state.onboarding_open = true;
        state.onboarding_usage = "personal".into();
        state.onboarding_name = "  Ada  ".into();
        state.onboarding_share = true;
        state.complete_onboarding(None);
        assert!(!state.onboarding_open);
        assert!(state.onboarding_done);
        assert_eq!(state.settings_profile_name, "Ada");
        assert!(state.share_telemetry);
    }

    #[test]
    fn replay_starts_at_the_first_step_with_the_known_name() {
        let mut state = UiState::mock();
        state.settings_profile_name = "Ada".into();
        state.share_telemetry = true;
        state.onboarding_step = 2;
        state.replay_onboarding();
        assert!(state.onboarding_open);
        assert_eq!(state.onboarding_step, 0);
        assert_eq!(state.onboarding_name, "Ada");
        assert!(state.onboarding_share);
    }

    #[test]
    fn a_first_run_opens_the_flow_and_a_finished_run_skips_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let desktop = desktop(&dir);
        let mut state = UiState::mock();

        state.open_onboarding_if_first_run(Some(&desktop));
        assert!(state.onboarding_open, "a fresh store is a first run");

        state.onboarding_usage = "work".into();
        state.onboarding_name = "Ada".into();
        state.onboarding_share = false;
        state.complete_onboarding(Some(&desktop));
        assert!(desktop.onboarding_done(), "finishing is persisted");
        assert_eq!(desktop.get_friendly_name(), "Ada");
        assert_eq!(desktop.get_usage(), "work");

        let mut returning = UiState::mock();
        returning.open_onboarding_if_first_run(Some(&desktop));
        assert!(
            !returning.onboarding_open,
            "a returning run is not ambushed by the flow"
        );
    }

    #[test]
    fn the_approval_mode_reaches_every_pane_and_the_next_launch() {
        let dir = tempfile::tempdir().expect("temp dir");
        let desktop = desktop(&dir);
        let mut state = UiState::mock();
        state.pane_controls_mut(1);
        state.pane_controls_mut(2);

        state.set_approval_mode(ApprovalMode::AlwaysApprove, Some(&desktop));

        assert_eq!(state.approval_mode, ApprovalMode::AlwaysApprove);
        assert!(state.auto_approve, "the legacy switch follows the mode");
        for pane_id in [1, 2] {
            assert_eq!(
                state.pane_controls(pane_id).approval_mode,
                ApprovalMode::AlwaysApprove,
                "pane {pane_id} reads the app-wide policy"
            );
        }
        assert_eq!(
            desktop.get_approval_mode().as_deref(),
            Some("always-approve"),
            "the label is what the store keeps"
        );

        let reloaded = UiState::from_desktop(&desktop);
        assert_eq!(
            reloaded.approval_mode,
            ApprovalMode::AlwaysApprove,
            "the next launch restores the choice"
        );
    }
}
