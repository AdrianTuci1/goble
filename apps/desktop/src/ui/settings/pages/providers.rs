//! Settings → Providers: the provider accounts this build can really reach.
//!
//! Three layers, all file-backed:
//!
//! - **Models** — the `[model.<slug>]` tables and the `[models] default` pick;
//! - **Accounts** — one row per configured provider (the API-key/base-URL
//!   account) plus the local agent CLIs the service detects on `PATH`. The
//!   subscription sign-ins and the ACP registry stay visible, disabled and
//!   honest ("not available in this build");
//! - **Agent CLIs** — one foldable group per catalog harness, with its real
//!   detection status, launch command and the `[harness.<id>]` override form.
//!
//! The Add/Edit forms are side sheets (`state.form`); see `pages::form_section`.
//! Every write goes through the same revision-fenced document editor.

use goble_core::config::{ConfigFile, HarnessView, ModelView};
use goble_core::harness::HarnessStatus;

use super::super::fields;
use super::super::model::{ActionModel, PageModel, RowModel, SectionModel};
use super::super::state::{HarnessForm, SettingsAction, SettingsUiState, StatusKind};
use super::{add_action, problem_section};

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    if let Some(problems) = problem_section(state, ConfigFile::Config) {
        sections.push(problems);
    }

    // [models] default — the model new sessions start on.
    let models = state.models();
    let mut default_section = SectionModel::new("default-model", "Default model").with_description(
        "The model new sessions start on. Written as [models] default in config.toml.",
    );
    if models.is_empty() {
        default_section = default_section.with_row(
            RowModel::new(
                "models.default",
                "No models configured",
                "Add a model below; its table is what the picker and this row read.",
            )
            .disabled()
            .with_status(StatusKind::Muted, "The file has no [model.*] table yet."),
        );
    } else {
        let options: Vec<String> = models.iter().map(|model| model.slug.clone()).collect();
        let selected = fields::default_model_index(state);
        default_section = default_section.with_row(
            RowModel::new(
                "models.default",
                "Default model",
                "The slug new sessions start on.",
            )
            .select("models.default", options, selected),
        );
    }
    sections.push(default_section);

    // [model.*] — one row per configured model.
    let mut model_rows: Vec<RowModel> = Vec::new();
    for model in models {
        model_rows.push(model_row(state, model));
    }
    if model_rows.is_empty() {
        model_rows.push(
            RowModel::new(
                "models-empty",
                "No providers yet",
                "A provider is a [model.<slug>] table: an endpoint, a model id and its key.",
            )
            .with_status(StatusKind::Muted, "Add one to send turns to an API."),
        );
    }
    sections.push(
        SectionModel::new("models", "Models")
            .with_description(
                "Each row is one [model.<slug>] table. The key is written inline in this 0600 file and never read back.",
            )
            .with_header_action(add_action("Add model", SettingsAction::NewModel))
            .with_rows(model_rows),
    );

    sections.push(accounts_section(state));
    // The agent CLIs are a stack of folds: one heading per CLI, its rows only
    // while the fold is open (the open state lives in `settings.folds`).
    sections.push(
        SectionModel::new("harnesses", "Agent CLIs").with_description(
            "The agent CLIs the catalog knows, one fold each. Detection resolves the launch command on PATH; Configure writes a [harness.<id>] override.",
        ),
    );
    sections.extend(agent_cli_folds(state));

    sections.push(
        SectionModel::new("files", "This file").with_row(super::open_file_row(
            ConfigFile::Config,
            "config.toml",
            &state.file_line(),
        )),
    );
    PageModel::new(sections)
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

/// The connected accounts: a provider account per `[model.*]` table (the API
/// key / base URL route this build has) and a detection row per local agent
/// CLI. Subscription sign-ins are the honest disabled rows at the bottom.
fn accounts_section(state: &SettingsUiState) -> SectionModel {
    let mut section = SectionModel::new("accounts", "Accounts").with_description(
        "What this build can really connect to: model providers whose key and base URL live in config.toml, and agent CLIs detected on PATH. Subscription sign-ins are not wired in.",
    );
    let models = state.models();
    if models.is_empty() {
        section = section.with_row(
            RowModel::new(
                "account.none",
                "No provider accounts",
                "Add a model provider below; its [model.<slug>] table is the account.",
            )
            .with_status(StatusKind::Muted, "Nothing connected yet."),
        );
    }
    for model in models {
        section = section.with_row(account_row(state, model));
    }

    if state.harnesses.is_empty() {
        section = section.with_row(
            RowModel::new(
                "account.harnesses",
                "Local agent CLIs",
                "Detection has not run yet: the service scans PATH from the catalog.",
            )
            .buttons(vec![ActionModel::new(
                "Scan PATH",
                SettingsAction::RefreshHarnesses,
            )
            .with_icon("refresh")])
            .with_status(
                StatusKind::Muted,
                "Scan PATH to list the harnesses this machine has installed.",
            ),
        );
    } else {
        // The connected local CLIs are the ones this machine can actually run:
        // installed on PATH, or given a configured override. The rest of the
        // catalog is one honest summary row rather than thirty-nine rows of
        // "not found".
        let connected: Vec<&HarnessStatus> = state
            .harnesses
            .iter()
            .filter(|harness| harness.installed || harness.configured)
            .collect();
        for harness in &connected {
            section = section.with_row(account_harness_row(state, harness));
        }
        let missing = state.harnesses.len().saturating_sub(connected.len());
        if connected.is_empty() {
            section = section.with_row(
                RowModel::new(
                    "account.harnesses.none",
                    "No local agent CLIs detected",
                    "None of the catalog's harness commands resolve on PATH; the Agent CLIs list below still lets you configure one.",
                )
                .buttons(vec![ActionModel::new(
                    "Scan PATH",
                    SettingsAction::RefreshHarnesses,
                )
                .with_icon("refresh")])
                .with_status(
                    StatusKind::Warning,
                    "No harness installed. Install one or point [harness] command at it.",
                ),
            );
        } else if missing > 0 {
            section = section.with_row(
                RowModel::new(
                    "account.harnesses.missing",
                    format!("{missing} more agent CLIs"),
                    "In the catalog, not installed and not configured: expand Agent CLIs below to configure one.",
                )
                .read_only(format!("{missing} not detected")),
            );
        }
    }

    // What this build cannot do: the hosted accounts.
    section = section
        .with_row(super::unavailable_row(
            "oauth-codex",
            "Sign in with Codex",
            "Uses a ChatGPT subscription instead of an API key.",
            "no OAuth sign-in",
        ))
        .with_row(super::unavailable_row(
            "oauth-claude",
            "Sign in with Claude",
            "Uses a Claude subscription instead of an API key.",
            "no OAuth sign-in",
        ))
        .with_row(super::unavailable_row(
            "oauth-copilot",
            "Sign in with GitHub Copilot",
            "Uses a Copilot subscription instead of an API key.",
            "no OAuth sign-in",
        ))
        .with_row(super::unavailable_row(
            "acp-registry",
            "Browse the ACP provider registry",
            "Download an agent CLI from a registry.",
            "no provider download or ACP registry",
        ));
    section
}

fn account_row(state: &SettingsUiState, model: &ModelView) -> RowModel {
    let mut hint = format!(
        "{} · {}",
        model.model_id,
        model
            .base_url
            .as_deref()
            .unwrap_or("provider default endpoint")
    );
    if model.is_default {
        hint.push_str(" · default");
    }
    let (kind, status) = match model.api_key_source {
        goble_core::config::ApiKeySource::Inline => (
            StatusKind::Success,
            "Connected · API key stored in config.toml".to_string(),
        ),
        goble_core::config::ApiKeySource::Env => (
            StatusKind::Info,
            format!(
                "Connected · API key from environment: {}",
                model.api_key_env.clone().unwrap_or_default()
            ),
        ),
        goble_core::config::ApiKeySource::Missing => (
            StatusKind::Warning,
            "Not connected · no API key configured (Edit to add one)".to_string(),
        ),
    };
    let mut buttons = vec![
        ActionModel::new("Test", SettingsAction::TestModel(model.slug.clone())).with_icon("plug"),
        ActionModel::new("Edit", SettingsAction::EditModel(model.slug.clone())).with_icon("edit"),
    ];
    if !model.is_default {
        buttons.push(
            ActionModel::new(
                "Make default",
                SettingsAction::SetDefaultModel(model.slug.clone()),
            )
            .with_icon("check"),
        );
    }
    buttons.push(
        ActionModel::new("Delete", SettingsAction::DeleteModel(model.slug.clone()))
            .with_icon("trash-02")
            .destructive(),
    );
    let mut row = RowModel::new(
        format!("account.model.{}", model.slug),
        model.name.clone().unwrap_or_else(|| model.slug.clone()),
        hint,
    )
    .buttons(buttons)
    .with_status(kind, status);
    if let Some((kind, text)) = state
        .test_results
        .get(&format!("account.model.{}", model.slug))
        .or_else(|| state.test_results.get(&format!("model.{}", model.slug)))
    {
        row = row.with_status(*kind, text.clone());
    }
    row.with_swatch(model_colour(model))
}

/// A provider row's chip: the key state, in the status palette (green when a
/// key is configured, amber when it is missing). Painted as a 14px swatch so
/// the Accounts list scans like T3's provider instance cards.
fn model_colour(model: &ModelView) -> goble_ui::color::ColorU {
    let scheme = goble_ui::theme::ThemePreset::GobleNight.colors();
    match model.api_key_source {
        goble_core::config::ApiKeySource::Inline => scheme.success,
        goble_core::config::ApiKeySource::Env => scheme.focus,
        goble_core::config::ApiKeySource::Missing => scheme.warning,
    }
}

fn account_harness_row(state: &SettingsUiState, harness: &HarnessStatus) -> RowModel {
    let (button, want) = if harness.enabled {
        ("Disable", false)
    } else {
        ("Enable", true)
    };
    let buttons = vec![
        ActionModel::new("Test", SettingsAction::TestHarness(harness.id.clone())).with_icon("plug"),
        ActionModel::new("Configure", SettingsAction::EditHarness(harness.id.clone()))
            .with_icon("edit"),
        ActionModel::new(
            button,
            SettingsAction::SetToggle {
                path: format!("harness.{}.enabled", harness.id),
                value: want,
            },
        ),
    ];
    let mut row = RowModel::new(
        format!("account.harness.{}", harness.id),
        harness.name.clone(),
        harness_command(harness),
    )
    .buttons(buttons)
    .with_status(harness_status_kind(harness), harness_status_line(harness));
    if let Some((kind, text)) = state.test_results.get(&format!("harness.{}", harness.id)) {
        row = row.with_status(*kind, text.clone());
    }
    let scheme = goble_ui::theme::ThemePreset::GobleNight.colors();
    let colour = if !harness.enabled {
        scheme.muted
    } else if harness.installed {
        scheme.success
    } else {
        scheme.warning
    };
    row.with_swatch(colour)
}

fn harness_status_kind(harness: &HarnessStatus) -> StatusKind {
    if !harness.enabled {
        StatusKind::Muted
    } else if let Some(error) = harness.error.as_ref() {
        if !error.is_empty() {
            StatusKind::Error
        } else if harness.installed {
            StatusKind::Success
        } else {
            StatusKind::Warning
        }
    } else if harness.installed {
        StatusKind::Success
    } else {
        StatusKind::Warning
    }
}

fn harness_status_line(harness: &HarnessStatus) -> String {
    if !harness.enabled {
        return "Disabled · [harness] enabled = false".to_string();
    }
    if let Some(error) = harness.error.as_ref() {
        if !error.is_empty() {
            return error.clone();
        }
    }
    if harness.installed {
        match harness.path.as_deref() {
            Some(path) if !path.is_empty() => format!("Installed · {path}"),
            _ => "Installed on PATH".to_string(),
        }
    } else {
        format!("Not found on PATH · {}", harness.command)
    }
}

fn harness_command(harness: &HarnessStatus) -> String {
    if harness.args.is_empty() {
        harness.command.clone()
    } else {
        format!("{} {}", harness.command, harness.args.join(" "))
    }
}

// ---------------------------------------------------------------------------
// Agent CLIs (one fold per harness)
// ---------------------------------------------------------------------------

/// One foldable group per agent CLI: heading = name + detection summary, body =
/// the launch command, the enabled switch and the override/test actions.
fn agent_cli_folds(state: &SettingsUiState) -> Vec<SectionModel> {
    if state.harnesses.is_empty() {
        let fallback: Vec<HarnessView> = state
            .config
            .as_ref()
            .map(|view| view.sections.harnesses.clone())
            .unwrap_or_default();
        if fallback.is_empty() {
            return vec![
                SectionModel::new("harnesses-empty", "No agent CLIs").with_row(
                    RowModel::new(
                        "harnesses-empty.scan",
                        "Detection has not run",
                        "Scan PATH with the catalog's harness commands.",
                    )
                    .buttons(vec![ActionModel::new(
                        "Scan PATH",
                        SettingsAction::RefreshHarnesses,
                    )
                    .with_icon("refresh")])
                    .with_status(
                        StatusKind::Muted,
                        "The scan resolves each CLI on PATH; it installs nothing.",
                    ),
                ),
            ];
        }
        return fallback
            .iter()
            .map(|harness| harness_fold_from_config(state, harness))
            .collect();
    }
    state
        .harnesses
        .iter()
        .map(|harness| harness_fold(state, harness))
        .collect()
}

fn harness_fold(state: &SettingsUiState, harness: &HarnessStatus) -> SectionModel {
    let fold_id = format!("harness.{}", harness.id);
    let summary = format!("{} · {}", harness_status_label(harness), harness.command);
    let mut section = cli_section(
        state,
        &fold_id,
        &harness.name,
        &summary,
        &harness.command,
        &harness.args,
        harness.installed,
        harness.path.clone(),
        harness.enabled,
        harness.configured,
        &format!("harness.{}", harness.id),
        &harness.id,
    );
    if let Some(error) = harness.error.clone() {
        if !error.is_empty() {
            section = section.with_row(
                RowModel::new(
                    format!("harness.{}.error", harness.id),
                    "Probe",
                    "The last detection error.",
                )
                .read_only(error.clone())
                .with_status(StatusKind::Error, error),
            );
        }
    }
    section
}

fn harness_fold_from_config(state: &SettingsUiState, harness: &HarnessView) -> SectionModel {
    let fold_id = format!("harness.{}", harness.id);
    let summary = format!(
        "{} · {}",
        if harness.enabled {
            "not scanned"
        } else {
            "disabled"
        },
        harness
            .command
            .clone()
            .unwrap_or_else(|| harness.name.clone())
    );
    cli_section(
        state,
        &fold_id,
        &harness.name,
        &summary,
        harness.command.as_deref().unwrap_or(&harness.name),
        &harness.args,
        false,
        None,
        harness.enabled,
        harness.configured,
        &format!("harness.{}", harness.id),
        &harness.id,
    )
}

fn harness_status_label(harness: &HarnessStatus) -> &'static str {
    if !harness.enabled {
        "disabled"
    } else if harness.installed {
        "installed"
    } else {
        "not found"
    }
}

#[allow(clippy::too_many_arguments)]
fn cli_section(
    state: &SettingsUiState,
    fold_id: &str,
    name: &str,
    summary: &str,
    command: &str,
    args: &[String],
    installed: bool,
    path: Option<String>,
    enabled: bool,
    configured: bool,
    row_prefix: &str,
    harness_id: &str,
) -> SectionModel {
    let open = state.folds.contains(fold_id);
    let launch = if args.is_empty() {
        command.to_string()
    } else {
        format!("{command} {}", args.join(" "))
    };
    let section = SectionModel::new(fold_id, name)
        .with_description(summary)
        .with_fold(open, SettingsAction::ToggleFold(fold_id.to_string()));
    if !open {
        return section;
    }
    let path_line = match path.as_deref() {
        Some(path) if !path.is_empty() => format!("{path} · {launch}"),
        _ => format!("catalog command · {launch}"),
    };
    let section_rows = vec![
        RowModel::new(
            format!("{row_prefix}.command"),
            "Launch command",
            if configured {
                "Override from [harness] in config.toml."
            } else {
                "From the built-in catalog."
            },
        )
        .read_only(launch.clone())
        .with_status(StatusKind::Muted, path_line),
        RowModel::new(
            format!("{row_prefix}.enabled"),
            "Enabled",
            "Offer this CLI in the harness picker.",
        )
        .toggle(format!("{row_prefix}.enabled"), enabled),
        RowModel::new(
            format!("{row_prefix}.actions"),
            "Actions",
            "Test runs the CLI's --version probe; Configure edits the [harness] override.",
        )
        .buttons(vec![
            ActionModel::new("Test", SettingsAction::TestHarness(harness_id.to_string()))
                .with_icon("plug"),
            ActionModel::new(
                "Configure",
                SettingsAction::EditHarness(harness_id.to_string()),
            )
            .with_icon("edit"),
            ActionModel::new(
                "Open config.toml",
                SettingsAction::OpenFile(ConfigFile::Config),
            )
            .with_icon("file"),
        ])
        .with_status(
            if installed {
                StatusKind::Success
            } else {
                StatusKind::Muted
            },
            if installed {
                "Detected on PATH."
            } else {
                "Not detected: install it or point [harness] command at it."
            },
        ),
    ];
    section.with_rows(section_rows)
}

// ---------------------------------------------------------------------------
// Rows and side-sheet sections
// ---------------------------------------------------------------------------

fn model_row(state: &SettingsUiState, model: &ModelView) -> RowModel {
    let mut hint = format!(
        "{} · {}",
        model.model_id,
        model
            .base_url
            .as_deref()
            .unwrap_or("no base URL (provider default)")
    );
    if model.is_default {
        hint.push_str(" · default");
    }
    if model.hidden {
        hint.push_str(" · hidden from the picker");
    }
    let key_state = match model.api_key_source {
        goble_core::config::ApiKeySource::Inline => "key in config.toml".to_string(),
        goble_core::config::ApiKeySource::Env => {
            format!("key from {}", model.api_key_env.clone().unwrap_or_default())
        }
        goble_core::config::ApiKeySource::Missing => "no API key".to_string(),
    };
    hint.push_str(" · ");
    hint.push_str(&key_state);
    let mut buttons = vec![
        ActionModel::new("Test", SettingsAction::TestModel(model.slug.clone())).with_icon("plug"),
        ActionModel::new("Edit", SettingsAction::EditModel(model.slug.clone())).with_icon("edit"),
    ];
    if !model.is_default {
        buttons.push(
            ActionModel::new(
                "Make default",
                SettingsAction::SetDefaultModel(model.slug.clone()),
            )
            .with_icon("check"),
        );
    }
    buttons.push(
        ActionModel::new("Delete", SettingsAction::DeleteModel(model.slug.clone()))
            .with_icon("trash-02")
            .destructive(),
    );
    let mut row = RowModel::new(
        format!("model.{}", model.slug),
        model.name.clone().unwrap_or_else(|| model.slug.clone()),
        hint,
    )
    .buttons(buttons);
    if let Some((kind, text)) = state.test_results.get(&format!("model.{}", model.slug)) {
        row = row.with_status(*kind, text.clone());
    }
    row
}

pub(super) fn model_form_section(
    state: &SettingsUiState,
    form: &super::super::state::ModelForm,
) -> SectionModel {
    let title = match &form.editing {
        Some(slug) => format!("Edit model · {slug}"),
        None => "Add model".to_string(),
    };
    let section = SectionModel::new("model-form", title)
        .with_description(
            "Saved to config.toml as one [model.<slug>] table. The key is write-only: it is never read back.",
        )
        .with_header_action(ActionModel::new("Cancel", SettingsAction::CancelForm));
    let field = |id: &str, fallback: &str| state.draft(id, fallback).to_string();
    let mut rows = vec![
        form_text(
            "model-form.slug",
            "Slug",
            "The table name: [model.<slug>].",
            &field("model-form.slug", &form.slug),
            "deepseek",
            form.editing.is_some(),
        ),
        form_text(
            "model-form.model_id",
            "Model id",
            "The id sent to the API. Empty uses the slug.",
            &field("model-form.model_id", &form.model_id),
            "deepseek-flash",
            false,
        ),
        form_text(
            "model-form.name",
            "Display name",
            "The label the picker shows.",
            &field("model-form.name", &form.name),
            "DeepSeek V4 Flash",
            false,
        ),
        form_text(
            "model-form.base_url",
            "Base URL",
            "The provider endpoint (http:// or https://).",
            &field("model-form.base_url", &form.base_url),
            "https://api.deepseek.com",
            false,
        ),
    ];
    rows.push(
        RowModel::new(
            "model-form.api_key",
            "API key",
            "Write-only: an empty value keeps the key already in the file.",
        )
        .secret("model-form.api_key", field("model-form.api_key", ""), false),
    );
    rows.push(form_text(
        "model-form.api_key_env",
        "API key environment variable",
        "Read the key from this variable at resolve time instead.",
        &field("model-form.api_key_env", &form.api_key_env),
        "DEEPSEEK_API_KEY",
        false,
    ));
    rows.push(form_text(
        "model-form.max_tokens",
        "Max completion tokens",
        "The response cap.",
        &field("model-form.max_tokens", &form.max_completion_tokens),
        "16384",
        false,
    ));
    rows.push(form_text(
        "model-form.context_window",
        "Context window",
        "The total context size in tokens.",
        &field("model-form.context_window", &form.context_window),
        "256000",
        false,
    ));
    rows.push(
        RowModel::new(
            "model-form.hidden",
            "Hidden",
            "Keep the entry out of the picker.",
        )
        .toggle("model-form.hidden", form.hidden),
    );
    rows.push(
        RowModel::new(
            "model-form.set_default",
            "Make default",
            "Also write [models] default = this slug.",
        )
        .toggle("model-form.set_default", form.set_default),
    );
    if let Some((kind, text)) = form.probe.clone() {
        rows.push(
            RowModel::new(
                "model-form.probe",
                "Connection test",
                "The last probe's answer.",
            )
            .read_only(if text.is_empty() {
                "—"
            } else {
                text.as_str()
            })
            .with_status(kind, text),
        );
    }
    rows.push(
        RowModel::new(
            "model-form.actions",
            "Save",
            "Test probes the endpoint before saving; Save writes the table.",
        )
        .buttons(vec![
            ActionModel::new("Test connection", SettingsAction::TestModelDraft).with_icon("plug"),
            ActionModel::new("Save model", SettingsAction::SaveModelForm).primary(),
            ActionModel::new("Cancel", SettingsAction::CancelForm),
        ]),
    );
    section.with_rows(rows)
}

pub(super) fn harness_form_section(state: &SettingsUiState, form: &HarnessForm) -> SectionModel {
    let title = format!("Configure {}", form.name);
    let field = |id: &str, fallback: &str| state.draft(id, fallback).to_string();
    SectionModel::new("harness-form", title)
        .with_description(
            "Writes one [harness.<id>] override in config.toml: where the CLI lives and whether the picker offers it.",
        )
        .with_header_action(ActionModel::new("Cancel", SettingsAction::CancelForm))
        .with_row(
            RowModel::new("harness-form.name", "Harness", "The catalog id this override belongs to.")
                .read_only(form.editing.clone().unwrap_or_default()),
        )
        .with_row(
            RowModel::new(
                "harness-form.command",
                "Launch command",
                "Empty restores the catalog's own command.",
            )
            .text(
                "harness-form.command",
                field("harness-form.command", &form.command),
                "claude",
            ),
        )
        .with_row(
            RowModel::new(
                "harness-form.args",
                "Arguments",
                "Space-separated; empty uses the catalog's arguments.",
            )
            .text(
                "harness-form.args",
                field("harness-form.args", &form.args),
                "--acp",
            ),
        )
        .with_row(
            RowModel::new(
                "harness-form.enabled",
                "Enabled",
                "Offer this CLI in the harness picker.",
            )
            .toggle("harness-form.enabled", form.enabled),
        )
        .with_row(
            RowModel::new(
                "harness-form.actions",
                "Save",
                "Save writes the override; Cancel leaves the file untouched.",
            )
            .buttons(vec![
                ActionModel::new("Save harness", SettingsAction::SaveHarnessForm).primary(),
                ActionModel::new("Cancel", SettingsAction::CancelForm),
            ]),
        )
}

fn form_text(
    id: &str,
    label: &str,
    hint: &str,
    value: &str,
    placeholder: &str,
    locked: bool,
) -> RowModel {
    let mut row = RowModel::new(id, label, hint).text(id, value, placeholder);
    if locked {
        // The slug is the table's identity: an edit renames nothing, so the
        // field shows the current slug and refuses a change.
        row = row.disabled().with_status(
            StatusKind::Muted,
            "The slug is the table's name; add a new model to use another.",
        );
    }
    row
}
