//! Writing the live theme into the shared [`AppContext`] each frame.

use goble_ui::color::ColorU;
use goble_ui::theme::Theme;

use super::RootView;

impl RootView {
    /// Write the live theme into the shared `AppContext` so every element
    /// re-renders with the current base theme + custom color-wheel overrides.
    pub(super) fn apply_theme(&mut self) {
        let Some(app_context) = self.app_context.clone() else {
            return;
        };
        let s = self.state.borrow();
        let mut theme = if s.settings_dark_mode {
            Theme::dark()
        } else {
            Theme::light()
        };
        let primary = s.theme_primary.as_deref().and_then(ColorU::from_hex);
        let secondary = s.theme_secondary.as_deref().and_then(ColorU::from_hex);
        let accent = s.theme_accent.as_deref().and_then(ColorU::from_hex);
        theme = theme.with_overrides(primary, secondary, accent);
        app_context.borrow_mut().theme = theme;
    }
}
