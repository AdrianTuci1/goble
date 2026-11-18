//! Settings → Agent: the approval policy, the input mode, and the remote
//! configuration read from `[remote]` / `[remote_agents.*]`.
//!
//! Remote is configured **by hand in the file** — the app reports what the file
//! says and where it is, and nothing on this page provisions a sandbox or
//! reaches a provider. That is the existing product decision, and the rows say
//! so rather than offering a button that cannot work.

use goble_core::config::{ConfigFile, RemoteAgentView};

use super::super::fields;
use super::super::model::{ActionModel, PageModel, RowModel, SectionModel};
use super::super::state::{SettingsAction, SettingsUiState, StatusKind};
use super::{problem_section, secret_row, text_row, toggle_row};

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    if let Some(problems) = problem_section(state, ConfigFile::Config) {
        sections.push(problems);
    }
    let approval = fields::approval_index(state);
    sections.push(
        SectionModel::new("approval", "Approval")
            .with_description("How a proposed command reaches the shell.")
            .with_row(
                RowModel::new(
                    "app.approval_mode",
                    "Approval mode",
                    "How a proposed command reaches the shell.",
                )
                .select(
                    "app.approval_mode",
                    fields::approval_modes()
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    approval,
                ),
            )
            .with_row(toggle_row(
                state,
                "app.auto_approve",
                "Auto-approve questions",
                "Approve `ask_user` questions without waiting.",
                false,
            ))
            .with_row(toggle_row(
                state,
                "app.vim_mode",
                "Vim mode",
                "Modal editing in the rich input.",
                false,
            )),
    );
    sections.push(
        SectionModel::new("remote", "Remote")
            .with_description(
                "How a harness runs somewhere other than this machine. The file is the source of truth; nothing here provisions a sandbox.",
            )
            .with_row(
                RowModel::new(
                    "remote.mode",
                    "Remote mode",
                    "`platform` ships a worker to a [remote_agents.*] table; `gateway` points at a goble-gateway.",
                )
                .select(
                    "remote.mode",
                    vec!["Platform".to_string(), "Gateway".to_string()],
                    fields::remote_mode_index(state),
                ),
            )
            .with_row(text_row(
                state,
                "remote.gateway_url",
                "Gateway URL",
                "The gateway workers connect to.",
                fields::remote_text(state, "gateway_url"),
                "https://gateway.example.com",
            ))
            .with_row(secret_row(
                state,
                "remote.worker_token",
                "Worker token",
                "The token the gateway was started with.",
                fields::remote_worker_token_set(state),
            ))
            .with_row(
                RowModel::new(
                    "remote.summary",
                    "Status",
                    "What the configuration says right now.",
                )
                .read_only(state.remote_summary())
                .with_status(StatusKind::Muted, state.remote_summary()),
            ),
    );

    let mut agent_rows: Vec<RowModel> = Vec::new();
    let agents: Vec<RemoteAgentView> = state
        .config
        .as_ref()
        .map(|view| view.sections.remote_agents.clone())
        .unwrap_or_default();
    for agent in &agents {
        let status = if agent.enabled {
            if agent.missing_credentials.is_empty() {
                "ready".to_string()
            } else {
                format!("missing {}", agent.missing_credentials.join(", "))
            }
        } else {
            "disabled".to_string()
        };
        let row = RowModel::new(
            format!("remote-agent.{}", agent.name),
            agent.name.clone(),
            format!("{} · {}", agent.provider, status),
        )
        .read_only(status);
        agent_rows.push(row);
    }
    if agent_rows.is_empty() {
        agent_rows.push(
            RowModel::new(
                "remote-agents-empty",
                "No remote agents configured",
                "Add a [remote_agents.<name>] table to config.toml to offer a sandbox provider.",
            )
            .with_status(StatusKind::Muted, "Turns run on this machine."),
        );
    }
    sections.push(
        SectionModel::new("remote-agents", "Environments")
            .with_description("One [remote_agents.<name>] table declares a sandbox provider the harness can be shipped to.")
            .with_header_action(ActionModel::new(
                "Open config.toml",
                SettingsAction::OpenFile(ConfigFile::Config),
            ))
            .with_rows(agent_rows)
            .with_row(super::unavailable_row(
                "remote-deploy",
                "Deploy a sandbox from Settings",
                "Ship a worker to a provider from this page.",
                "the page reports the file; provisioning is done by the app's own actions",
            )),
    );
    // The agent-led setup path stays: a conversation that asks for what is
    // missing and writes it with `goble-cli setup`. It is the one writer of
    // these sections besides the file itself.
    sections.push(
        SectionModel::new("agent-setup", "Set up with the agent")
            .with_description(
                "Open a conversation that asks for what is missing and writes it for you.",
            )
            .with_row(
                RowModel::new(
                    "setup.remote",
                    "Set up Remote",
                    "The agent asks for a gateway or a sandbox provider and writes the table.",
                )
                .buttons(vec![ActionModel::new(
                    "Start setup",
                    SettingsAction::SetupRemoteWithAgent,
                )
                .with_icon("robot")
                .primary()]),
            )
            .with_row(
                RowModel::new(
                    "setup.hub",
                    "Set up the hub",
                    "The same conversation for the hub's URL and token.",
                )
                .buttons(vec![ActionModel::new(
                    "Start setup",
                    SettingsAction::SetupHubWithAgent,
                )
                .with_icon("robot")
                .primary()]),
            ),
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
