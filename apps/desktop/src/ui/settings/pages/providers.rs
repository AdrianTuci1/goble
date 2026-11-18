//! Settings → Providers: the models `[model.*]` declare, the default model
//! `[models] default` names, and the agent CLIs `[harness.*]` configure.
//!
//! The page is file-backed end to end: the models it lists are the tables in
//! `config.toml`, the Add/Edit form writes `ModelDraft`s through the service, and
//! its Test button probes the endpoint the form names (a real HTTP request; the
//! app tests point it at a local mock server). Sign-ins this build does not have
//! (the harnesses' OAuth accounts) are shown disabled with an honest line rather
//! than hidden.

use goble_core::config::{ConfigFile, HarnessView, ModelView};

use super::super::fields;
use super::super::model::{ActionModel, ControlModel, PageModel, RowModel, SectionModel};
use super::super::state::{
    ModelForm, ModelFormField, ModelFormToggle, SettingsAction, SettingsForm, SettingsUiState,
    StatusKind,
};
use super::{add_action, problem_section};

pub fn page(state: &SettingsUiState) -> PageModel {
    let mut sections = Vec::new();
    if let Some(problems) = problem_section(state, ConfigFile::Config) {
        sections.push(problems);
    }
    if let Some(SettingsForm::Model(form)) = state.form.as_ref() {
        sections.push(model_form_section(state, form));
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

    // [model.*] — one row per model.
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

    // [harness.*] — the agent CLIs.
    let mut harness_rows: Vec<RowModel> = Vec::new();
    let harnesses: Vec<HarnessView> = state
        .config
        .as_ref()
        .map(|view| view.sections.harnesses.clone())
        .unwrap_or_default();
    for harness in &harnesses {
        harness_rows.push(harness_row(state, harness));
    }
    if harness_rows.is_empty() {
        harness_rows.push(
            RowModel::new(
                "harnesses-empty",
                "No harnesses configured",
                "Agent CLIs are discovered from the catalog; configure one to override where its binary lives.",
            )
            .with_status(StatusKind::Muted, "Nothing to configure in this build."),
        );
    }
    sections.push(
        SectionModel::new("harnesses", "Agent CLIs")
            .with_header_action(ActionModel::new(
                "Open config.toml",
                SettingsAction::OpenFile(ConfigFile::Config),
            ))
            .with_rows(harness_rows),
    );

    // What this build cannot do: the hosted accounts.
    sections.push(
        SectionModel::new("accounts", "Accounts")
            .with_description(
                "Signing in to a provider through its own account is not wired into this build; the API-key route above is.",
            )
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

fn harness_row(state: &SettingsUiState, harness: &HarnessView) -> RowModel {
    let path = format!("harness.{}.enabled", harness.id);
    let hint = if let Some(command) = harness.command.clone() {
        format!(
            "{} · override: {command}{}",
            harness.name,
            if harness.args.is_empty() {
                String::new()
            } else {
                format!(" {}", harness.args.join(" "))
            }
        )
    } else {
        format!("{} · catalog command", harness.name)
    };
    let buttons = vec![
        ActionModel::new("Test", SettingsAction::TestHarness(harness.id.clone())).with_icon("plug"),
        if harness.enabled {
            ActionModel::new("Disable", SettingsAction::SetToggle { path, value: false })
        } else {
            ActionModel::new("Enable", SettingsAction::SetToggle { path, value: true })
        },
    ];
    let mut row = RowModel::new(
        format!("harness.{}", harness.id),
        harness.name.clone(),
        hint,
    )
    .buttons(buttons);
    if !harness.enabled {
        row = row.with_status(StatusKind::Muted, "Disabled in config.toml.");
    }
    if let Some((kind, text)) = state.test_results.get(&format!("harness.{}", harness.id)) {
        row = row.with_status(*kind, text.clone());
    }
    row
}

fn model_form_section(state: &SettingsUiState, form: &ModelForm) -> SectionModel {
    let title = match &form.editing {
        Some(slug) => format!("Edit model · {slug}"),
        None => "Add model".to_string(),
    };
    let mut section = SectionModel::new("model-form", title)
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
