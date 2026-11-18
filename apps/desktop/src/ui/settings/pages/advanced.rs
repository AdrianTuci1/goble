//! Settings → Advanced: the files themselves — where they are, what mode they
//! have, the revision the page is holding, and any parse problem.
//!
//! This is the diagnostics page: it explains the file-backed model instead of
//! hiding it. There is no "reset everything" button in this build; the row for it
//! is honest about that.

use goble_core::config::ConfigFile;

use super::super::model::{ActionModel, PageModel, RowModel, SectionModel};
use super::super::state::{SettingsAction, SettingsUiState, StatusKind};
use super::unavailable_row;

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    for (file, label, description) in [
        (
            ConfigFile::Config,
            "config.toml",
            "App, theme, models, connectors, harnesses and remote configuration.",
        ),
        (
            ConfigFile::Environment,
            "environment.toml",
            "Named environment groups a session can start with.",
        ),
        (ConfigFile::Hosts, "hosts.toml", "Saved SSH targets."),
    ] {
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
        let mut section = SectionModel::new(format!("file.{}", file.file_name()), label)
            .with_description(description)
            .with_header_action(ActionModel::new("Open", SettingsAction::OpenFile(file)))
            .with_row(
                RowModel::new(
                    format!("file.{}.path", file.file_name()),
                    "Path",
                    "The file this page reads and writes.",
                )
                .read_only(path.clone())
                .with_status(StatusKind::Muted, path),
            )
            .with_row(
                RowModel::new(
                    format!("file.{}.mode", file.file_name()),
                    "Permissions",
                    "Settings files are written atomically with mode 0600.",
                )
                .read_only(mode.clone())
                .with_status(
                    if mode == "0600" {
                        StatusKind::Success
                    } else {
                        StatusKind::Warning
                    },
                    format!("mode {mode}"),
                ),
            )
            .with_row(
                RowModel::new(
                    format!("file.{}.revision", file.file_name()),
                    "Revision",
                    "The content hash the last read or write produced; a write fences against it.",
                )
                .read_only(revision.clone())
                .with_status(StatusKind::Muted, revision),
            );
        if problems > 0 {
            let detail = view
                .map(|view| {
                    view.problems
                        .iter()
                        .map(|problem| problem.line())
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default();
            section = section.with_row(
                RowModel::new(
                    format!("file.{}.problems", file.file_name()),
                    "Parse problems",
                    "Goble refuses to overwrite a file it cannot fully parse.",
                )
                .read_only(format!("{problems} problem(s)"))
                .with_status(StatusKind::Warning, detail),
            );
        } else {
            section = section.with_row(
                RowModel::new(
                    format!("file.{}.problems", file.file_name()),
                    "Parse problems",
                    "A file that is not TOML, or has a wrong-shaped section, is reported here.",
                )
                .read_only("none")
                .with_status(StatusKind::Success, "The file parses cleanly."),
            );
        }
        sections.push(section);
    }
    sections.push(
        SectionModel::new("advanced-actions", "Files")
            .with_row(
                RowModel::new(
                    "advanced.reload",
                    "Reload from disk",
                    "Re-read all three files; an edit made by hand (or by `goble-cli`) is picked up.",
                )
                .buttons(vec![
                    ActionModel::new("Reload", SettingsAction::Reload).with_icon("refresh"),
                    ActionModel::new(
                        "Open settings folder",
                        SettingsAction::OpenSettingsFolder,
                    )
                    .with_icon("folder-open"),
                ]),
            )
            .with_row(unavailable_row(
                "advanced-reset",
                "Reset all settings",
                "Delete the files and start from the defaults.",
                "destructive bulk reset is not wired; delete a row or edit the file instead",
            ))
            .with_row(unavailable_row(
                "advanced-raw",
                "Raw file editor",
                "Edit the TOML inside Goble.",
                "open the file in a pane instead",
            )),
    );
    sections.push(
        SectionModel::new("advanced-about-model", "The file is the source of truth")
            .with_description(
                "Every write is a document edit: comments, unknown keys and the file's own order survive. A write is atomic (temp file, fsync, rename) and takes a lock, so the app and `goble-cli setup` cannot interleave. A revision mismatch is reported as a conflict instead of overwriting the other writer.",
            )
            .with_row(
                RowModel::new(
                    "advanced.path",
                    "Home",
                    "The Goble home the app resolves at startup.",
                )
                .read_only(
                    state
                        .paths
                        .home
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "~/.goble".to_string()),
                ),
            ),
    );
    PageModel::new(sections)
}
