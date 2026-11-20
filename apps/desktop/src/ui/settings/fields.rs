//! Reading the settings projection for the scalar rows.
//!
//! The settings pages are rendered from a redacted [`ConfigView`]: the service
//! never hands the UI the file's text, only the typed sections. This module is
//! the one place that maps a dotted path (`app.friendly_name`,
//! `theme.preset`, `remote.gateway_url`) to that projection, so a page says
//! *which setting* it draws and not how to dig it out of three structs.
//!
//! Every reader returns the *stored* value: `None` means the key is absent,
//! which the schema defines as "the default is in force". A row shows the
//! default in that case and offers no reset; a write of the default value is a
//! `Set`, and the reset action is an `Unset` (see `actions::settings_actions`).

use goble_core::config::{
    AppView, ConfigOp, ConfigValue, RemoteView, ThemeView, APPROVAL_MODES, FIELDS,
};
use goble_ui::theme::ThemePreset;

use super::state::SettingsUiState;

/// The approval modes as the labels the composer writes.
pub fn approval_modes() -> Vec<&'static str> {
    APPROVAL_MODES.to_vec()
}

/// The palette options: `(label, value)`; `Default` has no stored value and is
/// the first option.
pub fn palette_options() -> Vec<(String, Option<String>)> {
    let mut options = vec![("Default base".to_string(), None)];
    for preset in ThemePreset::ALL {
        options.push((
            preset.display_name().to_string(),
            Some(preset.key().to_string()),
        ));
    }
    options
}

/// The `[app]` section of the loaded `config.toml`.
pub fn app(state: &SettingsUiState) -> AppView {
    state
        .config
        .as_ref()
        .map(|view| view.sections.app.clone())
        .unwrap_or_default()
}

/// The `[theme]` section.
pub fn theme(state: &SettingsUiState) -> ThemeView {
    state
        .config
        .as_ref()
        .map(|view| view.sections.theme.clone())
        .unwrap_or_default()
}

/// The `[remote]` section.
pub fn remote(state: &SettingsUiState) -> RemoteView {
    state
        .config
        .as_ref()
        .map(|view| view.sections.remote.clone())
        .unwrap_or_default()
}

pub fn app_bool(state: &SettingsUiState, key: &str, default: bool) -> bool {
    let app = app(state);
    match key {
        "telemetry" => app.telemetry.unwrap_or(default),
        "in_app_notifications" => app.in_app_notifications.unwrap_or(default),
        "invert_scroll" => app.invert_scroll.unwrap_or(default),
        "auto_save_on_close" => app.auto_save_on_close.unwrap_or(default),
        "auto_approve" => app.auto_approve.unwrap_or(default),
        "vim_mode" => app.vim_mode.unwrap_or(default),
        "send_immediately" => app.send_immediately.unwrap_or(default),
        _ => default,
    }
}

pub fn app_text(state: &SettingsUiState, key: &str) -> String {
    let app = app(state);
    match key {
        "friendly_name" => app.friendly_name.unwrap_or_default(),
        "usage" => app.usage.unwrap_or_default(),
        "approval_mode" => app.approval_mode.unwrap_or_else(|| "manual".to_string()),
        _ => String::new(),
    }
}

pub fn app_i64(state: &SettingsUiState, key: &str, default: i64) -> i64 {
    let app = app(state);
    match key {
        "scroll_speed" => app.scroll_speed.unwrap_or(default),
        _ => default,
    }
}

pub fn app_f64(state: &SettingsUiState, key: &str, default: f64) -> f64 {
    let app = app(state);
    match key {
        "font_size" => app.font_size.unwrap_or(default),
        _ => default,
    }
}

pub fn theme_bool(state: &SettingsUiState, key: &str, default: bool) -> bool {
    let theme = theme(state);
    match key {
        "dark" => theme.dark.unwrap_or(default),
        _ => default,
    }
}

pub fn theme_text(state: &SettingsUiState, key: &str) -> Option<String> {
    let theme = theme(state);
    match key {
        "preset" => theme.preset,
        "accent" => theme.accent,
        "primary" => theme.primary,
        "secondary" => theme.secondary,
        "bg" => theme.bg,
        "surface" => theme.surface,
        _ => None,
    }
}

pub fn remote_text(state: &SettingsUiState, key: &str) -> String {
    let remote = remote(state);
    match key {
        "mode" => remote.mode,
        "gateway_url" => remote.gateway_url.unwrap_or_default(),
        _ => String::new(),
    }
}

/// Whether an inline worker token is configured (the value itself is never
/// projected).
pub fn remote_worker_token_set(state: &SettingsUiState) -> bool {
    remote(state).worker_token.set
}

/// The `[models] default` slug.
pub fn default_model(state: &SettingsUiState) -> String {
    state
        .config
        .as_ref()
        .and_then(|view| view.sections.default_model.clone())
        .unwrap_or_default()
}

/// The display value the schema would show for a path, from the projection.
pub fn display_value(state: &SettingsUiState, path: &str) -> Option<String> {
    let mut segments = path.split('.');
    let section = segments.next()?;
    let key = segments.next()?;
    match section {
        "app" => match key {
            "friendly_name" | "usage" | "approval_mode" => Some(app_text(state, key)),
            "scroll_speed" => Some(app_i64(state, key, 50).to_string()),
            "font_size" => Some(app_f64(state, key, 1.0).to_string()),
            "telemetry"
            | "in_app_notifications"
            | "invert_scroll"
            | "auto_save_on_close"
            | "auto_approve"
            | "vim_mode"
            | "send_immediately" => {
                // The fallback is the schema's own default, so an absent key
                // compares equal to it and offers no reset.
                let fallback = schema_default(path)
                    .and_then(|value| value.parse::<bool>().ok())
                    .unwrap_or(false);
                Some(app_bool(state, key, fallback).to_string())
            }
            _ => None,
        },
        "theme" => {
            theme_text(state, key).or_else(|| Some(theme_bool(state, key, false).to_string()))
        }
        "remote" => Some(remote_text(state, key)),
        "models" if key == "default" => Some(default_model(state)),
        _ => None,
    }
}

/// The schema's display default for a path, if it has one.
pub fn schema_default(path: &str) -> Option<String> {
    let segments: Vec<String> = path.split('.').map(str::to_string).collect();
    FIELDS
        .iter()
        .find(|spec| {
            spec.path
                .iter()
                .zip(&segments)
                .all(|(a, b)| *a == b.as_str())
        })
        .and_then(|spec| spec.default.map(|value| value.to_string()))
}

/// The label/hint copy the schema holds for a path. The pages use it so a row
/// says what the file documentation says; a page that needs different copy for
/// its context overrides it.
pub fn schema_copy(path: &str) -> Option<(&'static str, &'static str)> {
    let segments: Vec<String> = path.split('.').map(str::to_string).collect();
    FIELDS
        .iter()
        .find(|spec| {
            spec.path.len() == segments.len()
                && spec
                    .path
                    .iter()
                    .zip(&segments)
                    .all(|(expected, actual)| *expected == "*" || *expected == actual.as_str())
        })
        .map(|spec| (spec.label, spec.description))
}

/// Whether the stored value is the effective default, so the reset affordance
/// is offered only when there is something to reset.
pub fn is_default(state: &SettingsUiState, path: &str) -> bool {
    let Some(default) = schema_default(path) else {
        return false;
    };
    match display_value(state, path) {
        Some(value) => value == default,
        None => true,
    }
}

/// The reset action for a path: an `Unset`, which restores the schema default
/// without writing a duplicate of it into the file.
pub fn reset_op(path: &str) -> ConfigOp {
    let segments: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    ConfigOp::unset(&segments)
}

/// A text write: an empty field unsets the key (the file's default), any other
/// value sets it.
pub fn text_op(path: &str, value: &str) -> ConfigOp {
    let segments: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    if value.is_empty() {
        ConfigOp::unset(&segments)
    } else {
        ConfigOp::set(&segments, ConfigValue::Str(value.to_string()))
    }
}

/// A toggle write.
pub fn bool_op(path: &str, value: bool) -> ConfigOp {
    let segments: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    ConfigOp::set(&segments, ConfigValue::Bool(value))
}

/// A select write: the option's stored value. `None` means "the default", and
/// the write is an unset.
pub fn select_op(path: &str, value: Option<&str>) -> ConfigOp {
    match value {
        None | Some("") => reset_op(path),
        Some(value) => {
            let segments: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
            ConfigOp::set(&segments, ConfigValue::Str(value.to_string()))
        }
    }
}

/// A slider write: integers where the schema says integer, floats otherwise.
pub fn number_op(path: &str, value: f32) -> ConfigOp {
    let segments: Vec<&str> = path.split('.').filter(|s| !s.is_empty()).collect();
    if path.ends_with("scroll_speed") {
        ConfigOp::set(&segments, ConfigValue::Int(value.round() as i64))
    } else {
        ConfigOp::set(
            &segments,
            ConfigValue::Float((value as f64 * 100.0).round() / 100.0),
        )
    }
}

/// The palette's selected index for the stored preset.
pub fn palette_index(state: &SettingsUiState) -> usize {
    let stored = theme_text(state, "preset").unwrap_or_default();
    palette_options()
        .into_iter()
        .position(|(_, value)| value.as_deref().unwrap_or("") == stored)
        .unwrap_or(0)
}

/// The palette's stored value at `index`, as a select op.
pub fn palette_select_op(path: &str, index: usize) -> ConfigOp {
    let options = palette_options();
    let value = options.get(index).and_then(|(_, value)| value.clone());
    select_op(path, value.as_deref())
}

/// The approval-mode's selected index.
pub fn approval_index(state: &SettingsUiState) -> usize {
    let stored = app_text(state, "approval_mode");
    approval_modes()
        .iter()
        .position(|mode| *mode == stored)
        .unwrap_or(1)
}

/// The remote mode's selected index.
pub fn remote_mode_index(state: &SettingsUiState) -> usize {
    match remote_text(state, "mode").as_str() {
        "gateway" => 1,
        _ => 0,
    }
}

/// The default model's selected index in the model list.
pub fn default_model_index(state: &SettingsUiState) -> usize {
    let stored = default_model(state);
    state
        .models()
        .iter()
        .position(|model| model.slug == stored)
        .unwrap_or(0)
}
