//! Settings → General: who you are, what the app tells you, and the two
//! behavioural switches (scroll direction and speed).

use goble_core::config::ConfigFile;

use super::super::fields;
use super::super::model::{PageModel, RowModel, SectionModel};
use super::super::state::{SettingsAction, SettingsUiState, StatusKind};
use super::{problem_section, text_row, toggle_row};

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    if let Some(problems) = problem_section(state, ConfigFile::Config) {
        sections.push(problems);
    }
    let friendly = fields::app_text(state, "friendly_name");
    sections.push(
        SectionModel::new("you", "You")
            .with_row(text_row(
                state,
                "app.friendly_name",
                "Friendly name",
                "How Goble addresses you.",
                friendly,
                "Your name",
            ))
            .with_row(
                RowModel::new(
                    "app.usage",
                    "What you use Goble for",
                    "The answer from onboarding.",
                )
                .read_only(if fields::app_text(state, "usage").is_empty() {
                    "Not set".to_string()
                } else {
                    fields::app_text(state, "usage")
                }),
            ),
    );
    sections.push(
        SectionModel::new("notifications", "Notifications")
            .with_row(toggle_row(
                state,
                "app.in_app_notifications",
                "In-app announcements",
                "The turn-finished banner inside Goble.",
                true,
            ))
            .with_row(super::unavailable_row(
                "system-notifications",
                "System notifications",
                "A banner from the operating system when a turn finishes.",
                "system notifications are not wired yet",
            )),
    );
    sections.push(
        SectionModel::new("behaviour", "Behaviour")
            .with_row(toggle_row(
                state,
                "app.telemetry",
                "Share anonymous usage data",
                "Crash reports and usage counts. Off unless you turn it on.",
                false,
            ))
            .with_row(toggle_row(
                state,
                "app.invert_scroll",
                "Invert scrolling",
                "Natural scrolling for the wheel.",
                false,
            ))
            .with_row(super::slider_row(
                state,
                "app.scroll_speed",
                "Scroll speed",
                "1 (slowest) to 100 (fastest).",
                fields::app_i64(state, "scroll_speed", 50) as f32,
                1.0,
                100.0,
                5.0,
            )),
    );
    sections.push(
        SectionModel::new("files", "This file")
            .with_row(super::open_file_row(
                ConfigFile::Config,
                "config.toml",
                &state.file_line(),
            ))
            .with_row(super::file_status_row(state, ConfigFile::Config)),
    );
    PageModel::new(sections)
}
