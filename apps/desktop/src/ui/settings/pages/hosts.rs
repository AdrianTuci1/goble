//! Settings → Conversations: the conversation list is the sidebar's job, while
//! the saved SSH bindings this page edits are in `hosts.toml`.
//!
//! A saved host is a name → target binding the app can open a pane on; the SSH
//! config rows are read-only because goble never writes the user's `~/.ssh`
//! (the existing product rule). Saved hosts and `~/.ssh/config` hosts are
//! both SSH host bindings, so they live together under one clearly labelled
//! "SSH hosts" section. Nothing on this page reads key material.

use goble_core::config::{ConfigFile, HostView};

use super::super::model::{ActionModel, PageModel, RowModel, SectionModel};
use super::super::state::{HostForm, SettingsAction, SettingsUiState, StatusKind};
use super::{add_action, problem_section, unavailable_row};

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    sections.push(
        SectionModel::new("conversations", "Conversations")
            .with_description(
                "Conversations open in panes and are listed in the sidebar; this page holds their SSH host bindings below.",
            )
            .with_row(
                RowModel::new(
                    "conversations.source",
                    "Conversation list",
                    "New, open, rename, star and archive conversations from the sidebar.",
                )
                .read_only("Sidebar")
                .with_status(StatusKind::Muted, "Managed from the sidebar"),
            )
            .with_row(
                RowModel::new(
                    "conversations.organize",
                    "Conversation organization",
                    "Groups, starred conversations and archived conversations follow the sidebar's project grouping.",
                )
                .read_only("Sidebar")
                .with_status(StatusKind::Muted, "Managed from the sidebar"),
            ),
    );
    if let Some(problems) = problem_section(state, ConfigFile::Hosts) {
        sections.push(problems);
    }

    let hosts = state.saved_hosts().to_vec();
    let mut rows: Vec<RowModel> = Vec::new();
    rows.push(
        RowModel::new(
            "saved-hosts",
            "Saved hosts",
            "Bindings recorded in hosts.toml; the file is the source of truth.",
        )
        .read_only(if hosts.is_empty() {
            "none".to_string()
        } else {
            format!("{} saved", hosts.len())
        })
        .with_status(StatusKind::Muted, "Add or edit them in this section."),
    );
    for host in &hosts {
        rows.push(host_row(state, host));
    }
    if hosts.is_empty() {
        rows.push(
            RowModel::new(
                "hosts-empty",
                "No saved hosts",
                "A saved host is a name, a hostname and optionally a user, port and identity file.",
            )
            .with_status(
                StatusKind::Muted,
                "Add one to open a pane on it from the app.",
            ),
        );
    }

    // ~/.ssh/config is read, never written.
    rows.push(
        RowModel::new(
            "ssh-config",
            "SSH config",
            "Read from ~/.ssh/config; Goble never writes this file.",
        )
        .read_only("~/.ssh/config")
        .with_status(StatusKind::Muted, "Read-only"),
    );
    let mut ssh_rows: Vec<RowModel> = Vec::new();
    if let Some(ssh) = state.ssh_hosts.as_ref() {
        for host in &ssh.hosts {
            ssh_rows.push(
                RowModel::new(
                    format!("ssh.{}", host.alias),
                    host.alias.clone(),
                    format!("{} · {}", host.target(), host.identity_files.join(", ")),
                )
                .read_only(format!("{}:{}", host.hostname, host.port)),
            );
        }
    }
    if ssh_rows.is_empty() {
        ssh_rows.push(
            RowModel::new(
                "ssh-empty",
                "No SSH hosts found",
                "`~/.ssh/config` has no concrete host blocks, or the directory is unreadable.",
            )
            .with_status(StatusKind::Muted, "Read-only; Goble never writes ~/.ssh."),
        );
    }
    rows.extend(ssh_rows);
    sections.push(
        SectionModel::new("ssh-hosts", "SSH hosts")
            .with_description(
                "Saved host bindings from hosts.toml plus the read-only hosts in ~/.ssh/config. Goble never writes ~/.ssh.",
            )
            .with_header_action(add_action("Add host", SettingsAction::NewHost))
            .with_rows(rows),
    );
    sections.push(
        SectionModel::new("hosts-unavailable", "SSH options not in this build")
            .with_row(unavailable_row(
                "ssh-keys",
                "Manage SSH keys",
                "Generate or import a key from Settings.",
                "key files are read only, never written",
            ))
            .with_row(unavailable_row(
                "known-hosts",
                "Known hosts",
                "Edit `~/.ssh/known_hosts` from here.",
                "the file is read only",
            )),
    );
    sections.push(
        SectionModel::new("files", "This file").with_row(super::open_file_row(
            ConfigFile::Hosts,
            "hosts.toml",
            &state
                .hosts
                .as_ref()
                .map(|view| view.path.clone())
                .unwrap_or_else(|| "~/.goble/hosts.toml".to_string()),
        )),
    );
    PageModel::new(sections)
}

fn host_row(state: &SettingsUiState, host: &HostView) -> RowModel {
    let mut hint = host.host.clone();
    if let Some(user) = host.user.clone() {
        hint = format!("{user}@{hint}");
    }
    if let Some(port) = host.port {
        hint.push_str(&format!(":{port}"));
    }
    if let Some(identity) = host.identity.clone() {
        hint.push_str(&format!(" · identity {identity}"));
    }
    let mut row =
        RowModel::new(format!("host.{}", host.name), host.name.clone(), hint).buttons(vec![
            ActionModel::new("Test", SettingsAction::TestHost(host.name.clone())).with_icon("plug"),
            ActionModel::new("Edit", SettingsAction::EditHost(host.name.clone())).with_icon("edit"),
            ActionModel::new("Delete", SettingsAction::DeleteHost(host.name.clone()))
                .with_icon("trash-02")
                .destructive(),
        ]);
    if let Some((kind, text)) = state.test_results.get(&format!("host.{}", host.name)) {
        row = row.with_status(*kind, text.clone());
    }
    row
}

pub(super) fn host_form_section(state: &SettingsUiState, form: &HostForm) -> SectionModel {
    let title = match &form.editing {
        Some(name) => format!("Edit host · {name}"),
        None => "Add host".to_string(),
    };
    let field = |id: &str, fallback: &str| state.draft(id, fallback).to_string();
    SectionModel::new("host-form", title)
        .with_description("Saved as one [hosts.<name>] entry in hosts.toml.")
        .with_header_action(ActionModel::new("Cancel", SettingsAction::CancelForm))
        .with_row(
            RowModel::new("host-form.name", "Name", "The row's name in this page.").text(
                "host-form.name",
                field("host-form.name", &form.name),
                "staging",
            ),
        )
        .with_row(
            RowModel::new(
                "host-form.host",
                "Host",
                "The hostname or IP to connect to.",
            )
            .text(
                "host-form.host",
                field("host-form.host", &form.host),
                "staging.example.com",
            ),
        )
        .with_row(
            RowModel::new(
                "host-form.user",
                "User",
                "The SSH user, if not your local one.",
            )
            .text(
                "host-form.user",
                field("host-form.user", &form.user),
                "deploy",
            ),
        )
        .with_row(
            RowModel::new("host-form.port", "Port", "Defaults to 22.").text(
                "host-form.port",
                field("host-form.port", &form.port),
                "22",
            ),
        )
        .with_row(
            RowModel::new(
                "host-form.identity",
                "Identity file",
                "The path to the private key, if not the SSH default.",
            )
            .text(
                "host-form.identity",
                field("host-form.identity", &form.identity),
                "~/.ssh/id_ed25519",
            ),
        )
        .with_row(
            RowModel::new(
                "host-form.actions",
                "Save",
                "Test connects with BatchMode and a five-second timeout.",
            )
            .buttons(vec![
                ActionModel::new("Save host", SettingsAction::SaveHostForm).primary(),
                ActionModel::new("Cancel", SettingsAction::CancelForm),
            ]),
        )
}
