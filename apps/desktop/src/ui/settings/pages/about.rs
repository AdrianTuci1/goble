//! Settings → About: what this build is, and where its files are.
//!
//! The page is deliberately plain: a version, the channel, the home directory and
//! the one line that says the app is local-first. Nothing here links out to a
//! website the build cannot open.

use super::super::model::{PageModel, RowModel, SectionModel};
use super::super::state::{SettingsUiState, StatusKind};
use super::unavailable_row;

pub fn page(state: &SettingsUiState) -> PageModel {
    let version = env!("CARGO_PKG_VERSION");
    let mut sections = Vec::new();
    sections.push(
        SectionModel::new("about", "Goble")
            .with_description(
                "A local-first agent workspace: conversations, terminals, files and settings that live on this machine.",
            )
            .with_row(
                RowModel::new("about.name", "Name", "The app this build runs.")
                    .read_only("Goble"),
            )
            .with_row(
                RowModel::new("about.version", "Version", "The version this build carries.")
                    .read_only(version)
                    .with_status(StatusKind::Muted, format!("v{version}")),
            )
            .with_row(
                RowModel::new("about.channel", "Channel", "The release channel from the update manifest.")
                    .read_only(if state.update_view.channel.is_empty() {
                        "not configured".to_string()
                    } else {
                        state.update_view.channel.clone()
                    }),
            )
            .with_row(
                RowModel::new(
                    "about.home",
                    "Home directory",
                    "Config, environment groups and saved hosts live here.",
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
    sections.push(
        SectionModel::new("about-files", "Files")
            .with_row(
                RowModel::new(
                    "about.config",
                    "config.toml",
                    "App, theme, providers, connectors, harnesses and remote configuration.",
                )
                .read_only(
                    state
                        .paths
                        .config
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "~/.goble/config.toml".to_string()),
                ),
            )
            .with_row(
                RowModel::new(
                    "about.environment",
                    "environment.toml",
                    "Named environment groups.",
                )
                .read_only(
                    state
                        .paths
                        .environment
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "~/.goble/environment.toml".to_string()),
                ),
            )
            .with_row(
                RowModel::new("about.hosts", "hosts.toml", "Saved SSH targets.").read_only(
                    state
                        .paths
                        .hosts
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "~/.goble/hosts.toml".to_string()),
                ),
            ),
    );
    sections.push(
        SectionModel::new("about-unavailable", "Not in this build")
            .with_row(unavailable_row(
                "about-report",
                "Report a problem",
                "Open the issue tracker from inside the app.",
                "the build has no issue-tracker link",
            ))
            .with_row(unavailable_row(
                "about-licenses",
                "Third-party licenses",
                "Read the bundled notices in the app.",
                "the notices ship beside the binary, not in this pane",
            )),
    );
    PageModel::new(sections)
}
