//! Settings → Appearance: the palette and its five channel overrides, the
//! interface text scale, and the editor's save-on-close choice.

use goble_core::config::ConfigFile;
use goble_ui::theme::ThemePreset;

use super::super::fields;
use super::super::model::{ActionModel, PageModel, RowModel, SectionModel};
use super::super::state::{SettingsAction, SettingsUiState, StatusKind};
use super::{problem_section, slider_row, text_row, toggle_row};

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    if let Some(problems) = problem_section(state, ConfigFile::Config) {
        sections.push(problems);
    }

    // The theme library: one row per palette. It is a plain section so the
    // swatches read as a gallery rather than as form fields, the way T3's
    // `ThemeLibrary` sits unframed above the Interface rows.
    let selected = fields::palette_index(state);
    let mut library = Vec::new();
    for (index, (label, value)) in fields::palette_options().into_iter().enumerate() {
        let description = value
            .as_deref()
            .and_then(ThemePreset::from_key)
            .map(|preset| preset.description().to_string())
            .unwrap_or_else(|| "The built-in base: neutral surfaces, no overrides.".to_string());
        let pill = if index == selected {
            "Selected".to_string()
        } else {
            "Use".to_string()
        };
        library.push(
            RowModel::new(format!("theme-library.{index}"), label, description).buttons(vec![{
                let action = ActionModel::new(
                    pill,
                    SettingsAction::SetSelect {
                        path: "theme.preset".to_string(),
                        index,
                    },
                );
                if index == selected {
                    action
                } else {
                    action.with_ghost()
                }
            }]),
        );
    }
    sections.push(
        SectionModel::new("theme-library", "Colors & themes")
            .with_description(
                "A palette sets the base colours; the five channels below override any one of them.",
            )
            .plain()
            .with_rows(library),
    );

    sections.push(
        SectionModel::new("theme", "Interface")
            .with_header_action(ActionModel::new(
                "Restore defaults",
                SettingsAction::ResetThemeColours,
            ))
            .with_row(toggle_row(
                state,
                "theme.dark",
                "Dark base",
                "The built-in dark or light scheme.",
                true,
            ))
            .with_row(channel_row(
                state,
                "theme.accent",
                "Accent",
                "Overrides the accent channel.",
            ))
            .with_row(channel_row(
                state,
                "theme.primary",
                "Primary",
                "Overrides the primary text channel.",
            ))
            .with_row(channel_row(
                state,
                "theme.secondary",
                "Secondary",
                "Overrides the muted text channel.",
            ))
            .with_row(channel_row(
                state,
                "theme.bg",
                "Background",
                "Overrides the page background.",
            ))
            .with_row(channel_row(
                state,
                "theme.surface",
                "Surface",
                "Overrides the pane surface.",
            )),
    );

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

/// One colour-channel row: a hex value the file stores, with the current colour
/// named in the status line.
fn channel_row(state: &SettingsUiState, path: &str, label: &str, hint: &str) -> RowModel {
    let stored = fields::theme_text(state, path.rsplit('.').next().unwrap_or(path));
    let value = stored.clone().unwrap_or_default();
    let mut row = text_row(state, path, label, hint, value, "#rrggbb");
    if let Some(colour) = stored {
        row = row.with_status(StatusKind::Muted, colour);
    } else {
        row = row.with_status(StatusKind::Muted, "Not overridden");
    }
    row
}
