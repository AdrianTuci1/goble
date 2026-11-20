//! Settings → Browser: the embedded preview surface this build does not have,
//! and the real Extensions/MCP servers it does.
//!
//! A connector's transport is one of `url` (streamable HTTP) or `command` +
//! `args` (stdio); the form writes one and unsets the other, because a table
//! with both is ambiguous to the runtime registry. The connector rows are the
//! same real rows the old Connectors page owned, reused here as the useful part
//! of a Browser page with no browser backend.

use goble_core::config::{ConfigFile, McpView};

use super::super::model::{ActionModel, PageModel, RowModel, SectionModel};
use super::super::state::{ConnectorForm, SettingsAction, SettingsUiState, StatusKind};
use super::{add_action, problem_section};

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    sections.push(
        SectionModel::new("browser", "Browser")
            .with_description(
                "An embedded preview surface is not included in this build. The Extensions below are real MCP servers the agent can use.",
            )
            .with_row(super::unavailable_row(
                "browser-surface",
                "Browser preview",
                "Open a URL in an embedded preview surface inside Goble.",
                "browser preview is unavailable in this build",
            ))
            .with_row(super::unavailable_row(
                "browser-inspector",
                "Page inspector",
                "Inspect a previewed page's DOM and network requests.",
                "there is no browser surface to inspect",
            )),
    );
    if let Some(problems) = problem_section(state, ConfigFile::Config) {
        sections.push(problems);
    }
    let servers: Vec<McpView> = state
        .config
        .as_ref()
        .map(|view| view.sections.mcp.clone())
        .unwrap_or_default();
    let mut rows = Vec::new();
    for server in &servers {
        rows.push(connector_row(state, server));
    }
    if rows.is_empty() {
        rows.push(
            RowModel::new(
                "mcp-empty",
                "No connectors yet",
                "A connector is a [mcp_servers.<name>] table: an HTTP endpoint or a stdio command.",
            )
            .with_status(
                StatusKind::Muted,
                "Add one to offer its tools to the agent.",
            ),
        );
    }
    sections.push(
        SectionModel::new("mcp", "MCP servers")
            .with_description(
                "Extensions the agent can use, declared in [mcp_servers.<name>] in config.toml and synced into the runtime registry on save. A server is enabled unless it says otherwise.",
            )
            .with_header_action(add_action("Add connector", SettingsAction::NewConnector))
            .with_rows(rows),
    );
    sections.push(
        SectionModel::new(
            "connector-unavailable",
            "Connector options not in this build",
        )
        .with_row(super::unavailable_row(
            "onepassword",
            "1Password secrets",
            "Pull a connector's credentials from a 1Password vault.",
            "no 1Password integration",
        ))
        .with_row(super::unavailable_row(
            "mcp-catalog",
            "Connector catalog",
            "Browse a hosted catalog of MCP servers and install one.",
            "no connector marketplace",
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

fn connector_row(state: &SettingsUiState, server: &McpView) -> RowModel {
    let mut hint = match server.transport() {
        "http" => format!("http · {}", server.url.clone().unwrap_or_default()),
        "stdio" => {
            let mut line = format!("stdio · {}", server.command.clone().unwrap_or_default());
            if !server.args.is_empty() {
                line.push(' ');
                line.push_str(&server.args.join(" "));
            }
            line
        }
        _ => "no transport configured".to_string(),
    };
    if !server.enabled {
        hint.push_str(" · disabled");
    }
    let path = format!("mcp_servers.{}.enabled", server.name);
    let buttons = vec![
        ActionModel::new("Test", SettingsAction::TestConnector(server.name.clone()))
            .with_icon("plug"),
        ActionModel::new("Edit", SettingsAction::EditConnector(server.name.clone()))
            .with_icon("edit"),
        if server.enabled {
            ActionModel::new("Disable", SettingsAction::SetToggle { path, value: false })
        } else {
            ActionModel::new("Enable", SettingsAction::SetToggle { path, value: true })
        },
        ActionModel::new(
            "Delete",
            SettingsAction::DeleteConnector(server.name.clone()),
        )
        .with_icon("trash-02")
        .destructive(),
    ];
    let mut row =
        RowModel::new(format!("mcp.{}", server.name), server.name.clone(), hint).buttons(buttons);
    if let Some((kind, text)) = state.test_results.get(&format!("mcp.{}", server.name)) {
        row = row.with_status(*kind, text.clone());
    }
    row
}

pub(super) fn connector_form_section(
    state: &SettingsUiState,
    form: &ConnectorForm,
) -> SectionModel {
    let title = match &form.editing {
        Some(name) => format!("Edit connector · {name}"),
        None => "Add connector".to_string(),
    };
    let field = |id: &str, fallback: &str| state.draft(id, fallback).to_string();
    let mut section = SectionModel::new("connector-form", title)
        .with_description("Saved as an [mcp_servers.<name>] table in config.toml.")
        .with_header_action(ActionModel::new("Cancel", SettingsAction::CancelForm))
        .with_row(
            RowModel::new(
                "connector-form.name",
                "Name",
                "The table name and the registry name.",
            )
            .text(
                "connector-form.name",
                field("connector-form.name", &form.name),
                "penpot",
            ),
        )
        .with_row(
            RowModel::new(
                "connector-form.transport",
                "Transport",
                "HTTP talks to a streamable endpoint; stdio runs a program.",
            )
            .select(
                "connector-form.transport",
                vec!["Streamable HTTP".to_string(), "stdio command".to_string()],
                form.transport,
            ),
        );
    if form.transport == 0 {
        section = section.with_row(
            RowModel::new("connector-form.url", "URL", "The streamable-HTTP endpoint.").text(
                "connector-form.url",
                field("connector-form.url", &form.url),
                "http://localhost:4400/mcp",
            ),
        );
    } else {
        section = section
            .with_row(
                RowModel::new(
                    "connector-form.command",
                    "Command",
                    "The stdio program to run.",
                )
                .text(
                    "connector-form.command",
                    field("connector-form.command", &form.command),
                    "npx",
                ),
            )
            .with_row(
                RowModel::new(
                    "connector-form.args",
                    "Arguments",
                    "Arguments for the command, separated by spaces.",
                )
                .text(
                    "connector-form.args",
                    field("connector-form.args", &form.args),
                    "-y @modelcontextprotocol/server-filesystem ~/code",
                ),
            );
    }
    section = section
        .with_row(
            RowModel::new(
                "connector-form.enabled",
                "Enabled",
                "Whether the server is synced into the runtime registry.",
            )
            .toggle("connector-form.enabled", form.enabled),
        )
        .with_row(
            RowModel::new(
                "connector-form.actions",
                "Save",
                "Test runs the server's initialize and tools/list before saving.",
            )
            .buttons(vec![
                ActionModel::new(
                    "Test",
                    SettingsAction::TestConnector(form.editing.clone().unwrap_or_default()),
                )
                .with_icon("plug")
                .disabled(),
                ActionModel::new("Save connector", SettingsAction::SaveConnectorForm).primary(),
                ActionModel::new("Cancel", SettingsAction::CancelForm),
            ]),
        );
    section
}
