//! The settings surface's one dispatch: the single call site for the
//! file-backed settings service.
//!
//! `make_settings_actions` builds the closure the settings tab's [`SettingsActions`]
//! holds; every rail row, control, field commit, form button and key gesture goes
//! through it. The work is split in three:
//!
//! 1. **Navigation and focus** mutate [`SettingsUiState`](crate::ui::settings::SettingsUiState)
//!    only, and resolve the page's row order from the same page model the
//!    renderer draws ([`crate::ui::settings::pages::page_model`]).
//! 2. **Scalar rows** build a path-op batch (`ConfigOp::set`/`unset`) and call
//!    `DesktopState::apply_setting_ops` with the revision the page rendered from.
//!    A stale revision is surfaced as the conflict notice, never overwritten.
//! 3. **Collections** (models, connectors, hosts, environments, vault, updates)
//!    call the service's own CRUD/probe/test functions, so the file and the
//!    runtime registry stay in step.
//!
//! Every error reaches the page as a sentence; a success replaces the view with
//! the fresh projection the service returned, so the revision held is always the
//! one on disk.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use goble_core::config::{
    ConfigError, ConfigFile, ConfigOp, ConfigRevision, ConfigValue, RemoteMode,
};
use goble_desktop_service::state::{
    HarnessDraft, HostDraft, McpDraft, McpTransportDraft, ModelDraft, ModelProbeDraft,
};
use goble_desktop_service::DesktopState;

use crate::state::UiState;
use crate::ui::settings::fields;
use crate::ui::settings::model::ControlModel;
use crate::ui::settings::pages;
use crate::ui::settings::state::{
    ConnectorForm, HarnessForm, HarnessFormField, HostForm, ModelForm, ModelFormField,
    ModelFormToggle, SettingsAction, SettingsActions, SettingsForm, SettingsUiState, StatusKind,
};
use crate::ui::SettingsCategory;

/// Build the settings tab's actions from the app state and the service.
pub fn make_settings_actions(
    state: Rc<std::cell::RefCell<UiState>>,
    desktop: Option<Arc<DesktopState>>,
    media: Rc<std::cell::RefCell<crate::media::MediaState>>,
) -> SettingsActions {
    SettingsActions::new(move |action| {
        let mut state_ref = state.borrow_mut();
        handle(&mut state_ref, desktop.as_ref(), &media, action);
    })
}

fn handle(
    state: &mut UiState,
    desktop: Option<&Arc<DesktopState>>,
    media: &Rc<std::cell::RefCell<crate::media::MediaState>>,
    action: SettingsAction,
) {
    // Every dispatch reads the page the leaf shows; the state's copy is kept in
    // step so a stale page cannot dispatch against the wrong rows. A state with
    // no settings leaf (a test fixture) keeps the page its own state names.
    if state.settings_pane().is_some() {
        state.settings.page = state.settings_page();
    }
    match action {
        SettingsAction::SetSendImmediately(value) => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            state.send_immediately = value;
            state.settings.send_immediately = value;
            write_ops(
                state,
                desktop,
                ConfigFile::Config,
                vec![fields::bool_op("app.send_immediately", value)],
            );
        }
        SettingsAction::Navigate(page) => {
            state.settings_set_page(page);
            state.settings.region = crate::ui::settings::SettingsRegion::Rail;
            state.settings.row_index = 0;
            state.settings.focused_field = None;
            state.settings.theme_select_open = false;
            state.settings.close_select_menus();
            state.settings.scroll.borrow_mut().reset();
            // The Providers page's accounts and per-CLI folds read live PATH
            // detection, so opening it re-scans.
            if page == SettingsCategory::Providers {
                load_harnesses(state, desktop);
            }
        }
        SettingsAction::FocusRail => {
            state.settings.region = crate::ui::settings::SettingsRegion::Rail;
            state.settings.search_focused = false;
            state.settings.focused_field = None;
        }
        SettingsAction::FocusPage => {
            let count = page_model(state).control_count();
            if count > 0 {
                state.settings.region = crate::ui::settings::SettingsRegion::Page;
                state.settings.row_index = state.settings.row_index.min(count - 1);
            }
        }
        SettingsAction::FocusSearch(focused) => {
            state.settings.search_focused = focused;
            if focused {
                state.settings.region = crate::ui::settings::SettingsRegion::Rail;
            }
        }
        SettingsAction::Search(query) => {
            state.settings.search = query;
            state.settings.search_focused = true;
            state.settings.region = crate::ui::settings::SettingsRegion::Rail;
        }
        SettingsAction::SearchChar(character) => {
            if character == "\u{8}" {
                state.settings.search.pop();
            } else if !character.is_empty() {
                state.settings.search.push_str(&character);
            }
        }
        SettingsAction::ClearSearch => {
            state.settings.search.clear();
            state.settings.search_focused = false;
        }
        SettingsAction::ShortcutsSearch(query) => {
            state.settings.shortcuts_search = query;
            state.settings.shortcuts_search_focused = true;
        }
        SettingsAction::ShortcutsSearchFocus(focused) => {
            state.settings.shortcuts_search_focused = focused;
        }
        SettingsAction::ShortcutsClearSearch => {
            state.settings.shortcuts_search.clear();
            state.settings.shortcuts_search_focused = false;
        }
        SettingsAction::ShortcutRemap(id) => {
            // Remapping is not in this build. The trash affordance is a real
            // press that says so; it never deletes a binding (the list is a
            // report of what the app dispatches, and there is nowhere to write
            // a removal to).
            let _ = id;
            state
                .settings
                .set_status(StatusKind::Info, pages::REMAP_UNAVAILABLE);
        }
        SettingsAction::Step(delta) => step_focus(state, delta),
        SettingsAction::Adjust(delta) => {
            if let Some(row) = focused_row(state) {
                if let Some(action) = row.adjust_action(delta) {
                    handle(state, desktop, media, action);
                    return;
                }
            }
            // Left on the page with nothing to adjust steps back to the rail.
            if delta < 0 && state.settings.region == crate::ui::settings::SettingsRegion::Page {
                state.settings.region = crate::ui::settings::SettingsRegion::Rail;
            }
        }
        SettingsAction::Activate => activate(state, desktop, media),
        SettingsAction::Back => {
            if state.settings.focused_field.is_some() {
                let id = state.settings.focused_field.clone().unwrap_or_default();
                handle(state, desktop, media, SettingsAction::ClearField { id });
            } else if state.settings.region == crate::ui::settings::SettingsRegion::Sheet {
                close_sheet(state);
            } else if state.settings.region == crate::ui::settings::SettingsRegion::Page {
                state.settings.region = crate::ui::settings::SettingsRegion::Rail;
            }
        }
        SettingsAction::ToggleFold(id) => {
            if !state.settings.folds.remove(&id) {
                state.settings.folds.insert(id);
            }
        }
        SettingsAction::RefreshHarnesses => load_harnesses(state, desktop),
        SettingsAction::ToggleThemeSelect(open) => {
            state.settings.theme_select_open = open;
        }
        SettingsAction::EditHarness(id) => {
            let status = state
                .settings
                .harnesses
                .iter()
                .find(|harness| harness.id == id)
                .cloned();
            let from_config = state
                .settings
                .config
                .as_ref()
                .and_then(|view| view.sections.harnesses.iter().find(|h| h.id == id))
                .cloned();
            let (name, command, args, enabled) = match (status, from_config) {
                (_status, Some(config)) => (
                    config.name.clone(),
                    config.command.clone().unwrap_or_default(),
                    config.args.join(" "),
                    config.enabled,
                ),
                (Some(status), None) => (
                    status.name.clone(),
                    String::new(),
                    status.args.join(" "),
                    status.enabled,
                ),
                (None, None) => (id.clone(), String::new(), String::new(), true),
            };
            state.settings.form = Some(SettingsForm::Harness(Box::new(HarnessForm {
                editing: Some(id),
                name,
                command,
                args,
                enabled,
            })));
            clear_form_drafts(state, "harness-form.");
            open_sheet(state);
        }
        SettingsAction::HarnessFormText { field, value } => {
            let id = match field {
                HarnessFormField::Command => "harness-form.command",
                HarnessFormField::Args => "harness-form.args",
            }
            .to_string();
            state.settings.set_draft(&id, value.clone());
            if let Some(SettingsForm::Harness(form)) = state.settings.form.as_mut() {
                match field {
                    HarnessFormField::Command => form.command = value,
                    HarnessFormField::Args => form.args = value,
                }
            }
        }
        SettingsAction::HarnessFormEnabled(enabled) => {
            if let Some(SettingsForm::Harness(form)) = state.settings.form.as_mut() {
                form.enabled = enabled;
            }
        }
        SettingsAction::SaveHarnessForm => save_harness_form(state, desktop),
        SettingsAction::SetDraft { id, value } => {
            state.settings.set_draft(&id, value.clone());
            // A form's draft lives in the drafts map too, so the form's own
            // field copy is updated for the save path.
            update_form_draft(state, &id, &value);
        }
        SettingsAction::CommitField { id } => commit_field(state, desktop, media, &id),
        SettingsAction::ClearField { id } => {
            state.settings.clear_draft(&id);
            if state.settings.focused_field.as_deref() == Some(id.as_str()) {
                state.settings.focused_field = None;
            }
            if id == "env_new_group" {
                state.settings.env_new_group.clear();
            }
        }
        SettingsAction::SetToggle { path, value } => {
            if let Some(updated) = form_toggle(&mut state.settings, &path, value) {
                if !updated {
                    return;
                }
                return;
            }
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            write_ops(
                state,
                desktop,
                ConfigFile::Config,
                vec![fields::bool_op(&path, value)],
            );
            if path.starts_with("harness.") {
                load_harnesses(state, Some(desktop));
            }
        }
        SettingsAction::SetSelect { path, index } => set_select(state, desktop, &path, index),
        SettingsAction::SetSlider { path, value } => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            write_ops(
                state,
                desktop,
                ConfigFile::Config,
                vec![fields::number_op(&path, value)],
            );
        }
        SettingsAction::SetText { path, value } => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let file = crate::ui::settings::model::path_file(&path);
            write_ops(state, desktop, file, vec![fields::text_op(&path, &value)]);
        }
        SettingsAction::Reset { path } => {
            if path == "env_new_group" {
                state.settings.env_new_group.clear();
                state.settings.clear_draft("env_new_group");
                return;
            }
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            write_ops(
                state,
                desktop,
                ConfigFile::Config,
                vec![fields::reset_op(&path)],
            );
        }
        SettingsAction::ResetThemeColours => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let ops = [
                fields::reset_op("theme.preset"),
                fields::reset_op("theme.accent"),
                fields::reset_op("theme.primary"),
                fields::reset_op("theme.secondary"),
                fields::reset_op("theme.bg"),
                fields::reset_op("theme.surface"),
            ];
            write_ops(state, desktop, ConfigFile::Config, ops.to_vec());
        }
        SettingsAction::Reload => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            state.refresh_settings(desktop);
            load_harnesses(state, Some(desktop));
            state
                .settings
                .set_status(StatusKind::Success, "Reloaded from the files.");
        }
        SettingsAction::OpenFile(file) => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            if let Some(path) = desktop.settings_path(file) {
                crate::actions::pane_ops::open_file_pane(
                    state,
                    path.to_string_lossy().to_string(),
                    Some(desktop),
                );
            } else {
                state.settings.notice(
                    StatusKind::Error,
                    "This build has no settings file to open.",
                );
            }
        }
        SettingsAction::OpenSettingsFolder => {
            if let Some(path) = state.settings.paths.home.clone() {
                crate::actions::pane_ops::open_file_pane(
                    state,
                    path.to_string_lossy().to_string(),
                    desktop,
                );
            }
        }
        // ---- models ----------------------------------------------------
        SettingsAction::NewModel => {
            state.settings.form = Some(SettingsForm::Model(Box::new(ModelForm::add())));
            clear_form_drafts(state, "model-form.");
            open_sheet(state);
        }
        SettingsAction::EditModel(slug) => {
            if let Some(model) = state
                .settings
                .models()
                .iter()
                .find(|model| model.slug == slug)
                .cloned()
            {
                state.settings.form = Some(SettingsForm::Model(Box::new(ModelForm::edit(&model))));
                clear_form_drafts(state, "model-form.");
                open_sheet(state);
            }
        }
        SettingsAction::DeleteModel(slug) => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let expected = state.settings.config_revision();
            match desktop.delete_config_model(&slug, expected.as_ref()) {
                Ok(view) => {
                    state.adopt_settings_view(view);
                    state.settings.set_status(
                        StatusKind::Success,
                        format!("Deleted [model.{slug}] from config.toml."),
                    );
                    state.refresh_settings(desktop);
                }
                Err(error) => write_error(state, desktop, error),
            }
        }
        SettingsAction::TestModel(slug) => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            state.settings.busy = true;
            let result = desktop.test_model_connection(&slug);
            state.settings.busy = false;
            let (kind, text) = match result {
                Ok(report) => (
                    if report.ok {
                        StatusKind::Success
                    } else {
                        StatusKind::Error
                    },
                    report.line(),
                ),
                Err(error) => (StatusKind::Error, error),
            };
            state
                .settings
                .test_results
                .insert(format!("model.{slug}"), (kind, text.clone()));
            state.settings.set_status(kind, text);
        }
        SettingsAction::TestHarness(id) => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let result = desktop.test_harness(&id);
            let (kind, text) = match result {
                Ok(status) => (
                    StatusKind::Success,
                    format!(
                        "{} · {}{}",
                        status.name,
                        status
                            .version
                            .clone()
                            .unwrap_or_else(|| "no version reported".to_string()),
                        match status.path.clone() {
                            Some(path) if !path.is_empty() => format!(" · {path}"),
                            _ => String::new(),
                        }
                    ),
                ),
                Err(error) => (StatusKind::Error, error),
            };
            state
                .settings
                .test_results
                .insert(format!("harness.{id}"), (kind, text.clone()));
            state.settings.set_status(kind, text);
        }
        SettingsAction::SetDefaultModel(slug) => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let expected = state.settings.config_revision();
            match desktop.set_default_model_config(&slug, expected.as_ref()) {
                Ok(view) => {
                    state.adopt_settings_view(view);
                    state.settings.set_status(
                        StatusKind::Success,
                        format!("New sessions start on {slug}."),
                    );
                    state.refresh_settings(desktop);
                }
                Err(error) => write_error(state, desktop, error),
            }
        }
        SettingsAction::ModelFormText { field, value } => {
            let id = model_form_id(field).to_string();
            state.settings.set_draft(&id, value.clone());
            update_model_form(state, field, value);
        }
        SettingsAction::ModelFormToggle { field, value } => {
            if let Some(SettingsForm::Model(form)) = state.settings.form.as_mut() {
                match field {
                    ModelFormToggle::Hidden => form.hidden = value,
                    ModelFormToggle::SetDefault => form.set_default = value,
                }
            }
        }
        SettingsAction::TestModelDraft => test_model_draft(state, desktop),
        SettingsAction::SaveModelForm => save_model_form(state, desktop),
        SettingsAction::CancelForm => close_sheet(state),
        // ---- connectors ------------------------------------------------
        SettingsAction::NewConnector => {
            state.settings.form = Some(SettingsForm::Connector(Box::new(ConnectorForm {
                editing: None,
                name: String::new(),
                transport: 0,
                url: String::new(),
                command: String::new(),
                args: String::new(),
                enabled: true,
            })));
            clear_form_drafts(state, "connector-form.");
            open_sheet(state);
        }
        SettingsAction::EditConnector(name) => {
            if let Some(server) = state
                .settings
                .config
                .as_ref()
                .and_then(|view| view.sections.mcp.iter().find(|mcp| mcp.name == name))
                .cloned()
            {
                state.settings.form = Some(SettingsForm::Connector(Box::new(ConnectorForm {
                    editing: Some(server.name.clone()),
                    name: server.name.clone(),
                    transport: if server.url.is_some() { 0 } else { 1 },
                    url: server.url.clone().unwrap_or_default(),
                    command: server.command.clone().unwrap_or_default(),
                    args: server.args.join(" "),
                    enabled: server.enabled,
                })));
                clear_form_drafts(state, "connector-form.");
                open_sheet(state);
            }
        }
        SettingsAction::DeleteConnector(name) => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let expected = state.settings.config_revision();
            match desktop.delete_mcp_config(&name, expected.as_ref()) {
                Ok(view) => {
                    state.adopt_settings_view(view);
                    state.settings.set_status(
                        StatusKind::Success,
                        format!("Deleted [mcp_servers.{name}] and its registry row."),
                    );
                    state.refresh_settings(desktop);
                }
                Err(error) => write_error(state, desktop, error),
            }
        }
        SettingsAction::TestConnector(name) => {
            if name.is_empty() {
                state
                    .settings
                    .set_status(StatusKind::Muted, "Save the connector first, then test it.");
                return;
            }
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let result = desktop.test_mcp_config(&name);
            let (kind, text) = match result {
                Ok(tools) => (
                    StatusKind::Success,
                    format!("connected · {} tool(s)", tools.len()),
                ),
                Err(error) => (StatusKind::Error, error),
            };
            state
                .settings
                .test_results
                .insert(format!("mcp.{name}"), (kind, text.clone()));
            state.settings.set_status(kind, text);
        }
        SettingsAction::ConnectorFormText { field, value } => {
            let id = connector_form_id(field).to_string();
            state.settings.set_draft(&id, value.clone());
            if let Some(SettingsForm::Connector(form)) = state.settings.form.as_mut() {
                match field {
                    crate::ui::settings::state::ConnectorFormField::Name => form.name = value,
                    crate::ui::settings::state::ConnectorFormField::Url => form.url = value,
                    crate::ui::settings::state::ConnectorFormField::Command => form.command = value,
                    crate::ui::settings::state::ConnectorFormField::Args => form.args = value,
                }
            }
        }
        SettingsAction::ConnectorFormTransport(transport) => {
            if let Some(SettingsForm::Connector(form)) = state.settings.form.as_mut() {
                form.transport = transport;
            }
        }
        SettingsAction::ConnectorFormEnabled(enabled) => {
            if let Some(SettingsForm::Connector(form)) = state.settings.form.as_mut() {
                form.enabled = enabled;
            }
        }
        SettingsAction::SaveConnectorForm => save_connector_form(state, desktop),
        // ---- hosts -----------------------------------------------------
        SettingsAction::NewHost => {
            state.settings.form = Some(SettingsForm::Host(Box::new(HostForm::default())));
            clear_form_drafts(state, "host-form.");
            open_sheet(state);
        }
        SettingsAction::EditHost(name) => {
            if let Some(host) = state
                .settings
                .saved_hosts()
                .iter()
                .find(|host| host.name == name)
                .cloned()
            {
                state.settings.form = Some(SettingsForm::Host(Box::new(HostForm {
                    editing: Some(host.name.clone()),
                    name: host.name.clone(),
                    host: host.host.clone(),
                    user: host.user.clone().unwrap_or_default(),
                    port: host.port.map(|port| port.to_string()).unwrap_or_default(),
                    identity: host.identity.clone().unwrap_or_default(),
                })));
                clear_form_drafts(state, "host-form.");
                open_sheet(state);
            }
        }
        SettingsAction::DeleteHost(name) => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let expected = state.settings.revision(ConfigFile::Hosts);
            match desktop.delete_host(&name, expected.as_ref()) {
                Ok(view) => {
                    state.adopt_settings_view(view);
                    state
                        .settings
                        .set_status(StatusKind::Success, format!("Deleted saved host {name}."));
                    state.refresh_settings(desktop);
                }
                Err(error) => write_error(state, desktop, error),
            }
        }
        SettingsAction::TestHost(name) => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let result = desktop.test_host(&name);
            let (kind, text) = match result {
                Ok(line) => (StatusKind::Success, line),
                Err(error) => (StatusKind::Error, error),
            };
            state
                .settings
                .test_results
                .insert(format!("host.{name}"), (kind, text.clone()));
            state.settings.set_status(kind, text);
        }
        SettingsAction::HostFormText { field, value } => {
            let id = host_form_id(field).to_string();
            state.settings.set_draft(&id, value.clone());
            if let Some(SettingsForm::Host(form)) = state.settings.form.as_mut() {
                match field {
                    crate::ui::settings::state::HostFormField::Name => form.name = value,
                    crate::ui::settings::state::HostFormField::Host => form.host = value,
                    crate::ui::settings::state::HostFormField::User => form.user = value,
                    crate::ui::settings::state::HostFormField::Port => form.port = value,
                    crate::ui::settings::state::HostFormField::Identity => form.identity = value,
                }
            }
        }
        SettingsAction::SaveHostForm => save_host_form(state, desktop),
        // ---- environments ----------------------------------------------
        SettingsAction::EnvNewGroup(value) => {
            state.settings.env_new_group = value.clone();
            state.settings.set_draft("env_new_group", value);
        }
        SettingsAction::EnvCreateGroup => {
            let name = state
                .settings
                .draft("env_new_group", &state.settings.env_new_group)
                .trim()
                .to_string();
            if name.is_empty() {
                state
                    .settings
                    .set_status(StatusKind::Muted, "Name the group first.");
                return;
            }
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let op = ConfigOp::set(&["groups", name.as_str()], ConfigValue::Table(Vec::new()));
            let expected = state.settings.revision(ConfigFile::Environment);
            match desktop.apply_setting_ops(ConfigFile::Environment, &[op], expected.as_ref()) {
                Ok(view) => {
                    state.adopt_settings_view(view);
                    state.settings.clear_draft("env_new_group");
                    state.settings.env_new_group.clear();
                    state.settings.set_status(
                        StatusKind::Success,
                        format!("Created the group {name} in environment.toml."),
                    );
                    state.refresh_settings(desktop);
                }
                Err(error) => write_error(state, desktop, error),
            }
        }
        SettingsAction::EnvOpenGroup(group) => {
            if state.settings.env_open_group.as_deref() == Some(group.as_str()) {
                state.settings.env_open_group = None;
            } else {
                state.settings.env_open_group = Some(group);
            }
        }
        SettingsAction::EnvDeleteGroup(group) => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let op = ConfigOp::unset(&["groups", group.as_str()]);
            let expected = state.settings.revision(ConfigFile::Environment);
            match desktop.apply_setting_ops(ConfigFile::Environment, &[op], expected.as_ref()) {
                Ok(view) => {
                    state.adopt_settings_view(view);
                    if state.settings.env_open_group.as_deref() == Some(group.as_str()) {
                        state.settings.env_open_group = None;
                    }
                    state.settings.set_status(
                        StatusKind::Success,
                        format!("Deleted the group {group} from environment.toml."),
                    );
                    state.refresh_settings(desktop);
                }
                Err(error) => write_error(state, desktop, error),
            }
        }
        SettingsAction::EnvVarName { group, value } => {
            let id = format!("env.{group}.new-name");
            state.settings.set_draft(&id, value);
        }
        SettingsAction::EnvVarValue { group, value } => {
            let id = format!("env.{group}.new-value");
            state.settings.set_draft(&id, value);
        }
        SettingsAction::EnvAddVariable { group } => {
            let name_id = format!("env.{group}.new-name");
            let value_id = format!("env.{group}.new-value");
            let name = state.settings.draft(&name_id, "").trim().to_string();
            let value = state.settings.draft(&value_id, "").to_string();
            if name.is_empty() || value.is_empty() {
                state
                    .settings
                    .set_status(StatusKind::Muted, "A variable needs a name and a value.");
                return;
            }
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let op = ConfigOp::set(
                &["groups", group.as_str(), name.as_str()],
                ConfigValue::Str(value),
            );
            let expected = state.settings.revision(ConfigFile::Environment);
            match desktop.apply_setting_ops(ConfigFile::Environment, &[op], expected.as_ref()) {
                Ok(view) => {
                    state.adopt_settings_view(view);
                    state.settings.clear_draft(&name_id);
                    state.settings.clear_draft(&value_id);
                    state.settings.set_status(
                        StatusKind::Success,
                        format!("Wrote {name} into the group {group}."),
                    );
                    state.refresh_settings(desktop);
                }
                Err(error) => write_error(state, desktop, error),
            }
        }
        SettingsAction::EnvDeleteVariable { group, name } => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let op = ConfigOp::unset(&["groups", group.as_str(), name.as_str()]);
            let expected = state.settings.revision(ConfigFile::Environment);
            match desktop.apply_setting_ops(ConfigFile::Environment, &[op], expected.as_ref()) {
                Ok(view) => {
                    state.adopt_settings_view(view);
                    state.settings.set_status(
                        StatusKind::Success,
                        format!("Removed {name} from the group {group}."),
                    );
                    state.refresh_settings(desktop);
                }
                Err(error) => write_error(state, desktop, error),
            }
        }
        // ---- vault -----------------------------------------------------
        SettingsAction::VaultPassphrase(value) => {
            state.settings.vault.passphrase = value.clone();
            state.settings.set_draft("vault.passphrase", value);
        }
        SettingsAction::VaultUnlock => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let passphrase = state
                .settings
                .draft("vault.passphrase", &state.settings.vault.passphrase)
                .to_string();
            match desktop.unlock_vault(passphrase) {
                Ok(keys) => {
                    state.settings.vault.unlocked = true;
                    state.settings.vault.passphrase.clear();
                    state.settings.clear_draft("vault.passphrase");
                    state.settings.vault.secrets = desktop
                        .list_vault_secrets()
                        .into_iter()
                        .map(|secret| (secret.key, secret.updated_at))
                        .collect();
                    state.settings.set_status(
                        StatusKind::Success,
                        format!("Vault unlocked · {} secret(s).", keys.len()),
                    );
                }
                Err(error) => state
                    .settings
                    .set_status(StatusKind::Error, format!("Unlock failed: {error}")),
            }
        }
        SettingsAction::VaultNewKey(value) => {
            state.settings.vault.new_key = value.clone();
            state.settings.set_draft("vault.new_key", value);
        }
        SettingsAction::VaultNewValue(value) => {
            state.settings.vault.new_value = value.clone();
            state.settings.set_draft("vault.new_value", value);
        }
        SettingsAction::VaultAdd => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            let key = state
                .settings
                .draft("vault.new_key", &state.settings.vault.new_key)
                .trim()
                .to_string();
            let value = state
                .settings
                .draft("vault.new_value", &state.settings.vault.new_value)
                .to_string();
            if key.is_empty() || value.is_empty() {
                state
                    .settings
                    .set_status(StatusKind::Muted, "A secret needs a name and a value.");
                return;
            }
            match desktop.set_vault_secret(&key, &value) {
                Ok(()) => {
                    state.settings.vault.new_key.clear();
                    state.settings.vault.new_value.clear();
                    state.settings.clear_draft("vault.new_key");
                    state.settings.clear_draft("vault.new_value");
                    state.settings.vault.secrets = desktop
                        .list_vault_secrets()
                        .into_iter()
                        .map(|secret| (secret.key, secret.updated_at))
                        .collect();
                    state
                        .settings
                        .set_status(StatusKind::Success, format!("Stored {key} in the vault."));
                }
                Err(error) => state.settings.set_status(
                    StatusKind::Error,
                    format!("Could not store the secret: {error}"),
                ),
            }
        }
        SettingsAction::VaultDelete(key) => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            match desktop.delete_vault_secret(&key) {
                Ok(()) => {
                    state
                        .settings
                        .vault
                        .secrets
                        .retain(|(stored, _)| *stored != key);
                    state
                        .settings
                        .set_status(StatusKind::Success, format!("Deleted {key}."));
                }
                Err(error) => state.settings.set_status(
                    StatusKind::Error,
                    format!("Could not delete the secret: {error}"),
                ),
            }
        }
        SettingsAction::VaultLock => {
            let Some(desktop) = desktop else {
                no_service(state);
                return;
            };
            desktop.set_vault_passphrase(String::new());
            state.settings.vault.unlocked = false;
            state.settings.vault.secrets.clear();
            state
                .settings
                .set_status(StatusKind::Muted, "Vault locked for this session.");
        }
        SettingsAction::SetupRemoteWithAgent => {
            crate::actions::make_actions::open_setup_conversation(
                state,
                desktop,
                media,
                goble_core::config::SetupTarget::Remote,
            );
        }
        SettingsAction::SetupHubWithAgent => {
            crate::actions::make_actions::open_setup_conversation(
                state,
                desktop,
                media,
                goble_core::config::SetupTarget::Hub,
            );
        }
        SettingsAction::CheckUpdates => {
            state.check_for_updates();
            state
                .settings
                .set_status(StatusKind::Muted, "Asking the release host…");
        }
        SettingsAction::InstallUpdate => {
            state.install_available_update();
            state
                .settings
                .set_status(StatusKind::Muted, "Installing the offered release…");
        }
    }
    // After any dispatch, re-read anything a write changed that the page does
    // not own: the app's live theme, the remote choosers' view and the vault.
    if let Some(desktop) = desktop {
        state.settings.update_view = state.update_view.clone();
        let _ = desktop;
    }
}

fn page_model(state: &UiState) -> crate::ui::settings::pages::PageModel {
    pages::page_model(state.settings.page, &state.settings)
}

fn focused_row(state: &UiState) -> Option<crate::ui::settings::RowModel> {
    match state.settings.region {
        crate::ui::settings::SettingsRegion::Sheet => sheet_focused_row(state),
        _ => page_model(state).row_at(state.settings.row_index).cloned(),
    }
}

/// The open side sheet's form, as a section (title, fields, Save/Cancel).
fn sheet_model(state: &UiState) -> Option<crate::ui::settings::SectionModel> {
    pages::form_section(&state.settings)
}

fn sheet_focused_row(state: &UiState) -> Option<crate::ui::settings::RowModel> {
    let section = sheet_model(state)?;
    section
        .rows
        .iter()
        .filter(|row| !row.disabled && row.control.is_interactive())
        .nth(state.settings.sheet_row)
        .cloned()
}

fn sheet_row_count(state: &UiState) -> usize {
    sheet_model(state)
        .map(|section| {
            section
                .rows
                .iter()
                .filter(|row| !row.disabled && row.control.is_interactive())
                .count()
        })
        .unwrap_or(0)
}

/// Open a form in the side sheet: the sheet owns the keyboard.
fn open_sheet(state: &mut UiState) {
    state.settings.region = crate::ui::settings::SettingsRegion::Sheet;
    state.settings.sheet_row = 0;
    state.settings.focused_field = None;
    state.settings.theme_select_open = false;
    state.settings.sheet_scroll.borrow_mut().reset();
}

/// Close the side sheet and give the page its keyboard back.
fn close_sheet(state: &mut UiState) {
    clear_all_form_drafts(state);
    state.settings.form = None;
    state.settings.focused_field = None;
    state.settings.sheet_row = 0;
    if state.settings.region == crate::ui::settings::SettingsRegion::Sheet {
        state.settings.region = crate::ui::settings::SettingsRegion::Page;
    }
    state.settings.sheet_scroll.borrow_mut().reset();
}

/// Re-scan the catalog's harnesses on PATH and store the detection result.
fn load_harnesses(state: &mut UiState, desktop: Option<&Arc<DesktopState>>) {
    let Some(desktop) = desktop else {
        return;
    };
    state.settings.harnesses = desktop.list_harnesses();
}

fn step_focus(state: &mut UiState, delta: i32) {
    let delta = delta.signum();
    if delta == 0 {
        return;
    }
    match state.settings.region {
        crate::ui::settings::SettingsRegion::Rail => {
            let count = SettingsCategory::ALL.len() as i32;
            let next = (state.settings.nav_index as i32 + delta).rem_euclid(count);
            let page = SettingsCategory::ALL[next as usize];
            state.settings_set_page(page);
            state.settings.nav_index = next as usize;
            state.settings.row_index = 0;
            state.settings.focused_field = None;
        }
        crate::ui::settings::SettingsRegion::Page => {
            let count = page_model(state).control_count();
            if count == 0 {
                return;
            }
            state.settings.row_index =
                (state.settings.row_index as i32 + delta).clamp(0, count as i32 - 1) as usize;
            state.settings.focused_field = None;
        }
        crate::ui::settings::SettingsRegion::Sheet => {
            let count = sheet_row_count(state);
            if count == 0 {
                return;
            }
            state.settings.sheet_row =
                (state.settings.sheet_row as i32 + delta).clamp(0, count as i32 - 1) as usize;
            state.settings.focused_field = None;
        }
    }
}

fn activate(
    state: &mut UiState,
    desktop: Option<&Arc<DesktopState>>,
    media: &Rc<std::cell::RefCell<crate::media::MediaState>>,
) {
    match state.settings.region {
        crate::ui::settings::SettingsRegion::Rail => {
            if state.settings.search_focused {
                state.settings_search_navigate();
            } else {
                let page = SettingsCategory::ALL[state
                    .settings
                    .nav_index
                    .min(SettingsCategory::ALL.len() - 1)];
                state.settings_set_page(page);
                state.settings.region = crate::ui::settings::SettingsRegion::Page;
                state.settings.row_index = 0;
            }
        }
        crate::ui::settings::SettingsRegion::Page => {
            let Some(row) = focused_row(state) else {
                return;
            };
            // A text row puts the caret in its field on the first Enter; a
            // second one commits (the key handler routes Enter to CommitField
            // while the caret is in).
            if let ControlModel::Text { id, .. } = row.control.clone() {
                if state.settings.focused_field.as_deref() != Some(id.as_str()) {
                    state.settings.focused_field = Some(id.clone());
                    state.settings.set_draft(&id, control_value_text(&row));
                    return;
                }
            }
            if let Some(action) = row.primary_action() {
                handle(state, desktop, media, action);
            }
        }
        crate::ui::settings::SettingsRegion::Sheet => {
            let Some(row) = focused_row(state) else {
                return;
            };
            if let ControlModel::Text { id, .. } = row.control.clone() {
                if state.settings.focused_field.as_deref() != Some(id.as_str()) {
                    state.settings.focused_field = Some(id.clone());
                    state.settings.set_draft(&id, control_value_text(&row));
                    return;
                }
            }
            if let Some(action) = row.primary_action() {
                handle(state, desktop, media, action);
            }
        }
    }
}

fn control_value_text(row: &crate::ui::settings::RowModel) -> String {
    match &row.control {
        ControlModel::Text { value, .. } => value.clone(),
        ControlModel::Select {
            options, selected, ..
        } => options.get(*selected).cloned().unwrap_or_default(),
        _ => String::new(),
    }
}

fn commit_field(
    state: &mut UiState,
    desktop: Option<&Arc<DesktopState>>,
    media: &Rc<std::cell::RefCell<crate::media::MediaState>>,
    id: &str,
) {
    state.settings.focused_field = None;
    // Form and draft-only fields are committed by the form's own Save button;
    // committing the row just keeps the draft.
    if is_form_field(id) {
        return;
    }
    if id == "env_new_group" || id.starts_with("env.") || id.starts_with("vault.") {
        return;
    }
    let value = state.settings.drafts.get(id).cloned().unwrap_or_default();
    let Some(desktop) = desktop else {
        no_service(state);
        return;
    };
    let path = id.to_string();
    write_ops(
        state,
        desktop,
        ConfigFile::Config,
        vec![fields::text_op(&path, &value)],
    );
    state.settings.clear_draft(id);
}

fn is_form_field(id: &str) -> bool {
    id.starts_with("model-form.")
        || id.starts_with("connector-form.")
        || id.starts_with("host-form.")
        || id.starts_with("harness-form.")
}

fn set_select(state: &mut UiState, desktop: Option<&Arc<DesktopState>>, path: &str, index: usize) {
    let Some(desktop) = desktop else {
        no_service(state);
        return;
    };
    match path {
        "connector-form.transport" => {
            if let Some(SettingsForm::Connector(form)) = state.settings.form.as_mut() {
                form.transport = index.min(1);
            }
            return;
        }
        "models.default" => {
            let slug = state
                .settings
                .models()
                .get(index)
                .map(|model| model.slug.clone());
            if let Some(slug) = slug {
                let expected = state.settings.config_revision();
                match desktop.set_default_model_config(&slug, expected.as_ref()) {
                    Ok(view) => {
                        state.adopt_settings_view(view);
                        state.settings.set_status(
                            StatusKind::Success,
                            format!("New sessions start on {slug}."),
                        );
                        state.refresh_settings(desktop);
                    }
                    Err(error) => write_error(state, desktop, error),
                }
            }
            return;
        }
        "theme.preset" => {
            // Picking a palette is the whole base, so the five channel
            // overrides are dropped with it: the page then shows the palette as
            // the preset defines it, exactly like the old theme picker.
            let mut ops = vec![fields::palette_select_op("theme.preset", index)];
            for key in ["accent", "primary", "secondary", "bg", "surface"] {
                ops.push(fields::reset_op(&format!("theme.{key}")));
            }
            state.settings.theme_select_open = false;
            write_ops(state, desktop, ConfigFile::Config, ops);
            return;
        }
        "app.approval_mode" => {
            let modes = fields::approval_modes();
            if let Some(mode) = modes.get(index) {
                let op = fields::select_op("app.approval_mode", Some(mode));
                write_ops(state, desktop, ConfigFile::Config, vec![op]);
            }
            return;
        }
        "remote.mode" => {
            let value = if index == 1 { "gateway" } else { "platform" };
            let op = fields::select_op("remote.mode", Some(value));
            write_ops(state, desktop, ConfigFile::Config, vec![op]);
            return;
        }
        _ => {}
    }
    // The row's select options are labels; the stored value is the label unless
    // a page built the options from a value list. The generic path stores the
    // option text, which is what `FIELDS` enums use.
    let Some(row) = focused_row(state) else {
        return;
    };
    let value = match &row.control {
        ControlModel::Select { options, .. } => options.get(index).cloned(),
        _ => None,
    };
    let Some(value) = value else {
        return;
    };
    let op = fields::select_op(path, Some(&value));
    write_ops(state, desktop, ConfigFile::Config, vec![op]);
}

fn write_ops(
    state: &mut UiState,
    desktop: &Arc<DesktopState>,
    file: ConfigFile,
    ops: Vec<ConfigOp>,
) {
    let expected = state.settings.revision(file);
    match desktop.apply_setting_ops(file, &ops, expected.as_ref()) {
        Ok(view) => {
            state.adopt_settings_view(view);
            state.settings.set_status(
                StatusKind::Success,
                format!("Saved to {}.", file.file_name()),
            );
            state.refresh_settings(desktop);
        }
        Err(error) => write_error(state, desktop, error),
    }
}

fn write_error(state: &mut UiState, desktop: &Arc<DesktopState>, error: ConfigError) {
    if error.is_conflict() {
        // The file is the truth: reload it and ask for the edit again instead
        // of overwriting the other writer's change.
        state.refresh_settings_files(desktop);
        let file = error.file().unwrap_or(ConfigFile::Config);
        state.settings.conflict_notice(file);
        return;
    }
    let file = error.file().unwrap_or(ConfigFile::Config);
    state.settings.notice(
        StatusKind::Error,
        format!("{}: {}", file.file_name(), error.notice()),
    );
}

fn no_service(state: &mut UiState) {
    state.settings.notice(
        StatusKind::Muted,
        "This build has no desktop service, so the settings files were not touched.",
    );
}

// ---------------------------------------------------------------------------
// The forms
// ---------------------------------------------------------------------------

fn clear_form_drafts(state: &mut UiState, prefix: &str) {
    let keys: Vec<String> = state
        .settings
        .drafts
        .keys()
        .filter(|key| key.starts_with(prefix))
        .cloned()
        .collect();
    for key in keys {
        state.settings.drafts.remove(&key);
    }
}

fn clear_all_form_drafts(state: &mut UiState) {
    clear_form_drafts(state, "model-form.");
    clear_form_drafts(state, "connector-form.");
    clear_form_drafts(state, "host-form.");
}

fn model_form_id(field: ModelFormField) -> &'static str {
    match field {
        ModelFormField::Slug => "model-form.slug",
        ModelFormField::ModelId => "model-form.model_id",
        ModelFormField::Name => "model-form.name",
        ModelFormField::BaseUrl => "model-form.base_url",
        ModelFormField::ApiKey => "model-form.api_key",
        ModelFormField::ApiKeyEnv => "model-form.api_key_env",
        ModelFormField::MaxCompletionTokens => "model-form.max_tokens",
        ModelFormField::ContextWindow => "model-form.context_window",
    }
}

fn connector_form_id(field: crate::ui::settings::state::ConnectorFormField) -> &'static str {
    use crate::ui::settings::state::ConnectorFormField as F;
    match field {
        F::Name => "connector-form.name",
        F::Url => "connector-form.url",
        F::Command => "connector-form.command",
        F::Args => "connector-form.args",
    }
}

fn host_form_id(field: crate::ui::settings::state::HostFormField) -> &'static str {
    use crate::ui::settings::state::HostFormField as F;
    match field {
        F::Name => "host-form.name",
        F::Host => "host-form.host",
        F::User => "host-form.user",
        F::Port => "host-form.port",
        F::Identity => "host-form.identity",
    }
}

fn update_model_form(state: &mut UiState, field: ModelFormField, value: String) {
    if let Some(SettingsForm::Model(form)) = state.settings.form.as_mut() {
        match field {
            ModelFormField::Slug => form.slug = value,
            ModelFormField::ModelId => form.model_id = value,
            ModelFormField::Name => form.name = value,
            ModelFormField::BaseUrl => form.base_url = value,
            ModelFormField::ApiKey => form.api_key = value,
            ModelFormField::ApiKeyEnv => form.api_key_env = value,
            ModelFormField::MaxCompletionTokens => form.max_completion_tokens = value,
            ModelFormField::ContextWindow => form.context_window = value,
        }
    }
}

fn update_form_draft(state: &mut UiState, id: &str, value: &str) {
    let value = value.to_string();
    match id {
        "model-form.slug" => update_model_form(state, ModelFormField::Slug, value),
        "model-form.model_id" => update_model_form(state, ModelFormField::ModelId, value),
        "model-form.name" => update_model_form(state, ModelFormField::Name, value),
        "model-form.base_url" => update_model_form(state, ModelFormField::BaseUrl, value),
        "model-form.api_key" => update_model_form(state, ModelFormField::ApiKey, value),
        "model-form.api_key_env" => update_model_form(state, ModelFormField::ApiKeyEnv, value),
        "model-form.max_tokens" => {
            update_model_form(state, ModelFormField::MaxCompletionTokens, value)
        }
        "model-form.context_window" => {
            update_model_form(state, ModelFormField::ContextWindow, value)
        }
        "connector-form.name" => {
            if let Some(SettingsForm::Connector(form)) = state.settings.form.as_mut() {
                form.name = value;
            }
        }
        "connector-form.url" => {
            if let Some(SettingsForm::Connector(form)) = state.settings.form.as_mut() {
                form.url = value;
            }
        }
        "connector-form.command" => {
            if let Some(SettingsForm::Connector(form)) = state.settings.form.as_mut() {
                form.command = value;
            }
        }
        "connector-form.args" => {
            if let Some(SettingsForm::Connector(form)) = state.settings.form.as_mut() {
                form.args = value;
            }
        }
        "host-form.name" => {
            if let Some(SettingsForm::Host(form)) = state.settings.form.as_mut() {
                form.name = value;
            }
        }
        "host-form.host" => {
            if let Some(SettingsForm::Host(form)) = state.settings.form.as_mut() {
                form.host = value;
            }
        }
        "host-form.user" => {
            if let Some(SettingsForm::Host(form)) = state.settings.form.as_mut() {
                form.user = value;
            }
        }
        "host-form.port" => {
            if let Some(SettingsForm::Host(form)) = state.settings.form.as_mut() {
                form.port = value;
            }
        }
        "host-form.identity" => {
            if let Some(SettingsForm::Host(form)) = state.settings.form.as_mut() {
                form.identity = value;
            }
        }
        _ => {}
    }
}

/// A toggle that belongs to an open form is handled here and answers `Some`.
fn form_toggle(state: &mut SettingsUiState, path: &str, value: bool) -> Option<bool> {
    match path {
        "model-form.hidden" => {
            if let Some(SettingsForm::Model(form)) = state.form.as_mut() {
                form.hidden = value;
                return Some(true);
            }
        }
        "model-form.set_default" => {
            if let Some(SettingsForm::Model(form)) = state.form.as_mut() {
                form.set_default = value;
                return Some(true);
            }
        }
        "connector-form.enabled" => {
            if let Some(SettingsForm::Connector(form)) = state.form.as_mut() {
                form.enabled = value;
                return Some(true);
            }
        }
        "harness-form.enabled" => {
            if let Some(SettingsForm::Harness(form)) = state.form.as_mut() {
                form.enabled = value;
                return Some(true);
            }
        }
        _ => {}
    }
    None
}

fn parse_optional<T: std::str::FromStr>(value: &str) -> Option<T> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        value.parse().ok()
    }
}

fn test_model_draft(state: &mut UiState, desktop: Option<&Arc<DesktopState>>) {
    let Some(desktop) = desktop else {
        no_service(state);
        return;
    };
    let Some(SettingsForm::Model(form)) = state.settings.form.clone() else {
        return;
    };
    let draft_row =
        |id: &str, form_value: &str| state.settings.draft(id, form_value).trim().to_string();
    let draft = ModelProbeDraft {
        base_url: non_empty(&draft_row("model-form.base_url", &form.base_url)),
        api_key: non_empty(&draft_row("model-form.api_key", &form.api_key)),
        api_key_env: non_empty(&draft_row("model-form.api_key_env", &form.api_key_env)),
        provider_hint: non_empty(&form.slug),
    };
    let result = desktop.probe_model_draft(&draft);
    let (kind, line) = match result {
        Ok(report) => (
            if report.ok {
                StatusKind::Success
            } else {
                StatusKind::Error
            },
            report.line(),
        ),
        Err(error) => (StatusKind::Error, error),
    };
    if let Some(SettingsForm::Model(form)) = state.settings.form.as_mut() {
        form.probe = Some((kind, line.clone()));
    }
    state.settings.set_status(kind, line);
}

fn save_model_form(state: &mut UiState, desktop: Option<&Arc<DesktopState>>) {
    let Some(desktop) = desktop else {
        no_service(state);
        return;
    };
    let Some(SettingsForm::Model(form)) = state.settings.form.clone() else {
        return;
    };
    let field = |id: &str, fallback: &str| state.settings.draft(id, fallback).trim().to_string();
    let slug = if form.editing.is_some() {
        form.editing.clone().unwrap_or_else(|| form.slug.clone())
    } else {
        field("model-form.slug", &form.slug)
    };
    if slug.is_empty() {
        state
            .settings
            .set_status(StatusKind::Error, "A model needs a slug (its table name).");
        return;
    }
    let api_key = field("model-form.api_key", &form.api_key);
    let draft = ModelDraft {
        name: Some(field("model-form.name", &form.name)),
        model_id: Some(field("model-form.model_id", &form.model_id)),
        base_url: Some(field("model-form.base_url", &form.base_url)),
        api_key: if api_key.is_empty() {
            None
        } else {
            Some(api_key)
        },
        api_key_env: Some(field("model-form.api_key_env", &form.api_key_env)),
        max_completion_tokens: parse_optional(&field(
            "model-form.max_tokens",
            &form.max_completion_tokens,
        )),
        context_window: parse_optional(&field("model-form.context_window", &form.context_window)),
        hidden: Some(form.hidden),
        set_default: form.set_default,
    };
    let expected = state.settings.config_revision();
    match desktop.upsert_config_model(&slug, &draft, expected.as_ref()) {
        Ok(view) => {
            state.adopt_settings_view(view);
            close_sheet(state);
            state.settings.set_status(
                StatusKind::Success,
                format!("Saved [model.{slug}] to config.toml."),
            );
            state.refresh_settings(desktop);
        }
        Err(error) => write_error(state, desktop, error),
    }
}

fn save_connector_form(state: &mut UiState, desktop: Option<&Arc<DesktopState>>) {
    let Some(desktop) = desktop else {
        no_service(state);
        return;
    };
    let Some(SettingsForm::Connector(form)) = state.settings.form.clone() else {
        return;
    };
    let field = |id: &str, fallback: &str| state.settings.draft(id, fallback).trim().to_string();
    let name = if form.editing.is_some() {
        form.editing.clone().unwrap_or_else(|| form.name.clone())
    } else {
        field("connector-form.name", &form.name)
    };
    if name.is_empty() {
        state.settings.set_status(
            StatusKind::Error,
            "A connector needs a name (its table key).",
        );
        return;
    }
    let transport = if form.transport == 0 {
        McpTransportDraft::Http {
            url: field("connector-form.url", &form.url),
        }
    } else {
        McpTransportDraft::Stdio {
            command: field("connector-form.command", &form.command),
            args: field("connector-form.args", &form.args)
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            env: BTreeMap::new(),
        }
    };
    let draft = McpDraft {
        transport,
        enabled: form.enabled,
    };
    let expected = state.settings.config_revision();
    match desktop.upsert_mcp_config(&name, &draft, expected.as_ref()) {
        Ok(view) => {
            state.adopt_settings_view(view);
            close_sheet(state);
            state.settings.set_status(
                StatusKind::Success,
                format!("Saved [mcp_servers.{name}] and synced the registry."),
            );
            state.refresh_settings(desktop);
        }
        Err(error) => write_error(state, desktop, error),
    }
}

fn save_host_form(state: &mut UiState, desktop: Option<&Arc<DesktopState>>) {
    let Some(desktop) = desktop else {
        no_service(state);
        return;
    };
    let Some(SettingsForm::Host(form)) = state.settings.form.clone() else {
        return;
    };
    let field = |id: &str, fallback: &str| state.settings.draft(id, fallback).trim().to_string();
    let draft = HostDraft {
        name: field("host-form.name", &form.name),
        host: field("host-form.host", &form.host),
        user: non_empty(&field("host-form.user", &form.user)),
        port: parse_optional::<u16>(&field("host-form.port", &form.port)),
        identity: non_empty(&field("host-form.identity", &form.identity)),
    };
    let expected = state.settings.revision(ConfigFile::Hosts);
    match desktop.upsert_host(form.editing.as_deref(), &draft, expected.as_ref()) {
        Ok(view) => {
            state.adopt_settings_view(view);
            close_sheet(state);
            state
                .settings
                .set_status(StatusKind::Success, format!("Saved host {}.", draft.name));
            state.refresh_settings(desktop);
        }
        Err(error) => write_error(state, desktop, error),
    }
}

fn save_harness_form(state: &mut UiState, desktop: Option<&Arc<DesktopState>>) {
    let Some(desktop) = desktop else {
        no_service(state);
        return;
    };
    let Some(SettingsForm::Harness(form)) = state.settings.form.clone() else {
        return;
    };
    let Some(id) = form.editing.clone().filter(|id| !id.trim().is_empty()) else {
        state
            .settings
            .set_status(StatusKind::Error, "A harness override needs a catalog id.");
        return;
    };
    let field = |id: &str, fallback: &str| state.settings.draft(id, fallback).trim().to_string();
    let command = non_empty(&field("harness-form.command", &form.command));
    let args: Vec<String> = field("harness-form.args", &form.args)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let draft = HarnessDraft {
        enabled: Some(form.enabled),
        command,
        // An empty argument list clears the override array rather than leaving
        // the catalog's arguments in place: the form shows what will launch.
        args: Some(args),
        env: None,
    };
    let expected = state.settings.config_revision();
    match desktop.upsert_harness(&id, &draft, expected.as_ref()) {
        Ok(view) => {
            state.adopt_settings_view(view);
            close_sheet(state);
            start_harness_scan(state, desktop);
            state.settings.set_status(
                StatusKind::Success,
                format!("Saved [harness.{id}] to config.toml."),
            );
        }
        Err(error) => write_error(state, desktop, error),
    }
}

/// After a harness write, re-read the detection so the row's status is the
/// service's answer, not a stale one.
fn start_harness_scan(state: &mut UiState, desktop: &Arc<DesktopState>) {
    state.settings.harnesses = desktop.list_harnesses();
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Keep the `RemoteMode` import referenced: the remote-mode select writes the
/// spelling this enum owns.
#[allow(dead_code)]
fn _mode(mode: RemoteMode) -> String {
    match mode {
        RemoteMode::Platform => "platform".to_string(),
        RemoteMode::Gateway => "gateway".to_string(),
    }
}

/// Keep `ConfigRevision` named: the dispatch always fences against the revision
/// the page rendered from.
#[allow(dead_code)]
fn _revision(_: &ConfigRevision) {}
