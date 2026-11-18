//! The twelve settings pages, as data.
//!
//! One module per page. Each returns a [`PageModel`]; [`super::render`] draws it
//! and the keyboard model reads the same description, so a page and its keys
//! cannot drift. The pages are the DSH information architecture (the same one
//! `goble_core::config::SettingsPage` enumerates):
//!
//! General · Appearance · Providers · Connectors · Agent · Environments ·
//! Hosts · Secrets · Shortcuts · Updates · Advanced · About.
//!
//! A row this build cannot perform is rendered disabled with an honest status
//! line ("Not available in this build · no OAuth sign-in"), never hidden and
//! never faked. Rows whose data is a settings file show where it lives; the file
//! is the source of truth.

mod about;
mod advanced;
mod agent;
mod appearance;
mod connectors;
mod environments;
mod general;
mod hosts;
mod providers;
mod secrets;
mod shortcuts;
mod updates;

use goble_core::config::ConfigFile;

use super::model::{ActionModel, ControlModel, RowModel, SectionModel};
use super::state::{SettingsAction, SettingsForm, SettingsUiState, StatusKind};

pub use super::model::{ActionStyle, PageModel};

/// The page model for `page`, from the projection the state holds.
pub fn page_model(page: super::super::SettingsCategory, state: &SettingsUiState) -> PageModel {
    use super::super::SettingsCategory as C;
    match page {
        C::General => general::page(state),
        C::Appearance => appearance::page(state),
        C::Providers => providers::page(state),
        C::Connectors => connectors::page(state),
        C::Agent => agent::page(state),
        C::Environments => environments::page(state),
        C::Hosts => hosts::page(state),
        C::Secrets => secrets::page(state),
        C::Shortcuts => shortcuts::page(state),
        C::Updates => updates::page(state),
        C::Advanced => advanced::page(state),
        C::About => about::page(state),
    }
}

// ---------------------------------------------------------------------------
// Row helpers shared by the pages
// ---------------------------------------------------------------------------

/// A boolean row: the value comes from the projection, the write is a `Set`.
pub fn toggle_row(
    state: &SettingsUiState,
    path: &str,
    label: &str,
    hint: &str,
    default: bool,
) -> RowModel {
    let value = super::fields::app_bool(state, last_segment(path), default);
    let mut row = RowModel::new(path, label, hint).toggle(path, value);
    if !super::fields::is_default(state, path) {
        row = row.with_reset(SettingsAction::Reset {
            path: path.to_string(),
        });
    }
    row
}

/// A text row: the draft overrides the projection while the user types, and
/// `Enter` commits it as a file write. An empty commit unsets the key.
pub fn text_row(
    state: &SettingsUiState,
    path: &str,
    label: &str,
    hint: &str,
    value: String,
    placeholder: &str,
) -> RowModel {
    let draft = state.draft(path, &value).to_string();
    let mut row = RowModel::new(path, label, hint).text(path, draft, placeholder);
    if !value.is_empty() {
        row = row.with_status(StatusKind::Muted, value.clone());
    }
    row
}

/// A write-only secret row.
pub fn secret_row(
    state: &SettingsUiState,
    path: &str,
    label: &str,
    hint: &str,
    configured: bool,
) -> RowModel {
    let draft = state.draft(path, "").to_string();
    RowModel::new(path, label, hint).secret(path, draft, configured)
}

/// A slider row.
pub fn slider_row(
    state: &SettingsUiState,
    path: &str,
    label: &str,
    hint: &str,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
) -> RowModel {
    let mut row = RowModel::new(path, label, format!("{hint} · {}", format_number(value)))
        .slider(path, value, min, max, step);
    if !super::fields::is_default(state, path) {
        row = row.with_reset(SettingsAction::Reset {
            path: path.to_string(),
        });
    }
    row
}

pub fn format_number(value: f32) -> String {
    if (value - value.round()).abs() < 0.0001 {
        format!("{}", value.round() as i64)
    } else {
        format!("{value:.2}")
    }
}

fn last_segment(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

/// A row that says a feature is not in this build: disabled, with the reason in
/// the status line and no fake control.
pub fn unavailable_row(id: &str, label: &str, hint: &str, why: &str) -> RowModel {
    RowModel::new(id, label, hint).disabled().with_status(
        StatusKind::Muted,
        format!("Not available in this build · {why}"),
    )
}

/// A read-only row reporting a value (the version, a path, a revision).
pub fn info_row(id: &str, label: &str, hint: &str, value: impl Into<String>) -> RowModel {
    RowModel::new(id, label, hint).read_only(value)
}

/// A row whose actions are buttons (open the file, reload, check).
pub fn action_row(id: &str, label: &str, hint: &str, actions: Vec<ActionModel>) -> RowModel {
    let mut row = RowModel::new(id, label, hint).buttons(actions);
    if row.primary_action().is_none() {
        row.disabled = false;
    }
    row
}

/// The header's "Add …" action a list section offers.
pub fn add_action(label: &str, action: SettingsAction) -> ActionModel {
    ActionModel::new(label, action).with_icon("plus")
}

/// The "open the file this page writes" row every file-backed page ends with.
pub fn open_file_row(file: ConfigFile, label: &str, path: &str) -> RowModel {
    action_row(
        "open-file",
        label,
        "The file is the source of truth: edits you make by hand are read back.",
        vec![
            ActionModel::new("Open file", SettingsAction::OpenFile(file)).with_icon("file"),
            ActionModel::new("Reload from file", SettingsAction::Reload).with_icon("refresh"),
        ],
    )
    .with_status(StatusKind::Muted, path.to_string())
}

/// The file-diagnostics block the page footer of a file-backed page can show.
pub fn file_status_row(state: &SettingsUiState, file: ConfigFile) -> RowModel {
    let view = match file {
        ConfigFile::Config => state.config.as_ref(),
        ConfigFile::Environment => state.environment.as_ref(),
        ConfigFile::Hosts => state.hosts.as_ref(),
    };
    let (path, mode, revision, problems) = match view {
        Some(view) => (
            view.path.clone(),
            view.mode.clone(),
            view.revision.short().to_string(),
            view.problems.len(),
        ),
        None => (
            "not loaded".to_string(),
            "missing".to_string(),
            "—".to_string(),
            0,
        ),
    };
    let mut row = info_row(
        "file-status",
        "File",
        "Where this page's data lives and the revision the last write fenced against.",
        format!("{path} · mode {mode} · revision {revision}"),
    );
    if problems > 0 {
        row = row.with_status(
            StatusKind::Warning,
            format!("{problems} parse problem(s) — the page will not overwrite the file"),
        );
    }
    row.with_status(StatusKind::Muted, path)
}

/// The notice a page shows when the file has problems, or nothing when it has
/// none.
pub fn problem_section(state: &SettingsUiState, file: ConfigFile) -> Option<SectionModel> {
    let view = match file {
        ConfigFile::Config => state.config.as_ref(),
        ConfigFile::Environment => state.environment.as_ref(),
        ConfigFile::Hosts => state.hosts.as_ref(),
    }?;
    if view.problems.is_empty() {
        return None;
    }
    let lines = view
        .problems
        .iter()
        .map(|problem| problem.line())
        .collect::<Vec<_>>()
        .join("\n");
    Some(
        SectionModel::new("file-problems", "File problems")
            .with_description(
                "The file could not be fully parsed. Goble will not overwrite it until this is fixed.",
            )
            .with_row(RowModel::new(
                "file-problems-row",
                "config.toml",
                "Every edit is refused while the file has problems.",
            )
            .read_only(format!("{} problem(s)", view.problems.len()))
            .with_status(StatusKind::Warning, lines)),
    )
}

/// The open add/edit form, as the side sheet that renders beside the page.
///
/// A form is not part of the page's own model: the page keeps drawing its list
/// behind the sheet, while the sheet owns its own title, fields, focus order and
/// Save/Cancel actions.
pub fn form_section(state: &SettingsUiState) -> Option<SectionModel> {
    match state.form.as_ref()? {
        SettingsForm::Model(form) => Some(providers::model_form_section(state, form)),
        SettingsForm::Connector(form) => Some(connectors::connector_form_section(state, form)),
        SettingsForm::Host(form) => Some(hosts::host_form_section(state, form)),
        SettingsForm::Harness(form) => Some(providers::harness_form_section(state, form)),
    }
}

/// The keyboard hint strip every page's footer names.
pub fn footer_hints() -> Vec<(&'static str, &'static [&'static str])> {
    vec![
        ("pages", &["↑", "↓"]),
        ("open", &["Enter"]),
        ("change", &["←", "→"]),
        ("search", &["/"]),
        ("back", &["Esc"]),
    ]
}

/// A control's current text for tests and for the keyboard model: the value a
/// row displays, without the widget.
pub fn control_text(row: &RowModel) -> String {
    match &row.control {
        ControlModel::Toggle { value, .. } => value.to_string(),
        ControlModel::Select {
            options, selected, ..
        } => options.get(*selected).cloned().unwrap_or_default(),
        ControlModel::Text { value, .. } => value.clone(),
        ControlModel::Slider { value, .. } => format_number(*value),
        ControlModel::ReadOnly { text } => text.clone(),
        ControlModel::Buttons(actions) => actions
            .iter()
            .map(|action| action.label.clone())
            .collect::<Vec<_>>()
            .join(" · "),
        ControlModel::ColorWheel { label, .. } => label.clone(),
        ControlModel::None => String::new(),
    }
}
