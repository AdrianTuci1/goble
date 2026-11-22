//! Settings → Notifications: the notification switch this build owns, the
//! honest row for the system notifications it does not, and a read-only update
//! status notice. Checking and installing an update stay on the Updates page.
//!
//! The toggle is file-backed: `app.in_app_notifications` is written through the
//! same `SetToggle` dispatch as every other scalar setting, and the row reads
//! the projection so an edit made by hand in config.toml shows up here.

use super::super::model::{PageModel, RowModel, SectionModel};
use super::super::state::{SettingsUiState, StatusKind};

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    sections.push(
        SectionModel::new("notifications", "Notifications")
            .with_description("Choose how Goble tells you a turn has finished.")
            .with_row(super::toggle_row(
                state,
                "app.in_app_notifications",
                "In-app announcements",
                "The turn-finished banner inside Goble.",
                true,
            ))
            .with_row(super::unavailable_row(
                "system-notifications",
                "System notifications",
                "An operating-system banner when a turn finishes while Goble is in the background.",
                "system notifications are not wired into this build",
            )),
    );

    // A read-only status notice, not the Updates page: the checker's view is
    // useful context here, but its Check and Install controls stay there.
    let view = &state.update_view;
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
    let mut status = SectionModel::new("update-status", "Update status")
        .with_description(
            "A read-only status line from the update checker; checking and installing live on the Updates page under Advanced.",
        )
        .with_row(
            RowModel::new("updates.version", "This build", "The build that is running.")
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
        );
    match view.available.as_ref() {
        Some(available) => {
            status = status.with_row(
                RowModel::new(
                    "updates.available",
                    "Update available",
                    "The release the checker last offered; install it from the Updates page.",
                )
                .read_only(available.detail())
                .with_status(
                    StatusKind::Info,
                    if available.rollback {
                        "A rollback to a previous build is offered."
                    } else {
                        "A newer build is published."
                    },
                ),
            );
        }
        None => {
            status = status.with_row(
                RowModel::new(
                    "updates.none",
                    "No update available",
                    "Nothing newer than this build is published on the channel.",
                )
                .read_only(view.phase.label().to_string()),
            );
        }
    }
    if let Some(error) = view.error.clone() {
        status = status.with_row(
            RowModel::new(
                "updates.error",
                "Last check failed",
                "The rows above still report the last known state.",
            )
            .read_only("error")
            .with_status(StatusKind::Error, error),
        );
    }
    sections.push(status);

    sections.push(
        SectionModel::new("notifications-note", "How notifications work").with_description(
            "The in-app switch is stored as [app] in_app_notifications in config.toml. System notifications need an operating-system backend this build does not include.",
        ),
    );
    PageModel::new(sections)
}
