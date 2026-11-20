//! Settings → Updates: this build, the last check, the offered release, and the
//! two controls the checker owns. Nothing here touches the network on its own:
//! the page draws the view `crate::update` published after the user asked.

use crate::update::UpdatePhase;

use super::super::model::{ActionModel, PageModel, RowModel, SectionModel};
use super::super::state::{SettingsAction, SettingsUiState, StatusKind};
use super::super::fields;

pub fn page(state: &SettingsUiState) -> PageModel {
    let view = &state.update_view;
    let mut sections = Vec::new();
    sections.push(
        SectionModel::new("notifications", "Notifications").with_row(super::toggle_row(
            state,
            "app.in_app_notifications",
            "In-app announcements",
            "The turn-finished banner inside Goble.",
            true,
        )),
    );
    if let Some(banner) = view.banner.as_ref() {
        sections.push(
            SectionModel::new("updates-banner", "Withdrawn build").with_row(
                RowModel::new(
                    "updates.banner",
                    format!("{} was withdrawn", banner.running),
                    "The publisher withdrew this version; the offers below are the replacement.",
                )
                .with_status(StatusKind::Warning, banner.detail.clone()),
            ),
        );
    }
    let version = if view.current_version.is_empty() {
        "unknown (this build carries no version)".to_string()
    } else {
        view.current_version.clone()
    };
    let channel = if view.channel.is_empty() {
        "unknown".to_string()
    } else {
        view.channel.clone()
    };
    sections.push(
        SectionModel::new("this-build", "This build")
            .with_row(
                RowModel::new("updates.version", "Version", "The build that is running.")
                    .read_only(version),
            )
            .with_row(
                RowModel::new(
                    "updates.channel",
                    "Channel",
                    "The release channel this build follows.",
                )
                .read_only(channel)
                .with_status(StatusKind::Muted, view.status.clone()),
            )
            .with_row(
                RowModel::new(
                    "updates.last-check",
                    "Last check",
                    "When this installation last asked the release host.",
                )
                .read_only(
                    view.last_check
                        .clone()
                        .unwrap_or_else(|| "never".to_string()),
                ),
            ),
    );

    let mut release = SectionModel::new("release", "Release").with_description(
        "The update checker compares the manifest against this build; installing restarts the app.",
    );
    match view.available.as_ref() {
        Some(available) => {
            let size = format_size(available.size);
            release = release.with_row(
                RowModel::new(
                    "updates.available",
                    format!("{} · {}", available.version, available.channel),
                    format!("{} · {size}", available.file_name),
                )
                .read_only(if available.rollback {
                    format!("rollback · {size}")
                } else {
                    size.clone()
                }),
            );
            if let Some(ready) = view.ready_version.clone() {
                release = release.with_row(
                    RowModel::new(
                        "updates.ready",
                        "Ready to restart",
                        "The release is downloaded, verified and installed.",
                    )
                    .read_only(ready)
                    .with_status(StatusKind::Success, "Restart installs it."),
                );
            }
        }
        None => {
            release = release.with_row(
                RowModel::new(
                    "updates.none",
                    "No update offered",
                    "Nothing newer than this build is published on the channel.",
                )
                .read_only(view.phase.label().to_string()),
            );
        }
    }
    release = release.with_row(
        RowModel::new(
            "updates.actions",
            "Check",
            "Ask the release host now; installing downloads, verifies and restarts.",
        )
        .buttons(vec![
            ActionModel::new("Check now", SettingsAction::CheckUpdates).with_icon("refresh"),
            {
                let button = ActionModel::new("Install and restart", SettingsAction::InstallUpdate)
                    .with_icon("arrow-up")
                    .primary();
                if view.can_install() {
                    button
                } else {
                    button.disabled()
                }
            },
        ]),
    );
    sections.push(release);
    if let Some(error) = view.error.clone() {
        sections.push(
            SectionModel::new("updates-error", "Last attempt").with_row(
                RowModel::new(
                    "updates.error",
                    "The last check did not finish",
                    "The other rows still report the last known state.",
                )
                .read_only("error")
                .with_status(StatusKind::Error, error),
            ),
        );
    }
    sections.push(
        SectionModel::new("updates-note", "How updates work")
            .with_description(
                "Goble verifies a release against a public key before installing it and keeps the previous build for a rollback. A build with no update host configured checks nothing.",
            )
            .with_row(
                RowModel::new(
                    "updates.phase",
                    "Phase",
                    "What the checker is doing right now.",
                )
                .read_only(view.phase.label().to_string()),
            ),
    );
    PageModel::new(sections)
}

fn format_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    format!("{:.1} MB", bytes as f64 / MB)
}

/// Keep the phase import used even when the compiler folds the match: the page
/// reports the phase label, and this helper is the one place that does.
#[allow(dead_code)]
fn phase_label(phase: UpdatePhase) -> String {
    phase.label().to_string()
}
