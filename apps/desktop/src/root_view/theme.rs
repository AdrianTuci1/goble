//! Writing the live theme into the shared [`AppContext`] each frame.

use goble_ui::color::ColorU;
use goble_ui::theme::{Theme, ThemePreset};

use super::RootView;

impl RootView {
    /// Write the live theme into the shared `AppContext` so every element
    /// re-renders with the current base theme + custom color-wheel overrides.
    ///
    /// A preset is the base when one is selected; the dark-mode switch styles
    /// the Default base only. Selecting a preset clears the five channel
    /// overrides, so the preset is shown exactly; editing a channel afterward
    /// applies the override on top of the base in force.
    pub(super) fn apply_theme(&mut self) {
        let Some(app_context) = self.app_context.clone() else {
            return;
        };
        let s = self.state.borrow();
        let mut theme = match s.theme_preset.as_deref().and_then(ThemePreset::from_key) {
            Some(preset) => Theme::preset(preset),
            None if s.settings_dark_mode => Theme::dark(),
            None => Theme::light(),
        };
        let primary = s.theme_primary.as_deref().and_then(ColorU::from_hex);
        let secondary = s.theme_secondary.as_deref().and_then(ColorU::from_hex);
        let accent = s.theme_accent.as_deref().and_then(ColorU::from_hex);
        let bg = s.theme_bg.as_deref().and_then(ColorU::from_hex);
        let surface = s.theme_surface.as_deref().and_then(ColorU::from_hex);
        theme = theme.with_overrides(primary, secondary, accent, bg, surface);
        app_context.borrow_mut().theme = theme;
    }
}
