//! Settings → General: who you are, what the app tells you, and the two
//! behavioural switches (scroll direction and speed).

use goble_core::config::ConfigFile;

use super::super::fields;
use super::super::model::{ActionModel, PageModel, RowModel, SectionModel};
use super::super::state::{SettingsAction, SettingsUiState, StatusKind};
use super::{problem_section, text_row, toggle_row};

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    // The reference General page: layout cards, permissions, execution and
    // voice. Controls with a real backend use it; the rest say so honestly.
    sections.push(
        SectionModel::new("permissions", "Permissions")
            .with_description(
                "Configure what the agent can do in local and remote conversations. Learn more.",
            )
            .with_row(
                RowModel::new(
                    "app.approval_mode",
                    "This Computer",
                    "Controls the actions the agent can take.",
                )
                .select(
                    "app.approval_mode",
                    fields::approval_modes()
                        .into_iter()
                        .map(|mode| match mode {
                            "manual" => "Default".to_string(),
                            "auto" => "Turbo".to_string(),
                            other => {
                                let mut chars = other.chars();
                                match chars.next() {
                                    Some(first) => {
                                        first.to_uppercase().collect::<String>() + chars.as_str()
                                    }
                                    None => String::new(),
                                }
                            }
                        })
                        .collect(),
                    fields::approval_index(state),
                ),
            )
            .with_row(
                RowModel::new(
                    "remote.mode",
                    "Remote",
                    "Controls permissions for remote conversations.",
                )
                .select(
                    "remote.mode",
                    vec!["Default".to_string(), "Turbo".to_string()],
                    fields::remote_mode_index(state),
                ),
            )
            .with_row(
                RowModel::new("permissions.advanced", "Advanced", "More permission controls.")
                    .buttons(vec![ActionModel::new(
                        "Open",
                        SettingsAction::Navigate(super::super::super::SettingsCategory::Agent),
                    )]),
            ),
    );
    sections.push(
        SectionModel::new("execution", "Execution")
            .with_row(
                RowModel::new(
                    "execution.queued",
                    "Queued Messages",
                    "Configure when follow-up messages are sent. The queue is the built-in behaviour in this build.",
                )
                .buttons({
                    let queue = ActionModel::new("Queue", SettingsAction::SetSendImmediately(false));
                    let now = ActionModel::new(
                        "Send Immediately",
                        SettingsAction::SetSendImmediately(true),
                    );
                    if state.send_immediately {
                        vec![queue, now.primary()]
                    } else {
                        vec![queue.primary(), now]
                    }
                }),
            )
            .with_row(
                RowModel::new(
                    "execution.shortcuts",
                    "Keyboard shortcuts",
                    "The shortcuts this build answers.",
                )
                .buttons(vec![ActionModel::new(
                    "Open",
                    SettingsAction::Navigate(super::super::super::SettingsCategory::Shortcuts),
                )
                .with_icon("keyboard")]),
            ),
    );
    sections.push(
        SectionModel::new("voice", "Voice mode")
            .with_row(super::unavailable_row(
                "voice.voice",
                "Voice",
                "The voice used in Voice mode.",
                "voice mode is not available in this build",
            ))
            .with_row(super::unavailable_row(
                "voice.microphone",
                "Microphone",
                "Input device for voice mode and dictation.",
                "audio capture is not available in this build",
            ))
            .with_row(super::unavailable_row(
                "voice.volume",
                "Volume",
                "How loud the voice is.",
                "audio output is not available in this build",
            )),
    );
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
