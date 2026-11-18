//! Settings → Appearance: the palette, the five channel overrides (with the
//! colour picker), the interface text scale, and the editor's save-on-close
//! choice.
//!
//! The palette choice is a real list: the row's trigger opens a menu of the
//! presets (each with a swatch, its description and the current check), and the
//! colour wheel below edits whichever channel the picker's buttons name. Both
//! write `[theme]` through the same document editor as every other row, so the
//! revision fence and the comment preservation hold here too.

use goble_core::config::ConfigFile;
use goble_ui::color::ColorU;
use goble_ui::theme::ThemePreset;

use super::super::fields;
use super::super::model::{ActionModel, ControlModel, PageModel, RowModel, SectionModel};
use super::super::state::{SettingsAction, SettingsUiState, StatusKind};
use super::{problem_section, slider_row, text_row, toggle_row};

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    if let Some(problems) = problem_section(state, ConfigFile::Config) {
        sections.push(problems);
    }

    // The palette: one trigger row plus, while open, the presets as a list.
    let options = fields::palette_options();
    let selected = fields::palette_index(state);
    let current = options
        .get(selected)
        .map(|(label, _)| label.clone())
        .unwrap_or_else(|| "Default base".to_string());
    let trigger = RowModel::new(
        "theme.preset",
        "Colour theme",
        "The palette this build starts from; the channels below can override any one of them.",
    )
    .buttons(vec![
        ActionModel::new(current, SettingsAction::ToggleThemeSelect(!state.theme_select_open))
            .with_icon("chevron-down")
            .primary(),
    ]);
    let mut preset_rows = vec![trigger];
    if state.theme_select_open {
        for (index, (label, value)) in options.into_iter().enumerate() {
            let description = value
                .as_deref()
                .and_then(ThemePreset::from_key)
                .map(|preset| preset.description().to_string())
                .unwrap_or_else(|| {
                    "The built-in base: neutral surfaces, no overrides.".to_string()
                });
            let is_selected = index == selected;
            let action = if is_selected {
                ActionModel::new(
                    "Selected",
                    SettingsAction::ToggleThemeSelect(false),
                )
                .with_icon("check")
            } else {
                ActionModel::new(
                    "Use",
                    SettingsAction::SetSelect {
                        path: "theme.preset".to_string(),
                        index,
                    },
                )
            };
            let mut row = RowModel::new(format!("theme.preset.option.{index}"), label, description)
                .buttons(vec![action])
                .with_status(
                    if is_selected {
                        StatusKind::Success
                    } else {
                        StatusKind::Muted
                    },
                    if is_selected {
                        "In use"
                    } else {
                        "Choose this palette"
                    },
                );
            if let Some(swatch) = palette_swatch(index) {
                row = row.with_swatch(swatch);
            }
            preset_rows.push(row);
        }
    }
    sections.push(
        SectionModel::new("theme", "Colors & themes")
            .with_description(
                "A palette sets the base colours; the colour picker below overrides any one channel.",
            )
            .with_header_action(ActionModel::new(
                "Restore defaults",
                SettingsAction::ResetThemeColours,
            ))
            .with_rows(preset_rows),
    );

    // The colour picker: which channel, then the wheel for it, then the raw
    // hex of every channel (the picker's numeric readout and its input).
    let target = state.color_target.min(channel_labels().len() - 1);
    let mut channel_actions: Vec<ActionModel> = Vec::new();
    for (index, (label, _, _)) in channel_labels().into_iter().enumerate() {
        let action = ActionModel::new(
            label,
            SettingsAction::SetColorTarget(index),
        );
        channel_actions.push(if index == target {
            action.primary()
        } else {
            action.with_ghost()
        });
    }
    let channel = channel_labels()[target].1;
    let picker_rows = vec![
        RowModel::new(
            "theme.color-target",
            "Channel",
            "Which theme channel the wheel edits.",
        )
        .buttons(channel_actions)
        .with_status(StatusKind::Muted, format!("Editing: {channel}")),
        RowModel::new(
            "theme.color-wheel",
            "Colour picker",
            "Drag the ring for hue and the square for saturation/brightness. Writes [theme] on change.",
        )
        .with_control(ControlModel::ColorWheel {
            channel: target,
            label: channel.to_string(),
            stored: fields::theme_text(state, channel_key(target)),
        }),
    ];
    let mut picker = SectionModel::new("color-picker", "Colour picker").with_rows(picker_rows);
    for (label, key, _) in channel_labels().into_iter() {
        let path = format!("theme.{key}");
        let stored = fields::theme_text(state, key);
        let value = stored.clone().unwrap_or_default();
        let mut row = text_row(
            state,
            &path,
            label,
            "The channel's #rrggbb override; the picker writes it too.",
            value,
            "#rrggbb",
        );
        row = row.with_status(
            StatusKind::Muted,
            stored
                .clone()
                .unwrap_or_else(|| "Not overridden".to_string()),
        );
        if let Some(colour) = stored.as_deref().and_then(ColorU::from_hex) {
            row = row.with_swatch(colour);
        }
        picker = picker.with_row(row.with_reset(SettingsAction::Reset {
            path: format!("theme.{key}"),
        }));
    }
    sections.push(picker);

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

/// The five channels in picker order: `(label, stored key, ColorToken name)`.
fn channel_labels() -> Vec<(&'static str, &'static str, goble_ui::theme::ColorToken)> {
    use goble_ui::theme::ColorToken as T;
    vec![
        ("Primary", "primary", T::Text),
        ("Secondary", "secondary", T::Muted),
        ("Accent", "accent", T::Accent),
        ("Background", "bg", T::Bg),
        ("Surface", "surface", T::Surface),
    ]
}

fn channel_key(index: usize) -> &'static str {
    channel_labels()
        .get(index)
        .map(|(_, key, _)| *key)
        .unwrap_or("accent")
}

/// A preset's swatch colour: the palette's accent, which is what distinguishes
/// the five bases at a glance.
fn palette_swatch(index: usize) -> Option<ColorU> {
    let options = fields::palette_options();
    let value = options.get(index)?.1.as_deref()?;
    ThemePreset::from_key(value).map(|preset| preset.colors().focus)
}
