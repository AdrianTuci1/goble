//! Settings → Appearance: the colour-theme dropdown, the five channel
//! overrides (each with a swatch that opens the palette), the interface text
//! scale, and the editor's save-on-close choice.
//!
//! The theme row is a real `Select`: its trigger names the palette in force and
//! the renderer draws an anchored menu of every preset (the current one checked)
//! through the page-level overlay. Each channel row keeps the editable hex field
//! and puts a colour square beside it; the square opens an anchored palette of
//! the active theme's own colours plus the preset accents, with a hex entry and a
//! reset. Every choice goes through the same document actions as the other
//! rows (`SetSelect`, `SetText`, `Reset`), so the revision fence, comment
//! preservation and file-backed truth hold here too. There is no HSV wheel.

use goble_core::config::ConfigFile;
use goble_ui::color::ColorU;
use goble_ui::theme::{Theme, ThemePreset};

use super::super::fields;
use super::super::model::{ActionModel, PageModel, RowModel, SectionModel};
use super::super::render::color_palette;
use super::super::state::{SettingsAction, SettingsUiState, StatusKind};
use super::{problem_section, slider_row, text_row, toggle_row};

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    if let Some(problems) = problem_section(state, ConfigFile::Config) {
        sections.push(problems);
    }

    // The palette: a real select, rendered as a trigger + anchored menu by the
    // page overlay. The options are the built-in default plus ThemePreset::ALL;
    // selecting one writes `theme.preset` and drops the five channel overrides.
    let options = fields::palette_options();
    let selected = fields::palette_index(state);
    let labels: Vec<String> = options.iter().map(|(label, _)| label.clone()).collect();
    let mut theme_row = RowModel::new(
        "theme.preset",
        "Colour theme",
        "The palette this build starts from; the channel rows below can override any one of them.",
    )
    .select("theme.preset", labels, selected);
    if let Some(swatch) = palette_swatch(state, selected) {
        theme_row = theme_row.with_swatch(swatch);
    }
    if state.file_read_only() {
        theme_row = theme_row.disabled();
    }
    let mut theme_section = SectionModel::new("theme", "Colors & themes")
        .with_description(
            "A palette sets the base colours; the channel rows below override any one of them.",
        )
        .with_row(theme_row);
    if !state.file_read_only() {
        theme_section = theme_section.with_header_action(ActionModel::new(
            "Restore defaults",
            SettingsAction::ResetThemeColours,
        ));
    }
    sections.push(theme_section);

    // The five channels, each a real `[theme]` key. The hex field is the same
    // text control every other page uses; the swatch next to it opens the
    // anchored palette.
    let mut channels = SectionModel::new("theme-channels", "Colour channels")
        .with_description(
            "Each channel is a #rrggbb override in [theme]. Unset a channel to use the palette's own colour.",
        );
    for (key, label, hint) in channel_rows() {
        let path = format!("theme.{key}");
        let stored = fields::theme_text(state, key);
        let value = stored.clone().unwrap_or_default();
        let color = color_palette::channel_color(state, key);
        let mut row = text_row(state, &path, label, hint, value, "#rrggbb")
            .with_swatch(color)
            .with_status(
                StatusKind::Muted,
                if stored.is_some() {
                    "Overridden in config.toml".to_string()
                } else {
                    format!("Using the palette's {}", label.to_lowercase())
                },
            );
        if stored.is_some() {
            row = row.with_reset(SettingsAction::Reset { path: path.clone() });
        }
        if state.file_read_only() {
            row = row.disabled();
        }
        channels = channels.with_row(row);
    }
    sections.push(channels);

    sections.push(
        SectionModel::new("text", "Text")
            .with_row(slider_row(
                state,
                "app.font_size",
                "Font size",
                "The interface text scale.",
                fields::app_f64(state, "font_size", 1.0) as f32,
                0.6,
                2.0,
                0.05,
            ))
            .with_row(toggle_row(
                state,
                "app.auto_save_on_close",
                "Save on close",
                "Save a file pane's edits instead of asking.",
                false,
            )),
    );
    sections.push(
        SectionModel::new("files", "This file").with_row(super::open_file_row(
            ConfigFile::Config,
            "config.toml",
            &state.file_line(),
        )),
    );
    PageModel::new(sections)
}

/// The five channels in the order the page names them:
/// `(stored key, label, hint)`.
fn channel_rows() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "accent",
            "Accent",
            "The brand colour: focus rings, selected rows and primary actions.",
        ),
        ("primary", "Primary", "The main foreground text colour."),
        (
            "secondary",
            "Secondary",
            "The muted foreground for descriptions and status lines.",
        ),
        ("bg", "Background", "The window and page background."),
        (
            "surface",
            "Surface",
            "The raised surface behind cards and panes.",
        ),
    ]
}

/// A palette's swatch: the preset's accent, or the built-in base's accent for
/// the "Default base" choice.
fn palette_swatch(state: &SettingsUiState, index: usize) -> Option<ColorU> {
    let options = fields::palette_options();
    if let Some(Some(value)) = options.get(index).map(|(_, value)| value.as_deref()) {
        if let Some(preset) = ThemePreset::from_key(value) {
            return Some(preset.accent());
        }
    }
    Some(if fields::theme_bool(state, "dark", true) {
        Theme::dark().accent_color()
    } else {
        Theme::light().accent_color()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::settings::model::ControlModel;

    #[test]
    fn the_theme_row_is_a_select_of_the_presets_and_the_wheel_is_gone() {
        let state = SettingsUiState::default();
        let model = page(&state);
        let theme = model
            .sections
            .iter()
            .flat_map(|section| section.rows.iter())
            .find(|row| row.id == "theme.preset")
            .expect("the theme row");
        match &theme.control {
            ControlModel::Select { path, options, .. } => {
                assert_eq!(path, "theme.preset");
                assert!(options.iter().any(|option| option == "Goble Night"));
                assert!(options.iter().any(|option| option == "Tokyo Night"));
            }
            other => panic!("the theme row is a select, got {other:?}"),
        }
        assert!(
            !model
                .interactive_rows()
                .iter()
                .any(|row| row.id == "theme.color-wheel"),
            "the HSV wheel is not on the page"
        );
        for key in ["accent", "primary", "secondary", "bg", "surface"] {
            let row = model
                .sections
                .iter()
                .flat_map(|section| section.rows.iter())
                .find(|row| row.id == format!("theme.{key}"))
                .unwrap_or_else(|| panic!("the {key} channel row"));
            assert!(row.swatch.is_some(), "the {key} row has a swatch");
            assert!(
                matches!(row.control, ControlModel::Text { .. }),
                "the {key} row keeps a real hex field"
            );
        }
    }
}
