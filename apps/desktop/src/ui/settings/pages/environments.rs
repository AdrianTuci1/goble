//! Settings → Environments: the groups in `environment.toml`.
//!
//! Two levels only — group name → variable name → value — and a value is a
//! literal string or an inline `{ vault = "<key>" }` reference. A literal value
//! is never projected back (the view carries `SecretState { set }` instead), so
//! a variable row says "value configured" or "vault · <key>"; a new value is
//! typed and written through the same comment-preserving document editor.

use goble_core::config::ConfigFile;

use super::super::model::{ActionModel, PageModel, RowModel, SectionModel};
use super::super::state::{SettingsAction, SettingsUiState, StatusKind};
use super::{problem_section, unavailable_row};

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    if let Some(problems) = problem_section(state, ConfigFile::Environment) {
        sections.push(problems);
    }
    // New group: one field and a button, in its own card.
    sections.push(
        SectionModel::new("new-group", "Add a group")
            .with_description(
                "A group is a named set of environment variables a session can start with. Its name is the table key in environment.toml.",
            )
            .with_row(
                RowModel::new(
                    "env.new_group",
                    "Group name",
                    "Letters, digits, `-` and `_`; the table is created when you add it.",
                )
                .text(
                    "env.new_group",
                    state.draft("env.new_group", &state.env_new_group),
                    "production",
                ),
            )
            .with_row(
                RowModel::new(
                    "env.create_group",
                    "Create",
                    "Create the group the field names.",
                )
                .buttons(vec![ActionModel::new(
                    "Add group",
                    SettingsAction::EnvCreateGroup,
                )
                .with_icon("plus")
                .primary()]),
            ),
    );

    let groups = state
        .environment
        .as_ref()
        .map(|view| view.sections.environment.clone())
        .unwrap_or_default();
    if groups.is_empty() {
        sections.push(
            SectionModel::new("groups-empty", "Groups")
                .with_description("No groups are declared in environment.toml yet.")
                .with_row(
                    RowModel::new(
                        "env.empty",
                        "No environment groups",
                        "Add one above; the file is created on the first write.",
                    )
                    .with_status(StatusKind::Muted, "Nothing to show."),
                )
                .with_footer_note(),
        );
    }
    for group in &groups {
        let mut rows = Vec::new();
        for variable in &group.variables {
            let (value_kind, button) = match &variable.value {
                goble_core::config::EnvValueView::Value(_) => {
                    ("value configured · never shown".to_string(), "Remove")
                }
                goble_core::config::EnvValueView::Vault { vault } => {
                    (format!("vault · {vault}"), "Remove")
                }
            };
            rows.push(
                RowModel::new(
                    format!("env.{}.{}", group.name, variable.name),
                    variable.name.clone(),
                    value_kind,
                )
                .buttons(vec![
                    ActionModel::new("Open", SettingsAction::EnvOpenGroup(group.name.clone()))
                        .with_icon("chevron-down"),
                    ActionModel::new(
                        button,
                        SettingsAction::EnvDeleteVariable {
                            group: group.name.clone(),
                            name: variable.name.clone(),
                        },
                    )
                    .with_icon("trash-02")
                    .destructive(),
                ]),
            );
        }
        // Add a variable to this group: name + write-only value + button.
        rows.push(
            RowModel::new(
                format!("env.{}.new-name", group.name),
                "New variable",
                "The variable's name, exactly as the program expects it.",
            )
            .text(
                &format!("env.{}.new-name", group.name),
                state.draft(&format!("env.{}.new-name", group.name), ""),
                "API_BASE",
            ),
        );
        rows.push(
            RowModel::new(
                format!("env.{}.new-value", group.name),
                "Value",
                "Write-only: the value is written to the file and never shown again.",
            )
            .secret(
                &format!("env.{}.new-value", group.name),
                state.draft(&format!("env.{}.new-value", group.name), ""),
                false,
            ),
        );
        rows.push(
            RowModel::new(
                format!("env.{}.add", group.name),
                "Add variable",
                "Write the name/value pair into this group.",
            )
            .buttons(vec![ActionModel::new(
                "Add variable",
                SettingsAction::EnvAddVariable {
                    group: group.name.clone(),
                },
            )
            .with_icon("plus")
            .primary()]),
        );
        let mut section =
            SectionModel::new(format!("env-group.{}", group.name), group.name.clone())
                .with_description(format!(
                    "{} variable(s) · [groups.{}]",
                    group.variables.len(),
                    group.name
                ))
                .with_header_action(ActionModel::new(
                    "Delete group",
                    SettingsAction::EnvDeleteGroup(group.name.clone()),
                ))
                .with_rows(rows);
        if state.env_open_group.as_deref() == Some(group.name.as_str()) {
            section = section.with_description(format!(
                "{} variable(s) · the group the Add rows above write into",
                group.variables.len()
            ));
        }
        sections.push(section);
    }
    sections.push(
        SectionModel::new("env-unavailable", "Not in this build").with_row(unavailable_row(
            "env-vault-create",
            "Create a vault entry",
            "Write `VAR = { vault = \"key\" }` from this page.",
            "vault-backed values are declared in the file",
        )),
    );
    sections.push(
        SectionModel::new("files", "This file").with_row(super::open_file_row(
            ConfigFile::Environment,
            "environment.toml",
            &state
                .environment
                .as_ref()
                .map(|view| view.path.clone())
                .unwrap_or_else(|| "~/.goble/environment.toml".to_string()),
        )),
    );
    PageModel::new(sections)
}

impl SectionModel {
    fn with_footer_note(mut self) -> Self {
        // An empty-groups card keeps its rows; the note is the description.
        self.description = "Add one above; the file is created on the first write.".to_string();
        self
    }
}
